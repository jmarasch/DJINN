//! Search history (Admin > Stats) and the wishlist: "Not in the library?"
//! lets people ask for a song; the admin sees the asks, and a library check
//! that adds the song marks it got.

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_store::{SearchCount, Wish};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

/// Setting keys: keep a search history (on unless turned off), and let
/// everyone use "Not in the library?" (otherwise admins only).
pub const SEARCH_HISTORY: &str = "search_history";
pub const WISHLIST_EVERYONE: &str = "wishlist_everyone";
const MAX_FIELD: usize = 200;
const SEARCH_ROWS: i64 = 100;
const STATUSES: [&str; 3] = ["open", "got", "ignored"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/stats/searches", get(searches).delete(clear_searches))
        .route("/wishes", get(list).post(add))
        .route("/wishes/enabled", get(enabled))
        .route("/wishes/{id}", put(update).delete(remove))
}

/// Notes a search in the background, if the history is on.
pub fn log_search(state: &AppState, user_id: i64, query: &str, results: usize) {
    let query = query.trim();
    if query.chars().count() < 2 {
        return;
    }
    let (state, query) = (
        state.clone(),
        query.chars().take(MAX_FIELD).collect::<String>(),
    );
    tokio::spawn(async move {
        if !settings::flag(&state.store, SEARCH_HISTORY, true)
            .await
            .unwrap_or(false)
        {
            return;
        }
        let results = i64::try_from(results).unwrap_or(i64::MAX);
        if let Err(e) = state.store.log_search(Some(user_id), &query, results).await {
            tracing::debug!(error = %e, "noting a search failed");
        }
    });
}

/// After a library check: open wishes the library now has are got.
pub async fn grant_wishes(state: &AppState) -> anyhow::Result<usize> {
    let mut granted = 0;
    for wish in state.store.wishes("open").await? {
        if let Some(id) = state
            .store
            .same_song(&wish.title, &wish.artist, None)
            .await?
        {
            state.store.set_wish(wish.id, "got", Some(id)).await?;
            granted += 1;
        }
    }
    if granted > 0 {
        tracing::info!(granted, "wishes now in the library");
    }
    Ok(granted)
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    since: i64,
    #[serde(default)]
    nothing_found: bool,
}

#[derive(Serialize)]
struct Searches {
    on: bool,
    searches: Vec<SearchCount>,
}

async fn searches(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Searches>> {
    Ok(Json(Searches {
        on: settings::flag(&state.store, SEARCH_HISTORY, true).await?,
        searches: state
            .store
            .search_counts(query.since, query.nothing_found, SEARCH_ROWS)
            .await?,
    }))
}

async fn clear_searches(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.clear_search_log().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Whether this person gets "Not in the library?".
async fn allowed(state: &AppState, user: &djinn_store::User) -> anyhow::Result<bool> {
    Ok(user.role == djinn_store::Role::Admin
        || settings::flag(&state.store, WISHLIST_EVERYONE, false).await?)
}

async fn enabled(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<bool>> {
    Ok(Json(allowed(&state, &user).await?))
}

#[derive(Deserialize)]
struct NewWish {
    artist: String,
    title: String,
}

async fn add(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<NewWish>,
) -> ApiResult<(StatusCode, Json<i64>)> {
    if !allowed(&state, &user).await? {
        return Err(ApiError::forbidden());
    }
    let (artist, title) = (body.artist.trim(), body.title.trim());
    if title.is_empty() || artist.is_empty() {
        return Err(ApiError::bad_request("give the artist and the song"));
    }
    if artist.chars().count() > MAX_FIELD || title.chars().count() > MAX_FIELD {
        return Err(ApiError::bad_request("that's too long"));
    }
    let id = state.store.add_wish(user.id, artist, title).await?;
    Ok((StatusCode::CREATED, Json(id)))
}

#[derive(Deserialize)]
struct ListQuery {
    status: Option<String>,
}

async fn list(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<Wish>>> {
    let status = query.status.as_deref().unwrap_or("open");
    if !STATUSES.contains(&status) {
        return Err(ApiError::bad_request("unknown status"));
    }
    Ok(Json(state.store.wishes(status).await?))
}

#[derive(Deserialize)]
struct Update {
    status: String,
}

async fn update(
    _: AdminUser,
    State(state): State<AppState>,
    RoutePath(id): RoutePath<i64>,
    Json(body): Json<Update>,
) -> ApiResult<StatusCode> {
    if !STATUSES.contains(&body.status.as_str()) {
        return Err(ApiError::bad_request("unknown status"));
    }
    if !state.store.set_wish(id, &body.status, None).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    RoutePath(id): RoutePath<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_wish(id).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::{Role, ScannedTrack};

    #[tokio::test]
    async fn wishes_need_permission_and_get_granted() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let listener = session(&state, "lu", Role::User).await;
        let wish = Some(r#"{"artist":"Band","title":"Song"}"#);

        let (status, _) = call(&state, Method::POST, "/api/wishes", wish, Some(&listener)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (_, body) = get_body(&state, "/api/wishes/enabled", &listener).await;
        assert_eq!(body, "false");
        settings::set_flag(&state.store, WISHLIST_EVERYONE, true)
            .await
            .unwrap();
        let (status, _) = call(&state, Method::POST, "/api/wishes", wish, Some(&listener)).await;
        assert_eq!(status, StatusCode::CREATED);

        let (_, body) = get_body(&state, "/api/wishes", &admin).await;
        assert!(body.contains("\"askers\":1"), "{body}");

        state
            .store
            .save_scanned(&[ScannedTrack {
                path: "/m/song.mp3".into(),
                title: "Song".into(),
                artist: Some("Band".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        assert_eq!(grant_wishes(&state).await.unwrap(), 1);
        let (_, body) = get_body(&state, "/api/wishes?status=got", &admin).await;
        assert!(body.contains("\"title\":\"Song\""), "{body}");
    }

    #[tokio::test]
    async fn searches_are_listed_for_admins() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        state.store.log_search(None, "zappa", 0).await.unwrap();
        let (status, body) =
            get_body(&state, "/api/stats/searches?nothing_found=true", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("zappa") && body.contains("\"on\":true"),
            "{body}"
        );
        let (status, _) = call(
            &state,
            Method::DELETE,
            "/api/stats/searches",
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
}
