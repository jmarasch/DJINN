//! The request queue, now playing, player controls, auto pool, and
//! runtime settings.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use djinn_store::{
    AddPoolError, AddRequestError, PoolEntry, PoolKind, QueueItem, RequestLogEntry, RequestOrigin,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::client::{self, ClientIp};
use crate::error::{ApiError, ApiResult};
use crate::events::{NowPlaying, OutputStatus, PlayerState};
use crate::jukebox::{self, QueueError};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/queue", get(queue).post(add_request))
        .route("/queue/{id}", delete(remove_request).patch(move_request))
        .route("/now-playing", get(now_playing))
        .route("/player/play", post(play))
        .route("/player/pause", post(pause))
        .route("/player/skip", post(skip))
        .route("/player/volume", put(set_volume))
        .route("/pool", get(pool).post(add_pool_entry))
        .route("/pool/{id}", delete(remove_pool_entry))
        .route("/settings", get(settings).put(update_settings))
        .route("/requests/log", get(request_log))
}

/// Maps queue rule violations to client errors; anything else is a 500.
fn queue_error(err: QueueError) -> ApiError {
    match err {
        QueueError::Add(AddRequestError::NotFound) | QueueError::NotFound => ApiError::not_found(),
        QueueError::Add(
            e @ (AddRequestError::AlreadyQueued
            | AddRequestError::LimitReached(_)
            | AddRequestError::QueueFull(_)),
        ) => ApiError::conflict(e.to_string()),
        QueueError::NotYours => ApiError::new(StatusCode::FORBIDDEN, err.to_string()),
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

async fn add_request(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<AddRequest>,
) -> ApiResult<(StatusCode, Json<QueueItem>)> {
    let origin = RequestOrigin {
        name: user.name().to_owned(),
        mac: client::mac_for(ip).await.or_else(|| user.mac.clone()),
        ip: ip.map(|ip| ip.to_string()),
    };
    let item = state
        .jukebox
        .lock()
        .await
        .add_request(&user, body.track_id, &origin)
        .await
        .map_err(queue_error)?;
    Ok((StatusCode::CREATED, Json(item)))
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

async fn pool(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<PoolEntry>>> {
    Ok(Json(state.store.pool_entries().await?))
}

#[derive(Deserialize)]
struct NewPoolEntry {
    kind: PoolKind,
    ref_id: i64,
}

async fn add_pool_entry(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewPoolEntry>,
) -> ApiResult<(StatusCode, Json<PoolEntry>)> {
    let entry = match state.store.add_pool_entry(body.kind, body.ref_id).await {
        Ok(entry) => entry,
        Err(AddPoolError::NotFound) => return Err(ApiError::not_found()),
        Err(e @ AddPoolError::AlreadyInPool) => return Err(ApiError::conflict(e.to_string())),
        Err(AddPoolError::Store(e)) => return Err(e.into()),
    };
    state.jukebox.lock().await.pool_changed().await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn remove_pool_entry(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.remove_pool_entry(id).await? {
        return Err(ApiError::not_found());
    }
    state.jukebox.lock().await.pool_changed().await?;
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
}

async fn settings(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Settings>> {
    let store = &state.store;
    Ok(Json(Settings {
        history_window: jukebox::setting(
            store,
            jukebox::HISTORY_WINDOW,
            jukebox::DEFAULT_HISTORY_WINDOW,
        )
        .await?,
        per_user_limit: jukebox::setting(
            store,
            jukebox::PER_USER_LIMIT,
            jukebox::DEFAULT_PER_USER_LIMIT,
        )
        .await?,
        max_queue: jukebox::setting(store, jukebox::MAX_QUEUE, 0).await?,
        guest_mode: state.guest_mode(),
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
    if !(0..=1000).contains(&body.max_queue) {
        return Err(ApiError::bad_request(
            "max_queue must be 0 (no cap) to 1000",
        ));
    }
    let store = &state.store;
    store
        .set_setting(jukebox::HISTORY_WINDOW, &body.history_window.to_string())
        .await?;
    store
        .set_setting(jukebox::PER_USER_LIMIT, &body.per_user_limit.to_string())
        .await?;
    store
        .set_setting(jukebox::MAX_QUEUE, &body.max_queue.to_string())
        .await?;
    store
        .set_setting(jukebox::GUEST_MODE, if body.guest_mode { "1" } else { "0" })
        .await?;
    state
        .guest_mode
        .store(body.guest_mode, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(body))
}

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<i64>,
}

/// Recent requests with who made them and from which device.
async fn request_log(
    _: AdminUser,
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<LogQuery>,
) -> ApiResult<Json<Vec<RequestLogEntry>>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    Ok(Json(state.store.request_log(limit).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    #[tokio::test]
    async fn queue_and_pool_status_codes() {
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
        assert_eq!(status, StatusCode::CONFLICT, "duplicate");
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

        let first = state.store.queue().await.unwrap()[0].id;
        let path = format!("/api/queue/{first}");
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

        let pool = format!(r#"{{"kind":"track","ref_id":{}}}"#, ids[4]);
        let (status, _) = call(&state, Method::POST, "/api/pool", Some(&pool), Some(&alice)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::POST, "/api/pool", Some(&pool), Some(&admin)).await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = call(&state, Method::POST, "/api/pool", Some(&pool), Some(&admin)).await;
        assert_eq!(status, StatusCode::CONFLICT);

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
