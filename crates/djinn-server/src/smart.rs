//! Smart playlists: songs and artists that sound alike, from the analysis
//! the background job stores. The index lives in memory and is rebuilt
//! when analysis finishes.

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::sound::SoundIndex;
use djinn_store::{ArtistSummary, Track};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::state::AppState;

const DEFAULT_LIMIT: usize = 30;
const MAX_LIMIT: usize = 100;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tracks/{id}/similar", get(similar_tracks))
        .route("/artists/{id}/similar", get(similar_artists))
}

/// Rebuilds the index from the database.
pub async fn refresh(state: &AppState) {
    let rows = match state.store.sound_rows(djinn_engine::FEATURES_VERSION).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!(error = %e, "loading sound features failed");
            return;
        }
    };
    let index = tokio::task::spawn_blocking(move || SoundIndex::build(rows))
        .await
        .unwrap_or_default();
    tracing::info!(songs = index.len(), "sound index ready");
    if let Ok(mut current) = state.sound.write() {
        *current = index;
    }
}

#[derive(Deserialize)]
struct Limit {
    limit: Option<usize>,
}

impl Limit {
    fn get(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }
}

#[derive(Serialize)]
struct SimilarTracks {
    tracks: Vec<Track>,
    /// False until the song has been analyzed.
    analyzed: bool,
}

async fn similar_tracks(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(limit): Query<Limit>,
) -> ApiResult<Json<SimilarTracks>> {
    let ids = state
        .sound
        .read()
        .map(|index| index.similar_tracks(id, limit.get()))
        .unwrap_or_default();
    Ok(Json(SimilarTracks {
        analyzed: !ids.is_empty(),
        tracks: state.store.tracks_by_ids(&ids).await?,
    }))
}

async fn similar_artists(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(limit): Query<Limit>,
) -> ApiResult<Json<Vec<ArtistSummary>>> {
    let ids = state
        .sound
        .read()
        .map(|index| index.similar_artists(id, limit.limit.unwrap_or(12).clamp(1, 50)))
        .unwrap_or_default();
    Ok(Json(state.store.artists_by_ids(&ids).await?))
}
