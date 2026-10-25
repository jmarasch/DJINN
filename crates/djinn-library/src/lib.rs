//! DJINN music library: scanner, tags, cover art, and search.

pub mod art;
mod tags;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use djinn_store::{ScannedTrack, Store, StoreError};
use serde::Serialize;
use walkdir::WalkDir;

/// File extensions the engine can decode.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav"];
/// Files read per batch; each batch is saved in one transaction.
const BATCH: usize = 64;
/// Threads reading tags in parallel, which mostly helps on network shares.
const READERS: usize = 4;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("scan task failed: {0}")]
    Task(String),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScanProgress {
    /// Audio files found on disk.
    pub found: usize,
    /// New or changed files to read.
    pub to_read: usize,
    /// Of those, how many have been read so far.
    pub read: usize,
    /// Files marked missing.
    pub removed: usize,
    pub done: bool,
}

#[derive(Clone)]
struct FoundFile {
    path: PathBuf,
    path_str: String,
    size: i64,
    mtime: i64,
}

/// Scans `roots`, saving new and changed files and marking vanished ones as
/// removed. Unchanged files (same size and mtime) are skipped. A root that
/// is missing (an unmounted share, say) is skipped without removing its
/// tracks.
pub async fn scan(
    store: &Store,
    roots: &[PathBuf],
    art_dir: &Path,
    mut progress: impl FnMut(&ScanProgress),
) -> Result<ScanProgress, LibraryError> {
    let walk_roots = roots.to_vec();
    let (found, available_roots) = tokio::task::spawn_blocking(move || walk(&walk_roots))
        .await
        .map_err(|e| LibraryError::Task(e.to_string()))?;

    let known = store.known_files().await?;
    let known_by_path: HashMap<&str, &djinn_store::KnownFile> =
        known.iter().map(|k| (k.path.as_str(), k)).collect();
    let to_read: Vec<FoundFile> = found
        .iter()
        .filter(|f| {
            known_by_path
                .get(f.path_str.as_str())
                .is_none_or(|k| k.removed || k.size != f.size || k.mtime != f.mtime)
        })
        .cloned()
        .collect();

    let mut status = ScanProgress {
        found: found.len(),
        to_read: to_read.len(),
        ..Default::default()
    };
    progress(&status);

    let folder_art = Arc::new(Mutex::new(HashMap::new()));
    for batch in to_read.chunks(BATCH) {
        let batch = batch.to_vec();
        let art_dir = art_dir.to_path_buf();
        let cache = Arc::clone(&folder_art);
        let scanned = tokio::task::spawn_blocking(move || read_batch(&batch, &art_dir, &cache))
            .await
            .map_err(|e| LibraryError::Task(e.to_string()))?;
        store.save_scanned(&scanned).await?;
        status.read += scanned.len();
        progress(&status);
    }

    let present: HashSet<&str> = found.iter().map(|f| f.path_str.as_str()).collect();
    let vanished: Vec<i64> = known
        .iter()
        .filter(|k| !k.removed && !present.contains(k.path.as_str()))
        .filter(|k| {
            available_roots
                .iter()
                .any(|r| Path::new(&k.path).starts_with(r))
        })
        .map(|k| k.id)
        .collect();
    store.mark_removed(&vanished).await?;
    status.removed = vanished.len();
    status.done = true;
    progress(&status);
    Ok(status)
}

/// Lists audio files under the roots. Returns them with the roots that could
/// actually be read.
fn walk(roots: &[PathBuf]) -> (Vec<FoundFile>, Vec<PathBuf>) {
    let mut found = Vec::new();
    let mut available = Vec::new();
    for root in roots {
        if !root.is_dir() {
            tracing::warn!(root = %root.display(), "library root missing, skipping");
            continue;
        }
        available.push(root.clone());
        for entry in WalkDir::new(root).follow_links(true) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    tracing::warn!(error = %e, "cannot read library entry");
                    continue;
                }
            };
            if !entry.file_type().is_file() || !is_audio(entry.path()) {
                continue;
            }
            let Some(path_str) = entry.path().to_str() else {
                tracing::warn!(path = %entry.path().display(), "skipping non-UTF-8 path");
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            found.push(FoundFile {
                path: entry.path().to_path_buf(),
                path_str: path_str.to_owned(),
                size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
                mtime,
            });
        }
    }
    (found, available)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

fn read_batch(
    batch: &[FoundFile],
    art_dir: &Path,
    cache: &tags::FolderArtCache,
) -> Vec<ScannedTrack> {
    let per_thread = batch.len().div_ceil(READERS).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = batch
            .chunks(per_thread)
            .map(|files| {
                s.spawn(move || {
                    files
                        .iter()
                        .map(|f| tags::read(&f.path, &f.path_str, f.size, f.mtime, art_dir, cache))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .flatten()
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures")
            .join(name)
    }

    #[tokio::test]
    async fn scan_adds_skips_and_removes() {
        let tmp = std::env::temp_dir().join(format!("djinn-scan-{}", std::process::id()));
        let root = tmp.join("music");
        let album = root.join("Artist/Album");
        std::fs::create_dir_all(&album).unwrap();
        std::fs::copy(fixture("gapless_a.flac"), album.join("01 First.flac")).unwrap();
        std::fs::copy(fixture("gapless_b.mp3"), album.join("02 Second.mp3")).unwrap();
        std::fs::write(album.join("notes.txt"), b"not audio").unwrap();
        std::fs::write(album.join("cover.jpg"), [0xFF, 0xD8, 0xFF, 0xE0, 9, 9]).unwrap();
        let art_dir = tmp.join("art");
        let missing_root = tmp.join("unmounted");

        let store = Store::open_in_memory().await.unwrap();
        let roots = [root.clone(), missing_root];
        let mut updates = 0;
        let first = scan(&store, &roots, &art_dir, |_| updates += 1)
            .await
            .unwrap();
        assert_eq!((first.found, first.read, first.removed), (2, 2, 0));
        assert!(first.done);
        assert!(updates >= 2);

        // Untagged files fall back to the file name and pick up folder art.
        let hits = store.search("first", 10).await.unwrap().tracks;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "01 First");
        let hash = hits[0].art_hash.clone().unwrap();
        assert!(art::path(&art_dir, &hash).is_some());

        // Nothing changed: nothing is read again.
        let second = scan(&store, &roots, &art_dir, |_| {}).await.unwrap();
        assert_eq!((second.found, second.read), (2, 0));

        std::fs::remove_file(album.join("02 Second.mp3")).unwrap();
        let third = scan(&store, &roots, &art_dir, |_| {}).await.unwrap();
        assert_eq!((third.found, third.removed), (1, 1));
        assert_eq!(store.track_count().await.unwrap(), 1);

        std::fs::remove_dir_all(tmp).unwrap();
    }
}
