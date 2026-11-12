//! DJINN music library: scanner, tags, cover art, and search.

pub mod art;
pub mod credits;
pub mod duplicates;
pub mod genres;
pub mod identify;
pub mod lyrics;
pub mod online;
pub mod playlist_file;
pub mod playlist_match;
pub mod radio;
pub mod sound;
mod tags;
pub use tags::{basics, Basics};
pub mod vibe;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use djinn_store::{ProblemKind, ScanProblem, ScannedTrack, Store, StoreError, TagEdit};
use serde::Serialize;
use walkdir::WalkDir;

/// File extensions the engine can decode.
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav"];
/// Audio formats DJINN doesn't play, reported on the Problems page instead
/// of skipped without a word.
const OTHER_AUDIO: &[&str] = &[
    "opus", "wma", "aif", "aiff", "aifc", "ape", "wv", "mpc", "dsf", "dff", "mka", "webm", "weba",
    "ac3", "dts", "amr", "tta", "ra", "rm", "au", "caf", "m4b", "m4p", "mp2", "spx", "mid", "midi",
];
/// Files read per batch; each batch is saved in one transaction.
const BATCH: usize = 64;
/// Threads reading tags in parallel, which mostly helps on network shares.
const READERS: usize = 4;
/// Bumped when the scanner starts extracting something new, so the next
/// scan re-reads every file once. 2: artwork sources, " - Topic" cleanup.
/// 3: separate artists for collaborations and features. 4: ReplayGain tags.
/// 6: original release years. 7: notes on unreadable tags.
const SCAN_VERSION: i64 = 7;
/// Set to anything else to make the next scan re-read every file.
pub const SCAN_VERSION_KEY: &str = "scan_version";
/// Artist names never split (one per line); unset means the defaults.
pub const KEEP_WHOLE_KEY: &str = "artist_keep_whole";
/// Whether a file's original release date wins over its own date.
pub const ORIGINAL_YEARS_KEY: &str = "original_years";

/// Whether years come from the original release, on unless turned off.
pub async fn original_years(store: &Store) -> Result<bool, LibraryError> {
    Ok(store.get_setting(ORIGINAL_YEARS_KEY).await?.as_deref() != Some("0"))
}

/// The never-split artist names in effect.
pub async fn keep_whole(store: &Store) -> Result<Vec<String>, LibraryError> {
    Ok(match store.get_setting(KEEP_WHOLE_KEY).await? {
        Some(list) => list
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
        None => credits::DEFAULT_KEEP_WHOLE
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("scan task failed: {0}")]
    Task(String),
}

/// The steps of a library check, in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScanPhase {
    /// Walking the music folders for audio files.
    #[default]
    Finding,
    /// Reading the tags of new and changed files.
    Reading,
    /// Marking missing files and choosing album and artist pictures.
    Sorting,
    Done,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ScanProgress {
    pub phase: ScanPhase,
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
    /// The folder directly below the library root that holds this file
    /// (its artist's folder in an Artist/Album/Song layout).
    artist_dir: Option<PathBuf>,
    size: i64,
    mtime: i64,
}

/// Scans `roots`, saving new and changed files and marking vanished ones as
/// removed. Unchanged files (same size and mtime) are skipped. A root that
/// is missing (an unmounted share, say) is skipped without removing its
/// tracks; tracks under a folder that is no longer a root are removed.
pub async fn scan(
    store: &Store,
    roots: &[PathBuf],
    art_dir: &Path,
    mut progress: impl FnMut(&ScanProgress),
) -> Result<ScanProgress, LibraryError> {
    progress(&ScanProgress::default());
    let walk_roots = roots.to_vec();
    let (found, available_roots, problems) = tokio::task::spawn_blocking(move || walk(&walk_roots))
        .await
        .map_err(|e| LibraryError::Task(e.to_string()))?;

    let known = store.known_files().await?;
    let rescan_all = store
        .get_setting(SCAN_VERSION_KEY)
        .await?
        .and_then(|v| v.parse::<i64>().ok())
        != Some(SCAN_VERSION);
    let known_by_path: HashMap<&str, &djinn_store::KnownFile> =
        known.iter().map(|k| (k.path.as_str(), k)).collect();
    let to_read: Vec<FoundFile> = found
        .iter()
        .filter(|f| {
            known_by_path
                .get(f.path_str.as_str())
                .is_none_or(|k| rescan_all || k.removed || k.size != f.size || k.mtime != f.mtime)
        })
        .cloned()
        .collect();

    let mut status = ScanProgress {
        phase: ScanPhase::Reading,
        found: found.len(),
        to_read: to_read.len(),
        ..Default::default()
    };
    progress(&status);

    let art_cache = Arc::new(tags::ArtCache::default());
    let fixes = Arc::new(Fixes::load(store).await?);
    for batch in to_read.chunks(BATCH) {
        let batch = batch.to_vec();
        let art_dir = art_dir.to_path_buf();
        let cache = Arc::clone(&art_cache);
        let fixes = Arc::clone(&fixes);
        let scanned =
            tokio::task::spawn_blocking(move || read_batch(&batch, &art_dir, &cache, &fixes))
                .await
                .map_err(|e| LibraryError::Task(e.to_string()))?;
        store.save_scanned(&scanned).await?;
        status.read += scanned.len();
        progress(&status);
    }

    status.phase = ScanPhase::Sorting;
    progress(&status);
    let present: HashSet<&str> = found.iter().map(|f| f.path_str.as_str()).collect();
    let vanished: Vec<i64> = known
        .iter()
        .filter(|k| !k.removed && !present.contains(k.path.as_str()))
        .filter(|k| {
            let path = Path::new(&k.path);
            // Gone from a root we could read, or its root was removed from
            // the library. A root that is merely missing keeps its tracks.
            available_roots.iter().any(|r| path.starts_with(r))
                || !roots.iter().any(|r| path.starts_with(r))
        })
        .map(|k| k.id)
        .collect();
    store.mark_removed(&vanished).await?;
    store.replace_walk_problems(&problems).await?;
    // Which pictures show only changes when files do.
    if status.read > 0 || !vanished.is_empty() {
        store.refresh_artwork().await?;
    }
    store
        .set_setting(SCAN_VERSION_KEY, &SCAN_VERSION.to_string())
        .await?;
    status.removed = vanished.len();
    status.phase = ScanPhase::Done;
    status.done = true;
    progress(&status);
    Ok(status)
}

/// Re-reads particular files (after an identification was applied, say)
/// and saves them, then refreshes artwork. Files outside `roots` or gone
/// from disk are skipped.
pub async fn reread(
    store: &Store,
    roots: &[PathBuf],
    art_dir: &Path,
    paths: &[String],
) -> Result<usize, LibraryError> {
    let files: Vec<FoundFile> = paths
        .iter()
        .filter_map(|p| {
            let path = PathBuf::from(p);
            let root = roots.iter().find(|r| path.starts_with(r))?;
            let meta = std::fs::metadata(&path).ok()?;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            let artist_dir = path.strip_prefix(root).ok().and_then(|rel| {
                let mut parts = rel.components();
                let first = parts.next()?;
                parts.next().map(|_| root.join(first))
            });
            Some(FoundFile {
                path_str: p.clone(),
                path,
                artist_dir,
                size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
                mtime,
            })
        })
        .collect();
    let art_dir = art_dir.to_path_buf();
    let fixes = Fixes::load(store).await?;
    let scanned = tokio::task::spawn_blocking(move || {
        let cache = tags::ArtCache::default();
        read_batch(&files, &art_dir, &cache, &fixes)
    })
    .await
    .map_err(|e| LibraryError::Task(e.to_string()))?;
    store.save_scanned(&scanned).await?;
    store.refresh_artwork().await?;
    Ok(scanned.len())
}

/// Lists audio files under the roots. Returns them with the roots that could
/// actually be read and what couldn't be added.
fn walk(roots: &[PathBuf]) -> (Vec<FoundFile>, Vec<PathBuf>, Vec<ScanProblem>) {
    let mut found = Vec::new();
    let mut available = Vec::new();
    let mut problems = Vec::new();
    let mut note = |path: &Path, kind: ProblemKind, detail: String| {
        problems.push(ScanProblem {
            path: path.to_string_lossy().into_owned(),
            kind,
            detail,
        });
    };
    for root in roots {
        if !root.is_dir() {
            tracing::warn!(root = %root.display(), "library root missing, skipping");
            note(root, ProblemKind::MissingFolder, String::new());
            continue;
        }
        available.push(root.clone());
        for entry in WalkDir::new(root).follow_links(true) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    tracing::warn!(path = ?e.path(), error = %e, "cannot read library entry");
                    let why = if e.loop_ancestor().is_some() {
                        "a shortcut that loops back to a folder above it".to_owned()
                    } else {
                        e.io_error()
                            .map_or_else(|| e.to_string(), ToString::to_string)
                    };
                    note(e.path().unwrap_or(root), ProblemKind::Unreadable, why);
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            if !is_audio(entry.path()) {
                if let Some(ext) = other_audio(entry.path()) {
                    note(entry.path(), ProblemKind::Unsupported, ext);
                }
                continue;
            }
            let Some(path_str) = entry.path().to_str() else {
                tracing::warn!(path = %entry.path().display(), "skipping non-UTF-8 path");
                note(entry.path(), ProblemKind::BadName, String::new());
                continue;
            };
            let meta = match entry.metadata() {
                Ok(meta) => meta,
                Err(e) => {
                    note(entry.path(), ProblemKind::Unreadable, e.to_string());
                    continue;
                }
            };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(0);
            let artist_dir = entry.path().strip_prefix(root).ok().and_then(|rel| {
                let mut parts = rel.components();
                let first = parts.next()?;
                // Files directly in the root have no artist folder.
                parts.next().map(|_| root.join(first))
            });
            found.push(FoundFile {
                path: entry.path().to_path_buf(),
                path_str: path_str.to_owned(),
                artist_dir,
                size: i64::try_from(meta.len()).unwrap_or(i64::MAX),
                mtime,
            });
        }
    }
    (found, available, problems)
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// A file listed under its name alone, with why its tags are missing.
fn untagged(f: &FoundFile, why: &str) -> ScannedTrack {
    ScannedTrack {
        path: f.path_str.clone(),
        size: f.size,
        mtime: f.mtime,
        title: f
            .path
            .file_stem()
            .map_or_else(|| f.path_str.clone(), |s| s.to_string_lossy().into_owned()),
        tag_problem: Some(why.to_owned()),
        ..Default::default()
    }
}

/// The extension, lower case, of an audio file in a format DJINN can't play.
fn other_audio(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    OTHER_AUDIO.contains(&ext.as_str()).then_some(ext)
}

/// What the admin set that changes how files read: artist names never
/// split, tag corrections, and which date is a song's year.
struct Fixes {
    keep_whole: Vec<String>,
    edits: HashMap<String, TagEdit>,
    original_years: bool,
}

impl Fixes {
    async fn load(store: &Store) -> Result<Self, LibraryError> {
        Ok(Self {
            keep_whole: keep_whole(store).await?,
            edits: store.tag_edits_by_path().await?,
            original_years: original_years(store).await?,
        })
    }
}

fn read_batch(
    batch: &[FoundFile],
    art_dir: &Path,
    cache: &tags::ArtCache,
    fixes: &Fixes,
) -> Vec<ScannedTrack> {
    let scan = tags::ScanContext {
        art_dir,
        cache,
        keep_whole: &fixes.keep_whole,
        edits: &fixes.edits,
        original_years: fixes.original_years,
    };
    let per_thread = batch.len().div_ceil(READERS).max(1);
    std::thread::scope(|s| {
        let handles: Vec<_> = batch
            .chunks(per_thread)
            .map(|files| {
                s.spawn(move || {
                    files
                        .iter()
                        .map(|f| {
                            // A file that crashes the tag reader is still
                            // listed, under its name, with a note.
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                tags::read(
                                    &f.path,
                                    &f.path_str,
                                    f.size,
                                    f.mtime,
                                    f.artist_dir.as_deref(),
                                    &scan,
                                )
                            }))
                            .unwrap_or_else(|_| {
                                tracing::warn!(path = f.path_str, "reading tags crashed");
                                untagged(f, "reading the tags crashed the tag reader")
                            })
                        })
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
        std::fs::write(album.join("03 Third.wma"), b"not played").unwrap();
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

        // What couldn't be added is noted: the .wma and the missing folder.
        let unsupported = store
            .problems(ProblemKind::Unsupported, 0, 10)
            .await
            .unwrap();
        assert_eq!(unsupported.len(), 1);
        assert_eq!(unsupported[0].detail, "wma");
        let missing = store
            .problems(ProblemKind::MissingFolder, 0, 10)
            .await
            .unwrap();
        assert_eq!(missing.len(), 1);

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

        // Taking the folder out of the library removes its tracks.
        let fourth = scan(&store, &[], &art_dir, |_| {}).await.unwrap();
        assert_eq!(fourth.removed, 1);
        assert_eq!(store.track_count().await.unwrap(), 0);

        std::fs::remove_dir_all(tmp).unwrap();
    }
}
