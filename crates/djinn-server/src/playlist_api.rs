//! Playlists (admin): create and edit them, and pick the one that plays
//! when nobody has requested anything.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use djinn_store::{AddEntryError, EntryKind, PlaylistDetail, PlaylistEntry, PlaylistSummary};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const MAX_NAME: usize = 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(list).post(create))
        .route("/playlists/stop", post(stop))
        .route("/playlists/wild/play", post(play_wild))
        .route("/playlists/{id}", get(detail).patch(update).delete(remove))
        .route("/playlists/{id}/play", post(play))
        .route("/playlists/{id}/entries", post(add_entry))
        .route(
            "/playlists/{id}/entries/{entry}",
            patch(move_entry).delete(remove_entry),
        )
}

#[derive(Serialize)]
struct ListItem {
    #[serde(flatten)]
    playlist: PlaylistSummary,
    /// Plays when the request queue is empty.
    active: bool,
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<ListItem>>> {
    let active = state.jukebox.lock().await.active_playlist();
    let playlists = state.store.playlists().await?;
    Ok(Json(
        playlists
            .into_iter()
            .map(|playlist| ListItem {
                active: Some(playlist.id) == active,
                playlist,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct NewPlaylist {
    name: String,
}

async fn create(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewPlaylist>,
) -> ApiResult<(StatusCode, Json<PlaylistSummary>)> {
    let name = validate_name(&body.name)?;
    let playlist = state.store.create_playlist(admin.id, &name).await?;
    Ok((StatusCode::CREATED, Json(playlist)))
}

async fn detail(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlaylistDetail>> {
    state
        .store
        .playlist(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[derive(Deserialize)]
struct PlaylistUpdate {
    name: Option<String>,
    shuffle: Option<bool>,
}

async fn update(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PlaylistUpdate>,
) -> ApiResult<StatusCode> {
    let name = body.name.as_deref().map(validate_name).transpose()?;
    if !state
        .store
        .update_playlist(id, name.as_deref(), body.shuffle)
        .await?
    {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_playlist(id).await? {
        return Err(ApiError::not_found());
    }
    let mut jukebox = state.jukebox.lock().await;
    if jukebox.active_playlist() == Some(id) {
        jukebox.stop_auto_mix().await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Play {
    /// Start from this song instead of the playlist's next one.
    from_track_id: Option<i64>,
}

/// Makes the playlist the one that plays and starts it now.
async fn play(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Play>,
) -> ApiResult<StatusCode> {
    if state.store.playlist(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    state
        .jukebox
        .lock()
        .await
        .play_playlist(id, body.from_track_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Wild play: random songs from the whole library when the requests run
/// out.
async fn play_wild(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state
        .jukebox
        .lock()
        .await
        .play_playlist(crate::jukebox::WILD_PLAY, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// No playlist plays when the requests run out.
async fn stop(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.jukebox.lock().await.stop_auto_mix().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewEntry {
    kind: EntryKind,
    ref_id: i64,
}

async fn add_entry(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<NewEntry>,
) -> ApiResult<(StatusCode, Json<PlaylistEntry>)> {
    let entry = match state
        .store
        .add_playlist_entry(id, body.kind, body.ref_id)
        .await
    {
        Ok(entry) => entry,
        Err(AddEntryError::NotFound) => return Err(ApiError::not_found()),
        Err(e @ AddEntryError::AlreadyIn) => return Err(ApiError::conflict(e.to_string())),
        Err(AddEntryError::Store(e)) => return Err(e.into()),
    };
    refresh_if_active(&state, id).await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

#[derive(Deserialize)]
struct MoveEntry {
    position: usize,
}

async fn move_entry(
    _: AdminUser,
    State(state): State<AppState>,
    Path((id, entry)): Path<(i64, i64)>,
    Json(body): Json<MoveEntry>,
) -> ApiResult<StatusCode> {
    if !state
        .store
        .move_playlist_entry(id, entry, body.position)
        .await?
    {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_entry(
    _: AdminUser,
    State(state): State<AppState>,
    Path((id, entry)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    if !state.store.remove_playlist_entry(id, entry).await? {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The jukebox lines up its next song from the active playlist, so it
/// needs to hear about changes to it.
async fn refresh_if_active(state: &AppState, id: i64) -> ApiResult<()> {
    let mut jukebox = state.jukebox.lock().await;
    if jukebox.active_playlist() == Some(id) {
        jukebox.auto_mix_changed().await?;
    }
    Ok(())
}

fn validate_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "playlist names are 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}
