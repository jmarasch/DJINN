//! REST API mounted under `/api`.

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
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
