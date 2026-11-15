//! The library editor's file tools beyond tags: copying songs to another
//! folder, the trash (deleted songs wait in `.djinn-trash` in their library
//! folder until it's emptied), `folder.jpg` from an album's cover, and
//! tidying folders a move or delete left with no songs (they go to the
//! trash too, with whatever else was in them). Everything is recorded in
//! `file_edits` for undo, needs the protected area unlocked and write
//! permission on every folder it touches, and is audited.

use std::path::{Component, Path, PathBuf};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_store::{FileEdit, NewFileEdit, User};
use serde::{Deserialize, Serialize};

use crate::auth::Device;
use crate::editor_api::{
    all_editable, move_across, stat, undo_dir, Applied, Failed, RenameResult, Renamed,
};
use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/copy", post(copy))
        .route("/editor/trash", get(trash).delete(empty_trash))
        .route("/editor/folder-art", post(folder_art))
}

/// A folder inside a library folder, with no way out of it.
pub(crate) async fn library_folder(state: &AppState, folder: &str) -> ApiResult<PathBuf> {
    let folder = PathBuf::from(folder.trim());
    let roots = crate::folders::library_roots(state).await?;
    if folder.as_os_str().is_empty()
        || !roots.iter().any(|r| folder.starts_with(r))
        || folder
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(ApiError::bad_request("that isn't inside a library folder"));
    }
    Ok(folder)
}

// --- Copy ---

#[derive(Deserialize)]
struct Copy {
    ids: Vec<i64>,
    /// The folder to copy them to, keeping their names.
    folder: String,
    #[serde(default)]
    preview: bool,
}

/// Copies songs (and the `.lrc` beside each) to another folder, where they
/// become songs of their own.
async fn copy(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Copy>,
) -> ApiResult<Json<RenameResult>> {
    if body.ids.is_empty() || body.ids.len() > 5_000 {
        return Err(ApiError::bad_request("choose 1 to 5000 songs"));
    }
    let folder = library_folder(&state, &body.folder).await?;
    let songs = state.store.editor_songs_by_id(&body.ids).await?;
    let mut taken = std::collections::HashSet::new();
    let mut files: Vec<Renamed> = songs
        .iter()
        .filter_map(|song| {
            let from = PathBuf::from(&song.path);
            let to = folder.join(from.file_name()?);
            let to_text = to.to_string_lossy().into_owned();
            let problem = if to == from {
                Some("it's there already".to_owned())
            } else if !taken.insert(to_text.to_lowercase()) {
                Some("another song would get the same place".to_owned())
            } else if to.exists() {
                Some("a file with that name is there already".to_owned())
            } else {
                None
            };
            Some(Renamed {
                id: song.id,
                from: song.path.clone(),
                to: to_text,
                problem,
            })
        })
        .collect();
    if body.preview {
        return Ok(Json(RenameResult { files, batch: None }));
    }
    all_editable(
        &state,
        &files.iter().map(|f| f.to.as_str()).collect::<Vec<_>>(),
    )
    .await?;
    let batch = state.store.next_file_batch().await?;
    let mut copied = Vec::new();
    for file in &mut files {
        if file.problem.is_some() {
            continue;
        }
        let (from, to) = (PathBuf::from(&file.from), PathBuf::from(&file.to));
        let done = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&from, &to)?;
            let lrc = from.with_extension("lrc");
            if lrc.is_file() && !to.with_extension("lrc").exists() {
                let _ = std::fs::copy(&lrc, to.with_extension("lrc"));
            }
            Ok(())
        })
        .await
        .map_err(anyhow::Error::from)?;
        match done {
            Ok(()) => copied.push(file.to.clone()),
            Err(e) => file.problem = Some(e.to_string()),
        }
    }
    if !copied.is_empty() {
        crate::library_api::reread(&state, &copied).await?;
    }
    for file in files.iter().filter(|f| f.problem.is_none()) {
        let (size, mtime) = stat(Path::new(&file.to)).unwrap_or((0, 0));
        let new_id = state.store.track_id_at(&file.to).await?;
        state
            .store
            .record_file_edit(&NewFileEdit {
                batch,
                track_id: new_id,
                path: &file.to,
                kind: "copy",
                before: &file.from,
                after: &file.to,
                size,
                mtime,
                made_by: Some(user.id),
            })
            .await?;
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_copy",
        &format!("{} files, approval {batch}", copied.len()),
    )
    .await;
    Ok(Json(RenameResult {
        files,
        batch: (!copied.is_empty()).then_some(batch),
    }))
}

// --- The trash ---

#[derive(Default, Serialize)]
struct Trash {
    files: u64,
    bytes: u64,
}

fn count(dir: &Path, into: &mut Trash) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(t) if t.is_dir() => count(&entry.path(), into),
            Ok(_) => {
                into.files += 1;
                into.bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
            Err(_) => {}
        }
    }
}

/// What the trash holds across the library folders.
async fn trash(_: Elevated, State(state): State<AppState>) -> ApiResult<Json<Trash>> {
    let roots = crate::folders::library_roots(&state).await?;
    let found = tokio::task::spawn_blocking(move || {
        let mut trash = Trash::default();
        for root in roots {
            count(&root.join(djinn_library::TRASH), &mut trash);
        }
        trash
    })
    .await
    .map_err(anyhow::Error::from)?;
    Ok(Json(found))
}

/// Deletes for good what's in the trash of every folder that allows
/// changes. After this those deletes can't be undone.
async fn empty_trash(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
) -> ApiResult<Json<Trash>> {
    let mut gone = Trash::default();
    for root in crate::editable::editable_roots(&state).await? {
        let trash = root.join(djinn_library::TRASH);
        if !trash.is_dir() {
            continue;
        }
        count(&trash, &mut gone);
        let removed = trash.clone();
        tokio::task::spawn_blocking(move || std::fs::remove_dir_all(removed))
            .await
            .map_err(anyhow::Error::from)?
            .map_err(anyhow::Error::from)?;
        let mut prefix = trash.to_string_lossy().into_owned();
        prefix.push(std::path::MAIN_SEPARATOR);
        state.store.trash_emptied(&prefix).await?;
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_empty_trash",
        &format!("{} files", gone.files),
    )
    .await;
    Ok(Json(gone))
}

// --- Folders left with no songs ---

fn has_audio(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true; // can't tell: leave it
    };
    entries.flatten().any(|entry| match entry.file_type() {
        Ok(t) if t.is_dir() => {
            entry.file_name() != djinn_library::TRASH && has_audio(&entry.path())
        }
        Ok(_) => entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                djinn_library::AUDIO_EXTENSIONS
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(e))
            }),
        Err(_) => true,
    })
}

/// Moves folders (and the ones above them, up to their library folder)
/// that no longer hold any songs to the trash, with what's left in them
/// (pictures, playlists, notes). Returns how many went.
pub(crate) async fn tidy(
    state: &AppState,
    user: &User,
    batch: i64,
    dirs: Vec<PathBuf>,
) -> ApiResult<usize> {
    let roots = crate::editable::editable_roots(state).await?;
    let mut seen = std::collections::HashSet::new();
    let mut gone = 0;
    for start in dirs {
        let Some(root) = roots.iter().find(|r| start.starts_with(r)) else {
            continue;
        };
        let mut dir = start;
        while dir != *root && dir.starts_with(root) && seen.insert(dir.clone()) {
            let check = dir.clone();
            let empty = tokio::task::spawn_blocking(move || check.is_dir() && !has_audio(&check))
                .await
                .map_err(anyhow::Error::from)?;
            if !empty {
                break;
            }
            let Ok(rel) = dir.strip_prefix(root) else {
                break;
            };
            let mut target = root.join(djinn_library::TRASH).join(rel);
            let mut n = 1;
            while target.exists() {
                n += 1;
                target = root
                    .join(djinn_library::TRASH)
                    .join(format!("{} ({n})", rel.to_string_lossy()));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(anyhow::Error::from)?;
            }
            std::fs::rename(&dir, &target).map_err(anyhow::Error::from)?;
            state
                .store
                .record_file_edit(&NewFileEdit {
                    batch,
                    track_id: None,
                    path: &dir.to_string_lossy(),
                    kind: "folder",
                    before: &dir.to_string_lossy(),
                    after: &target.to_string_lossy(),
                    size: 0,
                    mtime: 0,
                    made_by: Some(user.id),
                })
                .await?;
            gone += 1;
            let Some(parent) = dir.parent() else { break };
            dir = parent.to_path_buf();
        }
    }
    Ok(gone)
}

// --- folder.jpg ---

#[derive(Deserialize)]
struct FolderArt {
    /// Songs whose folders get one, from each folder's album cover.
    ids: Vec<i64>,
    /// Replace a folder.jpg already there (kept for undo).
    #[serde(default)]
    replace: bool,
}

/// Writes `folder.jpg` in each of these folders from the cover DJINN shows
/// for its songs, in one approval. Returns how many were written.
pub(crate) async fn write_folder_art(
    state: &AppState,
    user: &User,
    batch: i64,
    folders: Vec<(PathBuf, String)>,
    replace: bool,
) -> ApiResult<(usize, Vec<String>)> {
    let mut written = 0;
    let mut problems = Vec::new();
    for (dir, hash) in folders {
        let target = dir.join("folder.jpg");
        if target.exists() && !replace {
            continue;
        }
        let Some(source) = djinn_library::art::path(&state.art_dir, &hash) else {
            problems.push(format!("{}: its cover isn't in DJINN's art", dir.display()));
            continue;
        };
        let keep = undo_dir(state).join(format!("{batch}-folder-{written}.jpg"));
        let (t, k) = (target.clone(), keep.clone());
        let done = tokio::task::spawn_blocking(move || -> Result<Option<String>, String> {
            let jpeg =
                djinn_library::write::jpeg_of(&std::fs::read(&source).map_err(|e| e.to_string())?)?;
            let saved = if t.exists() {
                std::fs::create_dir_all(k.parent().unwrap_or(Path::new(".")))
                    .map_err(|e| e.to_string())?;
                std::fs::copy(&t, &k).map_err(|e| e.to_string())?;
                Some(k.to_string_lossy().into_owned())
            } else {
                None
            };
            std::fs::write(&t, jpeg).map_err(|e| e.to_string())?;
            Ok(saved)
        })
        .await
        .map_err(anyhow::Error::from)?;
        match done {
            Ok(saved) => {
                let (size, mtime) = stat(&target).unwrap_or((0, 0));
                state
                    .store
                    .record_file_edit(&NewFileEdit {
                        batch,
                        track_id: None,
                        path: &target.to_string_lossy(),
                        kind: "folder-art",
                        before: &serde_json::json!({ "saved": saved }).to_string(),
                        after: &target.to_string_lossy(),
                        size,
                        mtime,
                        made_by: Some(user.id),
                    })
                    .await?;
                written += 1;
            }
            Err(e) => problems.push(format!("{}: {e}", dir.display())),
        }
    }
    Ok((written, problems))
}

/// The folders of these songs, each with the cover DJINN shows for it.
pub(crate) async fn folders_with_covers(
    state: &AppState,
    ids: &[i64],
) -> ApiResult<Vec<(PathBuf, String)>> {
    let songs = state.store.editor_songs_by_id(ids).await?;
    let mut out: Vec<(PathBuf, String)> = Vec::new();
    for song in songs {
        let Some(dir) = Path::new(&song.path).parent().map(Path::to_path_buf) else {
            continue;
        };
        if out.iter().any(|(d, _)| *d == dir) {
            continue;
        }
        if let Some(hash) = song.art_hash {
            out.push((dir, hash));
        }
    }
    Ok(out)
}

async fn folder_art(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<FolderArt>,
) -> ApiResult<Json<Applied>> {
    if body.ids.is_empty() || body.ids.len() > 5_000 {
        return Err(ApiError::bad_request("choose 1 to 5000 songs"));
    }
    let folders = folders_with_covers(&state, &body.ids).await?;
    let targets: Vec<String> = folders
        .iter()
        .map(|(d, _)| d.join("folder.jpg").to_string_lossy().into_owned())
        .collect();
    all_editable(
        &state,
        &targets.iter().map(String::as_str).collect::<Vec<_>>(),
    )
    .await?;
    let batch = state.store.next_file_batch().await?;
    let (written, problems) = write_folder_art(&state, &user, batch, folders, body.replace).await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_folder_art",
        &format!("{written} folders, approval {batch}"),
    )
    .await;
    Ok(Json(Applied {
        songs: written,
        failed: problems
            .into_iter()
            .map(|error| Failed { id: 0, error })
            .collect(),
        batch: (written > 0).then_some(batch),
    }))
}

// --- Undo ---

/// Puts back one copy, tidied folder or folder.jpg. Returns the path to note.
pub(crate) async fn undo_one(
    state: &AppState,
    edit: &FileEdit,
) -> Result<(String, String, String), String> {
    match edit.kind.as_str() {
        "copy" => {
            let path = edit.after.clone();
            if Path::new(&path).is_file() {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            }
            let lrc = Path::new(&path).with_extension("lrc");
            let _ = std::fs::remove_file(lrc);
            if let Some(track) = edit.track_id {
                state
                    .store
                    .mark_removed(&[track])
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok((path, edit.after.clone(), String::new()))
        }
        "folder" => {
            let (back, from) = (PathBuf::from(&edit.before), PathBuf::from(&edit.after));
            if back.exists() {
                return Err(format!("{} is there again already", back.display()));
            }
            if let Some(parent) = back.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            move_across(&from, &back).map_err(|e| e.to_string())?;
            Ok((edit.before.clone(), edit.after.clone(), edit.before.clone()))
        }
        "folder-art" => {
            let saved = serde_json::from_str::<serde_json::Value>(&edit.before)
                .ok()
                .and_then(|v| v["saved"].as_str().map(str::to_owned));
            match saved {
                Some(file) => {
                    std::fs::copy(&file, &edit.after).map_err(|e| e.to_string())?;
                    let _ = std::fs::remove_file(file);
                }
                None => {
                    let _ = std::fs::remove_file(&edit.after);
                }
            }
            Ok((edit.after.clone(), "{}".into(), "{}".into()))
        }
        other => Err(format!("can't undo {other}")),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use djinn_store::{Role, TagChange};

    use crate::state::AppState;
    use crate::test_util::{get_body, session};

    async fn send(
        state: &AppState,
        method: &str,
        path: &str,
        body: &str,
        cookie: &str,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body.to_owned()))
            .unwrap();
        let res = crate::app(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn files_are_filed_by_pattern_copied_deleted_and_put_back() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let root = std::env::temp_dir().join(format!("djinn-files-{}", std::process::id()));
        let old = root.join("old");
        std::fs::create_dir_all(&old).unwrap();
        crate::folders::add_root_for_tests(&state, &root).await;
        state
            .store
            .set_setting(crate::editable::KEY, &root.to_string_lossy())
            .await
            .unwrap();
        let song = old.join("song.flac");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_a.flac"),
            &song,
        )
        .unwrap();
        std::fs::write(old.join("notes.txt"), "left behind").unwrap();
        let path = song.to_string_lossy().into_owned();
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: path.clone(),
                title: "song".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.track_id_at(&path).await.unwrap().unwrap();
        // Tags (as DJINN shows them) and a cover to file it by.
        let mut png = Vec::new();
        image::RgbImage::new(4, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let cover = djinn_library::art::store(&state.art_dir, &png).unwrap();
        state
            .store
            .edit_tags(
                &[id],
                &TagChange {
                    artist: Some(Some("Band".into())),
                    album: Some(Some("Record".into())),
                    track_no: Some(Some(3)),
                    cover: Some(Some(cover)),
                    ..TagChange::default()
                },
            )
            .await
            .unwrap();
        crate::library_api::reread(&state, std::slice::from_ref(&path))
            .await
            .unwrap();

        // Filed as Artist/Album/03 Title, with folder.jpg, the old folder tidied away.
        let rename = format!(
            r#"{{"ids":[{id}],"pattern":"{{album_artist}}/{{album}}/{{track}} {{title}}","remove_empty":true,"folder_art":true}}"#
        );
        let (status, done) = send(&state, "POST", "/api/editor/rename", &rename, &admin).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let filed = root.join("Band").join("Record").join("03 song.flac");
        assert!(filed.is_file());
        assert!(std::fs::read(root.join("Band/Record/folder.jpg"))
            .unwrap()
            .starts_with(&[0xFF, 0xD8]));
        assert!(
            !old.exists(),
            "the folder left with no songs went to the trash"
        );
        assert!(root
            .join(djinn_library::TRASH)
            .join("old/notes.txt")
            .is_file());
        let renamed = done["batch"].as_i64().unwrap();

        // Copied: a song of its own.
        let copies = root.join("copies");
        let body = format!(
            r#"{{"ids":[{id}],"folder":"{}"}}"#,
            copies.to_string_lossy()
        );
        let (status, done) = send(&state, "POST", "/api/editor/copy", &body, &admin).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let copy = copies.join("03 song.flac");
        let copy_id = state
            .store
            .track_id_at(&copy.to_string_lossy())
            .await
            .unwrap()
            .unwrap();
        assert_ne!(copy_id, id);

        // Deleted to the trash, then the trash emptied: no undo after that.
        let body = format!(r#"{{"ids":[{copy_id}],"delete":true,"remove_empty":true}}"#);
        let (status, done) = send(&state, "POST", "/api/editor/move", &body, &admin).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let deleted = done["batch"].as_i64().unwrap();
        assert!(!copy.exists() && !copies.exists());
        let (_, trash) = get_body(&state, "/api/editor/trash", &admin).await;
        assert!(trash.contains("\"files\":2"), "{trash}");
        let (status, _) = send(&state, "DELETE", "/api/editor/trash", "", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!root.join(djinn_library::TRASH).exists());
        let (status, _) = send(
            &state,
            "POST",
            &format!("/api/editor/undo/{deleted}"),
            "",
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        // The rename's tidied folder went with the trash too, so it can't come back either.
        let (status, _) = send(
            &state,
            "POST",
            &format!("/api/editor/undo/{renamed}"),
            "",
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn undo_puts_the_filing_back() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let root = std::env::temp_dir().join(format!("djinn-files-undo-{}", std::process::id()));
        let old = root.join("old");
        std::fs::create_dir_all(&old).unwrap();
        crate::folders::add_root_for_tests(&state, &root).await;
        state
            .store
            .set_setting(crate::editable::KEY, &root.to_string_lossy())
            .await
            .unwrap();
        let song = old.join("song.flac");
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_a.flac"),
            &song,
        )
        .unwrap();
        std::fs::write(old.join("notes.txt"), "left behind").unwrap();
        let path = song.to_string_lossy().into_owned();
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: path.clone(),
                title: "song".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.track_id_at(&path).await.unwrap().unwrap();
        let rename = format!(r#"{{"ids":[{id}],"pattern":"new/{{title}}","remove_empty":true}}"#);
        let (status, done) = send(&state, "POST", "/api/editor/rename", &rename, &admin).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert!(root.join("new/song.flac").is_file() && !old.exists());
        let (status, done) = send(
            &state,
            "POST",
            &format!("/api/editor/undo/{}", done["batch"]),
            "",
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert!(song.is_file() && old.join("notes.txt").is_file());
        assert_eq!(state.store.track_path(id).await.unwrap().unwrap(), path);
        std::fs::remove_dir_all(root).ok();
    }
}
