//! Protected areas: anything that changes the library (the folders and the
//! files in them) sits behind a lock. An admin opens it by entering their
//! password and, at the strictest level, a two-step code; the unlock belongs
//! to that session, ends when they leave the area, and also ends after a
//! stretch without use. The `Elevated` extractor is what the changing
//! routes ask for, so the lock holds on the server, not just on the screen.
//!
//! How strict it is is a setting with three levels. Every entry, failure and
//! change is written to an audit trail that an admin may turn off or purge.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_store::{AuditEntry, NewAudit, User};
use serde::{Deserialize, Serialize};

use crate::auth::{check_own_password, AdminUser, CurrentSession, Device};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use crate::{settings, two_step};

/// Setting key: how strict the lock is ("max", "medium" or "off").
pub const LEVEL: &str = "protected_level";
/// Setting key: whether the audit trail records anything. On unless "0".
pub const AUDIT_ON: &str = "audit_enabled";

/// An unlock ends after this long without a protected action.
const IDLE: Duration = Duration::from_secs(10 * 60);
/// ...and at the latest after this long, however busy.
const MOST: Duration = Duration::from_secs(60 * 60);
/// What must be typed to turn the lock off.
const OFF_WORD: &str = "OFF";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/protected", get(status))
        .route("/protected/enter", post(enter))
        .route("/protected/leave", post(leave))
        .route("/protected/level", put(set_level))
        .route(
            "/protected/audit",
            get(audit_page).put(set_audit).delete(purge_audit),
        )
        .route("/protected/locked", get(locked_accounts))
        .route("/protected/recovery/dismiss", post(dismiss_recovery))
        .route("/users/{id}/unlock", post(unlock_account))
}

// --- Levels ---

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Nothing beyond being signed in as an admin.
    Off,
    /// The admin's password.
    Medium,
    /// The admin's password and a two-step code.
    Max,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Medium => "medium",
            Level::Max => "max",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "off" => Some(Level::Off),
            "medium" => Some(Level::Medium),
            "max" => Some(Level::Max),
            _ => None,
        }
    }
}

/// The level in force; the strictest when the setting is missing or odd.
pub async fn level(state: &AppState) -> anyhow::Result<Level> {
    Ok(state
        .store
        .get_setting(LEVEL)
        .await?
        .as_deref()
        .and_then(Level::parse)
        .unwrap_or(Level::Max))
}

// --- Unlocks ---

struct Grant {
    user_id: i64,
    /// The level it was opened at; a stricter level since voids it.
    level: Level,
    started: Instant,
    last_used: Instant,
}

/// Which sessions have the protected areas open.
#[derive(Default)]
pub struct Elevations {
    grants: Mutex<HashMap<i64, Grant>>,
}

impl Elevations {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Grant>> {
        self.grants.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn open(&self, session: i64, user_id: i64, level: Level) {
        let now = Instant::now();
        let mut grants = self.lock();
        grants.retain(|_, g| Self::alive(g, now));
        grants.insert(
            session,
            Grant {
                user_id,
                level,
                started: now,
                last_used: now,
            },
        );
    }

    fn alive(grant: &Grant, now: Instant) -> bool {
        now.duration_since(grant.last_used) < IDLE && now.duration_since(grant.started) < MOST
    }

    /// True while the session's unlock stands (and counts as use).
    fn check(&self, session: i64, user_id: i64, level: Level) -> bool {
        let now = Instant::now();
        let mut grants = self.lock();
        match grants.get_mut(&session) {
            Some(g) if g.user_id == user_id && g.level >= level && Self::alive(g, now) => {
                g.last_used = now;
                true
            }
            Some(g) if !Self::alive(g, now) => {
                grants.remove(&session);
                false
            }
            _ => false,
        }
    }

    /// Seconds until the idle timeout, if the unlock stands.
    fn left(&self, session: i64, user_id: i64, level: Level) -> Option<u64> {
        let now = Instant::now();
        let grants = self.lock();
        let g = grants.get(&session)?;
        if g.user_id != user_id || g.level < level || !Self::alive(g, now) {
            return None;
        }
        let idle = IDLE.saturating_sub(now.duration_since(g.last_used));
        let most = MOST.saturating_sub(now.duration_since(g.started));
        Some(idle.min(most).as_secs())
    }

    fn close(&self, session: i64) -> bool {
        self.lock().remove(&session).is_some()
    }

    fn close_all(&self) {
        self.lock().clear();
    }
}

/// An admin whose session has the protected areas open, or any admin when
/// the lock is off. Rejects with 423 (locked) otherwise, which the pages
/// answer by asking for the password.
pub struct Elevated(pub User);

impl FromRequestParts<AppState> for Elevated {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let session = CurrentSession::from_request_parts(parts, state).await?;
        require_open(state, &session).await?;
        Ok(Self(session.user))
    }
}

/// What `Elevated` checks, for routes where only some requests need it (a
/// change to an admin's account, say): an admin with the protected areas
/// open, or the lock off. 423 otherwise.
pub async fn require_open(state: &AppState, session: &CurrentSession) -> ApiResult<()> {
    if !session.user.is_admin() {
        return Err(ApiError::forbidden());
    }
    let wanted = level(state).await?;
    if wanted == Level::Off
        || state
            .elevations
            .check(session.session_id, session.user.id, wanted)
    {
        return Ok(());
    }
    Err(ApiError::new(
        StatusCode::LOCKED,
        "this needs the protected area unlocked first",
    ))
}

// --- The audit trail ---

/// Writes one record unless the admin turned the trail off. Failing to write
/// never stops what was being done.
pub async fn audit(state: &AppState, user: &User, ip: Option<&str>, kind: &str, detail: &str) {
    match settings::flag(&state.store, AUDIT_ON, true).await {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            tracing::warn!(error = %e, "couldn't read the audit setting");
            return;
        }
    }
    let entry = NewAudit {
        user_id: Some(user.id),
        username: &user.username,
        ip,
        kind,
        detail,
    };
    if let Err(e) = state.store.log_audit(&entry).await {
        tracing::warn!(error = %e, "couldn't write the audit trail");
    }
}

// --- Routes ---

#[derive(Serialize)]
struct Status {
    level: Level,
    /// This session has the areas open (always true at Off).
    open: bool,
    /// Seconds until the unlock ends from lack of use.
    seconds_left: Option<u64>,
    /// Two-step sign-in is set up on this account.
    two_step: bool,
    /// This account can open the areas at the current level.
    can_open: bool,
    audit_on: bool,
    audit_count: i64,
    /// The host's recovery command was used (until an admin dismisses it).
    recovery_used: Option<djinn_store::RecoveryUsed>,
}

async fn status(
    AdminUser(user): AdminUser,
    session: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<Json<Status>> {
    let wanted = level(&state).await?;
    let two_step = state.store.two_step_secret(user.id).await?.is_some();
    let left = state.elevations.left(session.session_id, user.id, wanted);
    Ok(Json(Status {
        level: wanted,
        open: wanted == Level::Off || left.is_some(),
        seconds_left: if wanted == Level::Off { None } else { left },
        two_step,
        can_open: wanted != Level::Max || two_step,
        audit_on: settings::flag(&state.store, AUDIT_ON, true).await?,
        audit_count: state.store.audit_count().await?,
        recovery_used: state.store.recovery_used().await?,
    }))
}

/// Accounts locked after too many wrong codes, for Admin > Users.
async fn locked_accounts(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<i64>>> {
    Ok(Json(
        state
            .store
            .admin_accounts()
            .await?
            .into_iter()
            .filter(|a| a.locked)
            .map(|a| a.id)
            .collect(),
    ))
}

/// One admin lets a locked account sign in again.
async fn unlock_account(
    AdminUser(admin): AdminUser,
    device: Device,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> ApiResult<StatusCode> {
    let target = state
        .store
        .user(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !state.store.unlock_account(id).await? {
        return Err(ApiError::bad_request("that account isn't locked"));
    }
    tracing::info!(username = %target.username, by = %admin.username, "account unlocked");
    audit(
        &state,
        &admin,
        device.ip.as_deref(),
        "account_unlocked",
        &target.username,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Dismisses the banner about the host recovery command having been used.
async fn dismiss_recovery(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.dismiss_recovery_note().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Proof {
    password: String,
    #[serde(default)]
    code: String,
}

/// Checks the signed-in admin's password and, when `need_code`, a two-step
/// code. Wrong tries are counted and the lock-outs are the sign-in's.
async fn prove(
    state: &AppState,
    user: &User,
    device: &Device,
    proof: Proof,
    need_code: bool,
) -> ApiResult<()> {
    if !check_own_password(state, user, proof.password).await? {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "that password isn't right",
        ));
    }
    if !need_code {
        return Ok(());
    }
    if state.store.two_step_secret(user.id).await?.is_none() {
        return Err(ApiError::bad_request(
            "set up two-step sign-in on your account first (Account page); the strictest level needs it",
        ));
    }
    two_step::prove_code(state, user, device, "area", &proof.code).await
}

/// Opens the protected areas for this session.
async fn enter(
    AdminUser(user): AdminUser,
    session: CurrentSession,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<Proof>,
) -> ApiResult<StatusCode> {
    let wanted = level(&state).await?;
    if wanted == Level::Off {
        return Ok(StatusCode::NO_CONTENT);
    }
    if let Err(e) = prove(&state, &user, &device, body, wanted == Level::Max).await {
        audit(
            &state,
            &user,
            device.ip.as_deref(),
            "entry_failed",
            &format!("level {}: {}", wanted.as_str(), e.message()),
        )
        .await;
        return Err(e);
    }
    state.elevations.open(session.session_id, user.id, wanted);
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "entered",
        &format!("level {}", wanted.as_str()),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// Closes the protected areas again (leaving the screen, or Lock now).
async fn leave(
    AdminUser(user): AdminUser,
    session: CurrentSession,
    device: Device,
    State(state): State<AppState>,
) -> StatusCode {
    if state.elevations.close(session.session_id) {
        audit(&state, &user, device.ip.as_deref(), "left", "").await;
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct NewLevel {
    level: Level,
    #[serde(flatten)]
    proof: Proof,
    /// The word typed to confirm turning the lock off.
    #[serde(default)]
    confirm: String,
}

/// Changes how strict the lock is. Needs the password; leaving the
/// strictest level needs a two-step code too, so a stolen password can't
/// loosen the lock (an admin without two-step can't leave it at all, or a
/// second admin made without two-step would be a way around it); turning it
/// off needs the confirmation word.
async fn set_level(
    AdminUser(user): AdminUser,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<NewLevel>,
) -> ApiResult<StatusCode> {
    let now = level(&state).await?;
    let to = body.level;
    if to == now {
        return Ok(StatusCode::NO_CONTENT);
    }
    let two_step = state.store.two_step_secret(user.id).await?.is_some();
    if to == Level::Max && !two_step {
        return Err(ApiError::bad_request(
            "the strictest level needs two-step sign-in on your account; set it up on your Account page first",
        ));
    }
    if to == Level::Off && body.confirm.trim() != OFF_WORD {
        return Err(ApiError::bad_request(format!(
            "type {OFF_WORD} to turn the protection off"
        )));
    }
    let need_code = now == Level::Max;
    prove(&state, &user, &device, body.proof, need_code).await?;
    state.store.set_setting(LEVEL, to.as_str()).await?;
    // Everyone unlocks again under the new level.
    state.elevations.close_all();
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "level_changed",
        &format!("{} to {}", now.as_str(), to.as_str()),
    )
    .await;
    tracing::warn!(from = now.as_str(), to = to.as_str(), by = %user.username, "protected area level changed");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    offset: i64,
    limit: Option<i64>,
}

#[derive(Serialize)]
struct AuditPage {
    entries: Vec<AuditEntry>,
    total: i64,
}

async fn audit_page(
    _: AdminUser,
    State(state): State<AppState>,
    Query(page): Query<Page>,
) -> ApiResult<Json<AuditPage>> {
    let (entries, total) = state
        .store
        .audit_page(page.offset, page.limit.unwrap_or(100).clamp(1, 500))
        .await?;
    Ok(Json(AuditPage { entries, total }))
}

#[derive(Deserialize)]
struct AuditSetting {
    enabled: bool,
}

/// Turns the trail on or off. It's a protected action itself, and turning it
/// off is its last record.
async fn set_audit(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
    Json(body): Json<AuditSetting>,
) -> ApiResult<StatusCode> {
    let was = settings::flag(&state.store, AUDIT_ON, true).await?;
    if was == body.enabled {
        return Ok(StatusCode::NO_CONTENT);
    }
    if body.enabled {
        settings::set_flag(&state.store, AUDIT_ON, true).await?;
        audit(&state, &user, device.ip.as_deref(), "audit_on", "").await;
    } else {
        audit(&state, &user, device.ip.as_deref(), "audit_off", "").await;
        settings::set_flag(&state.store, AUDIT_ON, false).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct Purged {
    removed: u64,
}

/// Deletes every record. While the trail is on, one record of the purge
/// itself is written afterwards; turn it off first for no trace.
async fn purge_audit(
    Elevated(user): Elevated,
    device: Device,
    State(state): State<AppState>,
) -> ApiResult<Json<Purged>> {
    let removed = state.store.purge_audit().await?;
    audit(
        &state,
        &user,
        device.ip.as_deref(),
        "audit_purged",
        &format!("{removed} records"),
    )
    .await;
    Ok(Json(Purged { removed }))
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use djinn_store::Role;

    use super::*;
    use crate::test_util::{call, session};

    #[test]
    fn an_unlock_ends_when_idle_or_when_the_level_gets_stricter() {
        let grants = Elevations::default();
        grants.open(1, 7, Level::Medium);
        assert!(grants.check(1, 7, Level::Medium));
        assert!(grants.check(1, 7, Level::Off));
        // Stricter level voids it; another session or person doesn't share it.
        assert!(!grants.check(1, 7, Level::Max));
        assert!(!grants.check(2, 7, Level::Medium));
        assert!(!grants.check(1, 8, Level::Medium));
        assert!(grants.close(1));
        assert!(!grants.check(1, 7, Level::Medium));
        // Idle too long.
        grants.open(3, 7, Level::Max);
        if let Some(g) = grants.lock().get_mut(&3) {
            g.last_used = Instant::now() - IDLE - Duration::from_secs(1);
        }
        assert!(!grants.check(3, 7, Level::Max));
    }

    #[tokio::test]
    async fn changing_routes_are_locked_until_the_password_is_entered() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let (id, _) = state
            .store
            .user_credentials("admin")
            .await
            .unwrap()
            .unwrap();
        let hash = crate::auth::hash_password("right".into()).await.unwrap();
        state.store.set_password(id.id, &hash).await.unwrap();
        // Setting a password signs the account out everywhere: sign in again.
        {
            use sha2::{Digest, Sha256};
            state
                .store
                .create_session(
                    id.id,
                    &Sha256::digest(b"token-admin"),
                    djinn_store::now_ms() + 60_000,
                )
                .await
                .unwrap();
        }
        let put = |body: &'static str| {
            let state = state.clone();
            let who = admin.clone();
            async move {
                call(
                    &state,
                    Method::PUT,
                    "/api/protected/audit",
                    Some(body),
                    Some(&who),
                )
                .await
                .0
            }
        };
        // At the default level (max) nothing opens without two-step.
        assert_eq!(put(r#"{"enabled":false}"#).await, StatusCode::LOCKED);
        // Medium: the password opens it.
        state.store.set_setting(LEVEL, "medium").await.unwrap();
        assert_eq!(put(r#"{"enabled":false}"#).await, StatusCode::LOCKED);
        let (wrong, _body) = call(
            &state,
            Method::POST,
            "/api/protected/enter",
            Some(r#"{"password":"nope"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(wrong, StatusCode::UNAUTHORIZED, "{_body:?}");
        // The right password opens it, for this session only.
        let (ok, _) = call(
            &state,
            Method::POST,
            "/api/protected/enter",
            Some(r#"{"password":"right"}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(ok, StatusCode::NO_CONTENT);
        assert_eq!(put(r#"{"enabled":false}"#).await, StatusCode::NO_CONTENT);
        let other = session(&state, "other", Role::Admin).await;
        let (still_locked, _) = call(
            &state,
            Method::PUT,
            "/api/protected/audit",
            Some(r#"{"enabled":true}"#),
            Some(&other),
        )
        .await;
        assert_eq!(still_locked, StatusCode::LOCKED);
        // Off: any admin.
        state.store.set_setting(LEVEL, "off").await.unwrap();
        assert_eq!(put(r#"{"enabled":false}"#).await, StatusCode::NO_CONTENT);
        // Non-admins never.
        let user = session(&state, "sam", Role::User).await;
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/protected/audit",
            Some(r#"{"enabled":true}"#),
            Some(&user),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    /// What a stolen admin session (and even its password) can't do while the
    /// areas are locked: loosen the lock, make or change admins, strip
    /// two-step, or reset DJINN.
    #[tokio::test]
    async fn a_session_alone_cant_get_around_the_lock() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let (me, _) = state
            .store
            .user_credentials("admin")
            .await
            .unwrap()
            .unwrap();
        let hash = crate::auth::hash_password("right".into()).await.unwrap();
        state.store.set_password(me.id, &hash).await.unwrap();
        {
            use sha2::{Digest, Sha256};
            state
                .store
                .create_session(
                    me.id,
                    &Sha256::digest(b"token-admin"),
                    djinn_store::now_ms() + 60_000,
                )
                .await
                .unwrap();
        }
        let other = state
            .store
            .create_user("other", "x", Role::Admin)
            .await
            .unwrap();
        let send = |method: Method, uri: String, body: String| {
            let state = state.clone();
            let who = admin.clone();
            async move { call(&state, method, &uri, Some(&body), Some(&who)).await.0 }
        };

        // An admin without two-step can't take the lock down from the top.
        let off = format!(r#"{{"level":"off","password":"right","confirm":"{OFF_WORD}"}}"#);
        let status = send(Method::PUT, "/api/protected/level".into(), off).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(level(&state).await.unwrap(), Level::Max);

        let new_admin =
            r#"{"username":"sneaky","password":"three words here","role":"admin","role_id":1}"#;
        let status = send(Method::POST, "/api/users".into(), new_admin.into()).await;
        assert_eq!(status, StatusCode::LOCKED);
        let new_user =
            r#"{"username":"friend","password":"three words here","role":"user","role_id":1}"#;
        let status = send(Method::POST, "/api/users".into(), new_user.into()).await;
        assert_eq!(status, StatusCode::CREATED);

        let users = format!("/api/users/{}", other.id);
        let new_password = r#"{"password":"three words here"}"#;
        let status = send(Method::PATCH, users.clone(), new_password.into()).await;
        assert_eq!(status, StatusCode::LOCKED);
        let status = send(Method::DELETE, format!("{users}/two-step"), String::new()).await;
        assert_eq!(status, StatusCode::LOCKED);

        let reset = |what: &str| format!(r#"{{"what":"{what}"}}"#);
        let status = send(Method::POST, "/api/system/reset".into(), reset("settings")).await;
        assert_eq!(status, StatusCode::LOCKED);
        let status = send(Method::POST, "/api/system/reset".into(), reset("queue")).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn another_admin_can_unlock_a_locked_account() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let locked = state
            .store
            .create_user("locked", "x", Role::Admin)
            .await
            .unwrap();
        for _ in 0..djinn_store::CODE_FAILURE_LIMIT {
            state.store.note_code_failure(locked.id).await.unwrap();
        }
        let (status, body) =
            crate::test_util::get_body(&state, "/api/protected/locked", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, format!("[{}]", locked.id));
        let path = format!("/api/users/{}/unlock", locked.id);
        let (status, _) = call(&state, Method::POST, &path, Some("{}"), Some(&admin)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(!state.store.is_locked(locked.id).await.unwrap());
        // Not locked any more: nothing to do.
        let (status, _) = call(&state, Method::POST, &path, Some("{}"), Some(&admin)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let user = session(&state, "sam", Role::User).await;
        let (status, _) = call(&state, Method::POST, &path, Some("{}"), Some(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_trail_can_be_turned_off_and_purged() {
        let state = AppState::for_tests().await;
        state.store.set_setting(LEVEL, "off").await.unwrap();
        let admin = session(&state, "admin", Role::Admin).await;
        let send = |method: Method, path: &'static str, body: Option<&'static str>| {
            let state = state.clone();
            let who = admin.clone();
            async move { call(&state, method, path, body, Some(&who)).await }
        };
        // Turning it off writes its own last record; nothing after it.
        assert_eq!(
            send(
                Method::PUT,
                "/api/protected/audit",
                Some(r#"{"enabled":false}"#)
            )
            .await
            .0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(state.store.audit_count().await.unwrap(), 1);
        // Purging while off leaves no trace.
        assert_eq!(
            send(Method::DELETE, "/api/protected/audit", None).await.0,
            StatusCode::OK
        );
        assert_eq!(state.store.audit_count().await.unwrap(), 0);
        // On again: the turn-on and a later purge are recorded.
        send(
            Method::PUT,
            "/api/protected/audit",
            Some(r#"{"enabled":true}"#),
        )
        .await;
        send(Method::DELETE, "/api/protected/audit", None).await;
        let (page, _) = state.store.audit_page(0, 10).await.unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].kind, "audit_purged");
    }
}
