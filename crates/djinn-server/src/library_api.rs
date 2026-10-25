//! Browsing the library: search, tracks, albums, artists, art, rescans.

use std::sync::atomic::Ordering;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::{art, ScanProgress};
use djinn_store::{
    Album, AlbumDetail, ArtistDetail, ArtistSummary, Page, SearchResults, Track, TrackSort,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::state::AppState;

const DEFAULT_SEARCH_LIMIT: i64 = 20;
const MAX_SEARCH_LIMIT: i64 = 100;
const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 200;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/search", get(search))
        .route("/tracks", get(tracks))
        .route("/tracks/{id}", get(track))
        .route("/albums", get(albums))
        .route("/albums/{id}", get(album))
        .route("/artists", get(artists))
        .route("/artists/{id}", get(artist))
        .route("/art/{hash}", get(cover_art))
        .route("/library/rescan", post(rescan))
        .route("/library/artist-info", post(artist_info))
        .route("/library/status", get(status))
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    limit: Option<i64>,
}

async fn search(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<SearchResults>> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_SEARCH_LIMIT)
        .clamp(1, MAX_SEARCH_LIMIT);
    Ok(Json(state.store.search(&query.q, limit).await?))
}

#[derive(Deserialize)]
struct PageQuery {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
    #[serde(default)]
    sort: TrackSort,
}

impl PageQuery {
    fn bounds(&self) -> (i64, i64) {
        (
            self.offset.max(0),
            self.limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE),
        )
    }
}

async fn artists(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Page<ArtistSummary>>> {
    let (offset, limit) = page.bounds();
    Ok(Json(state.store.artists_page(offset, limit).await?))
}

async fn albums(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Page<Album>>> {
    let (offset, limit) = page.bounds();
    Ok(Json(state.store.albums_page(offset, limit).await?))
}

async fn tracks(
    _: CurrentUser,
    State(state): State<AppState>,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<Page<Track>>> {
    let (offset, limit) = page.bounds();
    Ok(Json(
        state.store.tracks_page(page.sort, offset, limit).await?,
    ))
}

async fn track(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Track>> {
    state
        .store
        .track(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn album(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<AlbumDetail>> {
    state
        .store
        .album(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

async fn artist(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<ArtistDetail>> {
    state
        .store
        .artist(id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
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

#[derive(Serialize)]
struct LibraryStatus {
    scanning: bool,
    last_scan: Option<ScanProgress>,
    tracks: i64,
}

async fn status(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<LibraryStatus>> {
    Ok(Json(LibraryStatus {
        scanning: state.scan.running.load(Ordering::Relaxed),
        last_scan: state.scan.last.lock().ok().and_then(|l| l.clone()),
        tracks: state.store.track_count().await?,
    }))
}

async fn rescan(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    if start_scan(&state) {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CONFLICT
    }
}

#[derive(Deserialize, Default)]
struct LookupRequest {
    /// Look up every artist again, not just new or stale ones.
    #[serde(default)]
    force: bool,
}

/// Looks up artist photos and bios online now (admin).
async fn artist_info(
    _: AdminUser,
    State(state): State<AppState>,
    body: Option<Json<LookupRequest>>,
) -> StatusCode {
    let force = body.map(|Json(b)| b.force).unwrap_or_default();
    if start_artist_lookup(&state, force) {
        StatusCode::ACCEPTED
    } else {
        StatusCode::CONFLICT
    }
}

/// Starts an online artist lookup in the background. Returns false if one
/// is already running.
pub fn start_artist_lookup(state: &AppState, force: bool) -> bool {
    if state.scan.lookup_running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        tracing::info!("looking up artist photos and bios online");
        let result =
            djinn_library::online::lookup_artists(&state.store, &state.art_dir, force).await;
        state.scan.lookup_running.store(false, Ordering::Release);
        match result {
            Ok(summary) => {
                tracing::info!(
                    checked = summary.checked,
                    photos = summary.photos,
                    bios = summary.bios,
                    "artist lookup finished"
                );
                let _ = state.events.send(ServerEvent::LibraryChanged);
            }
            Err(e) => tracing::error!(error = %e, "artist lookup failed"),
        }
    });
    true
}

/// Starts a library scan in the background. Returns false if one is
/// already running.
pub fn start_scan(state: &AppState) -> bool {
    if state.scan.running.swap(true, Ordering::AcqRel) {
        return false;
    }
    let state = state.clone();
    tokio::spawn(async move {
        tracing::info!("checking the library for changes");
        let started = std::time::Instant::now();
        let progress_state = state.clone();
        let roots = match crate::folders::library_roots(&state).await {
            Ok(roots) => roots,
            Err(e) => {
                tracing::error!(error = %format!("{e:#}"), "reading library folders failed");
                state.scan.running.store(false, Ordering::Release);
                return;
            }
        };
        let result = djinn_library::scan(&state.store, &roots, &state.art_dir, move |progress| {
            if let Ok(mut last) = progress_state.scan.last.lock() {
                *last = Some(progress.clone());
            }
            let _ = progress_state
                .events
                .send(ServerEvent::ScanProgress(progress.clone()));
        })
        .await;
        state.scan.running.store(false, Ordering::Release);
        match result {
            Ok(summary) => {
                tracing::info!(
                    found = summary.found,
                    read = summary.read,
                    removed = summary.removed,
                    secs = started.elapsed().as_secs_f32(),
                    "library check finished"
                );
                let _ = state.events.send(ServerEvent::LibraryChanged);
                if crate::jukebox::setting(&state.store, crate::jukebox::ONLINE_INFO, 0)
                    .await
                    .is_ok_and(|v| v == 1)
                {
                    start_artist_lookup(&state, false);
                }
                // New tracks may have joined albums and artists in the
                // active playlist.
                if let Err(e) = state.jukebox.lock().await.auto_mix_changed().await {
                    tracing::error!(error = %format!("{e:#}"), "refreshing the queue failed");
                }
            }
            Err(e) => tracing::error!(error = %e, "library scan failed"),
        }
    });
    true
}
