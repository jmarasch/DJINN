//! The request queue, now playing, player controls, and
//! runtime settings.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use djinn_store::{
    AddRequestError, EntryKind, QueueItem, RequestLogEntry, RequestOrigin, RequestOutcome,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::client::{self, ClientIp};
use crate::error::{ApiError, ApiResult};
use crate::events::{NowPlaying, OutputStatus, PlayerState};
use crate::jukebox::{self, QueueError};
use crate::settings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queue", get(queue).post(add_request))
        .route("/queue/{id}", delete(remove_request).patch(move_request))
        .route("/queue/all", post(add_all))
        .route("/now-playing", get(now_playing))
        .route("/player/play", post(play))
        .route("/player/pause", post(pause))
        .route("/player/skip", post(skip))
        .route("/player/skip-vote", post(skip_vote))
        .route("/player/volume", put(set_volume))
        .route("/player/crossfade", put(set_crossfade))
        .route("/settings", get(settings).put(update_settings))
        .route("/requests/log", get(request_log).delete(clear_request_log))
}

/// Maps queue rule violations to client errors; anything else is a 500.
fn queue_error(err: QueueError) -> ApiError {
    match err {
        QueueError::Add(AddRequestError::NotFound) | QueueError::NotFound => ApiError::not_found(),
        QueueError::Add(
            e @ (AddRequestError::AlreadyQueued
            | AddRequestError::AlreadyYours
            | AddRequestError::LimitReached(_)
            | AddRequestError::QueueFull(_)),
        ) => ApiError::conflict(e.to_string()),
        QueueError::NothingPlaying | QueueError::SkipResting => ApiError::conflict(err.to_string()),
        QueueError::NotYours | QueueError::SkipVotesOff => {
            ApiError::new(StatusCode::FORBIDDEN, err.to_string())
        }
        QueueError::Add(AddRequestError::Store(e)) | QueueError::Store(e) => e.into(),
        QueueError::Other(e) => e.into(),
    }
}

async fn queue(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<Vec<QueueItem>>> {
    Ok(Json(state.store.queue().await?))
}

#[derive(Deserialize)]
struct AddRequest {
    track_id: i64,
}

#[derive(Serialize)]
struct AddedRequest {
    #[serde(flatten)]
    item: QueueItem,
    /// "added" for a new song, "backed" when it was queued already.
    outcome: RequestOutcome,
}

async fn add_request(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<AddRequest>,
) -> ApiResult<(StatusCode, Json<AddedRequest>)> {
    let origin = RequestOrigin {
        name: user.name().to_owned(),
        mac: client::mac_for(ip).await.or_else(|| user.mac.clone()),
        ip: ip.map(|ip| ip.to_string()),
    };
    let (item, outcome) = state
        .jukebox
        .lock()
        .await
        .add_request(&user, body.track_id, &origin)
        .await
        .map_err(queue_error)?;
    let status = match outcome {
        RequestOutcome::Added => StatusCode::CREATED,
        RequestOutcome::Backed => StatusCode::OK,
    };
    Ok((status, Json(AddedRequest { item, outcome })))
}

#[derive(Deserialize)]
struct AddAll {
    kind: EntryKind,
    ref_id: i64,
}

#[derive(Serialize)]
struct Added {
    added: usize,
}

/// Queues a whole album or artist (admin), in play order.
async fn add_all(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<AddAll>,
) -> ApiResult<Json<Added>> {
    let ids = state.store.tracks_for(body.kind, body.ref_id).await?;
    if ids.is_empty() {
        return Err(ApiError::not_found());
    }
    let origin = RequestOrigin {
        name: admin.name().to_owned(),
        mac: client::mac_for(ip).await.or_else(|| admin.mac.clone()),
        ip: ip.map(|ip| ip.to_string()),
    };
    let added = state
        .jukebox
        .lock()
        .await
        .add_requests(&admin, &ids, &origin)
        .await
        .map_err(queue_error)?;
    Ok(Json(Added { added }))
}

async fn remove_request(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state
        .jukebox
        .lock()
        .await
        .remove_request(&user, id)
        .await
        .map_err(queue_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct MoveRequest {
    /// New 0-based index in the queue.
    position: usize,
}

async fn move_request(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<MoveRequest>,
) -> ApiResult<StatusCode> {
    state
        .jukebox
        .lock()
        .await
        .move_request(id, body.position)
        .await
        .map_err(queue_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct NowPlayingResponse {
    #[serde(flatten)]
    now_playing: NowPlaying,
    player: PlayerState,
    output: OutputStatus,
}

async fn now_playing(_: CurrentUser, State(state): State<AppState>) -> Json<NowPlayingResponse> {
    let jukebox = state.jukebox.lock().await;
    Json(NowPlayingResponse {
        now_playing: jukebox.now_playing(),
        player: jukebox.player_state(),
        output: jukebox.output_status(),
    })
}

async fn play(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.jukebox.lock().await.play();
    StatusCode::NO_CONTENT
}

async fn pause(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.jukebox.lock().await.pause();
    StatusCode::NO_CONTENT
}

async fn skip(AdminUser(admin): AdminUser, State(state): State<AppState>) -> StatusCode {
    state.jukebox.lock().await.skip(&admin);
    StatusCode::NO_CONTENT
}

#[derive(Serialize)]
struct SkipVote {
    skipped: bool,
}

/// Anyone's vote to skip the song playing.
async fn skip_vote(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<SkipVote>> {
    let skipped = state
        .jukebox
        .lock()
        .await
        .vote_skip(&user)
        .map_err(queue_error)?;
    Ok(Json(SkipVote { skipped }))
}

#[derive(Deserialize)]
struct Volume {
    /// 0 to 100.
    volume: u8,
}

async fn set_volume(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Volume>,
) -> ApiResult<StatusCode> {
    if body.volume > 100 {
        return Err(ApiError::bad_request("volume must be 0 to 100"));
    }
    state.jukebox.lock().await.set_volume(body.volume).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Crossfade {
    /// 0 (gapless) to 12.
    secs: u8,
}

async fn set_crossfade(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Crossfade>,
) -> ApiResult<StatusCode> {
    if u32::from(body.secs) > djinn_engine::MAX_CROSSFADE_SECS {
        return Err(ApiError::bad_request("crossfade must be 0 to 12 seconds"));
    }
    state.jukebox.lock().await.set_crossfade(body.secs).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
struct Settings {
    history_window: i64,
    per_user_limit: i64,
    /// Most songs the whole queue may hold; 0 means no cap.
    max_queue: i64,
    /// Guests join with just a name instead of an account.
    guest_mode: bool,
    /// Look up artist photos and bios online (Deezer, Wikipedia).
    #[serde(default)]
    online_artist_info: bool,
    /// Contact sent to MusicBrainz (email or website). Empty turns
    /// MusicBrainz lookups off.
    #[serde(default)]
    musicbrainz_contact: String,
    /// Artist names never split into separate artists, one per line.
    #[serde(default)]
    artist_keep_whole: String,
    /// The TV page (/tv) works without signing in.
    #[serde(default = "default_true")]
    tv_page: bool,
    /// Anyone signed in can play the party in their own browser.
    #[serde(default)]
    play_here: bool,
    /// Votes that skip a song; 0 turns skip voting off.
    #[serde(default)]
    skip_votes: i64,
    /// AcoustID application key, for identifying songs by sound.
    #[serde(default)]
    acoustid_key: String,
}

fn default_true() -> bool {
    true
}

async fn settings(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Settings>> {
    let store = &state.store;
    Ok(Json(Settings {
        history_window: settings::number(
            store,
            jukebox::HISTORY_WINDOW,
            jukebox::DEFAULT_HISTORY_WINDOW,
        )
        .await?,
        per_user_limit: settings::number(
            store,
            jukebox::PER_USER_LIMIT,
            jukebox::DEFAULT_PER_USER_LIMIT,
        )
        .await?,
        max_queue: settings::number(store, jukebox::MAX_QUEUE, 0).await?,
        guest_mode: state.guest_mode(),
        online_artist_info: settings::flag(store, settings::ONLINE_INFO, false).await?,
        musicbrainz_contact: store
            .get_setting(settings::MUSICBRAINZ_CONTACT)
            .await?
            .unwrap_or_default(),
        artist_keep_whole: djinn_library::keep_whole(store)
            .await
            .map_err(anyhow::Error::from)?
            .join("\n"),
        tv_page: crate::tv_api::enabled(&state).await?,
        play_here: settings::flag(store, crate::cast_api::PLAY_HERE, false).await?,
        skip_votes: settings::number(store, jukebox::SKIP_VOTES, 0).await?,
        acoustid_key: store
            .get_setting(crate::identify_api::ACOUSTID_KEY)
            .await?
            .unwrap_or_default(),
    }))
}

async fn update_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    if !(0..=500).contains(&body.history_window) {
        return Err(ApiError::bad_request("history_window must be 0 to 500"));
    }
    if !(1..=50).contains(&body.per_user_limit) {
        return Err(ApiError::bad_request("per_user_limit must be 1 to 50"));
    }
    if !(0..=100).contains(&body.skip_votes) {
        return Err(ApiError::bad_request("skip_votes must be 0 (off) to 100"));
    }
    if !(0..=1000).contains(&body.max_queue) {
        return Err(ApiError::bad_request(
            "max_queue must be 0 (no cap) to 1000",
        ));
    }
    let contact = body.musicbrainz_contact.trim();
    if contact.chars().count() > 200 || contact.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "the MusicBrainz contact should be an email address or website",
        ));
    }
    let store = &state.store;
    let old_contact = store
        .get_setting(settings::MUSICBRAINZ_CONTACT)
        .await?
        .unwrap_or_default();
    store
        .set_setting(settings::MUSICBRAINZ_CONTACT, contact)
        .await?;
    store
        .set_setting(jukebox::HISTORY_WINDOW, &body.history_window.to_string())
        .await?;
    store
        .set_setting(jukebox::PER_USER_LIMIT, &body.per_user_limit.to_string())
        .await?;
    store
        .set_setting(jukebox::MAX_QUEUE, &body.max_queue.to_string())
        .await?;
    settings::set_flag(store, settings::GUEST_MODE, body.guest_mode).await?;
    state
        .guest_mode
        .store(body.guest_mode, std::sync::atomic::Ordering::Relaxed);
    settings::set_flag(store, crate::tv_api::TV_PAGE, body.tv_page).await?;
    settings::set_flag(store, crate::cast_api::PLAY_HERE, body.play_here).await?;
    store
        .set_setting(jukebox::SKIP_VOTES, &body.skip_votes.to_string())
        .await?;
    let acoustid_key = body.acoustid_key.trim();
    if acoustid_key.len() > 64 || !acoustid_key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::bad_request(
            "that doesn't look like an AcoustID key",
        ));
    }
    store
        .set_setting(crate::identify_api::ACOUSTID_KEY, acoustid_key)
        .await?;
    state
        .jukebox
        .lock()
        .await
        .set_skip_votes_needed(u32::try_from(body.skip_votes).unwrap_or(0));
    let was_online = settings::flag(store, settings::ONLINE_INFO, false).await?;
    settings::set_flag(store, settings::ONLINE_INFO, body.online_artist_info).await?;
    // A changed never-split list means re-reading every file's artists.
    let keep_whole: Vec<&str> = body
        .artist_keep_whole
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let old_keep_whole = djinn_library::keep_whole(store)
        .await
        .map_err(anyhow::Error::from)?;
    if keep_whole != old_keep_whole {
        store
            .set_setting(djinn_library::KEEP_WHOLE_KEY, &keep_whole.join("\n"))
            .await?;
        store
            .set_setting(djinn_library::SCAN_VERSION_KEY, "0")
            .await?;
        crate::library_api::start_scan(&state);
    }
    // Turning lookups on, or adding MusicBrainz, looks everything up now
    // (MusicBrainz adds covers and genres to artists already checked).
    let added_musicbrainz = old_contact.trim().is_empty() && !contact.is_empty();
    if body.online_artist_info && (!was_online || added_musicbrainz) {
        crate::library_api::start_artist_lookup(&state, added_musicbrainz);
    }
    Ok(Json(body))
}

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<i64>,
}

/// When the request log was last cleared (Unix ms).
const LOG_CLEARED_AT: &str = "request_log_cleared_at";

/// Recent requests with who made them and from which device.
async fn request_log(
    _: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<LogQuery>,
) -> ApiResult<Json<Vec<RequestLogEntry>>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let since = settings::number(&state.store, LOG_CLEARED_AT, 0).await?;
    Ok(Json(state.store.request_log(since, limit).await?))
}

/// Empties the request log view. Request counts behind "top songs" are
/// kept.
async fn clear_request_log(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state
        .store
        .set_setting(LOG_CLEARED_AT, &djinn_store::now_ms().to_string())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    #[tokio::test]
    async fn queue_playlist_and_settings_status_codes() {
        let state = AppState::for_tests().await;
        let tracks: Vec<ScannedTrack> = (0..5)
            .map(|i| ScannedTrack {
                path: format!("/m/{i}.flac"),
                title: format!("Song {i}"),
                ..Default::default()
            })
            .collect();
        state.store.save_scanned(&tracks).await.unwrap();
        let ids: Vec<i64> = state
            .store
            .known_files()
            .await
            .unwrap()
            .iter()
            .map(|k| k.id)
            .collect();
        let admin = session(&state, "admin", Role::Admin).await;
        let alice = session(&state, "alice", Role::User).await;
        let bob = session(&state, "bob", Role::User).await;
        let post = |id: i64| format!(r#"{{"track_id":{id}}}"#);

        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(ids[0])),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(ids[0])),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(ids[0])),
            Some(&bob),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "someone else asking backs it");
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(ids[0])),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "asking twice");
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(9999)),
            Some(&bob),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        for &id in &ids[1..3] {
            call(
                &state,
                Method::POST,
                "/api/queue",
                Some(&post(id)),
                Some(&alice),
            )
            .await;
        }
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/queue",
            Some(&post(ids[3])),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "per-user limit");

        // Bob backs the first song, so he may step back from it, but the
        // second is Alice's alone.
        let queue = state.store.queue().await.unwrap();
        let path = format!("/api/queue/{}", queue[1].id);
        let (status, _) = call(&state, Method::DELETE, &path, None, Some(&bob)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "not bob's request");
        let (status, _) = call(
            &state,
            Method::PATCH,
            &path,
            Some(r#"{"position":2}"#),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "reorder is admin only");
        let (status, _) = call(
            &state,
            Method::PATCH,
            &path,
            Some(r#"{"position":2}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(&state, Method::DELETE, &path, None, Some(&alice)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists",
            Some(r#"{"name":"Chill"}"#),
            Some(&alice),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists",
            Some(r#"{"name":"Chill"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let list = state.store.playlists().await.unwrap()[0].id;
        let entry = format!(r#"{{"kind":"track","ref_id":{}}}"#, ids[4]);
        let path = format!("/api/playlists/{list}/entries");
        let (status, _) = call(&state, Method::POST, &path, Some(&entry), Some(&admin)).await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = call(&state, Method::POST, &path, Some(&entry), Some(&admin)).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let play = format!("/api/playlists/{list}/play");
        let (status, _) = call(&state, Method::POST, &play, Some("{}"), Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(state.jukebox.lock().await.active_playlist(), Some(list));
        let (status, _) = call(
            &state,
            Method::DELETE,
            &format!("/api/playlists/{list}"),
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(
            state.jukebox.lock().await.active_playlist(),
            None,
            "deleting stops it"
        );

        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/player/volume",
            Some(r#"{"volume":101}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(&state, Method::POST, "/api/player/skip", None, Some(&alice)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let rules = r#"{"history_window":25,"per_user_limit":3,"max_queue":4,"guest_mode":true}"#;
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/settings",
            Some(rules),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(state.guest_mode());
        let (status, _) = call(&state, Method::GET, "/api/requests/log", None, Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = call(&state, Method::GET, "/api/requests/log", None, Some(&bob)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
