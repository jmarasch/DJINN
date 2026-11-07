//! Favorites in the party app: hearts on songs, albums and artists, and
//! each person's list. The same likes as DJINN GO's. Guests have none.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_store::{LikeKind, Liked, LikedIds};

use crate::error::ApiResult;
use crate::playlist_api::Member;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/likes", get(liked))
        .route("/likes/ids", get(liked_ids))
        .route("/likes/{kind}/{id}", put(like).delete(unlike))
}

async fn liked(Member(me): Member, State(state): State<AppState>) -> ApiResult<Json<Liked>> {
    Ok(Json(state.store.liked(me.id).await?))
}

async fn liked_ids(Member(me): Member, State(state): State<AppState>) -> ApiResult<Json<LikedIds>> {
    Ok(Json(state.store.liked_ids(me.id).await?))
}

async fn like(
    Member(me): Member,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.id, kind, id, true).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlike(
    Member(me): Member,
    State(state): State<AppState>,
    Path((kind, id)): Path<(LikeKind, i64)>,
) -> ApiResult<StatusCode> {
    state.store.set_like(me.id, kind, id, false).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::state::AppState;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::{Role, ScannedTrack};

    #[tokio::test]
    async fn people_like_songs_and_guests_cant() {
        let state = AppState::for_tests().await;
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.mp3".into(),
                title: "A".into(),
                artist: Some("Band".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.library_track_ids().await.unwrap()[0];
        let sam = session(&state, "sam", Role::User).await;
        let guest = session(&state, "gus", Role::Guest).await;
        let path = format!("/api/likes/track/{id}");

        let (status, _) = call(&state, Method::PUT, &path, None, Some(&guest)).await;
        assert!(status.is_client_error(), "{status}");
        let (status, _) = call(&state, Method::PUT, &path, None, Some(&sam)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/likes/ids", &sam).await;
        assert!(body.contains(&format!("\"tracks\":[{id}]")), "{body}");
        let (_, body) = get_body(&state, "/api/likes", &sam).await;
        assert!(body.contains("\"title\":\"A\""), "{body}");
        let (status, _) = call(&state, Method::DELETE, &path, None, Some(&sam)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // Every song by the artist, for Show more and Play all.
        let artist = state.store.artists_page(0, 10).await.unwrap().items[0].id;
        let (status, body) = get_body(&state, &format!("/api/artists/{artist}/songs"), &sam).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"total\":1"), "{body}");
    }
}
