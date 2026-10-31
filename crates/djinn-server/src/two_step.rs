//! Two-step sign-in: a six-digit code from an authenticator app (TOTP,
//! RFC 6238) after the password, or a one-time recovery code.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use data_encoding::BASE32_NOPAD;
use djinn_store::{Role, User};
use hmac::{Hmac, KeyInit, Mac};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::auth::{AdminUser, CurrentSession};
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

/// 1 when admins must use two-step sign-in for DJINN GO.
pub const ADMIN_TWO_STEP: &str = "admin_two_step";

const STEP_SECS: i64 = 30;
const SECRET_BYTES: usize = 20;
const RECOVERY_CODES: usize = 10;
/// A password-checked sign-in waits this long for its code.
const TICKET_LIFE: Duration = Duration::from_secs(5 * 60);
const TICKET_TRIES: u8 = 5;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/two-step", get(status))
        .route("/me/two-step/start", post(start))
        .route("/me/two-step/confirm", post(confirm))
        .route("/me/two-step/off", post(turn_off))
        .route("/users/{id}/two-step", delete(admin_turn_off))
}

/// Whether `user` must use two-step sign-in for DJINN GO.
pub async fn required(state: &AppState, user: &User) -> anyhow::Result<bool> {
    Ok(match user.role {
        Role::Admin => settings::flag(&state.store, ADMIN_TWO_STEP, false).await?,
        Role::User => state.store.permissions(user).await?.require_two_step,
        Role::Guest => false,
    })
}

/// A new random secret, base32 as authenticator apps expect.
pub fn new_secret() -> anyhow::Result<String> {
    let mut bytes = [0u8; SECRET_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    Ok(BASE32_NOPAD.encode(&bytes))
}

/// The six-digit code for time step `step`.
pub(crate) fn code_at(secret: &[u8], step: i64) -> Option<u32> {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).ok()?;
    mac.update(&step.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[hash.len() - 1] & 0x0f);
    let bytes: [u8; 4] = hash.get(offset..offset + 4)?.try_into().ok()?;
    Some((u32::from_be_bytes(bytes) & 0x7fff_ffff) % 1_000_000)
}

/// The time step a code belongs to, allowing a step either side for clock
/// drift; None if it's wrong.
pub fn check_code(secret_b32: &str, code: &str, unix_secs: i64) -> Option<i64> {
    let secret = BASE32_NOPAD.decode(secret_b32.as_bytes()).ok()?;
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let wanted: u32 = digits.parse().ok()?;
    let now = unix_secs.div_euclid(STEP_SECS);
    (now - 1..=now + 1).find(|&step| code_at(&secret, step) == Some(wanted))
}

fn unix_secs() -> i64 {
    djinn_store::now_ms() / 1000
}

/// Accepts a code from the app (each once) or an unused recovery code.
pub async fn verify(state: &AppState, user_id: i64, code: &str) -> anyhow::Result<bool> {
    let Some(secret) = state.store.two_step_secret(user_id).await? else {
        return Ok(false);
    };
    if let Some(step) = check_code(&secret, code, unix_secs()) {
        return Ok(state.store.use_two_step_code(user_id, step).await?);
    }
    let recovery = normalize_recovery(code);
    if recovery.len() == 10 {
        return Ok(state
            .store
            .use_recovery_code(user_id, &recovery_hash(&recovery))
            .await?);
    }
    Ok(false)
}

fn normalize_recovery(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn recovery_hash(normalized: &str) -> Vec<u8> {
    Sha256::digest(normalized.as_bytes()).to_vec()
}

/// Ten codes like `k3m9x-p2q7w`, and their hashes.
fn new_recovery_codes() -> anyhow::Result<(Vec<String>, Vec<Vec<u8>>)> {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut codes = Vec::with_capacity(RECOVERY_CODES);
    let mut hashes = Vec::with_capacity(RECOVERY_CODES);
    for _ in 0..RECOVERY_CODES {
        let mut bytes = [0u8; 10];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
        let plain: String = bytes
            .iter()
            .map(|b| char::from(ALPHABET[usize::from(*b) % ALPHABET.len()]))
            .collect();
        hashes.push(recovery_hash(&plain));
        codes.push(format!("{}-{}", &plain[..5], &plain[5..]));
    }
    Ok((codes, hashes))
}

/// Sign-ins whose password was right, waiting for the code.
#[derive(Default)]
pub struct Tickets {
    waiting: Mutex<HashMap<String, Ticket>>,
}

#[derive(Clone)]
pub struct Ticket {
    pub user_id: i64,
    pub username: String,
    expires: Instant,
    tries: u8,
}

impl Tickets {
    pub fn issue(&self, user: &User) -> anyhow::Result<String> {
        let token = crate::auth::new_token()?;
        let now = Instant::now();
        let mut waiting = self
            .waiting
            .lock()
            .map_err(|_| anyhow::anyhow!("ticket lock poisoned"))?;
        waiting.retain(|_, t| t.expires > now);
        waiting.insert(
            token.clone(),
            Ticket {
                user_id: user.id,
                username: user.username.clone(),
                expires: now + TICKET_LIFE,
                tries: 0,
            },
        );
        Ok(token)
    }

    /// The ticket, if it's still good.
    pub fn get(&self, token: &str) -> Option<Ticket> {
        let waiting = self.waiting.lock().ok()?;
        waiting
            .get(token)
            .filter(|t| t.expires > Instant::now())
            .cloned()
    }

    /// A wrong code: after a few, the ticket is gone and the password is
    /// needed again.
    pub fn wrong(&self, token: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            if let Some(ticket) = waiting.get_mut(token) {
                ticket.tries += 1;
                if ticket.tries >= TICKET_TRIES {
                    waiting.remove(token);
                }
            }
        }
    }

    pub fn used(&self, token: &str) {
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(token);
        }
    }
}

#[derive(Serialize)]
struct Status {
    on: bool,
    /// DJINN GO asks for it for this account.
    required: bool,
    recovery_codes_left: i64,
}

async fn status(me: CurrentSession, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    Ok(Json(Status {
        on: me.user.two_step,
        required: required(&state, &me.user).await?,
        recovery_codes_left: state.store.recovery_codes_left(me.user.id).await?,
    }))
}

#[derive(Serialize)]
struct Setup {
    /// For typing into the app by hand.
    secret: String,
    /// otpauth:// link, also in the QR code.
    uri: String,
    qr_svg: String,
}

/// Starts turning two-step sign-in on: a new secret to scan. Nothing
/// changes until a code from it is confirmed.
async fn start(me: CurrentSession, State(state): State<AppState>) -> ApiResult<Json<Setup>> {
    if me.user.role == Role::Guest {
        return Err(ApiError::forbidden());
    }
    let secret = new_secret()?;
    state.store.begin_two_step(me.user.id, &secret).await?;
    let uri = format!(
        "otpauth://totp/DJINN:{name}?secret={secret}&issuer=DJINN&digits=6&period=30",
        name = urlencode(&me.user.username),
    );
    let qr_svg = QrCode::new(uri.as_bytes())
        .map_err(anyhow::Error::from)?
        .render::<svg::Color>()
        .min_dimensions(200, 200)
        .quiet_zone(true)
        .build();
    Ok(Json(Setup {
        secret,
        uri,
        qr_svg,
    }))
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
struct Code {
    code: String,
}

#[derive(Serialize)]
struct Confirmed {
    /// Shown once; each works one time if the phone is lost.
    recovery_codes: Vec<String>,
}

/// Turns two-step sign-in on once a code from the new secret checks out.
/// Signs out the account's other devices.
async fn confirm(
    me: CurrentSession,
    State(state): State<AppState>,
    Json(body): Json<Code>,
) -> ApiResult<Json<Confirmed>> {
    let pending = state
        .store
        .pending_two_step(me.user.id)
        .await?
        .ok_or_else(|| ApiError::bad_request("start setting it up first"))?;
    let step = check_code(&pending, &body.code, unix_secs()).ok_or_else(|| {
        ApiError::bad_request("that code doesn't match; check the time on your phone and try again")
    })?;
    let (codes, hashes) = new_recovery_codes()?;
    if !state
        .store
        .confirm_two_step(me.user.id, step, &hashes, Some(me.session_id))
        .await?
    {
        return Err(ApiError::bad_request("start setting it up first"));
    }
    tracing::info!(username = %me.user.username, "two-step sign-in turned on");
    Ok(Json(Confirmed {
        recovery_codes: codes,
    }))
}

#[derive(Deserialize)]
struct Password {
    password: String,
}

/// Turns two-step sign-in off, after the password.
async fn turn_off(
    me: CurrentSession,
    State(state): State<AppState>,
    Json(body): Json<Password>,
) -> ApiResult<StatusCode> {
    if !crate::auth::check_own_password(&state, &me.user, body.password).await? {
        return Err(ApiError::new(StatusCode::FORBIDDEN, "wrong password"));
    }
    state.store.clear_two_step(me.user.id).await?;
    tracing::info!(username = %me.user.username, "two-step sign-in turned off");
    Ok(StatusCode::NO_CONTENT)
}

/// For someone who lost their phone and their recovery codes.
async fn admin_turn_off(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !state.store.clear_two_step(id).await? {
        return Err(ApiError::not_found());
    }
    tracing::info!(admin = %admin.username, user = id, "two-step sign-in turned off by an admin");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_6238_codes() {
        // RFC 6238 test secret ("12345678901234567890") at T = 59 and
        // 1111111109 seconds: 94287082 and 07081804, last six digits.
        let secret = BASE32_NOPAD.encode(b"12345678901234567890");
        assert_eq!(check_code(&secret, "287082", 59), Some(1));
        assert_eq!(
            check_code(&secret, "081804", 1_111_111_109),
            Some(37_037_036)
        );
        // A step either side is fine, two isn't.
        assert_eq!(check_code(&secret, "287082", 59 + 30), Some(1));
        assert_eq!(check_code(&secret, "287082", 59 + 60), None);
        assert_eq!(check_code(&secret, "28708", 59), None);
        assert_eq!(check_code(&secret, "287 082", 59), Some(1));
    }

    #[test]
    fn recovery_codes_look_right() {
        let (codes, hashes) = new_recovery_codes().unwrap();
        assert_eq!(codes.len(), RECOVERY_CODES);
        assert_eq!(codes[0].len(), 11);
        assert_eq!(
            recovery_hash(&normalize_recovery(&codes[0].to_uppercase())),
            hashes[0]
        );
    }
}
