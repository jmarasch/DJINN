//! Rooms for people and admins: the list and switching rooms (everyone),
//! and making, renaming, deleting and linking rooms and placing speakers
//! (admins). Every change re-syncs the players (`rooms_rt::sync`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::{Room, RoomError};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/rooms", get(list).post(create))
        .route("/rooms/{id}", axum::routing::patch(rename).delete(remove))
        .route("/rooms/{id}/link", put(link))
        .route("/me/room", put(choose))
        .route("/outputs/{id}/room", put(place_output))
        .route("/rooms/sync", post(resync))
}

fn room_error(e: RoomError) -> ApiError {
    match e {
        RoomError::NameTaken => ApiError::conflict("a room with that name already exists"),
        RoomError::NoName => ApiError::bad_request("the room needs a name"),
        RoomError::MainStays => ApiError::bad_request("Main can't be deleted"),
        RoomError::NotFound => ApiError::not_found_with("there's no such room"),
        RoomError::Store(e) => e.into(),
    }
}

#[derive(Serialize)]
struct RoomList {
    rooms: Vec<Room>,
    /// The room this person is in.
    mine: i64,
}

async fn list(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<RoomList>> {
    Ok(Json(RoomList {
        rooms: state.store.rooms().await?,
        mine: state.room_of(user.id).await,
    }))
}

#[derive(Deserialize)]
struct Choose {
    room_id: i64,
}

/// Moves this person to another room (guests too).
async fn choose(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Choose>,
) -> ApiResult<StatusCode> {
    state
        .store
        .set_user_room(user.id, body.room_id)
        .await
        .map_err(room_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Named {
    name: String,
}

async fn create(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Named>,
) -> ApiResult<(StatusCode, Json<Room>)> {
    let name: String = body.name.trim().chars().take(40).collect();
    let room = state.store.create_room(&name).await.map_err(room_error)?;
    crate::rooms_rt::sync(&state).await?;
    Ok((StatusCode::CREATED, Json(room)))
}

async fn rename(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Named>,
) -> ApiResult<StatusCode> {
    let name: String = body.name.trim().chars().take(40).collect();
    state
        .store
        .rename_room(id, &name)
        .await
        .map_err(room_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state.store.delete_room(id).await.map_err(room_error)?;
    crate::rooms_rt::sync(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Link {
    /// The room to play along with; null plays its own music again.
    to: Option<i64>,
}

async fn link(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Link>,
) -> ApiResult<StatusCode> {
    state
        .store
        .link_room(id, body.to)
        .await
        .map_err(room_error)?;
    crate::rooms_rt::sync(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Place {
    room_id: i64,
}

/// Puts one of this computer's speakers in a room.
async fn place_output(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Place>,
) -> ApiResult<StatusCode> {
    if state.store.output(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    state
        .store
        .set_output_room(id, body.room_id)
        .await
        .map_err(room_error)?;
    crate::rooms_rt::apply_outputs(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn resync(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    crate::rooms_rt::sync(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn people_switch_rooms_and_admins_manage_them() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "boss", Role::Admin).await;
        let sam = session(&state, "sam", Role::User).await;
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/rooms",
            Some(r#"{"name":"Patio"}"#),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "only admins make rooms");
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/rooms",
            Some(r#"{"name":"Patio"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/rooms",
            Some(r#"{"name":"patio"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        let patio = state.store.rooms().await.unwrap()[1].id;

        let (_, body) = get_body(&state, "/api/rooms", &sam).await;
        assert!(
            body.contains("Patio") && body.contains(r#""mine":1"#),
            "{body}"
        );
        let choose = format!(r#"{{"room_id":{patio}}}"#);
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/me/room",
            Some(&choose),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, body) = get_body(&state, "/api/rooms", &sam).await;
        assert!(body.contains(&format!(r#""mine":{patio}"#)), "{body}");
        // The queue they see is the patio's.
        let (status, body) = get_body(&state, "/api/queue", &sam).await;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "[]"));

        let link = format!("/api/rooms/{patio}/link");
        let (status, _) = call(
            &state,
            Method::PUT,
            &link,
            Some(r#"{"to":1}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(std::sync::Arc::ptr_eq(
            &state.jukebox_for(patio),
            &state.jukebox
        ));
        let (status, _) = call(&state, Method::DELETE, "/api/rooms/1", None, Some(&admin)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "Main stays");
        let (status, _) = call(
            &state,
            Method::DELETE,
            &format!("/api/rooms/{patio}"),
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
}
