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
use djinn_store::{now_ms, CreateUserError, Role, User};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const COOKIE_NAME: &str = "djinn_session";
const SESSION_DAYS: i64 = 30;
const MIN_PASSWORD: usize = 6;
const MAX_PASSWORD: usize = 256;
const MAX_USERNAME: usize = 32;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/bootstrap", get(bootstrap_status).post(bootstrap))
        .route("/me", get(me))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", delete(delete_user))
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
    let cookie = start_session(&state, user.id).await?;
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

async fn me(CurrentUser(user): CurrentUser) -> Json<User> {
    Json(user)
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
    let cookie = start_session(&state, user.id).await?;
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

async fn new_user(
    state: &AppState,
    username: &str,
    password: String,
    role: Role,
) -> ApiResult<User> {
    let username = validate_username(username)?;
    let length = password.chars().count();
    if length < MIN_PASSWORD {
        return Err(ApiError::bad_request(format!(
            "password must be at least {MIN_PASSWORD} characters"
        )));
    }
    if length > MAX_PASSWORD {
        return Err(ApiError::bad_request("password is too long"));
    }
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

/// Creates a session and returns its Set-Cookie value.
async fn start_session(state: &AppState, user_id: i64) -> ApiResult<HeaderValue> {
    let token = new_token()?;
    let max_age = SESSION_DAYS * 24 * 60 * 60;
    state
        .store
        .create_session(user_id, &hash_token(&token), now_ms() + max_age * 1000)
        .await?;
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
    use crate::app;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    async fn call(
        state: &AppState,
        method: Method,
        path: &str,
        body: Option<&str>,
        cookie: Option<&str>,
    ) -> (StatusCode, Option<String>) {
        let mut req = Request::builder().method(method).uri(path);
        if body.is_some() {
            req = req.header("content-type", "application/json");
        }
        if let Some(cookie) = cookie {
            req = req.header(COOKIE, cookie);
        }
        let req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
        let res = app(state.clone()).oneshot(req).await.unwrap();
        let cookie = res
            .headers()
            .get(SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_owned);
        (res.status(), cookie)
    }

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
