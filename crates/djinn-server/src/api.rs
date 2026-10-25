//! REST API mounted under `/api`.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::art;
use serde::Serialize;

use crate::auth::{self, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/art/{hash}", get(cover_art))
        .merge(auth::router())
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

/// Cover art by content hash. Originals for now; `?size=` thumbnails come
/// with the thumbnail cache (Phase 2).
async fn cover_art(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let path = art::path(&state.art_dir, &hash).ok_or_else(ApiError::not_found)?;
    let bytes = tokio::fs::read(path).await?;
    Ok((
        [
            (header::CONTENT_TYPE, art::mime_type(&bytes)),
            // Content-addressed: a hash never changes meaning.
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        bytes,
    ))
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
