//! Admin > Users beyond the account list: roles, sign-up settings, invite
//! links, and signed-in devices (also for each user's own).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use djinn_store::{AccountRole, Invite, NewInvite, Permissions, RoleError, SessionInfo};
use serde::{Deserialize, Serialize};

use crate::auth::{
    self, new_token, signup_mode, signup_role, AdminUser, CurrentSession, SignupMode, SIGNUP_MODE,
    SIGNUP_ROLE,
};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const MAX_ROLE_NAME: usize = 32;
const MAX_NOTE: usize = 100;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/roles", get(list_roles).post(create_role))
        .route("/roles/{id}", delete(delete_role).put(update_role))
        .route("/signup", get(get_signup).put(put_signup))
        .route("/invites", get(list_invites).post(create_invite))
        .route("/invites/{id}", delete(delete_invite))
        .route(
            "/users/{id}/sessions",
            get(user_sessions).delete(end_user_sessions),
        )
        .route("/users/{id}/sessions/{session}", delete(end_user_session))
        .route(
            "/me/sessions",
            get(my_sessions).delete(end_my_other_sessions),
        )
        .route("/me/sessions/{session}", delete(end_my_session))
        .route(
            "/me/app-password",
            get(app_password)
                .post(new_app_password)
                .delete(remove_app_password),
        )
}

async fn list_roles(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<AccountRole>>> {
    Ok(Json(state.store.roles().await?))
}

#[derive(Deserialize)]
struct RoleBody {
    name: String,
    #[serde(flatten)]
    permissions: Permissions,
}

fn role_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_ROLE_NAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "pick a role name of 1 to {MAX_ROLE_NAME} characters"
        )));
    }
    if name.eq_ignore_ascii_case("admin") || name.eq_ignore_ascii_case("guest") {
        return Err(ApiError::bad_request(
            "Admin and Guest are built in already",
        ));
    }
    Ok(name.to_owned())
}

fn checked(mut p: Permissions) -> ApiResult<Permissions> {
    if !(0..=100).contains(&p.max_streams) {
        return Err(ApiError::bad_request("streams at once must be 0 to 100"));
    }
    // Without GO the rest of GO means nothing; keep them consistent.
    if !p.go {
        p.go_remote = false;
    }
    if !p.submit_music {
        p.submit_without_review = false;
    }
    Ok(p)
}

fn role_error(e: RoleError) -> ApiError {
    match e {
        RoleError::Taken => ApiError::conflict(e.to_string()),
        RoleError::Builtin => ApiError::bad_request(e.to_string()),
        RoleError::Store(e) => e.into(),
    }
}

async fn create_role(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<RoleBody>,
) -> ApiResult<(StatusCode, Json<AccountRole>)> {
    let name = role_name(&body.name)?;
    let role = state
        .store
        .create_role(&name, &checked(body.permissions)?)
        .await
        .map_err(role_error)?;
    Ok((StatusCode::CREATED, Json(role)))
}

async fn update_role(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<RoleBody>,
) -> ApiResult<Json<AccountRole>> {
    let name = role_name(&body.name)?;
    if !state
        .store
        .update_role(id, &name, &checked(body.permissions)?)
        .await
        .map_err(role_error)?
    {
        return Err(ApiError::not_found());
    }
    let role = state
        .store
        .role(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(role))
}

async fn delete_role(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    // Sign-ups set to a deleted role go to Listener (see `signup_role`).
    if !state.store.delete_role(id).await.map_err(role_error)? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, Deserialize)]
struct Signup {
    mode: SignupMode,
    role_id: i64,
}

async fn get_signup(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Signup>> {
    Ok(Json(Signup {
        mode: signup_mode(&state.store).await?,
        role_id: signup_role(&state.store).await?,
    }))
}

async fn put_signup(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Signup>,
) -> ApiResult<Json<Signup>> {
    if state.store.role(body.role_id).await?.is_none() {
        return Err(ApiError::bad_request("that role doesn't exist"));
    }
    state
        .store
        .set_setting(SIGNUP_MODE, body.mode.as_str())
        .await?;
    state
        .store
        .set_setting(SIGNUP_ROLE, &body.role_id.to_string())
        .await?;
    tracing::info!(mode = ?body.mode, role = body.role_id, "sign-ups changed");
    Ok(Json(body))
}

async fn list_invites(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Invite>>> {
    Ok(Json(state.store.invites().await?))
}

#[derive(Deserialize)]
struct InviteBody {
    role_id: i64,
    #[serde(default)]
    note: String,
    /// None: no limit.
    uses: Option<i64>,
    /// None: never expires.
    days: Option<i64>,
}

#[derive(Serialize)]
struct NewInviteLink {
    #[serde(flatten)]
    invite: Invite,
    /// The token for the link. Only shown now; DJINN keeps just its hash.
    token: String,
}

async fn create_invite(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<InviteBody>,
) -> ApiResult<(StatusCode, Json<NewInviteLink>)> {
    if state.store.role(body.role_id).await?.is_none() {
        return Err(ApiError::bad_request("that role doesn't exist"));
    }
    if body.uses.is_some_and(|n| !(1..=1000).contains(&n)) {
        return Err(ApiError::bad_request("uses must be 1 to 1000"));
    }
    if body.days.is_some_and(|n| !(1..=365).contains(&n)) {
        return Err(ApiError::bad_request("an invite lasts 1 to 365 days"));
    }
    let note: String = body.note.trim().chars().take(MAX_NOTE).collect();
    let token = new_token()?;
    let invite = state
        .store
        .create_invite(&NewInvite {
            token_hash: &auth::hash_token(&token),
            role_id: body.role_id,
            note: &note,
            uses_left: body.uses,
            expires_at: body.days.map(|d| djinn_store::now_ms() + d * DAY_MS),
            created_by: admin.id,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(NewInviteLink { invite, token })))
}

async fn delete_invite(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_invite(id).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// A signed-in device, marked when it's the one asking.
#[derive(Serialize)]
struct Device {
    #[serde(flatten)]
    session: SessionInfo,
    current: bool,
}

fn devices(sessions: Vec<SessionInfo>, current: Option<i64>) -> Vec<Device> {
    sessions
        .into_iter()
        .map(|session| Device {
            current: Some(session.id) == current,
            session,
        })
        .collect()
}

async fn user_sessions(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<Device>>> {
    Ok(Json(devices(state.store.user_sessions(id).await?, None)))
}

/// Signs a user out everywhere.
async fn end_user_sessions(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    state.store.end_other_sessions(id, None).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn end_user_session(
    _: AdminUser,
    State(state): State<AppState>,
    Path((id, session)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    if !state.store.end_session(id, session).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn my_sessions(
    me: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<Device>>> {
    let sessions = state.store.user_sessions(me.user.id).await?;
    Ok(Json(devices(sessions, Some(me.session_id))))
}

/// Signs out every other device.
async fn end_my_other_sessions(
    me: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    state
        .store
        .end_other_sessions(me.user.id, Some(me.session_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn end_my_session(
    me: CurrentSession,
    State(state): State<AppState>,
    Path(session): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.end_session(me.user.id, session).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct AppPassword {
    /// For Subsonic phone apps on DJINN GO; None until one is made.
    password: Option<String>,
}

async fn app_password(
    me: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<Json<AppPassword>> {
    Ok(Json(AppPassword {
        password: state.store.subsonic_key(me.user.id).await?,
    }))
}

/// Makes a new app password, replacing any old one (apps using it stop
/// working until they get the new one).
async fn new_app_password(
    me: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<Json<AppPassword>> {
    if me.user.role == djinn_store::Role::Guest {
        return Err(ApiError::forbidden());
    }
    // 20 letters and digits: typed into phone apps, so no symbols.
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    let password: String = bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(*b) % ALPHABET.len()]))
        .collect();
    state.store.set_subsonic_key(me.user.id, &password).await?;
    tracing::info!(username = %me.user.username, "app password made");
    Ok(Json(AppPassword {
        password: Some(password),
    }))
}

async fn remove_app_password(
    me: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<StatusCode> {
    state.store.delete_subsonic_key(me.user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::Method;
    use djinn_store::Role;

    #[tokio::test]
    async fn roles_invites_and_signups() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let sam = session(&state, "sam", Role::User).await;

        // Only admins manage roles.
        let dj = r#"{"name":"DJ","go":true,"go_remote":true,"max_streams":3,"downloads":false,
            "share_playlists":true,"submit_music":false,"submit_without_review":true,
            "unlimited_requests":true}"#;
        let (status, _) = call(&state, Method::POST, "/api/roles", Some(dj), Some(&sam)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::POST, "/api/roles", Some(dj), Some(&admin)).await;
        assert_eq!(status, StatusCode::CREATED);
        let roles = state.store.roles().await.unwrap();
        let dj = roles.iter().find(|r| r.name == "DJ").unwrap();
        assert!(
            !dj.permissions.submit_without_review,
            "no review-free submitting without submitting"
        );
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/roles",
            Some(r#"{"name":"admin"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Sign-ups are closed by default.
        let jo = r#"{"username":"jo","password":"correct horse"}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/signup", Some(jo), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // An invite link works while closed, into its role, and only once.
        let body = format!(r#"{{"role_id":{},"note":"band","uses":1,"days":7}}"#, dj.id);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/invites",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let invite = state.store.invites().await.unwrap().remove(0);
        assert_eq!((invite.uses_left, invite.note.as_str()), (Some(1), "band"));
        // The token only came back in the response; make a known one instead.
        let token = "known-token";
        state
            .store
            .create_invite(&NewInvite {
                token_hash: &auth::hash_token(token),
                role_id: dj.id,
                note: "",
                uses_left: Some(1),
                expires_at: None,
                created_by: 1,
            })
            .await
            .unwrap();
        let (status, text) = get_body(&state, &format!("/api/auth/invite/{token}"), "").await;
        assert_eq!(status, StatusCode::OK);
        assert!(text.contains("\"DJ\""), "{text}");
        let jo_invited =
            format!(r#"{{"username":"jo","password":"correct horse","invite":"{token}"}}"#);
        let (status, jo_cookie) = call(
            &state,
            Method::POST,
            "/api/auth/signup",
            Some(&jo_invited),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let (_, me) = get_body(&state, "/api/me", &jo_cookie.unwrap()).await;
        assert!(me.contains("\"role_name\":\"DJ\""), "{me}");
        assert!(me.contains("\"unlimited_requests\":true"), "{me}");
        let kim_invited =
            format!(r#"{{"username":"kim","password":"correct horse","invite":"{token}"}}"#);
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/auth/signup",
            Some(&kim_invited),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "used up");

        // Review: the account waits and can't sign in until approved.
        let review = r#"{"mode":"review","role_id":1}"#;
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/signup",
            Some(review),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let lee = r#"{"username":"lee","password":"correct horse"}"#;
        let (status, cookie) =
            call(&state, Method::POST, "/api/auth/signup", Some(lee), None).await;
        assert_eq!((status, cookie), (StatusCode::ACCEPTED, None));
        let (status, _) = call(&state, Method::POST, "/api/auth/login", Some(lee), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "waiting for approval");
        let lee_id = state
            .store
            .user_credentials("lee")
            .await
            .unwrap()
            .unwrap()
            .0
            .id;
        let (status, _) = call(
            &state,
            Method::PATCH,
            &format!("/api/users/{lee_id}"),
            Some(r#"{"status":"active"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, lee_cookie) =
            call(&state, Method::POST, "/api/auth/login", Some(lee), None).await;
        assert_eq!(status, StatusCode::OK);
        let lee_cookie = lee_cookie.unwrap();

        // Signed-in devices: their own, and turning the account off signs out.
        let (status, text) = get_body(&state, "/api/me/sessions", &lee_cookie).await;
        assert_eq!(status, StatusCode::OK);
        assert!(text.contains("\"current\":true"), "{text}");
        let (status, _) = call(
            &state,
            Method::PATCH,
            &format!("/api/users/{lee_id}"),
            Some(r#"{"status":"disabled"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = get_body(&state, "/api/me", &lee_cookie).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // Weak passwords are refused.
        let open = r#"{"mode":"open","role_id":1}"#;
        call(&state, Method::PUT, "/api/signup", Some(open), Some(&admin)).await;
        for weak in ["short", "password123", "zzzzzzzzzz", "maxwell99"] {
            let body = format!(r#"{{"username":"maxwell99","password":"{weak}"}}"#);
            let (status, _) =
                call(&state, Method::POST, "/api/auth/signup", Some(&body), None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{weak}");
        }
        let max = r#"{"username":"maxwell99","password":"blue tractor"}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/signup", Some(max), None).await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "open sign-ups start right away"
        );
    }
}
