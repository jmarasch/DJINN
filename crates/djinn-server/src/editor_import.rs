//! Bringing a new album into the library editor to work on before it goes
//! into the library. Files are uploaded into `<db dir>/imports/<batch>/`
//! (DJINN's own folder, never a music folder), checked to play, and shown in
//! the editor's grid like a folder's songs. Changes are written straight into
//! these copies (they're DJINN's own, so there's nothing to undo), the cover
//! window and Album layout work on them too, and **Add to the library**
//! moves them into a library folder the admin allowed changes in, named from
//! their tags, where a library check takes them in. That last step is
//! recorded like the editor's copies, so undo takes the songs out again.

use std::path::{Path, PathBuf};

use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_library::compare::{self, FileTags};
use djinn_library::write::{self as writer, FieldValue};
use djinn_store::{EditorSong, ImportFile, NewFileEdit};
use serde::{Deserialize, Serialize};

use crate::auth::Device;
use crate::editor_api::{
    all_editable, by_song, move_across, name_from, safe_name, stat, Applied, Change, Failed,
    RenameResult, Renamed,
};
use crate::error::{ApiError, ApiResult};
use crate::protected::{audit, Elevated};
use crate::state::AppState;

/// The biggest file an import takes (a long lossless track fits).
const FILE_MAX: usize = 1 << 30;
/// The most files one import holds.
const MAX_FILES: usize = 500;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/imports", get(list).post(start))
        .route("/editor/imports/{batch}", delete(throw_away))
        .route(
            "/editor/imports/{batch}/files",
            post(upload).layer(DefaultBodyLimit::max(FILE_MAX)),
        )
        .route("/editor/imports/{batch}/songs", get(songs))
        .route("/editor/imports/{batch}/add", post(add_to_library))
        .route("/editor/imports/files/{id}", delete(drop_file))
        .route("/editor/imports/apply", post(apply))
        .route("/editor/imports/ai-status", post(ai_status))
        .route("/editor/imports/cover", post(cover))
}

/// Where imports wait.
fn imports_dir(state: &AppState) -> PathBuf {
    crate::backup::data_dir(state).join("imports")
}

// --- Imports ---

#[derive(Serialize)]
struct Summary {
    batch: i64,
    files: i64,
    started_at: i64,
    /// From the first file's tags, to tell imports apart.
    album: Option<String>,
    artist: Option<String>,
}

async fn list(_: Elevated, State(state): State<AppState>) -> ApiResult<Json<Vec<Summary>>> {
    let mut out = Vec::new();
    for batch in state.store.import_batches().await? {
        let first = state
            .store
            .import_files(batch.batch)
            .await?
            .into_iter()
            .next();
        let tags = match first {
            Some(file) => {
                tokio::task::spawn_blocking(move || compare::read(Path::new(&file.path), true).ok())
                    .await
                    .map_err(anyhow::Error::from)?
            }
            None => None,
        };
        out.push(Summary {
            batch: batch.batch,
            files: batch.files,
            started_at: batch.started_at,
            album: tags.as_ref().and_then(|t| t.album.clone()),
            artist: tags.and_then(|t| t.album_artist.or(t.artist)),
        });
    }
    Ok(Json(out))
}

#[derive(Serialize)]
struct Started {
    batch: i64,
}

/// A number for a new import; it exists once a file is in it.
async fn start(_: Elevated, State(state): State<AppState>) -> ApiResult<Json<Started>> {
    Ok(Json(Started {
        batch: state.store.next_import_batch().await?,
    }))
}

#[derive(Deserialize)]
struct UploadQuery {
    /// The file's name, for its extension and the name it waits under.
    name: String,
}

#[derive(Serialize)]
struct Uploaded {
    id: i64,
}

/// One file into an import: it must be a song file DJINN plays.
async fn upload(
    Elevated(user): Elevated,
    State(state): State<AppState>,
    UrlPath(batch): UrlPath<i64>,
    Query(query): Query<UploadQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Uploaded>> {
    if batch < 1 {
        return Err(ApiError::bad_request("that isn't an import"));
    }
    let name = Path::new(&query.name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = Path::new(&name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .filter(|e| djinn_library::AUDIO_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| ApiError::bad_request("that isn't a kind of song file DJINN plays"))?;
    if body.len() < 1024 {
        return Err(ApiError::bad_request("that file is empty"));
    }
    if state.store.import_files(batch).await?.len() >= MAX_FILES {
        return Err(ApiError::bad_request(format!(
            "an import holds at most {MAX_FILES} files"
        )));
    }
    let dir = imports_dir(&state).join(batch.to_string());
    let stem = safe_name(
        Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
            .as_str(),
    );
    let stem = if stem.is_empty() {
        "song".to_owned()
    } else {
        stem
    };
    let size = i64::try_from(body.len()).unwrap_or(i64::MAX);
    let place = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Two files of one name (disc folders) both keep theirs, numbered.
        let mut to = dir.join(format!("{stem}.{ext}"));
        let mut n = 2;
        while to.exists() {
            to = dir.join(format!("{stem} ({n}).{ext}"));
            n += 1;
        }
        let part = to.with_extension(format!("{ext}.djinn-part"));
        std::fs::write(&part, &body).map_err(|e| e.to_string())?;
        let plays =
            std::panic::catch_unwind(|| djinn_engine::fingerprint(&part).is_ok()).unwrap_or(false);
        if !plays {
            let _ = std::fs::remove_file(&part);
            return Err("that file doesn't play".into());
        }
        std::fs::rename(&part, &to).map_err(|e| e.to_string())?;
        Ok(to)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(ApiError::bad_request)?;
    let path = place.to_string_lossy().into_owned();
    let id = state
        .store
        .add_import_file(batch, &name, &path, size, Some(user.id))
        .await?;
    Ok(Json(Uploaded { id }))
}

/// Deletes waiting files and forgets them; empty folders go too.
async fn delete_files(state: &AppState, files: Vec<ImportFile>) -> ApiResult<usize> {
    let ids: Vec<i64> = files.iter().map(|f| f.id).collect();
    let root = imports_dir(state);
    tokio::task::spawn_blocking(move || {
        for file in &files {
            let path = Path::new(&file.path);
            // Only ever inside the imports folder.
            if path.starts_with(&root) {
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_file(path.with_extension("lrc"));
                if let Some(dir) = path.parent() {
                    let _ = std::fs::remove_dir(dir);
                }
            }
        }
    })
    .await
    .map_err(anyhow::Error::from)?;
    state.store.remove_import_files(&ids).await?;
    Ok(ids.len())
}

/// Throws an import away, files and all.
async fn throw_away(
    _: Elevated,
    State(state): State<AppState>,
    UrlPath(batch): UrlPath<i64>,
) -> ApiResult<Json<Applied>> {
    let files = state.store.import_files(batch).await?;
    let songs = delete_files(&state, files).await?;
    Ok(Json(Applied {
        songs,
        ..Applied::default()
    }))
}

/// Takes one file out of an import.
async fn drop_file(
    _: Elevated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
) -> ApiResult<Json<Applied>> {
    let files = state.store.import_files_by_id(&[id]).await?;
    if files.is_empty() {
        return Err(ApiError::not_found());
    }
    let songs = delete_files(&state, files).await?;
    Ok(Json(Applied {
        songs,
        ..Applied::default()
    }))
}

// --- The grid ---

/// A waiting file's lyrics: 0 none, 1 words, 2 timed.
fn lyrics_level(path: &Path) -> i64 {
    match djinn_library::lyrics::local(path) {
        Some(f) if f.synced.is_some() => 2,
        Some(f) if f.plain.is_some() => 1,
        _ => 0,
    }
}

/// The AI work asked for a waiting file, as the grid shows it.
fn ai_job(file: &ImportFile, working: bool) -> Option<String> {
    file.ai_want.as_ref().map(|k| {
        if working {
            format!("{k} working")
        } else {
            format!("{k} asked")
        }
    })
}

/// A waiting file as the editor's grid shows a song, from its tags.
fn as_song(
    file: &ImportFile,
    tags: &FileTags,
    art_hash: Option<String>,
    working: bool,
) -> EditorSong {
    let path = Path::new(&file.path);
    let lyrics = lyrics_level(path);
    EditorSong {
        id: file.id,
        path: file.path.clone(),
        title: tags.title.clone(),
        artist: tags.artist.clone(),
        album: tags.album.clone(),
        album_artist: tags.album_artist.clone(),
        track_no: tags.track_no,
        disc_no: tags.disc_no,
        year: tags.year,
        genre: tags.genre.clone(),
        album_id: None,
        art_hash,
        duration_ms: compare::length_ms(path),
        lyrics,
        ai_job: ai_job(file, working),
    }
}

#[derive(Serialize)]
struct Row {
    #[serde(flatten)]
    song: EditorSong,
    file: Option<FileTags>,
    editable: bool,
    /// The name it was brought in with.
    rel_path: String,
    /// Waiting in an import, not in the library yet.
    staged: bool,
}

#[derive(Serialize)]
struct Songs {
    total: i64,
    editable: bool,
    songs: Vec<Row>,
}

/// Reads waiting files: each one's tags, and its cover into the art cache
/// so the grid and the cover window show it.
async fn read_files(state: &AppState, files: Vec<ImportFile>) -> ApiResult<Vec<Row>> {
    let original_years = djinn_library::original_years(&state.store).await?;
    let art = state.art_dir.clone();
    let working: std::collections::HashSet<i64> = files
        .iter()
        .filter(|f| crate::ai_work::import_working(state, f.id))
        .map(|f| f.id)
        .collect();
    Ok(tokio::task::spawn_blocking(move || {
        files
            .iter()
            .map(|file| {
                let path = Path::new(&file.path);
                let tags = compare::read(path, original_years).ok();
                let art_hash = writer::read_cover(path)
                    .ok()
                    .flatten()
                    .and_then(|(bytes, _)| djinn_library::art::store(&art, &bytes).ok());
                let fallback = FileTags {
                    title: file.name.clone(),
                    ..FileTags::default()
                };
                Row {
                    song: as_song(
                        file,
                        tags.as_ref().unwrap_or(&fallback),
                        art_hash,
                        working.contains(&file.id),
                    ),
                    file: tags,
                    editable: true,
                    rel_path: file.name.clone(),
                    staged: true,
                }
            })
            .collect()
    })
    .await
    .map_err(anyhow::Error::from)?)
}

async fn songs(
    _: Elevated,
    State(state): State<AppState>,
    UrlPath(batch): UrlPath<i64>,
) -> ApiResult<Json<Songs>> {
    let files = state.store.import_files(batch).await?;
    let songs = read_files(&state, files).await?;
    Ok(Json(Songs {
        total: i64::try_from(songs.len()).unwrap_or(0),
        editable: true,
        songs,
    }))
}

/// Waiting files' lyrics and AI work, for the grid to follow while the AI
/// works. Reads only the lyrics (an `.lrc` next to the file, or its tags),
/// not the tags, cover and length the whole grid needs.
async fn ai_status(
    _: Elevated,
    State(state): State<AppState>,
    Json(body): Json<crate::editor_api::AiStatusQuery>,
) -> ApiResult<Json<Vec<crate::editor_api::AiStatus>>> {
    if body.ids.len() > crate::editor_api::AI_STATUS_MAX {
        return Err(ApiError::bad_request("too many songs"));
    }
    let files = state.store.import_files_by_id(&body.ids).await?;
    let working: std::collections::HashSet<i64> = files
        .iter()
        .filter(|f| crate::ai_work::import_working(&state, f.id))
        .map(|f| f.id)
        .collect();
    Ok(Json(
        tokio::task::spawn_blocking(move || {
            files
                .iter()
                .map(|file| crate::editor_api::AiStatus {
                    id: file.id,
                    lyrics: lyrics_level(Path::new(&file.path)),
                    ai_job: ai_job(file, working.contains(&file.id)),
                })
                .collect()
        })
        .await
        .map_err(anyhow::Error::from)?,
    ))
}

/// Waiting files' titles and lengths, for Album layout to match a release.
pub(crate) async fn titles_and_lengths(
    state: &AppState,
    ids: &[i64],
) -> ApiResult<Vec<(i64, String, Option<i64>)>> {
    let files = state.store.import_files_by_id(ids).await?;
    let original_years = djinn_library::original_years(&state.store).await?;
    Ok(tokio::task::spawn_blocking(move || {
        files
            .iter()
            .map(|file| {
                let path = Path::new(&file.path);
                let title = compare::read(path, original_years)
                    .map(|t| t.title)
                    .unwrap_or_else(|_| file.name.clone());
                (file.id, title, compare::length_ms(path))
            })
            .collect()
    })
    .await
    .map_err(anyhow::Error::from)?)
}

#[derive(Deserialize)]
struct Apply {
    changes: Vec<Change>,
}

/// Changes from the grid, written into the waiting files.
async fn apply(
    _: Elevated,
    State(state): State<AppState>,
    Json(body): Json<Apply>,
) -> ApiResult<Json<Applied>> {
    let songs = by_song(body.changes, false)?;
    let ids: Vec<i64> = songs.iter().map(|(id, _)| *id).collect();
    let files = state.store.import_files_by_id(&ids).await?;
    let work: Vec<(i64, String, Vec<FieldValue>)> = songs
        .into_iter()
        .filter_map(|(id, fields)| {
            let file = files.iter().find(|f| f.id == id)?;
            Some((id, file.path.clone(), fields))
        })
        .collect();
    let mut applied = Applied::default();
    let results = tokio::task::spawn_blocking(move || {
        work.into_iter()
            .map(|(id, path, fields)| (id, writer::write_tags(Path::new(&path), &fields)))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(anyhow::Error::from)?;
    for (id, done) in results {
        match done {
            Ok(()) => applied.songs += 1,
            Err(error) => applied.failed.push(Failed { id, error }),
        }
    }
    Ok(Json(applied))
}

#[derive(Deserialize)]
struct CoverQuery {
    ids: String,
    #[serde(default)]
    remove: bool,
}

/// A cover (JPEG or PNG in the body) into waiting files, or none.
async fn cover(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<CoverQuery>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Applied>> {
    let ids: Vec<i64> = query
        .ids
        .split(',')
        .filter_map(|i| i.trim().parse().ok())
        .take(MAX_FILES)
        .collect();
    let image = if query.remove {
        None
    } else if body.starts_with(&[0xFF, 0xD8]) || body.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(body.to_vec())
    } else {
        return Err(ApiError::bad_request(
            "the cover should be a JPEG or PNG picture",
        ));
    };
    let files = state.store.import_files_by_id(&ids).await?;
    let results = tokio::task::spawn_blocking(move || {
        files
            .iter()
            .map(|f| {
                (
                    f.id,
                    writer::write_cover(Path::new(&f.path), image.as_deref()),
                )
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(anyhow::Error::from)?;
    let mut applied = Applied::default();
    for (id, done) in results {
        match done {
            Ok(()) => applied.songs += 1,
            Err(error) => applied.failed.push(Failed { id, error }),
        }
    }
    Ok(Json(applied))
}

// --- Into the library ---

#[derive(Deserialize)]
struct Add {
    /// The library folder (or a folder inside one) they go under.
    folder: String,
    /// Where each goes under it, like "{album_artist}/{album}/{track} {title}"
    /// (the fields of the editor's renaming; the extension is kept).
    pattern: String,
    /// Only some of the import's files; all when empty.
    #[serde(default)]
    ids: Vec<i64>,
    /// Only say where they'd go.
    #[serde(default)]
    preview: bool,
    /// Changes not written into the files yet, so a preview names the files
    /// as they will be once they are.
    #[serde(default)]
    changes: Vec<Change>,
}

/// A song with changes still waiting laid over its tags (the fields a file
/// name can use).
fn with_changes(song: &EditorSong, changes: &[Change]) -> EditorSong {
    let mut song = song.clone();
    let number = |v: &Option<String>| v.as_deref().and_then(|v| v.trim().parse::<i64>().ok());
    for change in changes.iter().filter(|c| c.id == song.id) {
        let text = change.value.clone().filter(|v| !v.trim().is_empty());
        match change.field.as_str() {
            "title" => song.title = text.unwrap_or_default(),
            "artist" => song.artist = text,
            "album" => song.album = text,
            "album_artist" => song.album_artist = text,
            "genre" => song.genre = text,
            "track_no" => song.track_no = number(&change.value),
            "disc_no" => song.disc_no = number(&change.value),
            "year" => song.year = number(&change.value),
            _ => {}
        }
    }
    song
}

/// Moves waiting files into the library, named from their tags, and has
/// the library take them in. Each is recorded like the editor's copies, so
/// undo from the history takes it out again.
async fn add_to_library(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    UrlPath(batch): UrlPath<i64>,
    Json(body): Json<Add>,
) -> ApiResult<Json<RenameResult>> {
    if !body.pattern.contains('{') {
        return Err(ApiError::bad_request(
            "the pattern needs at least one field, like {title}",
        ));
    }
    let folder = crate::editor_files::library_folder(&state, &body.folder).await?;
    let files: Vec<ImportFile> = state
        .store
        .import_files(batch)
        .await?
        .into_iter()
        .filter(|f| body.ids.is_empty() || body.ids.contains(&f.id))
        .collect();
    if files.is_empty() {
        return Err(ApiError::bad_request("there's nothing in that import"));
    }
    let rows = read_files(&state, files.clone()).await?;
    let mut taken = std::collections::HashSet::new();
    let mut planned: Vec<Renamed> = rows
        .iter()
        .map(|row| {
            let from = PathBuf::from(&row.song.path);
            let ext = from
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy()))
                .unwrap_or_default();
            let mut parts = name_from(&body.pattern, &with_changes(&row.song, &body.changes));
            let stem = parts.pop().unwrap_or_default();
            let to = parts
                .iter()
                .fold(folder.clone(), |dir, part| dir.join(part))
                .join(format!("{stem}{ext}"));
            let to_text = to.to_string_lossy().into_owned();
            let problem = if stem.is_empty() {
                Some("the name would be empty".to_owned())
            } else if !taken.insert(to_text.to_lowercase()) {
                Some("another song would get the same name".to_owned())
            } else if to.exists() {
                Some("a file with that name is there already".to_owned())
            } else {
                None
            };
            Renamed {
                id: row.song.id,
                from: row.song.path.clone(),
                to: to_text,
                problem,
            }
        })
        .collect();
    if body.preview {
        return Ok(Json(RenameResult {
            files: planned,
            batch: None,
        }));
    }
    all_editable(
        &state,
        &planned
            .iter()
            .filter(|f| f.problem.is_none())
            .map(|f| f.to.as_str())
            .collect::<Vec<_>>(),
    )
    .await?;
    let edit_batch = state.store.next_file_batch().await?;
    let mut added = Vec::new();
    let mut done_ids = Vec::new();
    let mut ai_lyrics: std::collections::HashMap<i64, (String, i64)> =
        std::collections::HashMap::new();
    for file in &mut planned {
        if file.problem.is_some() {
            continue;
        }
        let (from, to) = (PathBuf::from(&file.from), PathBuf::from(&file.to));
        let moved =
            tokio::task::spawn_blocking(move || -> std::io::Result<Option<(String, i64)>> {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                move_across(&from, &to)?;
                // Lyrics found for it go beside it, where the library reads them;
                // AI-made/synced ones go into DJINN's lyrics table instead.
                let lrc = from.with_extension("lrc");
                let mut ai_lrc = None;
                if lrc.is_file() {
                    if let Ok(text) = std::fs::read_to_string(&lrc) {
                        if let Some(info) = djinn_library::lyrics_import::header(&text).ai_info() {
                            let _ = std::fs::remove_file(&lrc);
                            ai_lrc = Some((text, info.kind));
                        } else if !to.with_extension("lrc").exists() {
                            let _ = move_across(&lrc, &to.with_extension("lrc"));
                        }
                    } else if !to.with_extension("lrc").exists() {
                        let _ = move_across(&lrc, &to.with_extension("lrc"));
                    }
                }
                Ok(ai_lrc)
            })
            .await
            .map_err(anyhow::Error::from)?;
        match moved {
            Ok(ai_lrc) => {
                added.push(file.to.clone());
                done_ids.push(file.id);
                if let Some(ai_lrc) = ai_lrc {
                    ai_lyrics.insert(file.id, ai_lrc);
                }
            }
            Err(e) => file.problem = Some(e.to_string()),
        }
    }
    state.store.remove_import_files(&done_ids).await?;
    // The import's folder goes once it's empty.
    let _ = std::fs::remove_dir(imports_dir(&state).join(batch.to_string()));
    if !added.is_empty() {
        crate::library_api::reread(&state, &added).await?;
    }
    let mut want: Vec<(i64, String)> = Vec::new();
    for file in planned.iter().filter(|f| f.problem.is_none()) {
        let (size, mtime) = stat(Path::new(&file.to)).unwrap_or((0, 0));
        let track = state.store.track_id_at(&file.to).await?;
        if let Some(track) = track {
            // Its lyrics now, not when the background reading gets to it.
            let to = PathBuf::from(&file.to);
            let lyrics = tokio::task::spawn_blocking(move || djinn_library::lyrics::local(&to))
                .await
                .map_err(anyhow::Error::from)?;
            let new = lyrics
                .as_ref()
                .map(|f| djinn_store::NewLyrics {
                    synced: f.synced.as_deref(),
                    plain: f.plain.as_deref(),
                    instrumental: f.instrumental,
                    source: f.source.as_str(),
                })
                .unwrap_or(djinn_store::NewLyrics::NONE);
            state.store.save_read_lyrics(track, mtime, &new).await?;
            if let Some((ai_text, kind)) = ai_lyrics.get(&file.id) {
                let _ = crate::lyrics_import::store_from_worker(
                    &state,
                    track,
                    ai_text,
                    *kind,
                    crate::ai_listener::LOCAL_KEY_NAME,
                )
                .await;
            }
            if let Some(kind) = files
                .iter()
                .find(|f| f.id == file.id)
                .and_then(|f| f.ai_want.clone())
            {
                crate::ai_work::note_import_moved(&state, file.id, track);
                want.push((track, kind));
            }
        }
        let name = files
            .iter()
            .find(|f| f.id == file.id)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        state
            .store
            .record_file_edit(&NewFileEdit {
                batch: edit_batch,
                track_id: track,
                path: &file.to,
                kind: "copy",
                before: &format!("imported: {name}"),
                after: &file.to,
                size,
                mtime,
                made_by: Some(user.id),
            })
            .await?;
    }
    // AI work asked for while it waited.
    for kind in ["listen", "sync"] {
        let ids: Vec<i64> = want.iter().filter(|w| w.1 == kind).map(|w| w.0).collect();
        if !ids.is_empty() {
            state.store.ai_ask(&ids, kind).await?;
        }
    }
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "editor_import",
        &format!(
            "{} files into {}, approval {edit_batch}",
            added.len(),
            folder.display()
        ),
    )
    .await;
    Ok(Json(RenameResult {
        files: planned,
        batch: (!added.is_empty()).then_some(edit_batch),
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn waiting_changes_name_the_file() {
        use super::{name_from, with_changes, Change, EditorSong};
        let song = EditorSong {
            id: 7,
            path: "/imports/1/01.flac".into(),
            title: "01".into(),
            artist: None,
            album: None,
            album_artist: None,
            track_no: None,
            disc_no: None,
            year: None,
            genre: None,
            album_id: None,
            art_hash: None,
            duration_ms: None,
            lyrics: 0,
            ai_job: None,
        };
        let change = |id, field: &str, value: &str| Change {
            id,
            field: field.into(),
            value: Some(value.into()),
        };
        let changes = vec![
            change(7, "title", "Kitchen Song"),
            change(7, "artist", "The Band"),
            change(7, "album", "First"),
            change(7, "track_no", "3"),
            change(8, "title", "Someone else's"),
        ];
        let named = with_changes(&song, &changes);
        assert_eq!(
            name_from("{album_artist}/{album}/{track} {title}", &named),
            vec!["The Band", "First", "03 Kitchen Song"]
        );
        // Changes for other files leave it alone.
        assert_eq!(with_changes(&song, &changes[4..]), song);
    }

    use axum::http::StatusCode;
    use djinn_store::Role;

    use crate::state::AppState;
    use crate::test_util::session;

    async fn send(
        state: &AppState,
        method: &str,
        path: &str,
        body: Vec<u8>,
        cookie: &str,
    ) -> (StatusCode, serde_json::Value) {
        use tower::ServiceExt;
        let req = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("cookie", cookie)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(body))
            .unwrap();
        let res = crate::app(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or_default())
    }

    #[tokio::test]
    async fn an_album_is_brought_in_worked_on_and_added_to_the_library() {
        let dir = std::env::temp_dir().join(format!("djinn-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("art")).unwrap();
        let state = AppState::for_tests_in(&dir).await;
        let admin = session(&state, "admin", Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        let music = dir.join("music");
        std::fs::create_dir_all(&music).unwrap();
        crate::folders::add_root_for_tests(&state, &music).await;
        state
            .store
            .set_setting(crate::editable::KEY, &music.to_string_lossy())
            .await
            .unwrap();
        let song = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_a.flac"),
        )
        .unwrap();

        let (status, started) = send(&state, "POST", "/api/editor/imports", vec![], &admin).await;
        assert_eq!(status, StatusCode::OK, "{started}");
        let batch = started["batch"].as_i64().unwrap();
        let upload = |name: &str| format!("/api/editor/imports/{batch}/files?name={name}");
        let (status, one) = send(
            &state,
            "POST",
            &upload("01%20One.flac"),
            song.clone(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{one}");
        let one = one["id"].as_i64().unwrap();
        // Something that isn't a song, or doesn't play, is refused.
        let (status, _) = send(&state, "POST", &upload("notes.txt"), song.clone(), &admin).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = send(&state, "POST", &upload("bad.flac"), vec![7; 4096], &admin).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Waiting, not in the library.
        let waiting = &state.store.import_files(batch).await.unwrap()[0].path;
        assert!(state.store.track_id_at(waiting).await.unwrap().is_none());
        let (status, list) = send(
            &state,
            "GET",
            &format!("/api/editor/imports/{batch}/songs"),
            vec![],
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(list["songs"][0]["staged"], true);
        assert_eq!(list["songs"][0]["rel_path"], "01 One.flac");

        // Tags written into the waiting copy.
        let changes = format!(
            r#"{{"changes":[{{"id":{one},"field":"title","value":"First"}},{{"id":{one},"field":"album_artist","value":"Band"}},{{"id":{one},"field":"album","value":"Record"}},{{"id":{one},"field":"track_no","value":"1"}},{{"id":{one},"field":"x:Composer","value":"Someone"}}]}}"#
        );
        let (status, done) = send(
            &state,
            "POST",
            "/api/editor/imports/apply",
            changes.into_bytes(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert_eq!(done["songs"], 1, "{done}");

        // Lyrics beside the waiting file (as Find lyrics leaves them), and
        // AI work asked for, which waits until it's in the library.
        let waiting = state.store.import_files(batch).await.unwrap()[0]
            .path
            .clone();
        std::fs::write(
            std::path::Path::new(&waiting).with_extension("lrc"),
            "[00:01.00]la la\n",
        )
        .unwrap();
        let (status, asked) = send(
            &state,
            "POST",
            "/api/editor/lyrics/generate",
            format!(r#"{{"ids":[{one}],"staged":true}}"#).into_bytes(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{asked}");
        assert_eq!(
            asked["songs"][0]["result"], "had",
            "it has lyrics now: {asked}"
        );
        state
            .store
            .set_import_ai_want(&[one], "sync")
            .await
            .unwrap();

        // Previewed, then added: filed by its tags and taken in by the library.
        let add = format!(
            r#"{{"folder":"{}","pattern":"{{album_artist}}/{{album}}/{{track}} {{title}}","preview":true}}"#,
            music.to_string_lossy()
        );
        let path = format!("/api/editor/imports/{batch}/add");
        let (status, plan) = send(&state, "POST", &path, add.clone().into_bytes(), &admin).await;
        assert_eq!(status, StatusCode::OK, "{plan}");
        let filed = music.join("Band").join("Record").join("01 First.flac");
        assert_eq!(plan["files"][0]["to"], filed.to_string_lossy().as_ref());
        assert!(!filed.exists(), "a preview moves nothing");
        let add = add.replace(r#","preview":true"#, "");
        let (status, done) = send(&state, "POST", &path, add.into_bytes(), &admin).await;
        assert_eq!(status, StatusCode::OK, "{done}");
        assert!(filed.is_file());
        let id = state
            .store
            .track_id_at(&filed.to_string_lossy())
            .await
            .unwrap()
            .expect("the library took it in");
        assert_eq!(state.store.track(id).await.unwrap().unwrap().title, "First");
        // Its lyrics came along and were read at once; the AI ask is queued.
        assert!(filed.with_extension("lrc").is_file());
        let songs = state.store.editor_songs_by_id(&[id]).await.unwrap();
        assert_eq!(songs[0].lyrics, 2);
        assert_eq!(songs[0].ai_job.as_deref(), Some("sync asked"));
        assert!(state.store.import_batches().await.unwrap().is_empty());

        // Undo from the history takes it out again.
        let batch = done["batch"].as_i64().unwrap();
        let (status, _) = send(
            &state,
            "POST",
            &format!("/api/editor/undo/{batch}"),
            vec![],
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!filed.exists());

        // Throwing an import away deletes what waits in it.
        let (_, started) = send(&state, "POST", "/api/editor/imports", vec![], &admin).await;
        let batch = started["batch"].as_i64().unwrap();
        let (status, _) = send(
            &state,
            "POST",
            &format!("/api/editor/imports/{batch}/files?name=a.flac"),
            song.clone(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let waiting = state.store.import_files(batch).await.unwrap();
        assert!(std::path::Path::new(&waiting[0].path).is_file());
        let (status, _) = send(
            &state,
            "DELETE",
            &format!("/api/editor/imports/{batch}"),
            vec![],
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!std::path::Path::new(&waiting[0].path).exists());
        assert!(state.store.import_batches().await.unwrap().is_empty());

        // AI-generated lyrics made for a staged import move into the DB as AI lyrics,
        // without placing an AI .lrc file into the library folder.
        let (_, started) = send(&state, "POST", "/api/editor/imports", vec![], &admin).await;
        let batch = started["batch"].as_i64().unwrap();
        let (status, up) = send(
            &state,
            "POST",
            &format!("/api/editor/imports/{batch}/files?name=02%20AI.flac"),
            song,
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ai_file_id = up["id"].as_i64().unwrap();
        let changes = format!(
            r#"{{"changes":[{{"id":{ai_file_id},"field":"title","value":"AITrack"}},{{"id":{ai_file_id},"field":"album_artist","value":"Band"}},{{"id":{ai_file_id},"field":"album","value":"Record"}},{{"id":{ai_file_id},"field":"track_no","value":"2"}}]}}"#
        );
        let (status, _) = send(
            &state,
            "POST",
            "/api/editor/imports/apply",
            changes.into_bytes(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let waiting = state.store.import_files(batch).await.unwrap();
        std::fs::write(
            std::path::Path::new(&waiting[0].path).with_extension("lrc"),
            "[ar:Band]\n[ti:AITrack]\n[by:lyrics-listener (AI-generated; model=small)]\n\n[00:00.00](AI-generated lyrics)\n[00:01.00]generated words\n",
        )
        .unwrap();
        let add_body = format!(
            r#"{{"folder":"{}","pattern":"{{album_artist}}/{{album}}/{{track}} {{title}}"}}"#,
            music.to_string_lossy()
        );
        let (status, done) = send(
            &state,
            "POST",
            &format!("/api/editor/imports/{batch}/add"),
            add_body.into_bytes(),
            &admin,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{done}");
        let ai_dest = music.join("Band").join("Record").join("02 AITrack.flac");
        assert!(ai_dest.is_file());
        assert!(!ai_dest.with_extension("lrc").exists());
        let ai_track_id = state
            .store
            .track_id_at(&ai_dest.to_string_lossy())
            .await
            .unwrap()
            .unwrap();
        let stored = state.store.lyrics(ai_track_id).await.unwrap().unwrap();
        assert_eq!(stored.ai, 1);
        assert_eq!(stored.source, "bulk");
        assert_eq!(stored.plain.as_deref(), Some("generated words"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
