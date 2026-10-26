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
use crate::events::{LookupStatus, ServerEvent};
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
        .route("/library/open-art-folder", post(open_art_folder))
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

#[derive(Deserialize)]
pub struct ArtQuery {
    /// Thumbnail size in pixels (128 or 512); omit for the original.
    size: Option<u32>,
}

/// Cover art by content hash, optionally as a thumbnail.
async fn cover_art(
    _: CurrentUser,
    State(state): State<AppState>,
    Path(hash): Path<String>,
    Query(query): Query<ArtQuery>,
) -> ApiResult<impl IntoResponse> {
    serve_art(&state, hash, query).await
}

pub async fn serve_art(
    state: &AppState,
    hash: String,
    query: ArtQuery,
) -> ApiResult<impl IntoResponse + use<>> {
    let art_dir = state.art_dir.as_ref().clone();
    let file = tokio::task::spawn_blocking(move || match query.size {
        Some(size) => art::thumbnail(&art_dir, &hash, size),
        None => Ok(art::path(&art_dir, &hash)),
    })
    .await
    .map_err(anyhow::Error::from)?
    .map_err(anyhow::Error::from)?
    .ok_or_else(ApiError::not_found)?;
    let bytes = tokio::fs::read(file).await?;
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
    /// Where cover art and artist photos are cached.
    art_dir: String,
}

async fn status(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<LibraryStatus>> {
    Ok(Json(LibraryStatus {
        scanning: state.scan.running.load(Ordering::Relaxed),
        last_scan: state.scan.last.lock().ok().and_then(|l| l.clone()),
        tracks: state.store.track_count().await?,
        art_dir: state.art_dir.display().to_string(),
    }))
}

/// Opens the image cache in the file manager of the computer DJINN runs
/// on (admin).
async fn open_art_folder(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    std::fs::create_dir_all(state.art_dir.as_ref()).map_err(anyhow::Error::from)?;
    open::that_detached(state.art_dir.as_ref()).map_err(|e| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("couldn't open the folder on this computer: {e}"),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// What a request to start background work did.
#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum StartOutcome {
    /// It started now.
    Started,
    /// One is already running; nothing new was started.
    Running,
    /// One is running; this one runs right after it.
    Queued,
}

#[derive(Serialize)]
struct StartResponse {
    status: StartOutcome,
}

fn started(status: StartOutcome) -> (StatusCode, Json<StartResponse>) {
    let code = match status {
        StartOutcome::Started | StartOutcome::Queued => StatusCode::ACCEPTED,
        StartOutcome::Running => StatusCode::OK,
    };
    (code, Json(StartResponse { status }))
}

async fn rescan(_: AdminUser, State(state): State<AppState>) -> (StatusCode, Json<StartResponse>) {
    started(if start_scan(&state) {
        StartOutcome::Started
    } else {
        StartOutcome::Running
    })
}

#[derive(Deserialize, Default)]
struct LookupRequest {
    /// Look up every artist again, not just new or stale ones.
    #[serde(default)]
    force: bool,
}

/// Looks up artist photos and bios online now (admin). If a lookup is
/// already running, a "recheck all" is queued to run right after it.
async fn artist_info(
    _: AdminUser,
    State(state): State<AppState>,
    body: Option<Json<LookupRequest>>,
) -> (StatusCode, Json<StartResponse>) {
    let force = body.map(|Json(b)| b.force).unwrap_or_default();
    if start_artist_lookup(&state, force) {
        return started(StartOutcome::Started);
    }
    if force {
        state.scan.lookup_force_next.store(true, Ordering::Release);
        return started(StartOutcome::Queued);
    }
    started(StartOutcome::Running)
}

/// Current lookup state, for clients and the SSE snapshot.
pub fn lookup_status(state: &AppState) -> LookupStatus {
    let last = state.scan.lookup_last.lock().ok().and_then(|l| l.clone());
    LookupStatus {
        running: state.scan.lookup_running.load(Ordering::Acquire),
        finished_at: last.as_ref().map(|(_, at)| *at),
        last: last.map(|(summary, _)| summary),
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
        let mut force = force;
        loop {
            let _ = state
                .events
                .send(ServerEvent::ArtistLookup(lookup_status(&state)));
            tracing::info!(force, "looking up artist photos and bios online");
            let contact = state
                .store
                .get_setting(crate::jukebox::MUSICBRAINZ_CONTACT)
                .await
                .ok()
                .flatten()
                .filter(|c| !c.trim().is_empty());
            let options = djinn_library::online::LookupOptions {
                force,
                musicbrainz_contact: contact,
            };
            let result =
                djinn_library::online::lookup_artists(&state.store, &state.art_dir, &options).await;
            match result {
                Ok(summary) => {
                    tracing::info!(
                        checked = summary.checked,
                        photos = summary.photos,
                        bios = summary.bios,
                        covers = summary.covers,
                        genres = summary.genres,
                        "artist lookup finished"
                    );
                    // A run with nothing due keeps the last useful summary.
                    let did_work = summary != Default::default();
                    if let Ok(mut last) = state.scan.lookup_last.lock() {
                        if did_work || last.is_none() {
                            *last = Some((summary, djinn_store::now_ms()));
                        }
                    }
                    let _ = state.events.send(ServerEvent::LibraryChanged);
                }
                Err(e) => tracing::error!(error = %e, "artist lookup failed"),
            }
            // A "recheck all" asked for while this ran goes next.
            if !state.scan.lookup_force_next.swap(false, Ordering::AcqRel) {
                break;
            }
            force = true;
        }
        state.scan.lookup_running.store(false, Ordering::Release);
        let _ = state
            .events
            .send(ServerEvent::ArtistLookup(lookup_status(&state)));
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
                crate::audio_api::start_measuring(&state).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn starting_work_twice_is_not_an_error() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        // Pretend a lookup is already running.
        state.scan.lookup_running.store(true, Ordering::Release);

        let lookup = "/api/library/artist-info";
        let (status, _) = call(
            &state,
            Method::POST,
            lookup,
            Some(r#"{"force":false}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "already running");
        assert!(!state.scan.lookup_force_next.load(Ordering::Acquire));

        let (status, _) = call(
            &state,
            Method::POST,
            lookup,
            Some(r#"{"force":true}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED, "recheck all is queued");
        assert!(state.scan.lookup_force_next.load(Ordering::Acquire));

        state.scan.running.store(true, Ordering::Release);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/library/rescan",
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "already checking for changes");
    }
}
