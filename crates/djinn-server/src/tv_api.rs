//! The TV page: a full-screen now-playing display for a TV or spare
//! tablet. Public and read-only, so a TV browser needs no sign-in; the
//! admin can turn it off.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::QueueItem;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::events::{NowPlaying, PlayerState};
use crate::settings;
use crate::state::AppState;

/// Setting key; on unless set to "0".
pub const TV_PAGE: &str = "tv_page";
/// Setting key: the TV page can play the party without signing in, which means
/// anyone who can open it can listen. Off unless set to "1".
pub const TV_SOUND: &str = "tv_sound";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tv", get(snapshot))
        .route("/tv/events", get(crate::sse::tv_events))
        .route("/tv/art/{hash}", get(art))
        .route("/tv/pictures", get(pictures))
}

pub async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(settings::flag(&state.store, TV_PAGE, true).await?)
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
    /// The live stream, so the TV can play the music itself: for a cast,
    /// which is given the secret, or when the admin allows it for everyone.
    stream_url: Option<String>,
    /// Tonight's numbers, when the admin shows them.
    tonight: Option<Tonight>,
}

/// The party so far, for the TV.
#[derive(Serialize)]
struct Tonight {
    songs: i64,
    requests: i64,
    /// The song asked for most (at least twice), as "Title, Artist".
    top_song: Option<String>,
    /// Who asked for the most songs (at least two).
    top_requester: Option<String>,
}

/// Setting key: show tonight's numbers on the TV page (on unless turned off).
pub const TV_STATS: &str = "tv_stats";
/// A break this long with nothing playing ends a party.
const PARTY_GAP_MS: i64 = 6 * 60 * 60 * 1000;

async fn tonight(state: &AppState) -> anyhow::Result<Option<Tonight>> {
    if !settings::flag(&state.store, TV_STATS, true).await? {
        return Ok(None);
    }
    let Some(from) = state.store.last_party_start(PARTY_GAP_MS).await? else {
        return Ok(None);
    };
    let filter = djinn_store::StatsFilter {
        from,
        go: false,
        ..djinn_store::StatsFilter::default()
    };
    let summary = state.store.stats_summary(&filter).await?;
    let top_song = state
        .store
        .top_requested(&filter, 1)
        .await?
        .into_iter()
        .find(|r| r.requests >= 2)
        .and_then(|r| {
            r.title
                .map(|t| r.artist.map_or_else(|| t.clone(), |a| format!("{t}, {a}")))
        });
    let top_requester = state
        .store
        .top_requesters(&filter, 1)
        .await?
        .into_iter()
        .find(|p| p.requests >= 2)
        .and_then(|p| p.name);
    Ok(Some(Tonight {
        songs: summary.plays,
        requests: summary.requests,
        top_song,
        top_requester,
    }))
}

#[derive(Deserialize)]
struct TvQuery {
    /// The stream's secret, from the address a cast opens.
    key: Option<String>,
}

async fn snapshot(
    State(state): State<AppState>,
    Query(query): Query<TvQuery>,
) -> ApiResult<Json<TvSnapshot>> {
    check_enabled(&state).await?;
    let token = state.stream_token();
    let sound = query.key.as_deref() == Some(token.as_str())
        || settings::flag(&state.store, TV_SOUND, false).await?;
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
            .filter(|_| sound)
            .map(|_| crate::cast_api::stream_path(&token)),
        tonight: tonight(&state).await?,
    }))
}

/// Slideshow pictures of the artist playing now (empty when off).
async fn pictures(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<djinn_store::ArtistPicture>>> {
    check_enabled(&state).await?;
    let artist = state
        .jukebox
        .lock()
        .await
        .now_playing()
        .current
        .and_then(|entry| entry.track.artist_id);
    Ok(Json(match artist {
        Some(id) => crate::library_api::pictures_for(&state, id).await?,
        None => Vec::new(),
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
