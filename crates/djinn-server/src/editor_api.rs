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
}

/// The most field changes one approval takes.
const MAX_CHANGES: usize = 20_000;
/// The largest cover picture taken.
const COVER_MAX: usize = 20 << 20;

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
    folder: String,
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
    inside_library(&state, &query.folder).await?;
    let editable = editable_roots(&state)
        .await?
        .iter()
        .any(|root| Path::new(&query.folder).starts_with(root));
    let (found, total) = state
        .store
        .editor_songs(&query.folder, query.offset, PAGE)
        .await?;
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
            .map(|(song, file)| Row { song, file })
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
            "rename" => {
                let (old, new) = (edit.before.clone(), edit.after.clone());
                match move_file(&new, &old) {
                    Ok(()) => {
                        let lrc = Path::new(&new).with_extension("lrc");
                        if lrc.is_file() {
                            let back = Path::new(&old).with_extension("lrc");
                            let _ = move_file(&lrc.to_string_lossy(), &back.to_string_lossy());
                        }
                        if let Some(track) = edit.track_id {
                            state.store.move_track_path(track, &old).await?;
                        }
                        Ok((old, new.clone(), edit.before.clone()))
                    }
                    Err(e) => Err(e),
                }
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
    all_editable(
        &state,
        &files.iter().map(|f| f.from.as_str()).collect::<Vec<_>>(),
    )
    .await?;
    let batch = state.store.next_file_batch().await?;
    let mut moved = Vec::new();
    for file in &mut files {
        if file.problem.is_some() {
            continue;
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
                kind: "rename",
                before: &file.from,
                after: &file.to,
                size,
                mtime,
                made_by: Some(user.id),
            })
            .await?;
        moved.push(file.to.clone());
    }
    if !moved.is_empty() {
        crate::library_api::reread(&state, &moved).await?;
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_rename",
        &format!("{} files, approval {batch}", moved.len()),
    )
    .await;
    Ok(Json(RenameResult {
        files,
        batch: (!moved.is_empty()).then_some(batch),
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
        std::fs::remove_dir_all(base).ok();
    }
}
