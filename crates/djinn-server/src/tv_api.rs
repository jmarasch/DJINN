//! The TV page: a full-screen now-playing display for a TV or spare
//! tablet. Public and read-only, so a TV browser needs no sign-in; the
//! admin can turn it off.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::QueueItem;
use serde::Serialize;

use crate::error::{ApiError, ApiResult};
use crate::events::{NowPlaying, PlayerState};
use crate::jukebox;
use crate::state::AppState;

/// Setting key; on unless set to "0".
pub const TV_PAGE: &str = "tv_page";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tv", get(snapshot))
        .route("/tv/events", get(crate::sse::tv_events))
        .route("/tv/art/{hash}", get(art))
}

pub async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(jukebox::setting(&state.store, TV_PAGE, 1).await? != 0)
}

pub async fn check_enabled(state: &AppState) -> ApiResult<()> {
    if enabled(state).await? {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "the TV page is turned off. The host can turn it on under Admin > Rules.",
        ))
    }
}

#[derive(Serialize)]
struct TvSnapshot {
    #[serde(flatten)]
    now_playing: NowPlaying,
    player: PlayerState,
    queue: Vec<QueueItem>,
    join_url: String,
    guest_mode: bool,
    /// The live stream, so the TV can play the music itself.
    stream_url: Option<String>,
}

async fn snapshot(State(state): State<AppState>) -> ApiResult<Json<TvSnapshot>> {
    check_enabled(&state).await?;
    let (now_playing, player) = {
        let jukebox = state.jukebox.lock().await;
        (jukebox.now_playing(), jukebox.player_state())
    };
    Ok(Json(TvSnapshot {
        now_playing,
        player,
        queue: state.store.queue().await?,
        join_url: state.join_url.to_string(),
        guest_mode: state.guest_mode(),
        stream_url: state
            .stream
            .as_ref()
            .map(|_| crate::cast_api::stream_path(&state.cast_token)),
    }))
}

/// Cover art for the TV page, which has no session. Hashes are only
/// learned from the TV feed, so this shows nothing the TV doesn't.
async fn art(
    State(state): State<AppState>,
    Path(hash): Path<String>,
    Query(query): Query<crate::library_api::ArtQuery>,
) -> ApiResult<impl IntoResponse> {
    check_enabled(&state).await?;
    crate::library_api::serve_art(&state, hash, query).await
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use crate::test_util::call;
    use axum::http::{Method, StatusCode};

    #[tokio::test]
    async fn tv_page_is_public_until_turned_off() {
        let state = AppState::for_tests().await;
        let (status, _) = call(&state, Method::GET, "/api/tv", None, None).await;
        assert_eq!(status, StatusCode::OK, "no sign-in needed");
        // The member feed still needs a session.
        let (status, _) = call(&state, Method::GET, "/api/events", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        state.store.set_setting(super::TV_PAGE, "0").await.unwrap();
        let (status, _) = call(&state, Method::GET, "/api/tv", None, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::GET, "/api/tv/events", None, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
