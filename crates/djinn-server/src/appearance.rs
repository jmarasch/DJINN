//! The picture behind every page of the party app: one of the built-in
//! backgrounds, or an image the admin uploads (Admin > System). Public, since
//! the sign-in page wears it too.

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::art;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Setting key: a built-in background's name, or `custom:<version>`.
pub const BACKGROUND: &str = "background";
/// Setting key: how strongly the picture shows, 0 to 100.
pub const STRENGTH: &str = "background_strength";

const DEFAULT_BACKGROUND: &str = "waves";
const DEFAULT_STRENGTH: i64 = 28;
/// The names the web app draws itself.
const BUILT_IN: [&str; 7] = [
    "waves",
    "plum-vinyl",
    "violet-vinyl",
    "tv",
    "aurora",
    "grid",
    "plain",
];
const MAX_UPLOAD: usize = 12 * 1024 * 1024;
/// Smaller than this looks soft behind a page.
const MIN_WIDTH: u32 = 800;
const MIN_HEIGHT: u32 = 450;
const MAX_EDGE: u32 = 2400;
const FILE: &str = "background.jpg";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub background: String,
    pub strength: i64,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/appearance", get(read).put(save))
        .route("/appearance/image", get(image).post(upload).delete(remove))
}

async fn current(state: &AppState) -> ApiResult<Appearance> {
    let background = state
        .store
        .get_setting(BACKGROUND)
        .await?
        .filter(|b| valid(b))
        .unwrap_or_else(|| DEFAULT_BACKGROUND.to_string());
    let strength = crate::settings::number(&state.store, STRENGTH, DEFAULT_STRENGTH)
        .await?
        .clamp(0, 100);
    Ok(Appearance {
        background,
        strength,
    })
}

fn valid(background: &str) -> bool {
    BUILT_IN.contains(&background)
        || background
            .strip_prefix("custom:")
            .is_some_and(|v| !v.is_empty() && v.len() < 20 && v.chars().all(|c| c.is_ascii_digit()))
}

async fn read(State(state): State<AppState>) -> ApiResult<Json<Appearance>> {
    Ok(Json(current(&state).await?))
}

async fn save(
    _: AdminUser,
    State(state): State<AppState>,
    Json(wanted): Json<Appearance>,
) -> ApiResult<Json<Appearance>> {
    if !valid(&wanted.background) {
        return Err(ApiError::bad_request("that isn't a background"));
    }
    if wanted.background.starts_with("custom:") && !has_image(&state) {
        return Err(ApiError::bad_request("upload a picture first"));
    }
    if !(0..=100).contains(&wanted.strength) {
        return Err(ApiError::bad_request("strength is 0 to 100"));
    }
    state
        .store
        .set_setting(BACKGROUND, &wanted.background)
        .await?;
    state
        .store
        .set_setting(STRENGTH, &wanted.strength.to_string())
        .await?;
    Ok(Json(current(&state).await?))
}

fn file(state: &AppState) -> Option<std::path::PathBuf> {
    state.art_dir.parent().map(|dir| dir.join(FILE))
}

fn has_image(state: &AppState) -> bool {
    file(state).is_some_and(|f| f.is_file())
}

/// The uploaded picture. The page asks with `?v=` so a new one isn't hidden
/// by a cached old one.
async fn image(State(state): State<AppState>) -> ApiResult<impl IntoResponse + use<>> {
    let path = file(&state).ok_or_else(ApiError::not_found)?;
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| ApiError::not_found())?;
    Ok((
        [
            (header::CONTENT_TYPE, art::mime_type(&bytes)),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        bytes,
    ))
}

async fn upload(
    _: AdminUser,
    State(state): State<AppState>,
    body: Body,
) -> ApiResult<Json<Appearance>> {
    let bytes = to_bytes(body, MAX_UPLOAD)
        .await
        .map_err(|_| ApiError::bad_request("the picture must be 12 MB or smaller"))?;
    let path = file(&state).ok_or_else(ApiError::not_found)?;
    let jpeg = tokio::task::spawn_blocking(move || {
        art::prepare_artist_image(&bytes, MIN_WIDTH, MIN_HEIGHT, MAX_EDGE)
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(|e| ApiError::bad_request(format!("couldn't read that picture: {e}")))?
    .ok_or_else(|| ApiError::bad_request("the picture should be at least 800 by 450 pixels"))?;
    // Written whole, then moved into place, so a page never gets half of it.
    let part = path.with_extension("part");
    tokio::fs::write(&part, &jpeg).await?;
    tokio::fs::rename(&part, &path).await?;
    let version = djinn_store::now_ms().to_string();
    state
        .store
        .set_setting(BACKGROUND, &format!("custom:{version}"))
        .await?;
    Ok(Json(current(&state).await?))
}

async fn remove(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Appearance>> {
    if let Some(path) = file(&state) {
        let _ = tokio::fs::remove_file(path).await;
    }
    if state
        .store
        .get_setting(BACKGROUND)
        .await?
        .is_some_and(|b| b.starts_with("custom:"))
    {
        state
            .store
            .set_setting(BACKGROUND, DEFAULT_BACKGROUND)
            .await?;
    }
    Ok(Json(current(&state).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    #[test]
    fn only_known_backgrounds_are_valid() {
        for good in ["waves", "aurora", "plain", "custom:1731400000000"] {
            assert!(valid(good), "{good}");
        }
        for bad in ["", "custom:", "custom:x", "../etc", "Waves", "custom:1/2"] {
            assert!(!valid(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn anyone_reads_it_and_admins_change_it() {
        let state = AppState::for_tests().await;
        let (status, body) = get_body(&state, "/api/appearance", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"background":"waves","strength":28}"#);
        let user = session(&state, "bo", Role::User).await;
        let admin = session(&state, "ana", Role::Admin).await;
        let grid = Some(r#"{"background":"grid","strength":40}"#);
        let (status, _) = call(&state, Method::PUT, "/api/appearance", grid, Some(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::PUT, "/api/appearance", grid, Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = get_body(&state, "/api/appearance", "").await;
        assert_eq!(body, r#"{"background":"grid","strength":40}"#);
        for bad in [
            r#"{"background":"nope","strength":40}"#,
            r#"{"background":"grid","strength":400}"#,
            r#"{"background":"custom:5","strength":40}"#,
        ] {
            let (status, _) = call(
                &state,
                Method::PUT,
                "/api/appearance",
                Some(bad),
                Some(&admin),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
        }
        let (status, _) = get_body(&state, "/api/appearance/image", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
