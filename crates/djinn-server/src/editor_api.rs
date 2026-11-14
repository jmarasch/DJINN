//! The library editor: its folder tree and grid (reading every library
//! folder, view only ones included), and the changes it makes, all behind
//! the protected areas' lock. Changes go to DJINN's database (tag
//! corrections, which the files never see) or, for folders the admin
//! allowed (`editable::require`), into the files themselves: every file
//! written is recorded in `file_edits` with what it said before, so each
//! approval can be undone, and is audited.

use std::path::{Path, PathBuf};

use axum::extract::{Path as UrlPath, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::compare::{self, FileTags};
use djinn_library::write::{self as writer, FieldValue};
use djinn_store::{EditorSong, FolderChild, NewFileEdit, TagChange};
use serde::{Deserialize, Serialize};

use crate::auth::Device;
use crate::editable::editable_roots;
use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

const PAGE: i64 = 200;
/// The most songs a search shows.
const FIND_LIMIT: i64 = 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/tree", get(tree))
        .route("/editor/songs", get(songs))
        .route("/editor/apply", post(apply))
        .route("/editor/history", get(history))
        .route("/editor/undo/{batch}", post(undo))
        .route(
            "/editor/cover",
            post(cover).layer(axum::extract::DefaultBodyLimit::max(COVER_MAX)),
        )
        .route("/editor/rename", post(rename))
        .route("/editor/move", post(move_songs))
        .route("/editor/mp3-length", post(mp3_length))
        .route(
            "/editor/replace/{id}",
            post(replace).layer(axum::extract::DefaultBodyLimit::max(REPLACE_MAX)),
        )
}

/// The most field changes one approval takes.
const MAX_CHANGES: usize = 20_000;
/// The largest cover picture taken.
const COVER_MAX: usize = 20 << 20;
/// The largest song file taken in place of another.
const REPLACE_MAX: usize = 1 << 30;

#[derive(Deserialize)]
struct TreeQuery {
    /// A folder; empty for the library folders themselves.
    #[serde(default)]
    path: String,
}

#[derive(Serialize)]
struct Node {
    name: String,
    path: String,
    songs: i64,
    /// Library folders only: whether changes are allowed in it.
    editable: Option<bool>,
}

#[derive(Serialize)]
struct Tree {
    folders: Vec<Node>,
    songs_here: i64,
}

fn join(parent: &str, name: &str) -> String {
    let sep = if parent.contains('\\') && !parent.contains('/') {
        '\\'
    } else {
        '/'
    };
    format!("{}{sep}{name}", parent.trim_end_matches(['/', '\\']))
}

/// The folder must be one of the library folders or inside one.
async fn inside_library(state: &AppState, folder: &str) -> ApiResult<Vec<PathBuf>> {
    let roots = crate::folders::library_roots(state).await?;
    if !roots.iter().any(|r| Path::new(folder).starts_with(r)) {
        return Err(ApiError::bad_request("that isn't inside a library folder"));
    }
    Ok(roots)
}

async fn tree(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<TreeQuery>,
) -> ApiResult<Json<Tree>> {
    if query.path.is_empty() {
        let editable = editable_roots(&state).await?;
        let mut folders = Vec::new();
        for root in crate::folders::library_roots(&state).await? {
            let path = root.to_string_lossy().into_owned();
            let listing = state.store.folder_listing(&path).await?;
            folders.push(Node {
                name: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                songs: listing.folders.iter().map(|f| f.songs).sum::<i64>() + listing.songs_here,
                editable: Some(editable.contains(&root)),
                path,
            });
        }
        return Ok(Json(Tree {
            folders,
            songs_here: 0,
        }));
    }
    inside_library(&state, &query.path).await?;
    let listing = state.store.folder_listing(&query.path).await?;
    Ok(Json(Tree {
        folders: listing
            .folders
            .into_iter()
            .map(|FolderChild { name, songs }| Node {
                path: join(&query.path, &name),
                name,
                songs,
                editable: None,
            })
            .collect(),
        songs_here: listing.songs_here,
    }))
}

#[derive(Deserialize)]
struct SongsQuery {
    /// A folder's songs, at any depth.
    #[serde(default)]
    folder: String,
    /// Or songs found by words (any of them, split by "|") in their
    /// artist, album artist, title or album, across the library.
    #[serde(default)]
    search: String,
    #[serde(default)]
    offset: i64,
    /// Also read each file's own tags (editing the library, not the database).
    #[serde(default)]
    files: bool,
}

#[derive(Serialize)]
struct Row {
    #[serde(flatten)]
    song: EditorSong,
    /// What the file says, when asked for; None if it couldn't be read.
    file: Option<FileTags>,
    /// Its folder allows changes to the file.
    editable: bool,
}

#[derive(Serialize)]
struct Songs {
    total: i64,
    /// Changes can be made here: the folder is allowed.
    editable: bool,
    songs: Vec<Row>,
}

async fn songs(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<SongsQuery>,
) -> ApiResult<Json<Songs>> {
    let roots = editable_roots(&state).await?;
    let (found, total) = if query.search.trim().is_empty() {
        inside_library(&state, &query.folder).await?;
        state
            .store
            .editor_songs(&query.folder, query.offset, PAGE)
            .await?
    } else {
        let terms: Vec<String> = query
            .search
            .split('|')
            .map(|t| t.trim().chars().take(100).collect::<String>())
            .filter(|t| !t.is_empty())
            .take(djinn_store::FIND_TERMS)
            .collect();
        let found = state.store.editor_find(&terms, FIND_LIMIT).await?;
        let total = i64::try_from(found.len()).unwrap_or(0);
        (found, total)
    };
    let editable = !query.folder.is_empty()
        && roots
            .iter()
            .any(|root| Path::new(&query.folder).starts_with(root));
    let files: Vec<Option<FileTags>> = if query.files {
        let original_years = djinn_library::original_years(&state.store).await?;
        let paths: Vec<String> = found.iter().map(|s| s.path.clone()).collect();
        tokio::task::spawn_blocking(move || {
            paths
                .iter()
                .map(|p| compare::read(Path::new(p), original_years).ok())
                .collect()
        })
        .await
        .map_err(anyhow::Error::from)?
    } else {
        vec![None; found.len()]
    };
    Ok(Json(Songs {
        total,
        editable,
        songs: found
            .into_iter()
            .zip(files)
            .map(|(song, file)| Row {
                editable: roots
                    .iter()
                    .any(|root| Path::new(&song.path).starts_with(root)),
                song,
                file,
            })
            .collect(),
    }))
}

// --- Changes ---

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Target {
    /// DJINN's own copy (tag corrections).
    Database,
    /// The files themselves.
    Library,
}

#[derive(Deserialize)]
struct Change {
    id: i64,
    field: String,
    /// The new value; null to go back to the file's (database) or remove
    /// it from the file (library).
    value: Option<String>,
}

#[derive(Deserialize)]
struct Apply {
    target: Target,
    changes: Vec<Change>,
}

#[derive(Serialize)]
struct Failed {
    id: i64,
    error: String,
}

#[derive(Default, Serialize)]
struct Applied {
    /// Songs changed.
    songs: usize,
    failed: Vec<Failed>,
    /// The approval's number in the history (files written), to undo it.
    batch: Option<i64>,
}

/// Groups the changes by song, in the order first seen, checking each.
fn by_song(changes: Vec<Change>, database: bool) -> ApiResult<Vec<(i64, Vec<FieldValue>)>> {
    if changes.is_empty() || changes.len() > MAX_CHANGES {
        return Err(ApiError::bad_request(format!(
            "send 1 to {MAX_CHANGES} changes"
        )));
    }
    let mut out: Vec<(i64, Vec<FieldValue>)> = Vec::new();
    for change in changes {
        let value = FieldValue {
            field: change.field,
            value: change
                .value
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty()),
        };
        // In the database, nothing means "back to the file's", which is
        // always allowed for a known field.
        let check = if database && value.value.is_none() {
            FieldValue {
                field: value.field.clone(),
                value: Some("1".into()),
            }
        } else {
            value.clone()
        };
        writer::check(&check).map_err(ApiError::bad_request)?;
        match out.iter_mut().find(|(id, _)| *id == change.id) {
            Some((_, fields)) => {
                fields.retain(|f| f.field != value.field);
                fields.push(value);
            }
            None => out.push((change.id, vec![value])),
        }
    }
    Ok(out)
}

/// A song's changes as a tag correction.
fn as_correction(fields: &[FieldValue]) -> TagChange {
    let mut change = TagChange::default();
    for f in fields {
        let text = || Some(f.value.clone());
        let number = || Some(f.value.as_deref().and_then(|v| v.parse::<i64>().ok()));
        match f.field.as_str() {
            "title" => change.title = text(),
            "artist" => change.artist = text(),
            "album" => change.album = text(),
            "album_artist" => change.album_artist = text(),
            "genre" => change.genre = text(),
            "year" => change.year = number(),
            "track_no" => change.track_no = number(),
            "disc_no" => change.disc_no = number(),
            _ => {}
        }
    }
    change
}

/// Clears the corrections of fields now written into the file, so the
/// file is what DJINN shows.
fn clear_corrections(fields: &[FieldValue]) -> TagChange {
    let cleared: Vec<FieldValue> = fields
        .iter()
        .map(|f| FieldValue {
            field: f.field.clone(),
            value: None,
        })
        .collect();
    as_correction(&cleared)
}

/// A file's size and change time (ms), as the scanner sees them.
fn stat(path: &Path) -> std::io::Result<(i64, i64)> {
    let meta = std::fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0);
    Ok((i64::try_from(meta.len()).unwrap_or(0), mtime))
}

/// Every file must be in a folder the admin allowed changes in.
async fn all_editable(state: &AppState, paths: &[&str]) -> ApiResult<()> {
    for path in paths {
        crate::editable::require(state, Path::new(path)).await?;
    }
    Ok(())
}

async fn apply(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Apply>,
) -> ApiResult<Json<Applied>> {
    let songs = by_song(body.changes, body.target == Target::Database)?;
    let ids: Vec<i64> = songs.iter().map(|(id, _)| *id).collect();
    let paths = state.store.track_paths(&ids).await?;
    let mut applied = Applied::default();
    match body.target {
        Target::Database => {
            let mut changed = Vec::new();
            for (id, fields) in &songs {
                changed.extend(
                    state
                        .store
                        .edit_tags(&[*id], &as_correction(fields))
                        .await?,
                );
            }
            applied.songs = changed.len();
            crate::library_api::reread(&state, &changed).await?;
            audit(
                &state,
                &user,
                device.ip.as_deref(),
                "editor_database",
                &format!("{} songs", applied.songs),
            )
            .await;
        }
        Target::Library => {
            let mut work: Vec<(i64, String, Vec<FieldValue>)> = Vec::new();
            for (id, fields) in songs {
                match paths.get(&id) {
                    Some(path) => work.push((id, path.clone(), fields)),
                    None => applied.failed.push(Failed {
                        id,
                        error: "that song isn't in the library".into(),
                    }),
                }
            }
            all_editable(
                &state,
                &work.iter().map(|w| w.1.as_str()).collect::<Vec<_>>(),
            )
            .await?;
            let original_years = djinn_library::original_years(&state.store).await?;
            let batch = state.store.next_file_batch().await?;
            let job: Vec<(i64, String, Vec<FieldValue>)> = work;
            let results = tokio::task::spawn_blocking(move || {
                job.into_iter()
                    .map(|(id, path, fields)| {
                        let file = Path::new(&path);
                        let done = compare::read(file, original_years).and_then(|before| {
                            writer::write_tags(file, &fields)?;
                            let (size, mtime) = stat(file).map_err(|e| e.to_string())?;
                            Ok((before, size, mtime))
                        });
                        (id, path, fields, done)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(anyhow::Error::from)?;
            let mut written = Vec::new();
            for (id, path, fields, done) in results {
                match done {
                    Ok((before, size, mtime)) => {
                        state
                            .store
                            .record_file_edit(&NewFileEdit {
                                batch,
                                track_id: Some(id),
                                path: &path,
                                kind: "tags",
                                before: &serde_json::to_string(&before)
                                    .map_err(anyhow::Error::from)?,
                                after: &serde_json::to_string(&fields)
                                    .map_err(anyhow::Error::from)?,
                                size,
                                mtime,
                                made_by: Some(user.id),
                            })
                            .await?;
                        state
                            .store
                            .edit_tags(&[id], &clear_corrections(&fields))
                            .await?;
                        written.push(path);
                    }
                    Err(error) => applied.failed.push(Failed { id, error }),
                }
            }
            applied.songs = written.len();
            if !written.is_empty() {
                applied.batch = Some(batch);
                crate::library_api::reread(&state, &written).await?;
            }
            audit(
                &state,
                &user,
                device.ip.as_deref(),
                "editor_files",
                &format!("{} files, approval {batch}", applied.songs),
            )
            .await;
        }
    }
    Ok(Json(applied))
}

async fn history(
    _: Elevated,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<djinn_store::FileBatch>>> {
    Ok(Json(state.store.file_batches(50).await?))
}

/// Where a file's cover is kept before the editor replaces it, for undo.
fn undo_dir(state: &AppState) -> PathBuf {
    crate::backup::data_dir(state).join("editor-undo")
}

/// Puts back what an approval changed in the files, newest first.
async fn undo(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    UrlPath(batch): UrlPath<i64>,
) -> ApiResult<Json<Applied>> {
    let edits = state.store.file_batch(batch).await?;
    if edits.is_empty() {
        return Err(ApiError::not_found());
    }
    if edits.iter().any(|e| e.undone_at.is_some()) {
        return Err(ApiError::conflict("that was undone already"));
    }
    if edits.iter().any(|e| e.kind.starts_with("undo")) {
        return Err(ApiError::bad_request(
            "an undo can't be undone; make the change again",
        ));
    }
    all_editable(
        &state,
        &edits.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
    )
    .await?;
    let original_years = djinn_library::original_years(&state.store).await?;
    let new_batch = state.store.next_file_batch().await?;
    let mut applied = Applied::default();
    let mut reread = Vec::new();
    for edit in edits {
        let id = edit.track_id.unwrap_or(0);
        let result: Result<(String, String, String), String> = match edit.kind.as_str() {
            "tags" => {
                let before: FileTags = serde_json::from_str(&edit.before)
                    .map_err(|e| e.to_string())
                    .unwrap_or_default();
                let after: Vec<FieldValue> = serde_json::from_str(&edit.after).unwrap_or_default();
                let restore: Vec<FieldValue> = after
                    .iter()
                    .map(|f| FieldValue {
                        field: f.field.clone(),
                        value: writer::value_of(&before, &f.field),
                    })
                    // A title can't be taken out; one the file never had stays.
                    .filter(|f| f.field != "title" || f.value.is_some())
                    .collect();
                let path = edit.path.clone();
                tokio::task::spawn_blocking(move || {
                    let file = Path::new(&path);
                    let now = compare::read(file, original_years)?;
                    writer::write_tags(file, &restore)?;
                    Ok((
                        path,
                        serde_json::to_string(&now).unwrap_or_default(),
                        serde_json::to_string(&restore).unwrap_or_default(),
                    ))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
            }
            "cover" => {
                let saved: Option<String> = serde_json::from_str::<serde_json::Value>(&edit.before)
                    .ok()
                    .and_then(|v| v["saved"].as_str().map(str::to_owned));
                let path = edit.path.clone();
                tokio::task::spawn_blocking(move || {
                    let image = match &saved {
                        Some(file) => Some(std::fs::read(file).map_err(|e| e.to_string())?),
                        None => None,
                    };
                    writer::write_cover(Path::new(&path), image.as_deref())?;
                    if let Some(file) = saved {
                        let _ = std::fs::remove_file(file);
                    }
                    Ok((path, "{}".to_owned(), "{}".to_owned()))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
            }
            "rename" | "set-aside" => {
                let (old, new) = (edit.before.clone(), edit.after.clone());
                if let Some(parent) = Path::new(&old).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match move_file(&new, &old) {
                    Ok(()) => {
                        let lrc = Path::new(&new).with_extension("lrc");
                        if lrc.is_file() {
                            let back = Path::new(&old).with_extension("lrc");
                            let _ = move_file(&lrc.to_string_lossy(), &back.to_string_lossy());
                        }
                        if let Some(track) = edit.track_id {
                            state.store.move_track_path(track, &old).await?;
                            if edit.kind == "set-aside" {
                                state.store.mark_restored(track).await?;
                            }
                        }
                        Ok((old, new.clone(), edit.before.clone()))
                    }
                    Err(e) => Err(e),
                }
            }
            "replace" => {
                let before: serde_json::Value =
                    serde_json::from_str(&edit.before).unwrap_or_default();
                let (old, saved) = (
                    before["path"].as_str().unwrap_or_default().to_owned(),
                    before["saved"].as_str().unwrap_or_default().to_owned(),
                );
                let new = edit.after.clone();
                let back = tokio::task::spawn_blocking({
                    let (old, new) = (old.clone(), new.clone());
                    move || -> Result<(), String> {
                        if !Path::new(&saved).is_file() {
                            return Err("the old file isn't kept any more".into());
                        }
                        std::fs::remove_file(&new).map_err(|e| e.to_string())?;
                        move_across(Path::new(&saved), Path::new(&old)).map_err(|e| e.to_string())
                    }
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
                match back {
                    Ok(()) => {
                        if let Some(track) = edit.track_id {
                            state.store.move_track_path(track, &old).await?;
                        }
                        Ok((old, new, String::new()))
                    }
                    Err(e) => Err(e),
                }
            }
            "mp3-length" => {
                let path = edit.path.clone();
                tokio::task::spawn_blocking(move || {
                    djinn_library::mpeg::remove_length_header(Path::new(&path))
                        .map(|_| (path, "{}".to_owned(), "{}".to_owned()))
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r)
            }
            other => Err(format!("can't undo {other}")),
        };
        match result {
            Ok((path, before, after)) => {
                let (size, mtime) = stat(Path::new(&path)).map_err(anyhow::Error::from)?;
                state
                    .store
                    .record_file_edit(&NewFileEdit {
                        batch: new_batch,
                        track_id: edit.track_id,
                        path: &path,
                        kind: &format!("undo-{}", edit.kind),
                        before: &before,
                        after: &after,
                        size,
                        mtime,
                        made_by: Some(user.id),
                    })
                    .await?;
                reread.push(path);
                applied.songs += 1;
            }
            Err(error) => applied.failed.push(Failed { id, error }),
        }
    }
    state.store.mark_file_batch_undone(batch).await?;
    crate::library_api::reread(&state, &reread).await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_undo",
        &format!("approval {batch}: {} files", applied.songs),
    )
    .await;
    applied.batch = Some(new_batch);
    Ok(Json(applied))
}

#[derive(Deserialize)]
struct CoverQuery {
    target: Target,
    /// The songs, by id, joined with commas.
    ids: String,
    /// Take the cover out (the body is empty).
    #[serde(default)]
    remove: bool,
    /// Database only: go back to the file's and folder's cover.
    #[serde(default)]
    reset: bool,
}

fn picture_ok(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(&[0x89, b'P', b'N', b'G'])
}

/// A new cover (JPEG or PNG in the body) for songs, or none.
async fn cover(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Query(query): Query<CoverQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Applied>> {
    let ids: Vec<i64> = query
        .ids
        .split(',')
        .filter_map(|i| i.trim().parse().ok())
        .take(5_000)
        .collect();
    if ids.is_empty() {
        return Err(ApiError::bad_request("choose the songs"));
    }
    let image = if query.remove || query.reset {
        None
    } else if picture_ok(&body) {
        Some(body.to_vec())
    } else {
        return Err(ApiError::bad_request(
            "the cover should be a JPEG or PNG picture",
        ));
    };
    let paths = state.store.track_paths(&ids).await?;
    let mut applied = Applied::default();
    match query.target {
        Target::Database => {
            let cover = match (&image, query.reset) {
                (_, true) => None,
                (None, false) => Some("none".to_owned()),
                (Some(bytes), false) => {
                    let art = state.art_dir.clone();
                    let bytes = bytes.clone();
                    Some(
                        tokio::task::spawn_blocking(move || {
                            djinn_library::art::store(&art, &bytes)
                        })
                        .await
                        .map_err(anyhow::Error::from)?
                        .map_err(anyhow::Error::from)?,
                    )
                }
            };
            let change = TagChange {
                cover: Some(cover),
                ..TagChange::default()
            };
            let changed = state.store.edit_tags(&ids, &change).await?;
            applied.songs = changed.len();
            crate::library_api::reread(&state, &changed).await?;
        }
        Target::Library => {
            if query.reset {
                return Err(ApiError::bad_request("reset is for the database"));
            }
            let work: Vec<(i64, String)> = ids
                .iter()
                .filter_map(|id| paths.get(id).map(|p| (*id, p.clone())))
                .collect();
            all_editable(
                &state,
                &work.iter().map(|w| w.1.as_str()).collect::<Vec<_>>(),
            )
            .await?;
            let batch = state.store.next_file_batch().await?;
            let keep = undo_dir(&state);
            let results = tokio::task::spawn_blocking(move || {
                let _ = std::fs::create_dir_all(&keep);
                work.into_iter()
                    .map(|(id, path)| {
                        let file = Path::new(&path);
                        let done = (|| {
                            let saved = match writer::read_cover(file)? {
                                Some((bytes, _)) => {
                                    let to = keep.join(format!("{batch}-{id}.img"));
                                    std::fs::write(&to, bytes).map_err(|e| e.to_string())?;
                                    Some(to.to_string_lossy().into_owned())
                                }
                                None => None,
                            };
                            writer::write_cover(file, image.as_deref())?;
                            let (size, mtime) = stat(file).map_err(|e| e.to_string())?;
                            Ok::<_, String>((saved, size, mtime))
                        })();
                        (id, path, done)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .map_err(anyhow::Error::from)?;
            let mut written = Vec::new();
            for (id, path, done) in results {
                match done {
                    Ok((saved, size, mtime)) => {
                        state
                            .store
                            .record_file_edit(&NewFileEdit {
                                batch,
                                track_id: Some(id),
                                path: &path,
                                kind: "cover",
                                before: &serde_json::json!({ "saved": saved }).to_string(),
                                after: &serde_json::json!({ "removed": query.remove }).to_string(),
                                size,
                                mtime,
                                made_by: Some(user.id),
                            })
                            .await?;
                        // The file's cover is what shows now.
                        state
                            .store
                            .edit_tags(
                                &[id],
                                &TagChange {
                                    cover: Some(None),
                                    ..TagChange::default()
                                },
                            )
                            .await?;
                        written.push(path);
                    }
                    Err(error) => applied.failed.push(Failed { id, error }),
                }
            }
            applied.songs = written.len();
            if !written.is_empty() {
                applied.batch = Some(batch);
                crate::library_api::reread(&state, &written).await?;
            }
        }
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_cover",
        &format!("{} songs", applied.songs),
    )
    .await;
    Ok(Json(applied))
}

// --- Renaming files ---

#[derive(Deserialize)]
struct Rename {
    ids: Vec<i64>,
    /// Like "{track} {title}": {disc}, {track}, {title}, {artist}, {album},
    /// {album_artist}, {year}; the extension is kept.
    pattern: String,
    /// Only say what the names would be.
    #[serde(default)]
    preview: bool,
}

#[derive(Serialize)]
struct Renamed {
    id: i64,
    from: String,
    to: String,
    /// Why it can't be renamed, if it can't.
    problem: Option<String>,
}

#[derive(Serialize)]
struct RenameResult {
    files: Vec<Renamed>,
    batch: Option<i64>,
}

/// Characters no file name may have, on any system DJINN runs on.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let squeezed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    squeezed
        .trim_matches(|c: char| c == '.' || c == ' ')
        .chars()
        .take(180)
        .collect()
}

/// A song's new file name from the pattern (without the extension).
fn name_from(pattern: &str, song: &EditorSong) -> String {
    let two = |n: Option<i64>| n.map(|n| format!("{n:02}")).unwrap_or_default();
    let text = |t: &Option<String>| t.clone().unwrap_or_default();
    let name = pattern
        .replace(
            "{disc}",
            &song.disc_no.map(|d| d.to_string()).unwrap_or_default(),
        )
        .replace("{track}", &two(song.track_no))
        .replace("{title}", &song.title)
        .replace("{artist}", &text(&song.artist))
        .replace("{album_artist}", &text(&song.album_artist))
        .replace("{album}", &text(&song.album))
        .replace(
            "{year}",
            &song.year.map(|y| y.to_string()).unwrap_or_default(),
        );
    safe_name(&name)
}

/// Moves a file, refusing to replace another.
fn move_file(from: &str, to: &str) -> Result<(), String> {
    if Path::new(to).exists() {
        return Err("a file with that name is there already".into());
    }
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

async fn rename(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Rename>,
) -> ApiResult<Json<RenameResult>> {
    if body.ids.is_empty() || body.ids.len() > 5_000 {
        return Err(ApiError::bad_request("choose 1 to 5000 songs"));
    }
    if !body.pattern.contains('{') {
        return Err(ApiError::bad_request(
            "the pattern needs at least one field, like {title}",
        ));
    }
    let songs = state.store.editor_songs_by_id(&body.ids).await?;
    let mut files = Vec::new();
    let mut taken = std::collections::HashSet::new();
    for song in &songs {
        let from = PathBuf::from(&song.path);
        let ext = from
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let stem = name_from(&body.pattern, song);
        let to = from.with_file_name(format!("{stem}{ext}"));
        let to_text = to.to_string_lossy().into_owned();
        let problem = if stem.is_empty() {
            Some("the name would be empty".to_owned())
        } else if to == from {
            Some("already named so".to_owned())
        } else if !taken.insert(to_text.to_lowercase()) {
            Some("another song would get the same name".to_owned())
        } else if to.exists() && !to_text.eq_ignore_ascii_case(&song.path) {
            Some("a file with that name is there already".to_owned())
        } else {
            None
        };
        files.push(Renamed {
            id: song.id,
            from: song.path.clone(),
            to: to_text,
            problem,
        });
    }
    if body.preview {
        return Ok(Json(RenameResult { files, batch: None }));
    }
    let (batch, moved) = relocate(&state, &user, &mut files, "rename").await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_rename",
        &format!("{} files, approval {batch}", moved),
    )
    .await;
    Ok(Json(RenameResult {
        files,
        batch: (moved > 0).then_some(batch),
    }))
}

/// Moves each file without a problem (with the `.lrc` beside it), points
/// its song at the new place and records it as one approval of `kind`
/// ("rename" or "set-aside", which also takes the song out of the
/// library). Returns the approval and how many moved.
async fn relocate(
    state: &AppState,
    user: &djinn_store::User,
    files: &mut [Renamed],
    kind: &str,
) -> ApiResult<(i64, usize)> {
    all_editable(
        state,
        &files
            .iter()
            .flat_map(|f| [f.from.as_str(), f.to.as_str()])
            .collect::<Vec<_>>(),
    )
    .await?;
    let batch = state.store.next_file_batch().await?;
    let mut moved = Vec::new();
    let mut aside = Vec::new();
    for file in files.iter_mut() {
        if file.problem.is_some() {
            continue;
        }
        if let Some(parent) = Path::new(&file.to).parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                file.problem = Some(e.to_string());
                continue;
            }
        }
        if let Err(e) = move_file(&file.from, &file.to) {
            file.problem = Some(e);
            continue;
        }
        // Lyrics beside the song keep its name.
        let lrc = Path::new(&file.from).with_extension("lrc");
        if lrc.is_file() {
            let _ = move_file(
                &lrc.to_string_lossy(),
                &Path::new(&file.to).with_extension("lrc").to_string_lossy(),
            );
        }
        state.store.move_track_path(file.id, &file.to).await?;
        let (size, mtime) = stat(Path::new(&file.to)).map_err(anyhow::Error::from)?;
        state
            .store
            .record_file_edit(&NewFileEdit {
                batch,
                track_id: Some(file.id),
                path: &file.to,
                kind,
                before: &file.from,
                after: &file.to,
                size,
                mtime,
                made_by: Some(user.id),
            })
            .await?;
        if kind == "set-aside" {
            aside.push(file.id);
        } else {
            moved.push(file.to.clone());
        }
    }
    if !aside.is_empty() {
        state.store.mark_removed(&aside).await?;
        state.store.refresh_artwork().await?;
        let _ = state
            .events
            .send(crate::events::ServerEvent::LibraryChanged);
    }
    if !moved.is_empty() {
        crate::library_api::reread(state, &moved).await?;
    }
    Ok((batch, moved.len() + aside.len()))
}

#[derive(Deserialize)]
struct Move {
    ids: Vec<i64>,
    /// The folder to move them to (inside a library folder), keeping their
    /// names; or none with `set_aside`.
    #[serde(default)]
    folder: Option<String>,
    /// Out of the library: into `.djinn-set-aside` in the song's own library
    /// folder, keeping its place under it. A library check skips it, and
    /// undo brings it back.
    #[serde(default)]
    set_aside: bool,
    #[serde(default)]
    preview: bool,
}

async fn move_songs(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Move>,
) -> ApiResult<Json<RenameResult>> {
    if body.ids.is_empty() || body.ids.len() > 5_000 {
        return Err(ApiError::bad_request("choose 1 to 5000 songs"));
    }
    let roots = crate::folders::library_roots(&state).await?;
    let folder = match (&body.folder, body.set_aside) {
        (_, true) => None,
        (Some(folder), false) if !folder.trim().is_empty() => {
            let folder = PathBuf::from(folder.trim());
            if !roots.iter().any(|r| folder.starts_with(r))
                || folder
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ApiError::bad_request("that isn't inside a library folder"));
            }
            Some(folder)
        }
        _ => return Err(ApiError::bad_request("choose the folder to move them to")),
    };
    let songs = state.store.editor_songs_by_id(&body.ids).await?;
    let mut files = Vec::new();
    let mut taken = std::collections::HashSet::new();
    for song in &songs {
        let from = PathBuf::from(&song.path);
        let to = match &folder {
            Some(folder) => from.file_name().map(|name| folder.join(name)),
            None => roots.iter().find(|r| from.starts_with(r)).and_then(|root| {
                from.strip_prefix(root)
                    .ok()
                    .map(|rel| root.join(djinn_library::SET_ASIDE).join(rel))
            }),
        };
        let Some(to) = to else { continue };
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
        files.push(Renamed {
            id: song.id,
            from: song.path.clone(),
            to: to_text,
            problem,
        });
    }
    if body.preview {
        return Ok(Json(RenameResult { files, batch: None }));
    }
    let kind = if body.set_aside {
        "set-aside"
    } else {
        "rename"
    };
    let (batch, moved) = relocate(&state, &user, &mut files, kind).await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        if body.set_aside {
            "editor_set_aside"
        } else {
            "editor_move"
        },
        &format!("{moved} files, approval {batch}"),
    )
    .await;
    Ok(Json(RenameResult {
        files,
        batch: (moved > 0).then_some(batch),
    }))
}

#[derive(Deserialize)]
struct Mp3Length {
    ids: Vec<i64>,
    #[serde(default)]
    preview: bool,
}

/// Writes a length header into variable bit rate MP3s that have none, so
/// every player shows their real length (DJINN counts it already).
async fn mp3_length(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Mp3Length>,
) -> ApiResult<Json<RenameResult>> {
    if body.ids.is_empty() || body.ids.len() > 5_000 {
        return Err(ApiError::bad_request("choose 1 to 5000 songs"));
    }
    let songs = state.store.editor_songs_by_id(&body.ids).await?;
    let paths: Vec<(i64, String)> = songs.iter().map(|s| (s.id, s.path.clone())).collect();
    let checked = tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|(id, path)| {
                let needs = path.to_lowercase().ends_with(".mp3")
                    && djinn_library::mpeg::needs_length_header(Path::new(&path));
                Renamed {
                    id,
                    from: path.clone(),
                    to: path,
                    problem: (!needs).then(|| "its length is right already".to_owned()),
                }
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(anyhow::Error::from)?;
    let mut files = checked;
    if body.preview {
        return Ok(Json(RenameResult { files, batch: None }));
    }
    all_editable(
        &state,
        &files
            .iter()
            .filter(|f| f.problem.is_none())
            .map(|f| f.from.as_str())
            .collect::<Vec<_>>(),
    )
    .await?;
    let batch = state.store.next_file_batch().await?;
    let mut written = Vec::new();
    for file in &mut files {
        if file.problem.is_some() {
            continue;
        }
        let path = file.from.clone();
        let done = tokio::task::spawn_blocking(move || {
            djinn_library::mpeg::add_length_header(Path::new(&path))
        })
        .await
        .map_err(anyhow::Error::from)?;
        match done {
            Ok(true) => {
                let (size, mtime) = stat(Path::new(&file.from)).map_err(anyhow::Error::from)?;
                state
                    .store
                    .record_file_edit(&NewFileEdit {
                        batch,
                        track_id: Some(file.id),
                        path: &file.from,
                        kind: "mp3-length",
                        before: "{}",
                        after: "{}",
                        size,
                        mtime,
                        made_by: Some(user.id),
                    })
                    .await?;
                written.push(file.from.clone());
            }
            Ok(false) => file.problem = Some("its length is right already".into()),
            Err(e) => file.problem = Some(e),
        }
    }
    if !written.is_empty() {
        crate::library_api::reread(&state, &written).await?;
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_mp3_length",
        &format!("{} files, approval {batch}", written.len()),
    )
    .await;
    Ok(Json(RenameResult {
        files,
        batch: (!written.is_empty()).then_some(batch),
    }))
}

#[derive(Deserialize)]
struct ReplaceQuery {
    /// The new file's name, for its type (its extension).
    name: String,
}

/// Copies a file where a rename can't go (another disk), else renames.
fn move_across(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    std::fs::remove_file(from)
}

/// Puts another file (the body) in place of a song's, keeping the song:
/// its plays, likes and playlists. The old file is kept for undo. The new
/// one must play; it is analyzed again.
async fn replace(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    Query(query): Query<ReplaceQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Applied>> {
    let old = state
        .store
        .track_path(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let ext = Path::new(&query.name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .filter(|e| djinn_library::AUDIO_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| ApiError::bad_request("that isn't a kind of song file DJINN plays"))?;
    if body.len() < 1024 {
        return Err(ApiError::bad_request("that file is empty"));
    }
    crate::editable::require(&state, Path::new(&old)).await?;
    let new_path = Path::new(&old).with_extension(&ext);
    let new_text = new_path.to_string_lossy().into_owned();
    if new_text != old && new_path.exists() {
        return Err(ApiError::conflict(
            "a file with the new name is there already",
        ));
    }
    let batch = state.store.next_file_batch().await?;
    let keep = undo_dir(&state);
    let old_ext = Path::new(&old)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let saved = keep.join(format!("{batch}-{id}.{old_ext}"));
    let (old_c, saved_c, new_c) = (PathBuf::from(&old), saved.clone(), new_path.clone());
    let done = tokio::task::spawn_blocking(move || -> Result<(), String> {
        // Written beside the old one first and checked, so nothing changes
        // if it won't play.
        let part = new_c.with_extension(format!("{ext}.djinn-part"));
        std::fs::write(&part, &body).map_err(|e| e.to_string())?;
        let checked =
            std::panic::catch_unwind(|| djinn_engine::fingerprint(&part).is_ok()).unwrap_or(false);
        if !checked {
            let _ = std::fs::remove_file(&part);
            return Err("the new file doesn't play".into());
        }
        std::fs::create_dir_all(saved_c.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        if let Err(e) = move_across(&old_c, &saved_c) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("couldn't keep the old file for undo: {e}"));
        }
        std::fs::rename(&part, &new_c).map_err(|e| {
            let _ = move_across(&saved_c, &old_c);
            e.to_string()
        })
    })
    .await
    .map_err(anyhow::Error::from)?;
    if let Err(error) = done {
        return Err(ApiError::bad_request(error));
    }
    if new_text != old {
        state.store.move_track_path(id, &new_text).await?;
    }
    let (size, mtime) = stat(&new_path).map_err(anyhow::Error::from)?;
    state
        .store
        .record_file_edit(&NewFileEdit {
            batch,
            track_id: Some(id),
            path: &new_text,
            kind: "replace",
            before: &serde_json::json!({ "path": old, "saved": saved.to_string_lossy() })
                .to_string(),
            after: &new_text,
            size,
            mtime,
            made_by: Some(user.id),
        })
        .await?;
    crate::library_api::reread(&state, std::slice::from_ref(&new_text)).await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_replace",
        &format!("song {id}, approval {batch}"),
    )
    .await;
    Ok(Json(Applied {
        songs: 1,
        failed: Vec::new(),
        batch: Some(batch),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use djinn_store::Role;

    use crate::state::AppState;
    use crate::test_util::{get_body, session};

    #[tokio::test]
    async fn the_tree_and_grid_are_locked_and_stay_inside_the_library() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let (status, _) = get_body(&state, "/api/editor/tree", &admin).await;
        assert_eq!(status, StatusCode::LOCKED);
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let (status, body) = get_body(&state, "/api/editor/tree", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"folders\""), "{body}");
        // A folder outside every library folder is refused.
        let (status, _) = get_body(&state, "/api/editor/songs?folder=/etc", &admin).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Posts JSON as the session in `cookie`; the status and body.
    async fn post_json(
        state: &AppState,
        path: &str,
        body: &str,
        cookie: &str,
    ) -> (StatusCode, String) {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
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
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn changes_go_to_the_database_or_into_allowed_files_and_can_be_undone() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let base = std::env::temp_dir().join(format!("djinn-editor-{}", std::process::id()));
        let (pool, quar) = (base.join("pool"), base.join("quar"));
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures/gapless_a.flac");
        let mut ids = Vec::new();
        for dir in [&pool, &quar] {
            std::fs::create_dir_all(dir).unwrap();
            crate::folders::add_root_for_tests(&state, dir).await;
            let song = dir.join("song.flac");
            std::fs::copy(&fixture, &song).unwrap();
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
            crate::library_api::reread(&state, std::slice::from_ref(&path))
                .await
                .unwrap();
            let id = state
                .store
                .editor_songs(&dir.to_string_lossy(), 0, 10)
                .await
                .unwrap()
                .0[0]
                .id;
            ids.push(id);
        }
        let (pool_song, quar_song) = (ids[0], ids[1]);
        let quar_file = quar.join("song.flac");
        let title_of = |id: i64| {
            let state = state.clone();
            async move { state.store.track(id).await.unwrap().unwrap().title }
        };
        // The database: a correction, the file untouched.
        let body = format!(
            r#"{{"target":"database","changes":[{{"id":{pool_song},"field":"title","value":"Fixed"}},{{"id":{pool_song},"field":"track_no","value":"4"}}]}}"#
        );
        let (status, text) = post_json(&state, "/api/editor/apply", &body, &admin).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(title_of(pool_song).await, "Fixed");
        assert_eq!(
            state.store.tag_edit(pool_song).await.unwrap().track_no,
            Some(4)
        );
        // Found by words across the library, each saying if its file may change.
        let (_, found) = get_body(
            &state,
            "/api/editor/songs?search=nothing%20here%20%7C%20FIXED",
            &admin,
        )
        .await;
        assert!(
            found.contains("\"title\":\"Fixed\"") && found.contains("\"editable\":false"),
            "{found}"
        );
        // A bad value is refused.
        let bad = format!(
            r#"{{"target":"database","changes":[{{"id":{pool_song},"field":"year","value":"85"}}]}}"#
        );
        assert_eq!(
            post_json(&state, "/api/editor/apply", &bad, &admin).await.0,
            StatusCode::BAD_REQUEST
        );
        // The files: refused until the folder is allowed.
        let write = format!(
            r#"{{"target":"library","changes":[{{"id":{quar_song},"field":"title","value":"In File"}},{{"id":{quar_song},"field":"track_no","value":"5"}}]}}"#
        );
        assert_eq!(
            post_json(&state, "/api/editor/apply", &write, &admin)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        state
            .store
            .set_setting(crate::editable::KEY, &quar.to_string_lossy())
            .await
            .unwrap();
        let (status, text) = post_json(&state, "/api/editor/apply", &write, &admin).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let file = djinn_library::write::read(&quar_file, true).unwrap();
        assert_eq!((file.title.as_str(), file.track_no), ("In File", Some(5)));
        assert_eq!(title_of(quar_song).await, "In File");
        let batch: i64 = serde_json::from_str::<serde_json::Value>(&text).unwrap()["batch"]
            .as_i64()
            .unwrap();
        // Renamed from the tags, with a preview first.
        let rename =
            format!(r#"{{"ids":[{quar_song}],"pattern":"{{track}} {{title}}","preview":true}}"#);
        let (_, text) = post_json(&state, "/api/editor/rename", &rename, &admin).await;
        assert!(text.contains("05 In File.flac"), "{text}");
        assert!(quar_file.exists(), "a preview changes nothing");
        let (status, text) = post_json(
            &state,
            "/api/editor/rename",
            &rename.replace(r#","preview":true"#, ""),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let renamed = quar.join("05 In File.flac");
        assert!(renamed.exists() && !quar_file.exists());
        assert_eq!(
            state.store.track_path(quar_song).await.unwrap().unwrap(),
            renamed.to_string_lossy()
        );
        let rename_batch: i64 = serde_json::from_str::<serde_json::Value>(&text).unwrap()["batch"]
            .as_i64()
            .unwrap();
        // A cover in the file.
        let png = [0x89u8, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0];
        let (status, _) = crate::test_util::send_bytes(
            &state,
            axum::http::Method::POST,
            &format!("/api/editor/cover?target=library&ids={quar_song}"),
            png.to_vec(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(djinn_library::write::read_cover(&renamed)
            .unwrap()
            .is_some());
        // Undo, newest first: the cover, the name, then the tags.
        let (_, history) = get_body(&state, "/api/editor/history", &admin).await;
        let cover_batch = serde_json::from_str::<serde_json::Value>(&history).unwrap()[0]["batch"]
            .as_i64()
            .unwrap();
        for undo in [cover_batch, rename_batch, batch] {
            let (status, text) =
                post_json(&state, &format!("/api/editor/undo/{undo}"), "", &admin).await;
            assert_eq!(status, StatusCode::OK, "{text}");
        }
        assert!(quar_file.exists() && !renamed.exists());
        assert!(djinn_library::write::read_cover(&quar_file)
            .unwrap()
            .is_none());
        let file = djinn_library::write::read(&quar_file, true).unwrap();
        assert_ne!(file.title, "In File");
        assert_eq!(file.track_no, None);
        assert_eq!(
            post_json(&state, &format!("/api/editor/undo/{batch}"), "", &admin)
                .await
                .0,
            StatusCode::CONFLICT
        );
        // The pool's file was never touched.
        assert_eq!(
            std::fs::read(pool.join("song.flac")).unwrap(),
            std::fs::read(&fixture).unwrap()
        );
        // Replaced by another recording, keeping the song; undo brings the old one back.
        let before = std::fs::read(&quar_file).unwrap();
        let other = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_b.flac"),
        )
        .unwrap();
        let (status, text) = crate::test_util::send_bytes(
            &state,
            axum::http::Method::POST,
            &format!("/api/editor/replace/{quar_song}?name=new.flac"),
            other.clone(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&text));
        assert_eq!(std::fs::read(&quar_file).unwrap(), other);
        let replaced: i64 = serde_json::from_slice::<serde_json::Value>(&text).unwrap()["batch"]
            .as_i64()
            .unwrap();
        let (status, _) = crate::test_util::send_bytes(
            &state,
            axum::http::Method::POST,
            &format!("/api/editor/replace/{quar_song}?name=junk.flac"),
            vec![0u8; 4096],
            &admin,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a file that won't play is refused"
        );
        assert_eq!(
            std::fs::read(&quar_file).unwrap(),
            other,
            "and nothing changed"
        );
        let (status, _) =
            post_json(&state, &format!("/api/editor/undo/{replaced}"), "", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(std::fs::read(&quar_file).unwrap(), before);
        // Set aside: out of the library, into the hidden folder; undo brings it back.
        let aside = format!(r#"{{"ids":[{quar_song}],"set_aside":true}}"#);
        let (status, text) = post_json(&state, "/api/editor/move", &aside, &admin).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let hidden = quar.join(djinn_library::SET_ASIDE).join("song.flac");
        assert!(hidden.exists() && !quar_file.exists());
        let in_quar = |state: AppState, dir: std::path::PathBuf| async move {
            state
                .store
                .editor_songs(&dir.to_string_lossy(), 0, 10)
                .await
                .unwrap()
                .1
        };
        assert_eq!(
            in_quar(state.clone(), quar.clone()).await,
            0,
            "out of the library"
        );
        let set_aside: i64 = serde_json::from_str::<serde_json::Value>(&text).unwrap()["batch"]
            .as_i64()
            .unwrap();
        let (status, _) =
            post_json(&state, &format!("/api/editor/undo/{set_aside}"), "", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(quar_file.exists() && !hidden.exists());
        assert_eq!(in_quar(state.clone(), quar.clone()).await, 1, "back in");
        // Moving into the pool (view only) is refused.
        let into_pool = format!(
            r#"{{"ids":[{quar_song}],"folder":"{}"}}"#,
            pool.to_string_lossy()
        );
        assert_eq!(
            post_json(&state, "/api/editor/move", &into_pool, &admin)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        std::fs::remove_dir_all(base).ok();
    }
}
