//! REST API mounted under `/api`.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::Serialize;

use crate::error::ApiResult;

use crate::state::AppState;
use crate::{audio_api, auth, folders, library_api, player_api, playlist_api, sse, tv_api};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/events", get(sse::events))
        .route("/join", get(join))
        .route("/join/qr.svg", get(join_qr))
        .merge(auth::router())
        .merge(library_api::router())
        .merge(player_api::router())
        .merge(playlist_api::router())
        .merge(audio_api::router())
        .merge(tv_api::router())
        .merge(folders::router())
        // Without this, unknown API paths would fall through to the SPA.
        .fallback(not_found)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Serialize)]
struct Join {
    url: String,
}

/// The address guests open. Public: the join page is shown on a screen
/// for people who aren't signed in yet.
async fn join(State(state): State<AppState>) -> Json<Join> {
    Json(Join {
        url: state.join_url.to_string(),
    })
}

async fn join_qr(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let code = QrCode::new(state.join_url.as_bytes()).map_err(anyhow::Error::from)?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(320, 320)
        .dark_color(svg::Color("#14111f"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], svg))
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
