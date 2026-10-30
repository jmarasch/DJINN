//! Back up and restore (admin): everything DJINN knows in one zip, and
//! putting a backup back, on this computer or another one with the music
//! somewhere else.
//!
//! A backup holds a snapshot of the database (settings, users, playlists,
//! history, speakers, corrections), the cached artwork, and a manifest
//! naming the music folders. Not the music, the AI models (downloaded
//! again on request) or config.toml (it belongs to the computer).
//!
//! Restoring happens at the next start, before the database opens: an
//! uploaded backup waits in `<db dir>/restore/` with the folder moves the
//! admin chose, DJINN restarts, swaps the database (keeping the old one as
//! `<name>.before-restore`) and then moves the songs to their new folders.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_store::{now_ms, Store};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Bumped when a backup's layout changes in a way older code can't read.
const FORMAT: u32 = 1;
const MANIFEST: &str = "manifest.json";
const DB_ENTRY: &str = "djinn.db";
const ART_PREFIX: &str = "art/";
/// Where an uploaded backup waits, under the database's folder.
pub const RESTORE_DIR: &str = "restore";
const UPLOAD: &str = "backup.zip";
const PLAN: &str = "plan.json";

#[derive(Debug, Deserialize, Serialize)]
struct Manifest {
    format: u32,
    /// The DJINN that made it.
    version: String,
    created_ms: i64,
    /// Music folders when it was made, to move songs from.
    roots: Vec<PathBuf>,
}

/// The folder moves the admin chose, waiting for the restart.
#[derive(Debug, Default, Deserialize, Serialize)]
struct Plan {
    moves: Vec<(PathBuf, PathBuf)>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/backup", get(download))
        .route(
            "/backup/restore",
            post(upload)
                .delete(cancel)
                // A backup with a big artwork cache can run to gigabytes.
                .layer(DefaultBodyLimit::disable()),
        )
        .route("/backup/restore/start", post(start))
}

/// The folder holding the database (and the art cache).
fn data_dir(state: &AppState) -> PathBuf {
    state
        .art_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

async fn download(_: AdminUser, State(state): State<AppState>) -> ApiResult<Response> {
    let work = data_dir(&state).join(format!("backup-{}", now_ms()));
    fs::create_dir_all(&work).map_err(anyhow::Error::from)?;
    let made = make_backup(&state, &work).await;
    let zip = match made {
        Ok(zip) => zip,
        Err(e) => {
            let _ = fs::remove_dir_all(&work);
            return Err(e.into());
        }
    };
    let file = tokio::fs::File::open(&zip)
        .await
        .map_err(anyhow::Error::from)?;
    let name = format!("djinn-backup-{}.zip", date(now_ms()));
    // Sends the file, then removes the working folder.
    let body = futures_util::stream::unfold(Some((file, work)), |open| async move {
        let (mut file, work) = open?;
        let mut buf = vec![0; 256 * 1024];
        match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
            Ok(0) => {
                drop(file);
                let _ = tokio::fs::remove_dir_all(&work).await;
                None
            }
            Ok(n) => {
                buf.truncate(n);
                Some((Ok(Bytes::from(buf)), Some((file, work))))
            }
            Err(e) => {
                drop(file);
                let _ = tokio::fs::remove_dir_all(&work).await;
                Some((Err(e), None))
            }
        }
    });
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_owned()),
        ],
        Body::from_stream(body),
    )
        .into_response())
}

/// Writes the backup into `work`; returns the zip's path.
async fn make_backup(state: &AppState, work: &Path) -> anyhow::Result<PathBuf> {
    let db = work.join(DB_ENTRY);
    state.store.snapshot(&db).await?;
    let manifest = Manifest {
        format: FORMAT,
        version: env!("CARGO_PKG_VERSION").to_owned(),
        created_ms: now_ms(),
        roots: crate::folders::library_roots(state).await?,
    };
    let zip = work.join(UPLOAD);
    let art_dir = state.art_dir.clone();
    let out = zip.clone();
    tokio::task::spawn_blocking(move || write_zip(&out, &manifest, &db, &art_dir)).await??;
    let _ = fs::remove_file(work.join(DB_ENTRY));
    Ok(zip)
}

fn write_zip(out: &Path, manifest: &Manifest, db: &Path, art_dir: &Path) -> anyhow::Result<()> {
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(out)?));
    let packed = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);
    // Images are compressed already.
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file(MANIFEST, packed)?;
    zip.write_all(&serde_json::to_vec_pretty(manifest)?)?;
    zip.start_file(DB_ENTRY, packed)?;
    io::copy(&mut File::open(db)?, &mut zip)?;
    // The cached images; thumbnails (in a subfolder) are made again.
    if let Ok(entries) = fs::read_dir(art_dir) {
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            zip.start_file(format!("{ART_PREFIX}{name}"), stored)?;
            io::copy(&mut File::open(entry.path())?, &mut zip)?;
        }
    }
    zip.finish()?.flush()?;
    Ok(())
}

/// "2026-10-29" for a time in ms since 1970 (UTC).
fn date(ms: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[derive(Serialize)]
struct Uploaded {
    version: String,
    created_ms: i64,
    folders: Vec<Folder>,
}

#[derive(Serialize)]
struct Folder {
    /// Where the music was.
    from: PathBuf,
    /// Whether that folder exists on this computer too.
    here: bool,
}

/// Takes an uploaded backup (the zip as the request body), checks it, and
/// keeps it until the admin starts the restore.
async fn upload(
    _: AdminUser,
    State(state): State<AppState>,
    body: Body,
) -> ApiResult<Json<Uploaded>> {
    let dir = data_dir(&state).join(RESTORE_DIR);
    let _ = tokio::fs::remove_dir_all(&dir).await;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let path = dir.join(UPLOAD);
    let saved = save_body(body, &path).await;
    let checked = match saved {
        Ok(()) => tokio::task::spawn_blocking({
            let path = path.clone();
            move || read_manifest(&path)
        })
        .await
        .map_err(anyhow::Error::from)?,
        Err(e) => Err(ApiError::bad_request(format!(
            "the upload didn't finish: {e}"
        ))),
    };
    let manifest = match checked {
        Ok(manifest) => manifest,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            return Err(e);
        }
    };
    Ok(Json(Uploaded {
        version: manifest.version,
        created_ms: manifest.created_ms,
        folders: manifest
            .roots
            .into_iter()
            .map(|from| Folder {
                here: from.is_dir(),
                from,
            })
            .collect(),
    }))
}

async fn save_body(body: Body, path: &Path) -> io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        file.write_all(&chunk.map_err(io::Error::other)?).await?;
    }
    file.flush().await
}

/// The backup's manifest, if it's a backup this DJINN can restore.
fn read_manifest(path: &Path) -> ApiResult<Manifest> {
    let not_backup = || ApiError::bad_request("that file isn't a DJINN backup");
    let mut zip = zip::ZipArchive::new(File::open(path).map_err(anyhow::Error::from)?)
        .map_err(|_| not_backup())?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|_| not_backup())?
        .read_to_string(&mut text)
        .map_err(|_| not_backup())?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|_| not_backup())?;
    zip.by_name(DB_ENTRY).map_err(|_| not_backup())?;
    if manifest.format > FORMAT || newer(&manifest.version, env!("CARGO_PKG_VERSION")) {
        return Err(ApiError::bad_request(format!(
            "this backup is from DJINN {}, newer than this one ({}). Update DJINN first.",
            manifest.version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    Ok(manifest)
}

/// Whether version `a` ("0.3.0") is newer than `b`.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .take(3)
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a) > parts(b)
}

#[derive(Deserialize)]
struct Start {
    /// Where each of the backup's music folders is on this computer; a
    /// folder left out keeps its old path (its songs show as missing).
    moves: Vec<(PathBuf, PathBuf)>,
}

/// Starts the restore: DJINN restarts and puts the backup in place.
async fn start(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Start>,
) -> ApiResult<StatusCode> {
    let dir = data_dir(&state).join(RESTORE_DIR);
    if !dir.join(UPLOAD).is_file() {
        return Err(ApiError::bad_request("upload a backup first"));
    }
    let mut moves = Vec::new();
    for (from, to) in body.moves {
        if to.as_os_str().is_empty() || to == from {
            continue;
        }
        if !to.is_absolute() || !to.is_dir() {
            return Err(ApiError::bad_request(format!(
                "{} isn't a folder on this computer",
                to.display()
            )));
        }
        moves.push((from, to));
    }
    let plan = serde_json::to_vec(&Plan { moves }).map_err(anyhow::Error::from)?;
    tokio::fs::write(dir.join(PLAN), plan)
        .await
        .map_err(anyhow::Error::from)?;
    tracing::info!("restoring a backup; restarting");
    state.update.restart(&state.shutdown_request);
    Ok(StatusCode::ACCEPTED)
}

async fn cancel(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let _ = tokio::fs::remove_dir_all(data_dir(&state).join(RESTORE_DIR)).await;
    Ok(StatusCode::NO_CONTENT)
}

/// A restore put in place at start, finished once the database opens.
#[derive(Debug)]
pub struct Restored {
    moves: Vec<(PathBuf, PathBuf)>,
    /// The database from before, to go back to if the backup won't open.
    before: PathBuf,
}

/// At start, before the database opens: if a restore is waiting, swaps the
/// backup's database in and adds its artwork. A failed restore is logged
/// and dropped, leaving the database as it was.
pub fn put_in_place(db: &Path, art_dir: &Path) -> Option<Restored> {
    let dir = db.parent()?.join(RESTORE_DIR);
    let plan = fs::read(dir.join(PLAN)).ok()?;
    let result = serde_json::from_slice::<Plan>(&plan)
        .map_err(anyhow::Error::from)
        .and_then(|plan| swap(db, art_dir, &dir.join(UPLOAD), plan));
    let _ = fs::remove_dir_all(&dir);
    match result {
        Ok(restored) => {
            tracing::info!("backup restored");
            Some(restored)
        }
        Err(e) => {
            tracing::error!(error = %e, "restoring the backup failed; nothing changed");
            None
        }
    }
}

fn swap(db: &Path, art_dir: &Path, zip_path: &Path, plan: Plan) -> anyhow::Result<Restored> {
    let mut zip = zip::ZipArchive::new(File::open(zip_path)?)?;
    let incoming = with_suffix(db, ".restoring");
    io::copy(&mut zip.by_name(DB_ENTRY)?, &mut File::create(&incoming)?)?;
    fs::create_dir_all(art_dir)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(name) = entry.name().strip_prefix(ART_PREFIX).map(str::to_owned) else {
            continue;
        };
        // Cache files are flat, named by hash: anything else is skipped.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            continue;
        }
        let target = art_dir.join(&name);
        if !target.exists() {
            io::copy(&mut entry, &mut File::create(target)?)?;
        }
    }
    let before = with_suffix(db, ".before-restore");
    for suffix in ["", "-wal", "-shm"] {
        let old = with_suffix(&before, suffix);
        let _ = fs::remove_file(&old);
        let current = with_suffix(db, suffix);
        if current.exists() {
            fs::rename(&current, &old)?;
        }
    }
    fs::rename(&incoming, db)?;
    Ok(Restored {
        moves: plan.moves,
        before,
    })
}

/// `path` with `suffix` added to its file name.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// The restored database didn't open: back to the one from before.
pub fn undo(db: &Path, restored: &Restored) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(with_suffix(db, suffix));
        let _ = fs::rename(
            with_suffix(&restored.before, suffix),
            with_suffix(db, suffix),
        );
    }
    tracing::error!("the backup's database didn't open; went back to the one from before");
}

/// Once the restored database is open: moves songs to their new folders.
pub async fn finish(
    store: &Store,
    config_roots: &[PathBuf],
    restored: &Restored,
) -> anyhow::Result<()> {
    for (from, to) in &restored.moves {
        let moved = store
            .move_library(
                &from.to_string_lossy(),
                &to.to_string_lossy(),
                std::path::MAIN_SEPARATOR,
            )
            .await?;
        tracing::info!(from = %from.display(), to = %to.display(), moved, "music folder moved");
    }
    crate::folders::move_roots(store, config_roots, &restored.moves).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_versions() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_793_301_966_375), "2026-10-29");
        assert!(newer("0.3.0", "0.2.5"));
        assert!(!newer("0.2.5", "0.2.5"));
        assert!(!newer("0.2.0", "0.10.0"));
    }

    #[test]
    fn a_backup_swaps_in_and_can_be_undone() {
        let dir = std::env::temp_dir().join(format!("djinn-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let art = dir.join("art");
        fs::create_dir_all(&art).unwrap();
        fs::write(art.join("aa.jpg"), b"cover").unwrap();
        fs::create_dir_all(art.join("thumbs")).unwrap();
        fs::write(art.join("thumbs/aa-128.jpg"), b"thumb").unwrap();
        let snapshot = dir.join("snapshot.db");
        fs::write(&snapshot, b"backup db").unwrap();
        let manifest = Manifest {
            format: FORMAT,
            version: env!("CARGO_PKG_VERSION").into(),
            created_ms: 1,
            roots: vec![PathBuf::from("/old/music")],
        };
        let zip = dir.join(UPLOAD);
        write_zip(&zip, &manifest, &snapshot, &art).unwrap();
        assert_eq!(read_manifest(&zip).unwrap().roots, manifest.roots);

        // On the "new computer": a different database and an empty cache.
        let db = dir.join("new/djinn.db");
        let new_art = dir.join("new/art");
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        fs::write(&db, b"current db").unwrap();
        let plan = Plan {
            moves: vec![("/old/music".into(), "/new/music".into())],
        };
        let restored = swap(&db, &new_art, &zip, plan).unwrap();
        assert_eq!(fs::read(&db).unwrap(), b"backup db");
        assert_eq!(fs::read(new_art.join("aa.jpg")).unwrap(), b"cover");
        assert!(
            !new_art.join("thumbs").exists(),
            "thumbnails are made again"
        );
        assert_eq!(restored.moves.len(), 1);

        undo(&db, &restored);
        assert_eq!(fs::read(&db).unwrap(), b"current db");
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn admins_download_and_upload_backups() {
        use crate::test_util::{send_bytes, session};
        use axum::http::Method;
        use djinn_store::Role;

        let dir = std::env::temp_dir().join(format!("djinn-backup-http-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("art")).unwrap();
        fs::write(dir.join("art/bb.png"), b"art").unwrap();
        let state = AppState::for_tests_in(&dir).await;
        let music = dir.join("music");
        fs::create_dir_all(&music).unwrap();
        crate::folders::add_root_for_tests(&state, &music).await;
        let admin = session(&state, "ana", Role::Admin).await;
        let user = session(&state, "sam", Role::User).await;

        let (status, _) = send_bytes(&state, Method::GET, "/api/backup", Vec::new(), &user).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, zip) =
            send_bytes(&state, Method::GET, "/api/backup", Vec::new(), &admin).await;
        assert_eq!(status, StatusCode::OK);
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("backup-"))
            .count();
        assert_eq!(leftovers, 0, "the working folder goes once it's sent");

        // The zip holds a database that opens, with the accounts in it.
        let saved = dir.join("saved.zip");
        fs::write(&saved, &zip).unwrap();
        assert_eq!(read_manifest(&saved).unwrap().roots, [music]);
        let mut archive = zip::ZipArchive::new(File::open(&saved).unwrap()).unwrap();
        let copy = dir.join("copy.db");
        io::copy(
            &mut archive.by_name(DB_ENTRY).unwrap(),
            &mut File::create(&copy).unwrap(),
        )
        .unwrap();
        assert!(archive.by_name("art/bb.png").is_ok());
        let restored = Store::open(&copy).await.unwrap();
        assert!(restored.admin_exists().await.unwrap());
        drop(restored);

        let (status, body) =
            send_bytes(&state, Method::POST, "/api/backup/restore", zip, &admin).await;
        assert_eq!(status, StatusCode::OK);
        let body = String::from_utf8(body).unwrap();
        assert!(body.contains(r#""here":true"#), "{body}");
        let (status, _) = send_bytes(
            &state,
            Method::POST,
            "/api/backup/restore",
            b"not a zip".to_vec(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn other_files_are_refused() {
        let dir = std::env::temp_dir().join(format!("djinn-notbackup-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("song.zip");
        fs::write(&path, b"not a zip").unwrap();
        assert!(read_manifest(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
