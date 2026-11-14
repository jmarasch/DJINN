//! The library editor's album layout: releases to compare an album's songs
//! with (every MusicBrainz release of the album, or a search on MusicBrainz,
//! Deezer or Apple), a release's track list with the songs matched to it,
//! and its cover. Nothing is changed here: the layout's choices go back to
//! the grid as changes to approve. Behind the protected areas' lock.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::releases::{self, Release, ReleaseHit, Source};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};
use crate::protected::Elevated;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/editor/releases", get(album_releases))
        .route("/editor/release-search", get(search))
        .route("/editor/release", get(release))
        .route("/editor/release-cover", get(cover))
}

fn lookup_failed(e: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, e.to_string())
}

async fn contact(state: &AppState) -> ApiResult<Option<String>> {
    Ok(crate::settings::musicbrainz(&state.store).await?)
}

#[derive(Deserialize)]
struct AlbumQuery {
    album_id: i64,
}

#[derive(Serialize)]
struct AlbumReleases {
    /// MusicBrainz knows the album (a library check looked it up).
    known: bool,
    releases: Vec<ReleaseHit>,
}

/// Every release MusicBrainz has of this album.
async fn album_releases(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<AlbumQuery>,
) -> ApiResult<Json<AlbumReleases>> {
    let (Some(group), Some(contact)) = (
        state.store.album_mbid(query.album_id).await?,
        contact(&state).await?,
    ) else {
        return Ok(Json(AlbumReleases {
            known: false,
            releases: Vec::new(),
        }));
    };
    let releases = releases::releases_of_group(&contact, &group)
        .await
        .map_err(lookup_failed)?;
    Ok(Json(AlbumReleases {
        known: true,
        releases,
    }))
}

#[derive(Deserialize)]
struct SearchQuery {
    source: Source,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: String,
}

async fn search(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Vec<ReleaseHit>>> {
    let clip = |t: &str| t.chars().take(200).collect::<String>();
    let contact = contact(&state).await?;
    let hits = releases::search(
        query.source,
        contact.as_deref(),
        &clip(&query.artist),
        &clip(&query.album),
    )
    .await
    .map_err(lookup_failed)?;
    Ok(Json(hits))
}

#[derive(Deserialize)]
struct ReleaseQuery {
    source: Source,
    id: String,
    /// The songs to lay out on it, by id, joined with commas.
    #[serde(default)]
    ids: String,
}

#[derive(Serialize)]
struct Matched {
    id: i64,
    /// The track (index in `tracks`) it matched.
    track: usize,
}

#[derive(Serialize)]
struct Laid {
    release: Release,
    matched: Vec<Matched>,
}

/// A release's tracks, with the songs matched to them by title.
async fn release(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<ReleaseQuery>,
) -> ApiResult<Json<Laid>> {
    let contact = contact(&state).await?;
    let found = releases::release(query.source, contact.as_deref(), &query.id)
        .await
        .map_err(lookup_failed)?
        .ok_or_else(ApiError::not_found)?;
    let ids: Vec<i64> = query
        .ids
        .split(',')
        .filter_map(|i| i.trim().parse().ok())
        .take(2_000)
        .collect();
    let mut songs = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(track) = state.store.track(id).await? {
            songs.push((id, track.title, track.duration_ms));
        }
    }
    let matched = releases::match_songs(&found, &songs)
        .into_iter()
        .map(|(id, track)| Matched { id, track })
        .collect();
    Ok(Json(Laid {
        release: found,
        matched,
    }))
}

#[derive(Deserialize)]
struct CoverQuery {
    source: Source,
    id: String,
}

/// A release's front cover, fetched by DJINN (so the page needs no other site).
async fn cover(_: Elevated, Query(query): Query<CoverQuery>) -> ApiResult<Response> {
    let bytes = releases::cover(query.source, &query.id)
        .await
        .map_err(lookup_failed)?
        .ok_or_else(ApiError::not_found)?;
    let kind = if bytes.starts_with(&[0x89, b'P']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Ok((
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        bytes,
    )
        .into_response())
}
