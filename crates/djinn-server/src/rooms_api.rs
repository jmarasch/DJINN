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
        .route("/rooms/{id}/quiet", get(get_quiet).put(put_quiet))
        .route("/rooms/{id}/rules", get(get_rules).put(put_rules))
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

async fn get_quiet(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<crate::rooms_rt::QuietRules>> {
    Ok(Json(crate::rooms_rt::quiet_rules(&state.store, id).await?))
}

async fn put_quiet(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<crate::rooms_rt::QuietRules>,
) -> ApiResult<StatusCode> {
    if id == djinn_store::MAIN_ROOM || state.store.room(id).await?.is_none() {
        return Err(ApiError::bad_request(
            "only rooms other than Main can follow Main",
        ));
    }
    let key = |name: &str| crate::jukebox::room_key(id, name);
    state
        .store
        .set_setting(
            &key(crate::rooms_rt::FOLLOW_MAIN),
            if body.follow_main { "1" } else { "0" },
        )
        .await?;
    let takeover = if body.takeover == "after_song" {
        "after_song"
    } else {
        "now"
    };
    state
        .store
        .set_setting(&key(crate::rooms_rt::TAKEOVER), takeover)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A room's own queue rules; a room without its own uses Main's.
#[derive(Serialize, Deserialize)]
struct RoomRules {
    /// Most requests the room's queue holds; 0 for no cap.
    max_queue: i64,
    /// Requests each person may have waiting in this room.
    per_user_limit: i64,
    /// Votes that skip a song; 0 turns skip voting off.
    skip_votes: i64,
}

async fn get_rules(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<RoomRules>> {
    use crate::jukebox::{
        room_number, DEFAULT_PER_USER_LIMIT, MAX_QUEUE, PER_USER_LIMIT, SKIP_VOTES,
    };
    Ok(Json(RoomRules {
        max_queue: room_number(&state.store, id, MAX_QUEUE, 0).await?,
        per_user_limit: room_number(&state.store, id, PER_USER_LIMIT, DEFAULT_PER_USER_LIMIT)
            .await?,
        skip_votes: room_number(&state.store, id, SKIP_VOTES, 0).await?,
    }))
}

async fn put_rules(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<RoomRules>,
) -> ApiResult<StatusCode> {
    use crate::jukebox::{room_key, MAX_QUEUE, PER_USER_LIMIT, SKIP_VOTES};
    if state.store.room(id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    if !(0..=1000).contains(&body.max_queue)
        || !(0..=100).contains(&body.per_user_limit)
        || !(0..=100).contains(&body.skip_votes)
    {
        return Err(ApiError::bad_request("those rules are out of range"));
    }
    for (key, value) in [
        (MAX_QUEUE, body.max_queue),
        (PER_USER_LIMIT, body.per_user_limit),
        (SKIP_VOTES, body.skip_votes),
    ] {
        state
            .store
            .set_setting(&room_key(id, key), &value.to_string())
            .await?;
    }
    state
        .jukebox_for(id)
        .lock()
        .await
        .set_skip_votes_needed(u32::try_from(body.skip_votes).unwrap_or(0));
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

    #[tokio::test]
    async fn a_room_can_have_its_own_rules() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "boss", Role::Admin).await;
        let sam = session(&state, "sam", Role::User).await;
        let songs: Vec<djinn_store::ScannedTrack> = ["a", "b", "c"]
            .iter()
            .map(|n| djinn_store::ScannedTrack {
                path: format!("/m/{n}.flac"),
                size: 1,
                mtime: 1,
                title: n.to_string(),
                ..Default::default()
            })
            .collect();
        state.store.save_scanned(&songs).await.unwrap();
        call(
            &state,
            Method::POST,
            "/api/rooms",
            Some(r#"{"name":"Patio"}"#),
            Some(&admin),
        )
        .await;
        let patio = state.store.rooms().await.unwrap()[1].id;
        let (_, body) = get_body(&state, &format!("/api/rooms/{patio}/rules"), &admin).await;
        assert!(
            body.contains(r#""per_user_limit":3"#),
            "Main's by default: {body}"
        );
        let rules = r#"{"max_queue":0,"per_user_limit":1,"skip_votes":0}"#;
        let (status, _) = call(
            &state,
            Method::PUT,
            &format!("/api/rooms/{patio}/rules"),
            Some(rules),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // A jukebox for the patio with no engine, so nothing leaves its queue.
        let mut patio_box = crate::jukebox::Jukebox::for_room(
            state.store.clone(),
            patio,
            Box::new(|_| {}),
            tokio::sync::broadcast::channel(16).0,
        )
        .await
        .unwrap();
        let sam_user = state
            .store
            .users()
            .await
            .unwrap()
            .into_iter()
            .find(|u| u.username == "sam")
            .unwrap();
        let ids: Vec<i64> = state
            .store
            .tracks_by_ids(&[1, 2, 3])
            .await
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        let origin = djinn_store::RequestOrigin::default();
        assert!(patio_box
            .add_request(&sam_user, ids[0], &origin)
            .await
            .is_ok());
        assert!(
            patio_box
                .add_request(&sam_user, ids[1], &origin)
                .await
                .is_err(),
            "one each in the patio"
        );
        // Main keeps its own limit of three.
        let mut main_box = crate::jukebox::Jukebox::new(
            state.store.clone(),
            Box::new(|_| {}),
            tokio::sync::broadcast::channel(16).0,
        )
        .await
        .unwrap();
        assert!(main_box
            .add_request(&sam_user, ids[1], &origin)
            .await
            .is_ok());
        assert!(main_box
            .add_request(&sam_user, ids[2], &origin)
            .await
            .is_ok());
        let _ = sam;
    }
}
