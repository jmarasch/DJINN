//! Accounts, sessions, and the request extractors that enforce them.
//!
//! Sessions are random tokens in an HttpOnly, SameSite=Lax cookie; the
//! database stores only their SHA-256 hash. Passwords are hashed with
//! Argon2id on the blocking pool.

use std::net::IpAddr;
use std::sync::OnceLock;

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::header::{COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_store::{
    now_ms, AccountStatus, CreateUserError, GuestSummary, NewSignIn, Permissions, Role,
    SignupError, Store, User, LISTENER_ROLE,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::{self, ClientIp};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{security, settings, two_step};

const COOKIE_NAME: &str = "djinn_session";
const SESSION_DAYS: i64 = 30;
/// Guests stay signed in for a year (renewed whenever they open the app),
/// so they're recognized at the next party.
const GUEST_SESSION_DAYS: i64 = 365;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
const MAX_USERNAME: usize = 32;
/// How stale a session's "last used" may get before it's updated, so
/// requests don't each write to the database.
const TOUCH_AFTER_MS: i64 = 5 * 60 * 1000;
/// Passwords too easy to guess, refused whatever their length.
const COMMON_PASSWORDS: &[&str] = &[
    "password",
    "password1",
    "password12",
    "password123",
    "passw0rd",
    "12345678",
    "123456789",
    "1234567890",
    "0123456789",
    "87654321",
    "11111111",
    "00000000",
    "12341234",
    "11223344",
    "qwertyui",
    "qwerty12",
    "qwerty123",
    "qwertyuiop",
    "1q2w3e4r",
    "1qaz2wsx",
    "asdfghjk",
    "zxcvbnm1",
    "abcd1234",
    "abc12345",
    "abcdefgh",
    "iloveyou",
    "iloveyou1",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "superman",
    "starwars",
    "whatever",
    "trustno1",
    "letmein1",
    "welcome1",
    "welcome123",
    "changeme",
    "admin123",
    "administrator",
    "monkey123",
    "dragon12",
    "master12",
    "computer",
    "internet",
    "jukebox1",
    "djinn123",
    "music123",
];

/// Who may create an account without an invite.
pub const SIGNUP_MODE: &str = "signup_mode";
/// The role new sign-ups get.
pub const SIGNUP_ROLE: &str = "signup_role";

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignupMode {
    /// Only invites, or the admin adding people.
    #[default]
    Closed,
    /// Anyone can sign up; the admin approves each account.
    Review,
    /// Anyone can sign up and start right away.
    Open,
}

impl SignupMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Review => "review",
            Self::Open => "open",
        }
    }
}

pub async fn signup_mode(store: &Store) -> anyhow::Result<SignupMode> {
    Ok(match store.get_setting(SIGNUP_MODE).await?.as_deref() {
        Some("review") => SignupMode::Review,
        Some("open") => SignupMode::Open,
        _ => SignupMode::Closed,
    })
}

/// The role sign-ups go into: the chosen one while it exists, else Listener.
pub async fn signup_role(store: &Store) -> anyhow::Result<i64> {
    let id = settings::number(store, SIGNUP_ROLE, LISTENER_ROLE).await?;
    Ok(match store.role(id).await? {
        Some(_) => id,
        None => LISTENER_ROLE,
    })
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/two-step", post(finish_two_step))
        .route("/me/password", post(change_password))
        .route("/auth/logout", post(logout))
        .route("/auth/bootstrap", get(bootstrap_status).post(bootstrap))
        .route("/auth/mode", get(mode))
        .route("/auth/guest", post(join_as_guest))
        .route("/auth/signup", post(sign_up))
        .route("/auth/invite/{token}", get(check_invite))
        .route("/me", get(me))
        .route("/users", get(list_users).post(create_user))
        .route("/users/{id}", delete(delete_user).patch(update_user))
        .route("/guests", get(list_guests))
}

/// The logged-in user and their session. Rejects with 401 when there is no
/// valid session.
pub struct CurrentSession {
    pub user: User,
    pub session_id: i64,
}

/// The logged-in user. Rejects with 401 when there is no valid session.
pub struct CurrentUser(pub User);

/// A logged-in admin. Rejects with 401 or 403.
pub struct AdminUser(pub User);

impl FromRequestParts<AppState> for CurrentSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let token = session_token(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let found = state
            .store
            .session(&hash_token(&token))
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        // Turning guest mode off signs guests out.
        if found.user.role == Role::Guest && !state.guest_mode() {
            return Err(ApiError::unauthorized());
        }
        if now_ms() - found.last_seen_at > TOUCH_AFTER_MS {
            let Ok(device) = Device::from_request_parts(parts, state).await;
            state
                .store
                .touch_session(
                    found.session_id,
                    device.ip.as_deref(),
                    device.agent.as_deref(),
                )
                .await?;
        }
        Ok(Self {
            user: found.user,
            session_id: found.session_id,
        })
    }
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let session = CurrentSession::from_request_parts(parts, state).await?;
        Ok(Self(session.user))
    }
}

/// Where a request came from: noted on the sessions it starts and in the
/// sign-in log, and checked by DJINN GO.
pub struct Device {
    pub addr: Option<IpAddr>,
    pub ip: Option<String>,
    pub agent: Option<String>,
    /// On the home network (or Tailscale).
    pub home: bool,
    /// Reached over HTTPS through a trusted proxy.
    pub https: bool,
    /// Came in on the DJINN GO port.
    pub go: bool,
}

impl FromRequestParts<AppState> for Device {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, std::convert::Infallible> {
        let origin = security::origin(
            security::peer(&parts.extensions),
            &parts.headers,
            &state.go.proxies(),
        );
        let agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.chars().take(300).collect());
        Ok(Self {
            addr: origin.ip,
            ip: origin.ip.map(|ip| ip.to_string()),
            agent,
            home: origin.home,
            https: origin.https,
            go: parts.extensions.get::<crate::go::GoRequest>().is_some(),
        })
    }
}

impl Device {
    fn via(&self) -> &'static str {
        if self.go {
            "go"
        } else {
            "party"
        }
    }
}

/// The signed-in user as the app sees it: the account, its role's name,
/// and what it may do.
#[derive(Serialize)]
pub struct Me {
    #[serde(flatten)]
    pub user: User,
    pub role_name: Option<String>,
    pub permissions: Permissions,
    /// DJINN GO asks this account for two-step sign-in (see `user.two_step`
    /// for whether it's on).
    pub two_step_required: bool,
}

pub async fn me_for(state: &AppState, user: User) -> ApiResult<Me> {
    let permissions = state.store.permissions(&user).await?;
    let role_name = match user.role {
        Role::User => state.store.role(user.role_id).await?.map(|r| r.name),
        Role::Admin | Role::Guest => None,
    };
    let two_step_required = two_step::required(state, &user).await?;
    Ok(Me {
        user,
        role_name,
        permissions,
        two_step_required,
    })
}

/// Records a sign-in attempt in the log.
async fn log_sign_in(
    state: &AppState,
    device: &Device,
    username: &str,
    user_id: Option<i64>,
    outcome: &str,
) -> ApiResult<()> {
    state
        .store
        .log_sign_in(&NewSignIn {
            user_id,
            username: &username.chars().take(MAX_USERNAME * 2).collect::<String>(),
            ip: device.ip.as_deref(),
            user_agent: device.agent.as_deref(),
            via: device.via(),
            outcome,
        })
        .await?;
    if outcome != "ok" {
        tracing::info!(username, ip = ?device.ip, via = device.via(), outcome, "sign-in refused");
    }
    Ok(())
}

/// Checks the signed-in user's own password, counting wrong ones toward
/// the lockout.
pub async fn check_own_password(
    state: &AppState,
    user: &User,
    password: String,
) -> ApiResult<bool> {
    if let Some(wait) = state.limiter.wait(&user.username, None) {
        return Err(too_many_tries(wait));
    }
    let Some((_, hash)) = state.store.user_credentials(&user.username).await? else {
        return Ok(false);
    };
    let ok = verify_password(password, hash).await?;
    if !ok {
        state.limiter.failed(&user.username, None);
    }
    Ok(ok)
}

fn too_many_tries(wait: std::time::Duration) -> ApiError {
    let minutes = wait.as_secs().div_ceil(60).max(1);
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        format!(
            "too many tries; wait {minutes} {} and try again",
            if minutes == 1 { "minute" } else { "minutes" }
        ),
    )
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

#[derive(Serialize)]
struct NeedsCode {
    /// The password was right; send the code with this to finish.
    two_step: bool,
    ticket: String,
}

/// Signs in with a password. Accounts with two-step sign-in get a ticket
/// to send with their code instead of a session. Too many wrong passwords
/// for an account or from an address lock sign-ins for a while.
async fn login(
    State(state): State<AppState>,
    device: Device,
    Json(body): Json<Credentials>,
) -> ApiResult<Response> {
    let username = body.username.trim();
    if let Some(wait) = state.limiter.wait(username, device.addr) {
        log_sign_in(&state, &device, username, None, "locked_out").await?;
        return Err(too_many_tries(wait));
    }
    let found = state.store.user_credentials(username).await?;
    // Verify against a dummy hash for unknown users so response time doesn't
    // reveal which usernames exist.
    let (user, hash) = match found {
        Some((user, hash)) => (Some(user), hash),
        None => (None, dummy_hash().to_owned()),
    };
    let user_id = user.as_ref().map(|u| u.id);
    let valid = verify_password(body.password, hash).await?;
    let Some(user) = user.filter(|_| valid) else {
        state.limiter.failed(username, device.addr);
        log_sign_in(&state, &device, username, user_id, "wrong_password").await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "wrong username or password",
        ));
    };
    state.limiter.succeeded(username);
    // Only said once the password is right, so it reveals nothing.
    let refused = match user.status {
        AccountStatus::Active => None,
        AccountStatus::Pending => Some((
            "waiting",
            "your account is waiting for the host to approve it",
        )),
        AccountStatus::Disabled => Some(("turned_off", "this account has been turned off")),
    };
    let refused = match refused {
        Some(r) => Some(r),
        None if device.go => go_refusal(&state, &user, &device).await?,
        None => None,
    };
    if let Some((outcome, message)) = refused {
        log_sign_in(&state, &device, username, Some(user.id), outcome).await?;
        return Err(ApiError::new(StatusCode::FORBIDDEN, message));
    }
    if user.two_step {
        let ticket = state.tickets.issue(&user)?;
        return Ok(Json(NeedsCode {
            two_step: true,
            ticket,
        })
        .into_response());
    }
    log_sign_in(&state, &device, username, Some(user.id), "ok").await?;
    let cookie = start_session(&state, user.id, SESSION_DAYS, &device).await?;
    Ok((
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(me_for(&state, user).await?),
    )
        .into_response())
}

/// Why `user` can't use DJINN GO from `device`, if they can't: for the log
/// and to show them.
pub async fn go_refusal(
    state: &AppState,
    user: &User,
    device: &Device,
) -> ApiResult<Option<(&'static str, &'static str)>> {
    let permissions = state.store.permissions(user).await?;
    Ok(if user.role == Role::Guest || !permissions.go {
        Some(("no_go", "your account can't use DJINN GO; ask the host"))
    } else if !device.home && !permissions.go_remote {
        Some((
            "not_home",
            "your account can only use DJINN GO on the home network",
        ))
    } else {
        None
    })
}

#[derive(Deserialize)]
struct TwoStepCode {
    ticket: String,
    code: String,
}

/// Finishes a two-step sign-in with the code from the app, or a recovery
/// code.
async fn finish_two_step(
    State(state): State<AppState>,
    device: Device,
    Json(body): Json<TwoStepCode>,
) -> ApiResult<impl IntoResponse> {
    let ticket = state.tickets.get(&body.ticket).ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "that took too long; sign in again",
        )
    })?;
    if let Some(wait) = state.limiter.wait(&ticket.username, device.addr) {
        state.tickets.used(&body.ticket);
        log_sign_in(
            &state,
            &device,
            &ticket.username,
            Some(ticket.user_id),
            "locked_out",
        )
        .await?;
        return Err(too_many_tries(wait));
    }
    if !two_step::verify(&state, ticket.user_id, &body.code).await? {
        state.tickets.wrong(&body.ticket);
        state.limiter.failed(&ticket.username, device.addr);
        log_sign_in(
            &state,
            &device,
            &ticket.username,
            Some(ticket.user_id),
            "wrong_code",
        )
        .await?;
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "that code didn't work; try the newest one",
        ));
    }
    state.tickets.used(&body.ticket);
    let user = state
        .store
        .user(ticket.user_id)
        .await?
        .filter(|u| u.status == AccountStatus::Active)
        .ok_or_else(ApiError::unauthorized)?;
    log_sign_in(&state, &device, &user.username, Some(user.id), "ok").await?;
    let cookie = start_session(&state, user.id, SESSION_DAYS, &device).await?;
    Ok((
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(me_for(&state, user).await?),
    ))
}

#[derive(Deserialize)]
struct NewPassword {
    current: String,
    new: String,
}

/// Changes your own password. Every other device is signed out; this one
/// gets a new session.
async fn change_password(
    me: CurrentSession,
    State(state): State<AppState>,
    device: Device,
    Json(body): Json<NewPassword>,
) -> ApiResult<impl IntoResponse> {
    if me.user.role == Role::Guest {
        return Err(ApiError::bad_request("guests don't have passwords"));
    }
    if !check_own_password(&state, &me.user, body.current).await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "your current password is wrong",
        ));
    }
    validate_password(&body.new, &me.user.username)?;
    let hash = hash_password(body.new).await?;
    state.store.set_password(me.user.id, &hash).await?;
    tracing::info!(username = %me.user.username, "password changed");
    let cookie = start_session(&state, me.user.id, SESSION_DAYS, &device).await?;
    Ok((
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, cookie)]),
    ))
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
            renewed = Some(session_cookie(&token, max_age, false)?);
        }
    }
    Ok((
        AppendHeaders(renewed.map(|c| (SET_COOKIE, c))),
        Json(me_for(&state, user).await?),
    ))
}

#[derive(Serialize)]
struct Mode {
    guest_mode: bool,
    setup_needed: bool,
    /// Name this device used last time, so it can rejoin with one tap.
    welcome_back: Option<String>,
    /// Whether people can create their own accounts.
    signup: SignupMode,
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
        signup: signup_mode(&state.store).await?,
    }))
}

#[derive(Deserialize)]
struct SignUp {
    username: String,
    password: String,
    /// The token from an invite link.
    #[serde(default)]
    invite: Option<String>,
}

#[derive(Serialize)]
struct Pending {
    pending: bool,
}

/// Creates an account from the sign-in screen: through an invite link
/// (signed in right away, in the invite's role), or as sign-ups allow.
/// Accounts waiting for approval get 202 and no session.
async fn sign_up(
    State(state): State<AppState>,
    device: Device,
    Json(body): Json<SignUp>,
) -> ApiResult<Response> {
    if !state.store.admin_exists().await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "DJINN isn't set up yet",
        ));
    }
    let username = validate_username(&body.username)?;
    validate_password(&body.password, &username)?;
    let invite = body.invite.filter(|t| !t.is_empty());
    let mode = signup_mode(&state.store).await?;
    if invite.is_none() && mode == SignupMode::Closed {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "sign-ups are closed; ask the host for an invite link",
        ));
    }
    let hash = hash_password(body.password).await?;
    let user = match invite {
        Some(token) => state
            .store
            .sign_up_with_invite(&username, &hash, &hash_token(&token))
            .await
            .map_err(|e| match e {
                SignupError::Taken => ApiError::conflict("username is already taken"),
                SignupError::BadInvite => ApiError::new(StatusCode::FORBIDDEN, e.to_string()),
                SignupError::Store(e) => e.into(),
            })?,
        None => {
            let status = match mode {
                SignupMode::Review => AccountStatus::Pending,
                SignupMode::Open | SignupMode::Closed => AccountStatus::Active,
            };
            let role_id = signup_role(&state.store).await?;
            match state
                .store
                .create_account(&username, &hash, Role::User, role_id, status)
                .await
            {
                Ok(user) => user,
                Err(CreateUserError::Taken) => {
                    return Err(ApiError::conflict("username is already taken"))
                }
                Err(CreateUserError::Store(e)) => return Err(e.into()),
            }
        }
    };
    tracing::info!(username = %user.username, status = ?user.status, "account signed up");
    if user.status == AccountStatus::Pending {
        return Ok((StatusCode::ACCEPTED, Json(Pending { pending: true })).into_response());
    }
    let cookie = start_session(&state, user.id, SESSION_DAYS, &device).await?;
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(me_for(&state, user).await?),
    )
        .into_response())
}

#[derive(Serialize)]
struct InviteCheck {
    role: String,
}

/// Whether an invite link still works, for the sign-up page. Public.
async fn check_invite(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Json<InviteCheck>> {
    let invite = state
        .store
        .invite_by_token(&hash_token(&token))
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "this invite link has expired or been used up",
            )
        })?;
    Ok(Json(InviteCheck {
        role: invite.role_name,
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
    device: Device,
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
    let cookie = start_session(&state, user.id, GUEST_SESSION_DAYS, &device).await?;
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(me_for(&state, user).await?),
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
    device: Device,
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
    let user = new_user(
        &state,
        &body.username,
        body.password,
        Role::Admin,
        LISTENER_ROLE,
    )
    .await?;
    if let Ok(mut token) = state.bootstrap_token.lock() {
        *token = None;
    }
    tracing::info!(username = %user.username, "admin account created");
    let cookie = start_session(&state, user.id, SESSION_DAYS, &device).await?;
    Ok((
        StatusCode::CREATED,
        AppendHeaders([(SET_COOKIE, cookie)]),
        Json(me_for(&state, user).await?),
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
    /// The account's role (see `roles`).
    #[serde(default = "default_role_id")]
    role_id: i64,
}

fn default_role() -> Role {
    Role::User
}

fn default_role_id() -> i64 {
    LISTENER_ROLE
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
    if state.store.role(body.role_id).await?.is_none() {
        return Err(ApiError::bad_request("that role doesn't exist"));
    }
    let user = new_user(
        &state,
        &body.username,
        body.password,
        body.role,
        body.role_id,
    )
    .await?;
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
    /// Moves the account to another role (see `roles`).
    role_id: Option<i64>,
    /// Approves (active), turns off (disabled) or back on an account.
    status: Option<AccountStatus>,
    /// New password; signs the user out everywhere.
    password: Option<String>,
}

/// Edits a user: request limit exemption, admin or not, role, approval,
/// turning off, or password. Guests only have the exemption.
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
    if let Some(role_id) = body.role_id.filter(|&r| r != user.role_id) {
        if user.role == Role::Guest {
            return Err(ApiError::bad_request("guests don't have roles"));
        }
        if state.store.role(role_id).await?.is_none() {
            return Err(ApiError::bad_request("that role doesn't exist"));
        }
        state.store.set_account_role(id, role_id).await?;
    }
    if let Some(status) = body.status.filter(|&s| s != user.status) {
        if user.role == Role::Guest {
            return Err(ApiError::bad_request(
                "guests can't be turned off; remove them instead",
            ));
        }
        if status == AccountStatus::Pending {
            return Err(ApiError::bad_request("an account can't go back to waiting"));
        }
        if id == admin.id {
            return Err(ApiError::bad_request("you can't turn off your own account"));
        }
        if user.role == Role::Admin
            && status == AccountStatus::Disabled
            && state.store.admin_count().await? <= 1
        {
            return Err(ApiError::bad_request("there must be at least one admin"));
        }
        state.store.set_status(id, status).await?;
        tracing::info!(username = %user.username, status = ?status, "account status changed");
    }
    if let Some(password) = body.password {
        if user.role == Role::Guest {
            return Err(ApiError::bad_request("guests don't have passwords"));
        }
        validate_password(&password, &user.username)?;
        let hash = hash_password(password).await?;
        state.store.set_password(id, &hash).await?;
    }
    if let Some(unlimited) = body.unlimited {
        state.store.set_unlimited(id, unlimited).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// New passwords: long enough, not the username, not a common one, and
/// not one character over and over.
fn validate_password(password: &str, username: &str) -> ApiResult<()> {
    let length = password.chars().count();
    if length < MIN_PASSWORD {
        return Err(ApiError::bad_request(format!(
            "password must be at least {MIN_PASSWORD} characters"
        )));
    }
    if length > MAX_PASSWORD {
        return Err(ApiError::bad_request("password is too long"));
    }
    let lower = password.to_lowercase();
    let mut chars = lower.chars();
    let first = chars.next();
    if lower == username.trim().to_lowercase()
        || COMMON_PASSWORDS.contains(&lower.as_str())
        || chars.all(|c| Some(c) == first)
    {
        return Err(ApiError::bad_request(
            "that password is too easy to guess; try a few words together",
        ));
    }
    Ok(())
}

async fn new_user(
    state: &AppState,
    username: &str,
    password: String,
    role: Role,
    role_id: i64,
) -> ApiResult<User> {
    let username = validate_username(username)?;
    validate_password(&password, &username)?;
    let hash = hash_password(password).await?;
    match state
        .store
        .create_account(&username, &hash, role, role_id, AccountStatus::Active)
        .await
    {
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
async fn start_session(
    state: &AppState,
    user_id: i64,
    days: i64,
    device: &Device,
) -> ApiResult<HeaderValue> {
    let token = new_token()?;
    let max_age = days * 24 * 60 * 60;
    state
        .store
        .create_session_from(
            user_id,
            &hash_token(&token),
            now_ms() + max_age * 1000,
            device.ip.as_deref(),
            device.agent.as_deref(),
        )
        .await?;
    session_cookie(&token, max_age, device.https)
}

/// The session cookie; `secure` (over HTTPS) keeps browsers from ever
/// sending it unencrypted.
fn session_cookie(token: &str, max_age: i64, secure: bool) -> ApiResult<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let cookie =
        format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}");
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

pub(crate) fn hash_token(token: &str) -> Vec<u8> {
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
