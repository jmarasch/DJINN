//! The library editor's album layout: releases to compare an album's songs
//! with (every MusicBrainz release of the album, or a search on MusicBrainz,
//! Deezer or Apple), a release's track list with the songs matched to it,
//! and its cover. Nothing is changed here: the layout's choices go back to
//! the grid as changes to approve. Behind the protected areas' lock.
//! Spotify needs the admin's developer app (Admin > Library): its client
//! ID and secret are kept in settings, and the secret is never sent back.

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use djinn_library::releases::Accounts;
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
        .route("/spotify", get(spotify_status).put(set_spotify))
}

/// Setting keys: the Spotify developer app's client ID and secret.
const SPOTIFY_ID: &str = "spotify_client_id";
const SPOTIFY_SECRET: &str = "spotify_client_secret";

/// The accounts the lookups may use: MusicBrainz's contact, Spotify's app.
async fn accounts(state: &AppState) -> ApiResult<Accounts> {
    let id = state
        .store
        .get_setting(SPOTIFY_ID)
        .await?
        .filter(|v| !v.is_empty());
    let secret = state
        .store
        .get_setting(SPOTIFY_SECRET)
        .await?
        .filter(|v| !v.is_empty());
    Ok(Accounts {
        musicbrainz: crate::settings::musicbrainz(&state.store).await?,
        spotify: id.zip(secret),
    })
}

#[derive(Serialize)]
struct SpotifyStatus {
    /// The app's client ID (not secret); None when there's no account.
    client_id: Option<String>,
    has_secret: bool,
}

async fn spotify_status(
    _: crate::auth::AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<SpotifyStatus>> {
    let id = state
        .store
        .get_setting(SPOTIFY_ID)
        .await?
        .filter(|v| !v.is_empty());
    let secret = state
        .store
        .get_setting(SPOTIFY_SECRET)
        .await?
        .filter(|v| !v.is_empty());
    Ok(Json(SpotifyStatus {
        client_id: id,
        has_secret: secret.is_some(),
    }))
}

#[derive(Deserialize)]
struct SpotifyAccount {
    client_id: String,
    /// Empty keeps the saved secret (when the ID stays the same).
    #[serde(default)]
    client_secret: String,
}

fn plain_key(text: &str) -> bool {
    !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Saves the Spotify app after Spotify accepts it; empty removes it.
async fn set_spotify(
    _: crate::auth::AdminUser,
    State(state): State<AppState>,
    Json(body): Json<SpotifyAccount>,
) -> ApiResult<StatusCode> {
    let id = body.client_id.trim();
    let mut secret = body.client_secret.trim().to_owned();
    if id.is_empty() {
        state.store.set_setting(SPOTIFY_ID, "").await?;
        state.store.set_setting(SPOTIFY_SECRET, "").await?;
        return Ok(StatusCode::NO_CONTENT);
    }
    if secret.is_empty() && state.store.get_setting(SPOTIFY_ID).await?.as_deref() == Some(id) {
        secret = state
            .store
            .get_setting(SPOTIFY_SECRET)
            .await?
            .unwrap_or_default();
    }
    if !plain_key(id) || !plain_key(&secret) {
        return Err(ApiError::bad_request(
            "paste the Client ID and the Client Secret from the app's settings on developer.spotify.com",
        ));
    }
    releases::spotify_token(&(id.to_owned(), secret.clone()))
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    state.store.set_setting(SPOTIFY_ID, id).await?;
    state.store.set_setting(SPOTIFY_SECRET, &secret).await?;
    Ok(StatusCode::NO_CONTENT)
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
    let accounts = accounts(&state).await?;
    let hits = releases::search(
        query.source,
        &accounts,
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
    let accounts = accounts(&state).await?;
    let found = releases::release(query.source, &accounts, &query.id)
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
async fn cover(
    _: Elevated,
    State(state): State<AppState>,
    Query(query): Query<CoverQuery>,
) -> ApiResult<Response> {
    let accounts = accounts(&state).await?;
    let bytes = releases::cover(query.source, &accounts, &query.id)
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

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    use crate::state::AppState;
    use crate::test_util::{call, get_body, session};

    #[tokio::test]
    async fn the_spotify_account_is_admin_only_checked_and_its_secret_kept_back() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let guest = session(&state, "guest", Role::Guest).await;
        let (status, _) = get_body(&state, "/api/spotify", &guest).await;
        assert!(status.is_client_error(), "{status}");
        let (status, body) = get_body(&state, "/api/spotify", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("\"client_id\":null") && body.contains("\"has_secret\":false"),
            "{body}"
        );
        // Not what Spotify hands out: refused before Spotify is asked.
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/spotify",
            Some(r#"{"client_id":"abc def","client_secret":"x;y"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        // A saved secret is never sent back; an empty ID removes the account.
        state
            .store
            .set_setting(super::SPOTIFY_ID, "abc123")
            .await
            .unwrap();
        state
            .store
            .set_setting(super::SPOTIFY_SECRET, "s3cr3t")
            .await
            .unwrap();
        let (_, body) = get_body(&state, "/api/spotify", &admin).await;
        assert!(
            body.contains("abc123") && !body.contains("s3cr3t"),
            "{body}"
        );
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/spotify",
            Some(r#"{"client_id":""}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/spotify", &admin).await;
        assert!(body.contains("\"client_id\":null"), "{body}");
    }
}
