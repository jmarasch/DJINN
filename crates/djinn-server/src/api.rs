//! REST API mounted under `/api`.

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::state::AppState;
use crate::{auth, library_api, player_api, sse};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/events", get(sse::events))
        .merge(auth::router())
        .merge(library_api::router())
        .merge(player_api::router())
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

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
