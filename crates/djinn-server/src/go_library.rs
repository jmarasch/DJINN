//! DJINN GO: your own library. Likes, history, where you left off, your
//! own playlists, personal radio, and sending a song to the party. Only on
//! the GO port; the guard there has already checked the role.

use std::collections::HashSet;

use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::{
    GoPlaylist, GoPlaylistDetail, GoPrefs, LikeKind, Liked, LikedIds, Played, RequestOrigin,
    Resume, Track, User,
};
use serde::{Deserialize, Serialize};

use crate::auth::{CurrentSession, Device};
use crate::error::{ApiError, ApiResult};
use crate::go::GoRequest;
use crate::state::AppState;

const MAX_PLAYLIST_NAME: usize = 100;
const MAX_PLAYLIST_SONGS: usize = 5000;
const RADIO_SONGS: usize = 30;
/// Personal radio avoids songs heard this recently.
const RADIO_FRESH_MS: i64 = 3 * 60 * 60 * 1000;
/// ...and leans away from songs skipped this recently.
const RADIO_SKIPS_MS: i64 = 30 * 24 * 60 * 60 * 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/go/home", get(home))
        .route("/go/likes", get(liked))
        .route("/go/likes/ids", get(liked_ids))
        .route("/go/likes/{kind}/{id}", put(like).delete(unlike))
        .route("/go/plays", post(played))
        .route("/go/history", get(history))
        .route("/go/resume", get(resume).put(save_resume))
        .route("/go/prefs", get(prefs).put(set_prefs))
        .route("/go/playlists", get(playlists).post(create_playlist))
        .route(
            "/go/playlists/{id}",
            get(playlist).patch(update_playlist).delete(delete_playlist),
        )
        .route("/go/playlists/{id}/songs", post(add_songs).put(set_songs))
        .route("/go/radio/{id}", get(radio))
        .route("/go/party/{id}", post(send_to_party))
}

/// A signed-in user, on the GO port only (404 elsewhere).
pub struct GoUser {
    pub user: User,
}

impl FromRequestParts<AppState> for GoUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        if parts.extensions.get::<GoRequest>().is_none() {
            return Err(ApiError::not_found());
        }
        let session = CurrentSession::from_request_parts(parts, state).await?;
        Ok(Self { user: session.user })
    }
}

#[derive(Serialize)]
struct Home {
    resume: Option<ResumeWithSongs>,
    recent: Vec<Track>,
    playlists: Vec<GoPlaylist>,
    newest: Vec<Track>,
}

#[derive(Serialize)]
struct ResumeWithSongs {
    #[serde(flatten)]
    resume: Resume,
    songs: Vec<Track>,
}

async fn resume_with_songs(state: &AppState, user_id: i64) -> ApiResult<Option<ResumeWithSongs>> {
    let Some(resume) = state.store.resume(user_id).await? else {
        return Ok(None);
    };
    let songs = state.store.tracks_by_ids(&resume.queue).await?;
    if songs.is_empty() {
        return Ok(None);
    }
    Ok(Some(ResumeWithSongs { resume, songs }))
}

/// The start page: where you left off, lately played, your playlists, and
/// what's new in the library.
async fn home(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Home>> {
    let newest = state
        .store
        .tracks_page(djinn_store::TrackSort::Added, 0, 20)
        .await?
        .items;
    Ok(Json(Home {
        resume: resume_with_songs(&state, me.user.id).await?,
        recent: state.store.recently_played(me.user.id, 20).await?,
        playlists: state.store.go_playlists(me.user.id).await?,
        newest,
    }))
}

async fn liked(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Liked>> {
    Ok(Json(state.store.liked(me.user.id).await?))
}

async fn liked_ids(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<LikedIds>> {
    Ok(Json(state.store.liked_ids(me.user.id).await?))
}

async fn like(
    me: GoUser,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.user.id, kind, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlike(
    me: GoUser,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.user.id, kind, id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PlayReport {
    track_id: i64,
    /// How much was heard; under 30 seconds counts as a skip.
    ms_played: i64,
}

/// The player reports each song when it ends or is left.
async fn played(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<PlayReport>,
) -> ApiResult<StatusCode> {
    state
        .store
        .record_go_play(me.user.id, body.track_id, body.ms_played)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    offset: i64,
}

async fn history(
    me: GoUser,
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> ApiResult<Json<Vec<Played>>> {
    Ok(Json(
        state
            .store
            .go_history(me.user.id, query.offset.max(0), 100)
            .await?,
    ))
}

async fn resume(
    me: GoUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Option<ResumeWithSongs>>> {
    Ok(Json(resume_with_songs(&state, me.user.id).await?))
}

/// The player saves where it is every so often, and on pause.
async fn save_resume(
    me: GoUser,
    State(state): State<AppState>,
    Json(mut body): Json<Resume>,
) -> ApiResult<StatusCode> {
    body.queue.truncate(MAX_PLAYLIST_SONGS);
    state.store.save_resume(me.user.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn prefs(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<GoPrefs>> {
    Ok(Json(state.store.go_prefs(me.user.id).await?))
}

async fn set_prefs(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<GoPrefs>,
) -> ApiResult<StatusCode> {
    state.store.set_go_prefs(me.user.id, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn playlists(me: GoUser, State(state): State<AppState>) -> ApiResult<Json<Vec<GoPlaylist>>> {
    Ok(Json(state.store.go_playlists(me.user.id).await?))
}

#[derive(Deserialize)]
struct NewPlaylist {
    name: String,
    #[serde(default)]
    songs: Vec<i64>,
}

fn playlist_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_PLAYLIST_NAME {
        return Err(ApiError::bad_request(format!(
            "pick a name of 1 to {MAX_PLAYLIST_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

async fn create_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Json(body): Json<NewPlaylist>,
) -> ApiResult<(StatusCode, Json<GoPlaylistDetail>)> {
    let name = playlist_name(&body.name)?;
    let id = state.store.create_go_playlist(me.user.id, &name).await?;
    if !body.songs.is_empty() {
        let songs: Vec<i64> = body.songs.into_iter().take(MAX_PLAYLIST_SONGS).collect();
        state
            .store
            .set_go_playlist_tracks(me.user.id, id, &songs)
            .await?;
    }
    let detail = state
        .store
        .go_playlist(me.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(detail)))
}

async fn playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<GoPlaylistDetail>> {
    let detail = state
        .store
        .go_playlist(me.user.id, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(detail))
}

#[derive(Deserialize)]
struct PlaylistChange {
    name: Option<String>,
    shared: Option<bool>,
}

async fn update_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PlaylistChange>,
) -> ApiResult<StatusCode> {
    let name = body.name.as_deref().map(playlist_name).transpose()?;
    if body.shared == Some(true) && !state.store.permissions(&me.user).await?.share_playlists {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "your account can't share playlists",
        ));
    }
    if !state
        .store
        .update_go_playlist(me.user.id, id, name.as_deref(), body.shared)
        .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_playlist(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_go_playlist(me.user.id, id).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Songs {
    songs: Vec<i64>,
}

async fn add_songs(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    if body.songs.len() > MAX_PLAYLIST_SONGS
        || !state
            .store
            .add_to_go_playlist(me.user.id, id, &body.songs)
            .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Replaces a playlist's songs: for removing and reordering.
async fn set_songs(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    if body.songs.len() > MAX_PLAYLIST_SONGS
        || !state
            .store
            .set_go_playlist_tracks(me.user.id, id, &body.songs)
            .await?
    {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Personal radio from a song: songs that sound like it, skipping ones
/// you heard in the last few hours and putting ones you skipped lately
/// last. Without sound analysis, the same artist and genre.
async fn radio(
    me: GoUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Track>>> {
    let seed = state
        .store
        .track(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let now = djinn_store::now_ms();
    let (fresh_played, fresh_skipped) = state
        .store
        .go_listening_since(me.user.id, now - RADIO_FRESH_MS)
        .await?;
    let (_, skipped) = state
        .store
        .go_listening_since(me.user.id, now - RADIO_SKIPS_MS)
        .await?;
    let heard: HashSet<i64> = fresh_played
        .into_iter()
        .chain(fresh_skipped)
        .chain([id])
        .collect();
    let skipped: HashSet<i64> = skipped.into_iter().collect();
    let mut ids = state
        .sound
        .read()
        .map(|index| index.similar_tracks(id, RADIO_SONGS * 3))
        .unwrap_or_default();
    if ids.is_empty() {
        ids = fallback_radio(&state, &seed).await?;
    }
    ids.retain(|t| !heard.contains(t));
    // Stable: keeps sound order within "not skipped" and "skipped".
    ids.sort_by_key(|t| skipped.contains(t));
    ids.truncate(RADIO_SONGS);
    let mut songs = vec![seed];
    songs.extend(state.store.tracks_by_ids(&ids).await?);
    Ok(Json(songs))
}

/// Same artist, then same genre, shuffled: radio without sound analysis.
async fn fallback_radio(state: &AppState, seed: &Track) -> ApiResult<Vec<i64>> {
    let mut ids = Vec::new();
    if let Some(artist) = seed.artist_id {
        ids.extend(
            state
                .store
                .tracks_for(djinn_store::EntryKind::Artist, artist)
                .await?,
        );
    }
    if let Some(genre) = seed.genre.as_deref() {
        let first = genre.split([',', ';', '/']).next().unwrap_or("").trim();
        if !first.is_empty() {
            let found = state.store.search(first, 100).await?;
            ids.extend(found.tracks.iter().map(|t| t.id));
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    shuffle(&mut ids);
    Ok(ids)
}

fn shuffle(ids: &mut [i64]) {
    use rand::seq::SliceRandom;
    ids.shuffle(&mut rand::rng());
}

#[derive(Serialize)]
struct SentToParty {
    /// "added", or "backed" when someone had already asked for it.
    outcome: djinn_store::RequestOutcome,
}

/// Asks the party for a song, like requesting it from the party app. Only
/// from home: the party plays there.
async fn send_to_party(
    me: GoUser,
    State(state): State<AppState>,
    device: Device,
    Path(id): Path<i64>,
) -> ApiResult<Json<SentToParty>> {
    if !device.home {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "you can send songs to the party from home",
        ));
    }
    let origin = RequestOrigin {
        name: me.user.name().to_owned(),
        mac: me.user.mac.clone(),
        ip: device.ip.clone(),
    };
    let (_, outcome) = state
        .jukebox
        .lock()
        .await
        .add_request(&me.user, id, &origin)
        .await
        .map_err(crate::player_api::queue_error)?;
    Ok(Json(SentToParty { outcome }))
}
