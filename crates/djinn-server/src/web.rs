//! Serves the Svelte UI embedded in the binary.
//!
//! Release builds embed `web/dist`. Debug builds read it from disk, so
//! `npm run build` takes effect without recompiling.

use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
#[allow_missing = true]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(response) = asset(path) {
        return response;
    }
    // Paths with an extension are real files that don't exist. Anything else
    // is a client-side route, so hand it to the SPA.
    if path
        .rsplit('/')
        .next()
        .is_some_and(|name| name.contains('.'))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    asset("index.html").unwrap_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "DJINN web UI is not built. Run `npm ci && npm run build` in web/ and rebuild.",
        )
            .into_response()
    })
}

/// Which web app build is embedded: its main script's hashed file name.
/// Clients compare it with their own and reload when DJINN was updated.
pub fn build_id() -> Option<String> {
    let index = Assets::get("index.html")?;
    let html = std::str::from_utf8(&index.data).ok()?;
    let start = html.find("/assets/index-")? + 1;
    let end = start + html[start..].find(".js")? + 3;
    Some(html[start..end].to_owned())
}

fn asset(path: &str) -> Option<Response> {
    let file = Assets::get(path)?;
    // Vite puts content-hashed files under assets/, so they never change.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(
        (
            [
                (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
                (header::CACHE_CONTROL, cache.to_owned()),
            ],
            file.data,
        )
            .into_response(),
    )
}
