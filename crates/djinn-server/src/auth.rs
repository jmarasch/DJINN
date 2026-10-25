//! Accounts, sessions, and the request extractors that enforce them.
//!
//! Sessions are random tokens in an HttpOnly, SameSite=Lax cookie; the
//! database stores only their SHA-256 hash. Passwords are hashed with
//! Argon2id on the blocking pool.

use std::sync::OnceLock;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{AppendHeaders, IntoResponse};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_store::{now_ms, CreateUserError, GuestSummary, Role, User};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::{self, ClientIp};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const COOKIE_NAME: &str = "djinn_session";
const SESSION_DAYS: i64 = 30;
/// Guests stay signed in for a year (renewed whenever they open the app),
/// so they're recognized at the next party.
const GUEST_SESSION_DAYS: i64 = 365;
const MIN_PASSWORD: usize = 6;
const MAX_PASSWORD: usize = 256;
const MAX_USERNAME: usize = 32;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/bootstrap", get(bootstrap_status).post(bootstrap))
        .route("/auth/mode", get(mode))
        .route("/auth/guest", post(join_as_guest))
        .route("/me", get(me))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", delete(delete_user).patch(update_user))
        .route("/guests", get(list_guests))
}

/// The logged-in user. Rejects with 401 when there is no valid session.
pub struct CurrentUser(pub User);

/// A logged-in admin. Rejects with 401 or 403.
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let user = state
            .store
            .session_user(&hash_token(&token))
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        // Turning guest mode off signs guests out.
        if user.role == Role::Guest && !state.guest_mode() {
            return Err(ApiError::unauthorized());
        }
        Ok(Self(user))
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts, state).await?;
        if !user.is_admin() {
            return Err(ApiError::forbidden());
        }
        Ok(Self(user))
    }
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> ApiResult<impl IntoResponse> {
    let found = state.store.user_credentials(body.username.trim()).await?;
    // Verify against a dummy hash for unknown users so response time doesn't
    // reveal which usernames exist.
    let (user, hash) = match found {
        Some((user, hash)) => (Some(user), hash),
        None => (None, dummy_hash().to_owned()),
    };
    let valid = verify_password(body.password, hash).await?;
    let user = user
        .filter(|_| valid)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "wrong username or password"))?;
    let cookie = start_session(&state, user.id, SESSION_DAYS).await?;
    Ok((AppendHeaders([(SET_COOKIE, cookie)]), Json(user)))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    if let Some(token) = session_token(&headers) {
        state.store.delete_session(&hash_token(&token)).await?;
    }
    let clear =
        HeaderValue::from_static("djinn_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    Ok((StatusCode::NO_CONTENT, AppendHeaders([(SET_COOKIE, clear)])))
}

/// The signed-in user. For guests this also renews the session and its
/// cookie for another year.
async fn me(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    let mut renewed = None;
    if user.role == Role::Guest {
        if let Some(token) = session_token(&headers) {
            let max_age = GUEST_SESSION_DAYS * 24 * 60 * 60;
            state
                .store
                .extend_session(&hash_token(&token), now_ms() + max_age * 1000)
                .await?;
            renewed = Some(session_cookie(&token, max_age)?);
        }
    }
    Ok((AppendHeaders(renewed.map(|c| (SET_COOKIE, c))), Json(user)))
}

#[derive(Serialize)]
struct Mode {
    guest_mode: bool,
    setup_needed: bool,
    /// Name this device used last time, so it can rejoin with one tap.
    welcome_back: Option<String>,
}

/// How people get in. Public, for the sign-in screen.
async fn mode(State(state): State<AppState>, ClientIp(ip): ClientIp) -> ApiResult<Json<Mode>> {
    let guest_mode = state.guest_mode();
    let welcome_back = match guest_mode {
        true => match client::mac_for(ip).await {
            Some(mac) => state
                .store
                .guest_by_mac(&mac)
                .await?
                .map(|u| u.name().to_owned()),
            None => None,
        },
        false => None,
    };
    Ok(Json(Mode {
        guest_mode,
        setup_needed: !state.store.admin_exists().await?,
        welcome_back,
    }))
}

#[derive(Deserialize)]
struct GuestJoin {
    name: String,
}

/// Joins with just a name while guest mode is on. A device seen before
/// (by MAC address) keeps its identity under the new name.
async fn join_as_guest(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(body): Json<GuestJoin>,
) -> ApiResult<impl IntoResponse> {
    if !state.guest_mode() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "guest access is off, please sign in",
        ));
    }
    let name = validate_display_name(&body.name)?;
    let mac = client::mac_for(ip).await;
    let ip = ip.map(|ip| ip.to_string());
    let user = state
        .store
        .join_as_guest(&name, mac.as_deref(), ip.as_deref())
        .await?;
    tracing::info!(name = %name, mac = ?mac, ip = ?ip, "guest joined");
    let cookie = start_session(&state, user.id, GUEST_SESSION_DAYS).await?;
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(user),
    ))
}

#[derive(Serialize)]
struct BootstrapStatus {
    needed: bool,
}

async fn bootstrap_status(State(state): State<AppState>) -> ApiResult<Json<BootstrapStatus>> {
    Ok(Json(BootstrapStatus {
        needed: !state.store.admin_exists().await?,
    }))
}

#[derive(Deserialize)]
struct BootstrapRequest {
    token: String,
    username: String,
    password: String,
}

/// Creates the first admin using the one-time token printed at startup.
async fn bootstrap(
    State(state): State<AppState>,
    Json(body): Json<BootstrapRequest>,
) -> ApiResult<impl IntoResponse> {
    let expected = state
        .bootstrap_token
        .lock()
        .map_err(|_| anyhow::anyhow!("bootstrap lock poisoned"))?
        .clone();
    let token_ok = expected
        .as_deref()
        .is_some_and(|t| constant_time_eq(t.as_bytes(), body.token.as_bytes()));
    if !token_ok || state.store.admin_exists().await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "setup link is invalid or already used",
        ));
    }
    let user = new_user(&state, &body.username, body.password, Role::Admin).await?;
    if let Ok(mut token) = state.bootstrap_token.lock() {
        *token = None;
    }
    tracing::info!(username = %user.username, "admin account created");
    let cookie = start_session(&state, user.id, SESSION_DAYS).await?;
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(user),
    ))
}

async fn list_users(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<User>>> {
    Ok(Json(state.store.users().await?))
}

#[derive(Deserialize)]
struct NewUser {
    username: String,
    password: String,
    #[serde(default = "default_role")]
    role: Role,
}

fn default_role() -> Role {
    Role::User
}

async fn create_user(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewUser>,
) -> ApiResult<impl IntoResponse> {
    if body.role == Role::Guest {
        return Err(ApiError::bad_request(
            "guest accounts are not available yet",
        ));
    }
    let user = new_user(&state, &body.username, body.password, body.role).await?;
    Ok((StatusCode::CREATED, Json(user)))
}

async fn delete_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if id == admin.id {
        return Err(ApiError::bad_request("you cannot delete your own account"));
    }
    if !state.store.delete_user(id).await? {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_guests(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<GuestSummary>>> {
    Ok(Json(state.store.guests().await?))
}

#[derive(Deserialize)]
struct UserUpdate {
    unlimited: Option<bool>,
    role: Option<Role>,
    /// New password; signs the user out everywhere.
    password: Option<String>,
}

/// Edits a user: request limit exemption, role, or password. Guests only
/// have the exemption.
async fn update_user(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UserUpdate>,
) -> ApiResult<StatusCode> {
    let user = state
        .store
        .user(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if let Some(role) = body.role.filter(|&r| r != user.role) {
        if user.role == Role::Guest || role == Role::Guest {
            return Err(ApiError::bad_request(
                "guests and accounts can't swap roles",
            ));
        }
        if id == admin.id {
            return Err(ApiError::bad_request("you can't change your own role"));
        }
        if user.role == Role::Admin && state.store.admin_count().await? <= 1 {
            return Err(ApiError::bad_request("there must be at least one admin"));
        }
        state.store.set_role(id, role).await?;
    }
    if let Some(password) = body.password {
        if user.role == Role::Guest {
            return Err(ApiError::bad_request("guests don't have passwords"));
        }
        validate_password(&password)?;
        let hash = hash_password(password).await?;
        state.store.set_password(id, &hash).await?;
    }
    if let Some(unlimited) = body.unlimited {
        state.store.set_unlimited(id, unlimited).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn validate_password(password: &str) -> ApiResult<()> {
    let length = password.chars().count();
    if length < MIN_PASSWORD {
        return Err(ApiError::bad_request(format!(
            "password must be at least {MIN_PASSWORD} characters"
        )));
    }
    if length > MAX_PASSWORD {
        return Err(ApiError::bad_request("password is too long"));
    }
    Ok(())
}

async fn new_user(
    state: &AppState,
    username: &str,
    password: String,
    role: Role,
) -> ApiResult<User> {
    let username = validate_username(username)?;
    validate_password(&password)?;
    let hash = hash_password(password).await?;
    match state.store.create_user(&username, &hash, role).await {
        Ok(user) => Ok(user),
        Err(CreateUserError::Taken) => Err(ApiError::conflict("username is already taken")),
        Err(CreateUserError::Store(e)) => Err(e.into()),
    }
}

fn validate_username(username: &str) -> ApiResult<String> {
    let username = username.trim();
    let valid_chars = username
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.'));
    if username.is_empty() || username.chars().count() > MAX_USERNAME || !valid_chars {
        return Err(ApiError::bad_request(format!(
            "username must be 1 to {MAX_USERNAME} letters, digits, spaces, or . _ -"
        )));
    }
    Ok(username.to_owned())
}

/// Guest names: anything printable, 1 to 32 characters.
fn validate_display_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_USERNAME || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(format!(
            "pick a name of 1 to {MAX_USERNAME} characters"
        )));
    }
    Ok(name.to_owned())
}

/// Creates a session and returns its Set-Cookie value.
async fn start_session(state: &AppState, user_id: i64, days: i64) -> ApiResult<HeaderValue> {
    let token = new_token()?;
    let max_age = days * 24 * 60 * 60;
    state
        .store
        .create_session(user_id, &hash_token(&token), now_ms() + max_age * 1000)
        .await?;
    session_cookie(&token, max_age)
}

fn session_cookie(token: &str, max_age: i64) -> ApiResult<HeaderValue> {
    let cookie =
        format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    Ok(HeaderValue::from_str(&cookie).map_err(anyhow::Error::from)?)
}

fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_owned())
        .filter(|v| !v.is_empty())
}

/// 32 random bytes, hex encoded.
pub fn new_token() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn hash_password(password: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))
    })
    .await?
}

async fn verify_password(password: String, hash: String) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let parsed =
            PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("stored hash is invalid: {e}"))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await?
}

/// A valid Argon2 hash of a random password, for timing-safe failed logins.
fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let password = new_token().unwrap_or_default();
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::call;
    use axum::http::Method;

    #[tokio::test]
    async fn bootstrap_login_and_admin_gate() {
        let state = AppState::for_tests().await;
        *state.bootstrap_token.lock().unwrap() = Some("secret".to_owned());

        let (status, _) = call(&state, Method::GET, "/api/me", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let bad = r#"{"token":"nope","username":"admin","password":"hunter22"}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/bootstrap", Some(bad), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let good = r#"{"token":"secret","username":"admin","password":"hunter22"}"#;
        let (status, admin) = call(
            &state,
            Method::POST,
            "/api/auth/bootstrap",
            Some(good),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let admin = admin.unwrap();

        // The token is single use.
        let (status, _) = call(
            &state,
            Method::POST,
            "/api/auth/bootstrap",
            Some(good),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = call(&state, Method::GET, "/api/me", None, Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);

        let new = r#"{"username":"guest1","password":"partytime"}"#;
        let (status, _) = call(&state, Method::POST, "/api/users", Some(new), Some(&admin)).await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, _) = call(&state, Method::POST, "/api/users", Some(new), Some(&admin)).await;
        assert_eq!(status, StatusCode::CONFLICT);

        let wrong = r#"{"username":"guest1","password":"wrong-one"}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/login", Some(wrong), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let login = r#"{"username":"GUEST1","password":"partytime"}"#;
        let (status, user) = call(&state, Method::POST, "/api/auth/login", Some(login), None).await;
        assert_eq!(status, StatusCode::OK);
        let user = user.unwrap();

        // Regular users can't manage accounts.
        let (status, _) = call(&state, Method::GET, "/api/users", None, Some(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let (status, _) = call(&state, Method::POST, "/api/auth/logout", None, Some(&user)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(&state, Method::GET, "/api/me", None, Some(&user)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn guest_mode_join_and_switch_off() {
        let state = AppState::for_tests().await;
        let join = r#"{"name":"  Party Sam 🎉 "}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/guest", Some(join), None).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "guest mode is off by default"
        );

        state
            .guest_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let (status, guest) = call(&state, Method::POST, "/api/auth/guest", Some(join), None).await;
        assert_eq!(status, StatusCode::CREATED);
        let guest = guest.unwrap();
        let (status, renewed) = call(&state, Method::GET, "/api/me", None, Some(&guest)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(renewed.as_deref(), Some(guest.as_str()), "cookie renewed");

        let bad = r#"{"name":"   "}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/guest", Some(bad), None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // Switching guest mode off signs guests out.
        state
            .guest_mode
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let (status, _) = call(&state, Method::GET, "/api/me", None, Some(&guest)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn user_management_guardrails() {
        use crate::test_util::session;
        let state = AppState::for_tests().await;
        let admin = session(&state, "boss", Role::Admin).await;
        let me = state
            .store
            .user_credentials("boss")
            .await
            .unwrap()
            .unwrap()
            .0;
        let dj = state
            .store
            .create_user("dj", "x", Role::User)
            .await
            .unwrap();
        let patch = |id: i64| format!("/api/users/{id}");

        let (status, _) = call(
            &state,
            Method::PATCH,
            &patch(me.id),
            Some(r#"{"role":"user"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "can't demote yourself");
        let (status, _) = call(
            &state,
            Method::PATCH,
            &patch(dj.id),
            Some(r#"{"role":"admin"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, _) = call(
            &state,
            Method::PATCH,
            &patch(dj.id),
            Some(r#"{"password":"123"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "too short");
        let (status, _) = call(
            &state,
            Method::PATCH,
            &patch(dj.id),
            Some(r#"{"password":"longenough"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let login = r#"{"username":"dj","password":"longenough"}"#;
        let (status, _) = call(&state, Method::POST, "/api/auth/login", Some(login), None).await;
        assert_eq!(status, StatusCode::OK, "new password works");
    }

    #[test]
    fn username_rules() {
        assert_eq!(
            validate_username("  DJ Jazzy-Jeff ").unwrap(),
            "DJ Jazzy-Jeff"
        );
        assert!(validate_username("").is_err());
        assert!(validate_username("bad<script>").is_err());
        assert!(validate_username(&"x".repeat(33)).is_err());
    }
}
