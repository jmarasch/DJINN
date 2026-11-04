//! Playlists: everyone signed in has their own, except guests. Only the
//! owner changes one or decides who else sees it. Starting one as what plays
//! when nobody has requested anything takes the `auto_mix` permission.

use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use djinn_store::{
    AddEntryError, EntryKind, PlaylistDetail, PlaylistEntry, PlaylistSummary, Role, ShareTarget,
    User,
};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub(crate) const MAX_NAME: usize = 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/playlists", get(list).post(create))
        .route("/playlists/stop", post(stop))
        .route("/playlists/wild/play", post(play_wild))
        .route("/playlists/radio/play", post(play_radio))
        .route("/playlists/from-songs", post(from_songs))
        .route("/playlists/share-targets", get(share_targets))
        .route("/playlists/{id}", get(detail).patch(update).delete(remove))
        .route("/playlists/{id}/play", post(play))
        .route("/playlists/{id}/shares", get(shares))
        .route(
            "/playlists/{id}/shares/{user}",
            put(add_share).delete(remove_share),
        )
        .route("/playlists/{id}/entries", post(add_entry))
        .route(
            "/playlists/{id}/entries/{entry}",
            patch(move_entry).delete(remove_entry),
        )
}

/// Someone signed in who can have playlists: not a guest.
pub(crate) struct Member(pub User);

impl FromRequestParts<AppState> for Member {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        if user.role == Role::Guest {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "guests can't have playlists",
            ));
        }
        Ok(Self(user))
    }
}

/// Checks a playlist is `user`'s to change. One they can only see is
/// forbidden; one they can't even see doesn't exist, as far as they know.
async fn must_own(state: &AppState, user: &User, id: i64) -> ApiResult<()> {
    if state.store.playlist_owner(id).await? == Some(user.id) {
        return Ok(());
    }
    if state.store.playlist_for(user.id, id).await?.is_some() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "only its owner can change a playlist",
        ));
    }
    Err(ApiError::not_found())
}

async fn must_start_auto_mix(state: &AppState, user: &User) -> ApiResult<()> {
    if state.store.permissions(user).await?.auto_mix {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        "your account can't choose what the party plays",
    ))
}

async fn must_share(state: &AppState, user: &User) -> ApiResult<()> {
    if state.store.permissions(user).await?.share_playlists {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::FORBIDDEN,
        "your account can't share playlists",
    ))
}

#[derive(Serialize)]
struct ListItem {
    #[serde(flatten)]
    playlist: PlaylistSummary,
    /// Plays when the request queue is empty.
    active: bool,
    /// Songs in it that can't play.
    problem_songs: i64,
}

async fn list(
    Member(user): Member,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<ListItem>>> {
    let active = state.jukebox.lock().await.active_playlist();
    let playlists = state.store.playlists_for(user.id).await?;
    let problems: HashMap<i64, i64> = state
        .store
        .playlist_problem_counts()
        .await?
        .into_iter()
        .collect();
    Ok(Json(
        playlists
            .into_iter()
            .map(|playlist| ListItem {
                active: Some(playlist.id) == active,
                problem_songs: problems.get(&playlist.id).copied().unwrap_or(0),
                playlist,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct NewPlaylist {
    name: String,
}

async fn create(
    Member(user): Member,
    State(state): State<AppState>,
    Json(body): Json<NewPlaylist>,
) -> ApiResult<(StatusCode, Json<PlaylistSummary>)> {
    let name = validate_name(&body.name)?;
    let playlist = state.store.create_playlist(user.id, &name).await?;
    Ok((StatusCode::CREATED, Json(playlist)))
}

async fn detail(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<PlaylistDetail>> {
    state
        .store
        .playlist_for(user.id, id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

#[derive(Deserialize)]
struct PlaylistUpdate {
    name: Option<String>,
    shuffle: Option<bool>,
    /// Everyone can see and play it.
    shared: Option<bool>,
}

async fn update(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<PlaylistUpdate>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    let name = body.name.as_deref().map(validate_name).transpose()?;
    if let Some(shared) = body.shared {
        if shared {
            must_share(&state, &user).await?;
        }
        state.store.set_playlist_shared(id, shared).await?;
    }
    if !state
        .store
        .update_playlist(id, name.as_deref(), body.shuffle)
        .await?
    {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    if !state.store.delete_playlist(id).await? {
        return Err(ApiError::not_found());
    }
    let mut jukebox = state.jukebox.lock().await;
    if jukebox.active_playlist() == Some(id) {
        jukebox.stop_auto_mix().await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Play {
    /// Start from this song instead of the playlist's next one.
    from_track_id: Option<i64>,
}

/// Makes the playlist the one that plays and starts it now.
async fn play(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Play>,
) -> ApiResult<StatusCode> {
    must_start_auto_mix(&state, &user).await?;
    if state.store.playlist_for(user.id, id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    state
        .jukebox
        .lock()
        .await
        .play_playlist(id, body.from_track_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct FromSongs {
    name: String,
    track_ids: Vec<i64>,
}

/// Makes a playlist out of a list of songs (search results by
/// description, say).
async fn from_songs(
    Member(user): Member,
    State(state): State<AppState>,
    Json(body): Json<FromSongs>,
) -> ApiResult<(StatusCode, Json<PlaylistSummary>)> {
    let name = validate_name(&body.name)?;
    if body.track_ids.is_empty() || body.track_ids.len() > 500 {
        return Err(ApiError::bad_request("pick 1 to 500 songs"));
    }
    let playlist = state.store.create_playlist(user.id, &name).await?;
    for &id in &body.track_ids {
        match state
            .store
            .add_playlist_entry(playlist.id, EntryKind::Track, id)
            .await
        {
            Ok(_) | Err(AddEntryError::AlreadyIn) | Err(AddEntryError::NotFound) => {}
            Err(e) => return Err(anyhow::Error::from(e).into()),
        }
    }
    let id = playlist.id;
    let summary = state.store.playlist_summary(id).await?.unwrap_or(playlist);
    Ok((StatusCode::CREATED, Json(summary)))
}

/// Wild play: random songs from the whole library when the requests run
/// out.
async fn play_wild(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state
        .jukebox
        .lock()
        .await
        .play_playlist(crate::jukebox::WILD_PLAY, None)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Radio from a song, an artist, or a genre: songs like it when the
/// requests run out, steered by what the party plays and skips.
async fn play_radio(
    _: AdminUser,
    State(state): State<AppState>,
    Json(seed): Json<crate::jukebox::RadioSeed>,
) -> ApiResult<StatusCode> {
    use crate::jukebox::RadioSeed;
    let name = match &seed {
        RadioSeed::Track { id } => state.store.track(*id).await?.map(|t| t.title),
        RadioSeed::Artist { id } => state.store.artist(*id).await?.map(|a| a.artist.name),
        RadioSeed::Genre { name } => {
            Some(djinn_library::vibe::normalize(name)).filter(|n| !n.is_empty())
        }
    }
    .ok_or_else(ApiError::not_found)?;
    state.jukebox.lock().await.start_radio(seed, name).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// No playlist plays when the requests run out.
async fn stop(Member(user): Member, State(state): State<AppState>) -> ApiResult<StatusCode> {
    must_start_auto_mix(&state, &user).await?;
    state.jukebox.lock().await.stop_auto_mix().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewEntry {
    kind: EntryKind,
    ref_id: i64,
}

async fn add_entry(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<NewEntry>,
) -> ApiResult<(StatusCode, Json<PlaylistEntry>)> {
    must_own(&state, &user, id).await?;
    let entry = match state
        .store
        .add_playlist_entry(id, body.kind, body.ref_id)
        .await
    {
        Ok(entry) => entry,
        Err(AddEntryError::NotFound) => return Err(ApiError::not_found()),
        Err(e @ AddEntryError::AlreadyIn) => return Err(ApiError::conflict(e.to_string())),
        Err(AddEntryError::Store(e)) => return Err(e.into()),
    };
    refresh_if_active(&state, id).await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

#[derive(Deserialize)]
struct MoveEntry {
    position: usize,
}

async fn move_entry(
    Member(user): Member,
    State(state): State<AppState>,
    Path((id, entry)): Path<(i64, i64)>,
    Json(body): Json<MoveEntry>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    if !state
        .store
        .move_playlist_entry(id, entry, body.position)
        .await?
    {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_entry(
    Member(user): Member,
    State(state): State<AppState>,
    Path((id, entry)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    if !state.store.remove_playlist_entry(id, entry).await? {
        return Err(ApiError::not_found());
    }
    refresh_if_active(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Who your playlists can be shared with.
async fn share_targets(
    Member(user): Member,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<ShareTarget>>> {
    must_share(&state, &user).await?;
    Ok(Json(state.store.share_targets(user.id).await?))
}

/// The particular people a playlist of yours is shared with.
async fn shares(
    Member(user): Member,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<ShareTarget>>> {
    must_own(&state, &user, id).await?;
    Ok(Json(state.store.playlist_shares(id).await?))
}

async fn add_share(
    Member(user): Member,
    State(state): State<AppState>,
    Path((id, person)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    must_share(&state, &user).await?;
    if !state.store.share_playlist(id, person).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Stopping sharing is always allowed, even if the account no longer can share.
async fn remove_share(
    Member(user): Member,
    State(state): State<AppState>,
    Path((id, person)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    must_own(&state, &user, id).await?;
    state.store.unshare_playlist(id, person).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The jukebox lines up its next song from the active playlist, so it
/// needs to hear about changes to it.
async fn refresh_if_active(state: &AppState, id: i64) -> ApiResult<()> {
    let mut jukebox = state.jukebox.lock().await;
    if jukebox.active_playlist() == Some(id) {
        jukebox.auto_mix_changed().await?;
    }
    Ok(())
}

pub(crate) fn validate_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "playlist names are 1 to {MAX_NAME} characters"
        )));
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::{Permissions, ScannedTrack};

    async fn id_of(state: &AppState, name: &str) -> i64 {
        state
            .store
            .users()
            .await
            .unwrap()
            .into_iter()
            .find(|u| u.username == name)
            .unwrap()
            .id
    }

    /// Gives a role these permissions and puts `name` in it.
    async fn give(state: &AppState, name: &str, permissions: Permissions) {
        let role = state.store.create_role(name, &permissions).await.unwrap();
        let id = id_of(state, name).await;
        state.store.set_account_role(id, role.id).await.unwrap();
    }

    async fn new_list(state: &AppState, cookie: &str, name: &str) -> i64 {
        let body = format!(r#"{{"name":"{name}"}}"#);
        let (status, _) = call(
            state,
            Method::POST,
            "/api/playlists",
            Some(&body),
            Some(cookie),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (_, list) = get_body(state, "/api/playlists", cookie).await;
        let list: serde_json::Value = serde_json::from_str(&list).unwrap();
        list.as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    }

    #[tokio::test]
    async fn everyone_has_their_own_and_owners_decide_who_sees_them() {
        let state = AppState::for_tests().await;
        let sam = session(&state, "sam", Role::User).await;
        let jo = session(&state, "jo", Role::User).await;
        let admin = session(&state, "root", Role::Admin).await;
        give(
            &state,
            "sam",
            Permissions {
                share_playlists: true,
                ..Default::default()
            },
        )
        .await;
        let list = new_list(&state, &sam, "Road trip").await;
        let path = format!("/api/playlists/{list}");
        let status = |cookie: &str, method: Method, path: String, body: Option<&'static str>| {
            let state = state.clone();
            let cookie = cookie.to_owned();
            async move { call(&state, method, &path, body, Some(&cookie)).await.0 }
        };

        // Private, even from an admin.
        for cookie in [&jo, &admin] {
            assert_eq!(
                get_body(&state, &path, cookie).await.0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                status(cookie, Method::DELETE, path.clone(), None).await,
                StatusCode::NOT_FOUND
            );
        }

        // Shared with one person: they can see it but not change it.
        let jo_id = id_of(&state, "jo").await;
        let share = format!("{path}/shares/{jo_id}");
        assert_eq!(
            status(&jo, Method::PUT, share.clone(), None).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(&sam, Method::PUT, share.clone(), None).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(get_body(&state, &path, &jo).await.0, StatusCode::OK);
        assert_eq!(
            get_body(&state, &path, &admin).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            status(
                &jo,
                Method::PATCH,
                path.clone(),
                Some(r#"{"name":"Mine now"}"#)
            )
            .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            status(&sam, Method::DELETE, share, None).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(get_body(&state, &path, &jo).await.0, StatusCode::NOT_FOUND);

        // Shared with everyone: an account that can't share isn't able to.
        give(&state, "jo", Permissions::default()).await;
        let open = Some(r#"{"shared":true}"#);
        assert_eq!(
            status(&sam, Method::PATCH, path.clone(), open).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(get_body(&state, &path, &admin).await.0, StatusCode::OK);
        let jos = new_list(&state, &jo, "Jo's").await;
        assert_eq!(
            status(&jo, Method::PATCH, format!("/api/playlists/{jos}"), open).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn starting_the_auto_mix_takes_the_permission() {
        let state = AppState::for_tests().await;
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.flac".into(),
                title: "A".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let sam = session(&state, "sam", Role::User).await;
        let jo = session(&state, "jo", Role::User).await;
        let admin = session(&state, "root", Role::Admin).await;
        give(
            &state,
            "jo",
            Permissions {
                auto_mix: true,
                ..Default::default()
            },
        )
        .await;
        let sams = new_list(&state, &sam, "Sam's").await;
        let jos = new_list(&state, &jo, "Jo's").await;
        let play = |list: i64, cookie: &str| {
            let state = state.clone();
            let cookie = cookie.to_owned();
            async move {
                let path = format!("/api/playlists/{list}/play");
                call(&state, Method::POST, &path, Some("{}"), Some(&cookie))
                    .await
                    .0
            }
        };

        assert_eq!(play(sams, &sam).await, StatusCode::FORBIDDEN);
        assert_eq!(state.jukebox.lock().await.active_playlist(), None);
        assert_eq!(play(jos, &jo).await, StatusCode::NO_CONTENT);
        assert_eq!(state.jukebox.lock().await.active_playlist(), Some(jos));
        // The permission doesn't open up other people's private lists.
        assert_eq!(play(sams, &jo).await, StatusCode::NOT_FOUND);
        assert_eq!(play(sams, &admin).await, StatusCode::NOT_FOUND);
        let stop = call(
            &state,
            Method::POST,
            "/api/playlists/stop",
            None,
            Some(&sam),
        )
        .await
        .0;
        assert_eq!(stop, StatusCode::FORBIDDEN);
        let stop = call(&state, Method::POST, "/api/playlists/stop", None, Some(&jo))
            .await
            .0;
        assert_eq!(stop, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn guests_cant_have_playlists() {
        let state = AppState::for_tests().await;
        let guest = session(&state, "guest", Role::Guest).await;
        let body = Some(r#"{"name":"Mine"}"#);
        let (status, _) = call(&state, Method::POST, "/api/playlists", body, Some(&guest)).await;
        assert_ne!(status, StatusCode::CREATED);
        let import = Some(r#"{"text":"Rihanna - S&M"}"#);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/playlists/import",
            import,
            Some(&guest),
        )
        .await;
        assert_ne!(status, StatusCode::CREATED);
        assert!(state.store.playlists().await.unwrap().is_empty());
    }
}
