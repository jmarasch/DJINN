//! Casting: the live stream that cast devices and the TV page play, and
//! the admin API that finds devices and starts and stops casts.
//!
//! The stream URL carries a random token instead of a session cookie,
//! because Chromecasts, TVs, and Sonos speakers fetch it themselves and
//! can't sign in. Only DJINN hands the URL out.

use axum::body::Body;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_cast::stream::{self, LiveStream};
use djinn_cast::{CastStatus, Metadata};
use djinn_store::Store;
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::state::AppState;

/// Setting key holding the stream token.
const CAST_TOKEN: &str = "cast_token";
/// Setting key: devices being cast to, so casts resume after a restart.
const CAST_TARGETS: &str = "cast_targets";
/// How long to listen for devices.
const DISCOVERY: Duration = Duration::from_secs(4);

/// Routes outside /api: devices fetch these without cookies.
pub fn stream_router() -> Router<AppState> {
    Router::new()
        .route("/cast/{token}/live.mp3", get(live))
        .route("/cast/{token}/join.png", get(join_png))
}

/// Admin routes under /api.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/cast", get(list))
        .route("/cast/discover", post(discover))
        .route("/cast/stop-all", post(stop_all))
        .route("/cast/{id}/start", post(start))
        .route("/cast/{id}/stop", post(stop))
        .route("/cast/{id}/volume", put(volume))
}

#[derive(Serialize)]
struct CastList {
    devices: Vec<CastStatus>,
    /// Players connected to the stream (casts and TV pages).
    listeners: usize,
}

fn cast_list(state: &AppState) -> CastList {
    CastList {
        devices: state.caster.status(),
        listeners: state.stream.as_ref().map_or(0, LiveStream::listeners),
    }
}

async fn list(_: AdminUser, State(state): State<AppState>) -> Json<CastList> {
    Json(cast_list(&state))
}

async fn discover(_: AdminUser, State(state): State<AppState>) -> Json<CastList> {
    state.caster.discover(DISCOVERY).await;
    Json(cast_list(&state))
}

async fn start(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<CastList>> {
    start_cast(&state, &id)?;
    save_targets(&state).await?;
    Ok(Json(cast_list(&state)))
}

async fn stop(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<CastList>> {
    state.caster.stop(&id).await;
    // The session ends a moment later; save the list without it now.
    let targets: Vec<String> = state
        .caster
        .active()
        .into_iter()
        .filter(|t| *t != id)
        .collect();
    state
        .store
        .set_setting(CAST_TARGETS, &targets.join("\n"))
        .await?;
    Ok(Json(cast_list(&state)))
}

async fn stop_all(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.caster.stop_all().await;
    state.store.set_setting(CAST_TARGETS, "").await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Volume {
    /// 0.0 to 1.0.
    level: f32,
}

async fn volume(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Volume>,
) -> ApiResult<StatusCode> {
    if !(0.0..=1.0).contains(&body.level) {
        return Err(ApiError::bad_request("volume must be 0 to 1"));
    }
    state.caster.set_volume(&id, body.level).await;
    Ok(StatusCode::NO_CONTENT)
}

fn start_cast(state: &AppState, id: &str) -> ApiResult<()> {
    if state.stream.is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the audio stream isn't running",
        ));
    }
    let base = state.join_url.trim_end_matches('/');
    let token = state.cast_token.as_str();
    let meta = Metadata {
        title: "DJINN".to_owned(),
        subtitle: "Scan the code to add songs".to_owned(),
        image_url: Some(format!("{base}/cast/{token}/join.png")),
    };
    state
        .caster
        .start(id, format!("{base}{}", stream_path(token)), meta)
        .map_err(|e| ApiError::not_found_with(e.to_string()))
}

async fn save_targets(state: &AppState) -> ApiResult<()> {
    state
        .store
        .set_setting(CAST_TARGETS, &state.caster.active().join("\n"))
        .await?;
    Ok(())
}

/// Background work at startup: forwards cast status to clients, and picks
/// up the casts that were running when DJINN last stopped.
pub fn spawn_background(state: &AppState) {
    let forward = state.clone();
    tokio::spawn(async move {
        let mut changes = forward.caster.subscribe();
        while changes.changed().await.is_ok() {
            let _ = forward
                .events
                .send(ServerEvent::CastStatus(forward.caster.status()));
            // Coalesce bursts of changes.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    let resume = state.clone();
    tokio::spawn(async move {
        let targets = match resume.store.get_setting(CAST_TARGETS).await {
            Ok(Some(targets)) if !targets.trim().is_empty() => targets,
            _ => return,
        };
        resume.caster.discover(DISCOVERY).await;
        for id in targets.lines().map(str::trim).filter(|t| !t.is_empty()) {
            match start_cast(&resume, id) {
                Ok(()) => tracing::info!(device = id, "resuming cast"),
                Err(_) => tracing::info!(device = id, "cast device not found; not resuming"),
            }
        }
    });
}

/// The join QR code as a PNG: what TVs show while they play the party.
async fn join_png(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    if *state.cast_token != token {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(code) = qrcode::QrCode::new(state.join_url.as_bytes()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let image = code
        .render::<image::Luma<u8>>()
        .min_dimensions(720, 720)
        .build();
    let mut png = std::io::Cursor::new(Vec::new());
    if image.write_to(&mut png, image::ImageFormat::Png).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], png.into_inner()).into_response()
}

/// The stream token, made on first use.
pub async fn token(store: &Store) -> anyhow::Result<String> {
    if let Some(token) = store.get_setting(CAST_TOKEN).await? {
        return Ok(token);
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    store.set_setting(CAST_TOKEN, &token).await?;
    Ok(token)
}

/// Path of the live stream, relative to the server.
pub fn stream_path(token: &str) -> String {
    format!("/cast/{token}/live.mp3")
}

async fn live(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let Some(stream) = state.stream.as_ref().filter(|_| *state.cast_token == token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    live_response(stream)
}

fn live_response(stream: &LiveStream) -> Response {
    let stream::Listener {
        backlog,
        live,
        guard,
    } = stream.subscribe();
    let live = BroadcastStream::new(live).filter_map(move |frame| {
        // The guard counts this listener for as long as the response
        // streams.
        let _ = &guard;
        // A slow client that skipped frames (lagged) picks up again at the
        // next one; MP3 resyncs on its own.
        std::future::ready(frame.ok())
    });
    let backlog = futures_util::stream::iter(backlog);
    let body = backlog.chain(live).map(Ok::<_, std::convert::Infallible>);
    (
        [
            (header::CONTENT_TYPE, stream::CONTENT_TYPE),
            (header::CACHE_CONTROL, "no-cache, no-store"),
            // DLNA renderers look for these before playing a live stream.
            (
                header::HeaderName::from_static("transfermode.dlna.org"),
                "Streaming",
            ),
            (
                header::HeaderName::from_static("contentfeatures.dlna.org"),
                "DLNA.ORG_PN=MP3;DLNA.ORG_OP=00;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
            ),
            (header::HeaderName::from_static("icy-name"), "DJINN"),
        ],
        Body::from_stream(body),
    )
        .into_response()
}
