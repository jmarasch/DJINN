//! DJINN GO: personal listening on its own port, off until the admin turns
//! it on.
//!
//! The GO port serves only what GO needs (a fixed list of API routes, the
//! GO web page, and song files) so it can be reached from away from home
//! without exposing the party controls or Admin. Every signed-in route
//! checks the role allows GO, from here or only at home, and that two-step
//! sign-in is on where it's required. Songs are sent as they are; the
//! device decodes them.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_store::SignIn;
use futures_util::StreamExt;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::{go_refusal, new_token, AdminUser, CurrentSession, Device};
use crate::client::DirectTls;
use crate::error::{ApiError, ApiResult};
use crate::go_announce::{self, GO_DISCOVERABLE, SERVER_NAME};
use crate::go_app;
use crate::go_upnp;
use crate::security::{self, TRUSTED_PROXIES};
use crate::state::AppState;
use crate::tls::{self, ENCRYPTION};
use crate::two_step::{self, ADMIN_TWO_STEP};
use crate::{api, settings, web};

/// 1 while DJINN GO is on.
pub const GO_ENABLED: &str = "go_enabled";
/// 1 when the GO listener accepts requests outside the home network.
pub const GO_ALLOW_REMOTE: &str = "go_allow_remote";
/// 1 when signing in from outside the home network may go unencrypted.
const GO_ALLOW_PLAIN_REMOTE: &str = "go_allow_plain_remote";
pub const GO_HTTPS_HOSTNAME: &str = "go_https_hostname";
pub const GO_UPNP: &str = "go_upnp";
/// What the app does with kept songs at sign-out: "delete" or "keep".
const GO_SIGN_OUT_SONGS: &str = "go_sign_out_songs";
/// What it does after too long away from the server: "lock", "delete" or "off".
const GO_AWAY_ACTION: &str = "go_away_action";
const GO_AWAY_DAYS: &str = "go_away_days";
const DEFAULT_AWAY_DAYS: i64 = 30;
const MAX_AWAY_DAYS: i64 = 3650;
/// The default GO address.
pub const DEFAULT_LISTEN: SocketAddr =
    SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 8067);
/// A song link works this long, enough for a long song paused a while.
const GRANT_LIFE: Duration = Duration::from_secs(4 * 60 * 60);
/// A device counts as playing for this long after its last request.
const ACTIVE_FOR: Duration = Duration::from_secs(90);
/// The window for the streaming speed shown to the admin.
const SPEED_WINDOW: Duration = Duration::from_secs(60);

/// Marks requests that came in on the GO port.
#[derive(Clone, Copy, Debug)]
pub struct GoRequest;

/// DJINN GO's running state.
pub struct GoState {
    listen: SocketAddr,
    /// Mirrors the `go_enabled` setting.
    enabled: AtomicBool,
    allow_remote: AtomicBool,
    allow_plain_remote: AtomicBool,
    upnp: Mutex<Option<go_upnp::Mapping>>,
    /// Telling phones on the network about GO, while it's on.
    announcer: Mutex<Option<go_announce::Announcer>>,
    /// Shared with the main port's listener.
    pub encryption: Arc<tls::Encryption>,
    /// Mirrors the `trusted_proxies` setting.
    proxies: RwLock<Vec<IpNet>>,
    /// Stops the running listener.
    running: Mutex<Option<watch::Sender<bool>>>,
    /// Why the listener couldn't start, if it couldn't.
    error: Mutex<Option<String>>,
    grants: Mutex<HashMap<String, Grant>>,
    /// Per session: what it's playing and when it last asked for more.
    playing: Mutex<HashMap<i64, Playing>>,
    /// Bytes sent, for the speed shown to the admin.
    sent: Mutex<VecDeque<(Instant, u64)>>,
}

/// Permission to fetch one song's file, behind a random link.
#[derive(Clone)]
struct Grant {
    user_id: i64,
    username: String,
    session_id: i64,
    title: String,
    path: PathBuf,
    max_streams: i64,
    downloads: bool,
    /// Play an MP3 at this bit rate instead of the original.
    convert_kbps: Option<u32>,
    /// That bit rate is an average.
    vbr: bool,
    /// A copy to keep, not listening: doesn't count as a stream.
    offline: bool,
    expires: Instant,
}

#[derive(Clone)]
struct Playing {
    user_id: i64,
    username: String,
    title: String,
    at: Instant,
}

impl Default for GoState {
    fn default() -> Self {
        Self::new(DEFAULT_LISTEN)
    }
}

impl GoState {
    pub fn new(listen: SocketAddr) -> Self {
        Self {
            listen,
            enabled: AtomicBool::new(false),
            allow_remote: AtomicBool::new(false),
            allow_plain_remote: AtomicBool::new(false),
            upnp: Mutex::default(),
            announcer: Mutex::default(),
            encryption: Arc::default(),
            proxies: RwLock::default(),
            running: Mutex::default(),
            error: Mutex::default(),
            grants: Mutex::default(),
            playing: Mutex::default(),
            sent: Mutex::default(),
        }
    }

    pub fn proxies(&self) -> Vec<IpNet> {
        self.proxies.read().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn port(&self) -> u16 {
        self.listen.port()
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    fn allow_remote(&self) -> bool {
        self.allow_remote.load(Ordering::Relaxed)
    }

    /// Turns GO on without a listener, for tests of its router.
    #[cfg(test)]
    pub fn enable_for_tests(&self) {
        self.enabled.store(true, Ordering::Relaxed);
        self.allow_remote.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn trust_local_proxy_for_tests(&self) {
        if let Ok(mut proxies) = self.proxies.write() {
            *proxies = security::parse_proxies("127.0.0.1");
        }
    }
}

/// Reads GO's settings and starts it if it's on. At startup and after the
/// admin changes them.
pub async fn apply(state: &AppState) -> anyhow::Result<()> {
    let proxies = state.store.get_setting(TRUSTED_PROXIES).await?;
    if let Ok(mut slot) = state.go.proxies.write() {
        *slot = security::parse_proxies(proxies.as_deref().unwrap_or(""));
    }
    let on = settings::flag(&state.store, GO_ENABLED, false).await?;
    state.go.enabled.store(on, Ordering::Relaxed);
    let allow_remote = settings::flag(&state.store, GO_ALLOW_REMOTE, false).await?;
    state.go.allow_remote.store(allow_remote, Ordering::Relaxed);
    let allow_plain = settings::flag(&state.store, GO_ALLOW_PLAIN_REMOTE, false).await?;
    state
        .go
        .allow_plain_remote
        .store(allow_plain, Ordering::Relaxed);
    let hostname = state
        .store
        .get_setting(GO_HTTPS_HOSTNAME)
        .await?
        .unwrap_or_default();
    let upnp = settings::flag(&state.store, GO_UPNP, false).await?;
    if on {
        start(state).await;
    } else {
        stop(state);
    }
    apply_encryption(state).await?;
    announce(state, on).await?;
    let can_map = on && state.go.running.lock().is_ok_and(|r| r.is_some()) && allow_remote && upnp;
    if let Ok(mut slot) = state.go.upnp.lock() {
        if let Some(current) = slot.as_ref() {
            if !can_map || current.hostname != hostname {
                if let Some(old) = slot.take() {
                    old.stop();
                }
            }
        }
        if can_map && slot.is_none() && go_upnp::valid_hostname(&hostname) {
            *slot = Some(go_upnp::start(hostname, state.shutdown.subscribe()));
        }
    }
    Ok(())
}

/// Offers encryption or not, to match the setting. Takes effect for each new
/// connection, on both ports, so nothing has to restart.
pub async fn apply_encryption(state: &AppState) -> anyhow::Result<()> {
    let encryption = &state.go.encryption;
    if !settings::flag(&state.store, ENCRYPTION, true).await? {
        encryption.set(None);
    } else if !encryption.is_on() {
        let dir = state
            .art_dir
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("tls");
        match tls::load_or_make(&dir) {
            Ok(identity) => encryption.set(Some(identity)),
            // Without it everything still works as it did, unencrypted.
            Err(e) => tracing::error!(error = %e, "couldn't set up encrypted connections"),
        }
    }
    Ok(())
}

/// Starts, stops or renews the announcement to match the settings.
async fn announce(state: &AppState, on: bool) -> anyhow::Result<()> {
    let go = &state.go;
    let wanted = on
        && go.running.lock().is_ok_and(|r| r.is_some())
        && settings::flag(&state.store, GO_DISCOVERABLE, true).await?;
    let name = go_announce::server_name(&state.store).await?;
    let fingerprint = go.encryption.fingerprint();
    let stale = go
        .announcer
        .lock()
        .ok()
        .and_then(|mut slot| match slot.as_ref() {
            Some(current)
                if !wanted || current.name != name || current.fingerprint != fingerprint =>
            {
                slot.take()
            }
            _ => None,
        });
    if let Some(old) = stale {
        old.stop();
    }
    let missing = go.announcer.lock().is_ok_and(|slot| slot.is_none());
    if wanted && missing {
        match go_announce::Announcer::start(&state.store, go.listen.port(), fingerprint).await {
            Ok(announcer) => {
                if let Ok(mut slot) = go.announcer.lock() {
                    *slot = Some(announcer);
                }
            }
            // GO still works when typed in; finding it is the only thing lost.
            Err(e) => tracing::warn!(error = %e, "couldn't announce DJINN GO on the network"),
        }
    }
    Ok(())
}

async fn start(state: &AppState) {
    let go = &state.go;
    if go.running.lock().is_ok_and(|r| r.is_some()) {
        return;
    }
    let listener = match tokio::net::TcpListener::bind(go.listen).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(addr = %go.listen, error = %e, "DJINN GO couldn't start");
            if let Ok(mut error) = go.error.lock() {
                *error = Some(format!("couldn't use {}: {e}", go.listen));
            }
            return;
        }
    };
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let mut shutdown = state.shutdown.subscribe();
    let shutting_down = state.shutdown.subscribe();
    let closing = state.clone();
    let listener = match tls::listen(listener, Arc::clone(&go.encryption)) {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, "DJINN GO couldn't start");
            return;
        }
    };
    let app = router(state.clone()).into_make_service_with_connect_info::<tls::Peer>();
    tokio::spawn(async move {
        let stopped = async move {
            tokio::select! {
                _ = stop_rx.wait_for(|stop| *stop) => {}
                _ = shutdown.wait_for(|stop| *stop) => {}
            }
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(stopped)
            .await
        {
            tracing::error!(error = %e, "DJINN GO stopped");
        }
        if *shutting_down.borrow() {
            let announcer = closing.go.announcer.lock().ok().and_then(|mut a| a.take());
            if let Some(announcer) = announcer {
                announcer.stop();
            }
        }
    });
    if let Ok(mut running) = go.running.lock() {
        *running = Some(stop_tx);
    }
    if let Ok(mut error) = go.error.lock() {
        *error = None;
    }
    tracing::info!(addr = %go.listen, "DJINN GO listening");
}

fn stop(state: &AppState) {
    let running = state.go.running.lock().ok().and_then(|mut r| r.take());
    if let Some(stop) = running {
        stop.send_replace(true);
        tracing::info!("DJINN GO stopped");
    }
}

/// Everything the GO port serves.
pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router())
        .route("/go/stream/{token}", get(stream))
        .route(go_app::APK_PATH, get(go_app::download))
        .merge(crate::subsonic::router())
        .fallback(web::go_handler)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(middleware::from_fn(security::same_origin))
        .layer(middleware::from_fn_with_state(state.clone(), headers))
        .layer(middleware::from_fn(crate::client::note_peer))
        .with_state(state)
}

/// Admin > GO, on the main port; and the song links, which only work on GO.
pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/go/status", get(status))
        .route("/go/settings", put(save_settings))
        .route("/go/sign-ins", get(sign_ins))
        .route("/go/play/{id}", get(play))
        .route("/go/offline/{id}", get(offline_link))
        .route("/go/address", get(address))
        .route("/go/hello", get(go_announce::hello))
        .route("/go/app", get(go_app::latest))
        .route("/go/keep-rules", get(keep_rules))
}

/// Which API routes GO answers, and who for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    /// Anyone: signing in and up.
    Public,
    /// Signed in, even before two-step sign-in is set up where required.
    Setup,
    /// Signed in with everything GO asks for.
    Listener,
}

/// `*` matches one path segment, a trailing `**` any rest.
const ROUTES: &[(Method, &str, Access)] = &[
    (Method::GET, "/api/health", Access::Public),
    (Method::GET, "/api/go/hello", Access::Public),
    (Method::GET, "/api/go/app", Access::Public),
    (Method::GET, "/api/auth/mode", Access::Public),
    (Method::POST, "/api/auth/login", Access::Public),
    (Method::POST, "/api/auth/two-step", Access::Public),
    (Method::POST, "/api/auth/signup", Access::Public),
    (Method::GET, "/api/auth/invite/*", Access::Public),
    (Method::POST, "/api/auth/logout", Access::Public),
    (Method::GET, "/api/me", Access::Setup),
    (Method::POST, "/api/me/password", Access::Setup),
    (Method::POST, "/api/me/username", Access::Setup),
    (Method::GET, "/api/me/two-step", Access::Setup),
    (Method::POST, "/api/me/two-step/start", Access::Setup),
    (Method::POST, "/api/me/two-step/confirm", Access::Setup),
    (Method::POST, "/api/me/two-step/off", Access::Setup),
    (Method::GET, "/api/me/sessions", Access::Setup),
    (Method::DELETE, "/api/me/sessions", Access::Setup),
    (Method::DELETE, "/api/me/sessions/*", Access::Setup),
    (Method::GET, "/api/me/app-password", Access::Listener),
    (Method::POST, "/api/me/app-password", Access::Listener),
    (Method::DELETE, "/api/me/app-password", Access::Listener),
    (Method::GET, "/api/search", Access::Listener),
    (Method::GET, "/api/search/ask", Access::Listener),
    (Method::GET, "/api/tracks", Access::Listener),
    (Method::GET, "/api/tracks/jumps", Access::Listener),
    (Method::GET, "/api/tracks/*", Access::Listener),
    (Method::GET, "/api/tracks/*/similar", Access::Listener),
    (Method::GET, "/api/tracks/*/tags", Access::Listener),
    (Method::GET, "/api/albums", Access::Listener),
    (Method::GET, "/api/albums/jumps", Access::Listener),
    (Method::GET, "/api/albums/*", Access::Listener),
    (Method::GET, "/api/artists", Access::Listener),
    (Method::GET, "/api/artists/jumps", Access::Listener),
    (Method::GET, "/api/artists/*", Access::Listener),
    (Method::GET, "/api/artists/*/similar", Access::Listener),
    (Method::GET, "/api/art/*", Access::Listener),
    (Method::GET, "/api/genres", Access::Listener),
    (Method::GET, "/api/genres/*", Access::Listener),
    (Method::GET, "/api/go/play/*", Access::Listener),
    (Method::GET, "/api/go/offline/*", Access::Listener),
    (Method::POST, "/api/go/offline/versions", Access::Listener),
    (Method::GET, "/api/go/offline-suggested", Access::Listener),
    (Method::GET, "/api/go/home", Access::Listener),
    (Method::GET, "/api/go/address", Access::Setup),
    (Method::GET, "/api/go/keep-rules", Access::Listener),
    (Method::GET, "/api/go/likes", Access::Listener),
    (Method::GET, "/api/go/likes/ids", Access::Listener),
    (Method::PUT, "/api/go/likes/*/*", Access::Listener),
    (Method::DELETE, "/api/go/likes/*/*", Access::Listener),
    (Method::POST, "/api/go/plays", Access::Listener),
    (Method::GET, "/api/go/history", Access::Listener),
    (Method::GET, "/api/go/resume", Access::Listener),
    (Method::PUT, "/api/go/resume", Access::Listener),
    (Method::GET, "/api/go/prefs", Access::Listener),
    (Method::PUT, "/api/go/prefs", Access::Listener),
    (Method::GET, "/api/go/playlists", Access::Listener),
    (Method::POST, "/api/go/playlists", Access::Listener),
    (Method::GET, "/api/go/playlists/*", Access::Listener),
    (Method::PATCH, "/api/go/playlists/*", Access::Listener),
    (Method::DELETE, "/api/go/playlists/*", Access::Listener),
    (Method::POST, "/api/go/playlists/*/songs", Access::Listener),
    (Method::PUT, "/api/go/playlists/*/songs", Access::Listener),
    (Method::POST, "/api/playlists/import", Access::Listener),
    (Method::GET, "/api/playlists/youtube-key", Access::Listener),
    (
        Method::GET,
        "/api/playlists/share-targets",
        Access::Listener,
    ),
    (Method::GET, "/api/playlists/*/shares", Access::Listener),
    (Method::PUT, "/api/playlists/*/shares/*", Access::Listener),
    (
        Method::DELETE,
        "/api/playlists/*/shares/*",
        Access::Listener,
    ),
    (
        Method::DELETE,
        "/api/playlists/*/entries/*",
        Access::Listener,
    ),
    (Method::GET, "/api/go/radio/*", Access::Listener),
    (Method::POST, "/api/go/party/*", Access::Listener),
    (Method::GET, "/api/go/submissions", Access::Listener),
    (Method::POST, "/api/go/submissions", Access::Listener),
    (Method::DELETE, "/api/go/submissions/*", Access::Listener),
];

fn matches(pattern: &str, path: &str) -> bool {
    let mut want = pattern.split('/');
    let mut got = path.trim_end_matches('/').split('/');
    loop {
        match (want.next(), got.next()) {
            (Some("**"), _) => return true,
            (Some("*"), Some(seg)) if !seg.is_empty() => {}
            (Some(w), Some(g)) if w == g => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn access(method: &Method, path: &str) -> Option<Access> {
    ROUTES
        .iter()
        .find(|(m, pattern, _)| m == method && matches(pattern, path))
        .map(|&(_, _, access)| access)
}

/// Whether a path takes a password or a session, which mustn't cross the
/// internet unencrypted. Song links are only good for one song, and cast devices
/// can't use DJINN's own certificate; the page and the app hold nothing private.
fn carries_sign_in(path: &str) -> bool {
    const SAYS_NOTHING_PRIVATE: &[&str] = &[
        "/api/health",
        "/api/go/hello",
        "/api/go/app",
        "/api/auth/mode",
    ];
    path.starts_with("/rest/")
        || (path.starts_with("/api/") && !SAYS_NOTHING_PRIVATE.contains(&path))
}

/// Lets through only what GO serves, to whom it should.
async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    match check(&state, request).await {
        Ok(request) => next.run(request).await,
        Err(e) => e.into_response(),
    }
}

async fn check(state: &AppState, request: Request) -> ApiResult<Request> {
    if !state.go.enabled() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DJINN GO is off",
        ));
    }
    let origin = security::origin(
        security::peer(request.extensions()),
        request.headers(),
        &state.go.proxies(),
    );
    if !state.go.allow_remote() && !origin.home {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "DJINN GO only accepts home network and Tailscale connections",
        ));
    }
    if !origin.home
        && !origin.https
        && !state.go.allow_plain_remote.load(Ordering::Relaxed)
        && request.extensions().get::<DirectTls>().is_none()
        && carries_sign_in(request.uri().path())
    {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "away from home, DJINN GO needs an encrypted connection: open it with https://. \
             If this comes through a proxy or tunnel on this computer, add it to the trusted \
             proxies in Admin > GO; to allow it unencrypted anyway, there's a switch there too",
        ));
    }
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(GoRequest);
    let path = parts.uri.path().to_owned();
    if path.starts_with("/rest/") {
        // Subsonic apps: each call carries its own sign-in (see subsonic.rs).
        return Ok(Request::from_parts(parts, body));
    }
    if !path.starts_with("/api/") && path != "/api" {
        // The page, its files, and song links (which carry their own key).
        if parts.method != Method::GET && parts.method != Method::HEAD {
            return Err(ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "not allowed"));
        }
        return Ok(Request::from_parts(parts, body));
    }
    let access = access(&parts.method, &path).ok_or_else(ApiError::not_found)?;
    if access != Access::Public {
        let session = CurrentSession::from_request_parts(&mut parts, state).await?;
        let Ok(device) = Device::from_request_parts(&mut parts, state).await;
        if let Some((_, message)) = go_refusal(state, &session.user, &device).await? {
            return Err(ApiError::new(StatusCode::FORBIDDEN, message));
        }
        if access == Access::Listener
            && !session.user.two_step
            && two_step::required(state, &session.user).await?
        {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "turn on two-step sign-in to use DJINN GO",
            ));
        }
    }
    Ok(Request::from_parts(parts, body))
}

/// Security headers on everything GO sends.
async fn headers(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let origin = security::origin(
        security::peer(request.extensions()),
        request.headers(),
        &state.go.proxies(),
    );
    let mut response = next.run(request).await;
    let status = response.status();
    if status.is_server_error() {
        tracing::error!(ip = ?origin.ip, %method, status = status.as_u16(),
                        elapsed_ms = started.elapsed().as_millis(), "GO request failed");
    } else if status.is_client_error() {
        tracing::warn!(ip = ?origin.ip, %method, status = status.as_u16(),
                       elapsed_ms = started.elapsed().as_millis(), "GO request refused");
    } else {
        tracing::trace!(ip = ?origin.ip, %method, status = status.as_u16(),
                        elapsed_ms = started.elapsed().as_millis(), "GO request");
    }
    security::set_headers(response.headers_mut(), security::GO_CSP, origin.https);
    response
}

#[derive(Serialize)]
struct PlayLinks {
    /// Plays the song; works for a few hours.
    url: String,
    /// Saves the file, when the role allows downloads.
    download_url: Option<String>,
    /// The file's type, so the player can check it can play it.
    mime: &'static str,
    /// Volume for even loudness (ReplayGain, as set for the party), 1 for
    /// unchanged. The device applies it.
    gain: f32,
    /// `url` plays an MP3 converted from the original.
    converted: bool,
    /// Asking again with `?convert=1` would get an MP3 (converting is on
    /// and the role allows it).
    can_convert: bool,
}

#[derive(Deserialize)]
struct PlayQuery {
    /// Converted to MP3, when converting is available; the original
    /// otherwise.
    #[serde(default)]
    convert: bool,
}

/// The type browsers know a song file by.
fn mime_for(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" | "mp4" | "m4b" | "aac" | "alac" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/ogg; codecs=opus",
        "wav" => "audio/wav",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

/// A short-lived link to one song's file. Only on the GO port.
async fn play(
    me: CurrentSession,
    State(state): State<AppState>,
    device: Device,
    Path(id): Path<i64>,
    Query(query): Query<PlayQuery>,
) -> ApiResult<Json<PlayLinks>> {
    if !device.go {
        return Err(ApiError::not_found());
    }
    let convert_kbps = crate::convert::available(&state, &me.user).await?;
    let convert = query.convert && convert_kbps.is_some();
    let track = state
        .store
        .track(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let path = state
        .store
        .track_path(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let permissions = state.store.permissions(&me.user).await?;
    let path = PathBuf::from(path);
    let mime = if convert {
        "audio/mpeg"
    } else {
        mime_for(&path)
    };
    let replay_gain = state.jukebox.lock().await.replay_gain();
    let gain = match state.store.gain_info(id).await? {
        Some(info) => replay_gain.linear(&info),
        None => 1.0,
    };
    let token = new_token()?;
    let now = Instant::now();
    if let Ok(mut grants) = state.go.grants.lock() {
        grants.retain(|_, g| g.expires > now);
        grants.insert(
            token.clone(),
            Grant {
                user_id: me.user.id,
                username: me.user.name().to_owned(),
                session_id: me.session_id,
                title: match &track.artist {
                    Some(artist) => format!("{artist} - {}", track.title),
                    None => track.title.clone(),
                },
                path,
                max_streams: permissions.max_streams,
                downloads: permissions.downloads,
                convert_kbps: convert_kbps.filter(|_| convert),
                vbr: false,
                offline: false,
                expires: now + GRANT_LIFE,
            },
        );
    }
    let url = format!("/go/stream/{token}");
    Ok(Json(PlayLinks {
        download_url: permissions.downloads.then(|| format!("{url}?download=1")),
        url,
        mime,
        gain,
        converted: convert,
        can_convert: convert_kbps.is_some(),
    }))
}

#[derive(Deserialize)]
struct OfflineQuery {
    /// An MP3 at about this bit rate for lossless songs; the original without.
    kbps: Option<u32>,
    /// `kbps` is an average, not a constant.
    #[serde(default)]
    vbr: bool,
}

#[derive(Serialize)]
struct OfflineLink {
    url: String,
    /// What the file at `url` will be.
    mime: &'static str,
    gain: f32,
    /// `url` is an MP3 made from the original.
    converted: bool,
    /// Asked to convert, but this account can't: `url` is the original.
    convert_refused: bool,
    /// Changes when the file does, so an old copy can be replaced.
    version: String,
}

/// Whether converting a song saves anything: compressed formats stay as
/// they are.
fn lossless(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    ["flac", "wav", "aif", "aiff"]
        .iter()
        .any(|l| ext.eq_ignore_ascii_case(l))
}

/// A link to fetch a copy of a song to keep on a device (the app's downloads
/// and smart cache). Needs the role's downloads permission, and neither counts
/// as a stream nor shows as listening. Only on the GO port.
async fn offline_link(
    me: CurrentSession,
    State(state): State<AppState>,
    device: Device,
    Path(id): Path<i64>,
    Query(query): Query<OfflineQuery>,
) -> ApiResult<Json<OfflineLink>> {
    if !device.go {
        return Err(ApiError::not_found());
    }
    let permissions = state.store.permissions(&me.user).await?;
    if !permissions.downloads {
        return Err(ApiError::forbidden());
    }
    let track = state
        .store
        .track(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let path = PathBuf::from(
        state
            .store
            .track_path(id)
            .await?
            .ok_or_else(ApiError::not_found)?,
    );
    let version = state
        .store
        .track_versions(&[id])
        .await?
        .pop()
        .map(|v| v.version)
        .ok_or_else(ApiError::not_found)?;
    let wanted = query.kbps.filter(|_| lossless(&path));
    let allowed = crate::convert::available(&state, &me.user).await?.is_some();
    let kbps = wanted.filter(|_| allowed).map(crate::convert::snap);
    let replay_gain = state.jukebox.lock().await.replay_gain();
    let gain = match state.store.gain_info(id).await? {
        Some(info) => replay_gain.linear(&info),
        None => 1.0,
    };
    let mime = if kbps.is_some() {
        "audio/mpeg"
    } else {
        mime_for(&path)
    };
    let token = new_token()?;
    let now = Instant::now();
    if let Ok(mut grants) = state.go.grants.lock() {
        grants.retain(|_, g| g.expires > now);
        grants.insert(
            token.clone(),
            Grant {
                user_id: me.user.id,
                username: me.user.name().to_owned(),
                session_id: me.session_id,
                title: track.title,
                path,
                max_streams: 0,
                downloads: true,
                convert_kbps: kbps,
                vbr: query.vbr,
                offline: true,
                expires: now + GRANT_LIFE,
            },
        );
    }
    Ok(Json(OfflineLink {
        url: format!("/go/stream/{token}"),
        mime,
        gain,
        converted: kbps.is_some(),
        convert_refused: wanted.is_some() && !allowed,
        version,
    }))
}

#[derive(Deserialize)]
struct StreamQuery {
    #[serde(default)]
    download: bool,
}

/// Sends a song's file as it is (with ranges, for seeking) to a link from
/// `play`. Refuses when the account is already playing on as many other
/// devices as its role allows.
async fn stream(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<StreamQuery>,
    request: Request,
) -> Response {
    let now = Instant::now();
    let grant = state
        .go
        .grants
        .lock()
        .ok()
        .and_then(|grants| grants.get(&token).filter(|g| g.expires > now).cloned());
    let Some(grant) = grant else {
        return ApiError::new(StatusCode::FORBIDDEN, "this link has expired").into_response();
    };
    // Signing out, a new password or a turned-off account ends its links too.
    match state.store.session_is_live(grant.session_id).await {
        Ok(true) => {}
        Ok(false) => {
            return ApiError::new(StatusCode::FORBIDDEN, "this link has expired").into_response()
        }
        Err(e) => return ApiError::from(e).into_response(),
    }
    if query.download && !grant.downloads {
        return ApiError::forbidden().into_response();
    }
    // Downloads are always the original file.
    let path = match grant.convert_kbps.filter(|_| !query.download) {
        Some(kbps) => {
            match crate::convert::converted_as(&state, &grant.path, kbps, grant.vbr).await {
                Ok(path) => path,
                Err(e) => return e.into_response(),
            }
        }
        None => grant.path.clone(),
    };
    send_song(
        &state,
        &Listening {
            user_id: grant.user_id,
            username: grant.username,
            session_id: grant.session_id,
            title: grant.title,
            max_streams: grant.max_streams,
            counts: !grant.offline,
        },
        &path,
        query.download,
        request,
    )
    .await
}

/// Who's playing a song, for the "streams at once" limit and Admin > GO.
pub struct Listening {
    pub user_id: i64,
    pub username: String,
    /// The device: a session, or an app.
    pub session_id: i64,
    pub title: String,
    pub max_streams: i64,
    /// Someone listening, not a copy being kept.
    pub counts: bool,
}

/// Sends a song's file as it is (with ranges, for seeking), unless the
/// account is already playing on as many other devices as its role
/// allows. `download` asks the browser to save it.
pub async fn send_song(
    state: &AppState,
    who: &Listening,
    path: &std::path::Path,
    download: bool,
    request: Request,
) -> Response {
    let now = Instant::now();
    if let Some(mut playing) = state.go.playing.lock().ok().filter(|_| who.counts) {
        playing.retain(|_, p| now.duration_since(p.at) < ACTIVE_FOR);
        let elsewhere = playing
            .iter()
            .filter(|(session, p)| p.user_id == who.user_id && **session != who.session_id)
            .count();
        if who.max_streams > 0 && i64::try_from(elsewhere).unwrap_or(i64::MAX) >= who.max_streams {
            return ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                format!(
                    "you're already listening on {elsewhere} other {}; stop one first",
                    if elsewhere == 1 { "device" } else { "devices" }
                ),
            )
            .into_response();
        }
        playing.insert(
            who.session_id,
            Playing {
                user_id: who.user_id,
                username: who.username.clone(),
                title: who.title.clone(),
                at: now,
            },
        );
    }
    let mut response = match ServeFile::new(path).oneshot(request).await {
        Ok(response) => response.map(Body::new),
        Err(e) => match e {},
    };
    if let Some(length) = response
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if let Ok(mut sent) = state.go.sent.lock() {
            sent.retain(|(at, _)| now.duration_since(*at) < SPEED_WINDOW);
            sent.push_back((now, length));
        }
    }
    let h = response.headers_mut();
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=14400"),
    );
    if download {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().replace(['"', '\\'], ""))
            .unwrap_or_else(|| "song".to_owned());
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
            h.insert(header::CONTENT_DISPOSITION, value);
        }
    }
    let (parts, body) = response.into_parts();
    let audit = TransferAudit {
        user_id: who.user_id,
        session_id: who.session_id,
        bytes: 0,
        finished: std::cell::Cell::new(false),
    };
    let chunks = futures_util::stream::try_unfold(
        (Box::pin(body.into_data_stream()), audit),
        |(mut source, mut audit)| async move {
            match source.next().await {
                Some(Ok(chunk)) => {
                    audit.bytes = audit.bytes.saturating_add(chunk.len() as u64);
                    Ok(Some((chunk, (source, audit))))
                }
                Some(Err(error)) => {
                    tracing::error!(user_id = audit.user_id, session_id = audit.session_id,
                        bytes = audit.bytes, %error, "GO stream read failed");
                    Err(error)
                }
                None => {
                    audit.finished.set(true);
                    Ok(None)
                }
            }
        },
    );
    Response::from_parts(parts, Body::from_stream(chunks))
}

struct TransferAudit {
    user_id: i64,
    session_id: i64,
    bytes: u64,
    finished: std::cell::Cell<bool>,
}

impl Drop for TransferAudit {
    fn drop(&mut self) {
        if self.finished.get() {
            tracing::trace!(
                user_id = self.user_id,
                session_id = self.session_id,
                bytes = self.bytes,
                "GO stream finished"
            );
        } else {
            tracing::trace!(
                user_id = self.user_id,
                session_id = self.session_id,
                bytes = self.bytes,
                "GO stream disconnected"
            );
        }
    }
}

#[derive(Serialize)]
struct Listener {
    username: String,
    title: String,
    seconds_ago: u64,
}

#[derive(Serialize)]
struct Status {
    /// The Android app this server carries, and how to get it.
    android_app: Option<AndroidApp>,
    /// Encrypted connections are offered.
    encryption: bool,
    /// For comparing with what the app shows.
    security_code: Option<String>,
    keep: KeepRules,
    /// What the admin named this server; empty means `default_name`.
    server_name: String,
    default_name: String,
    /// Phones on the network can find it.
    discoverable: bool,
    enabled: bool,
    allow_remote: bool,
    allow_plain_remote: bool,
    https_hostname: String,
    upnp: bool,
    upnp_error: Option<String>,
    upnp_external_ip: Option<String>,
    upnp_verified: bool,
    running: bool,
    /// Why it couldn't start.
    error: Option<String>,
    port: u16,
    /// Addresses on the home network.
    addresses: Vec<String>,
    /// This computer's Tailscale address, when Tailscale is running.
    tailscale: Option<String>,
    trusted_proxies: String,
    admin_two_step: bool,
    listening: Vec<Listener>,
    /// Megabits a second sent over the last minute.
    mbps: f64,
}

async fn status_of(state: &AppState) -> anyhow::Result<Status> {
    let go = &state.go;
    let now = Instant::now();
    let mut listening: Vec<Listener> = go
        .playing
        .lock()
        .map(|playing| {
            playing
                .values()
                .filter(|p| now.duration_since(p.at) < ACTIVE_FOR)
                .map(|p| Listener {
                    username: p.username.clone(),
                    title: p.title.clone(),
                    seconds_ago: now.duration_since(p.at).as_secs(),
                })
                .collect()
        })
        .unwrap_or_default();
    listening.sort_by_key(|l| l.seconds_ago);
    let bytes: u64 = go
        .sent
        .lock()
        .map(|sent| {
            sent.iter()
                .filter(|(at, _)| now.duration_since(*at) < SPEED_WINDOW)
                .map(|(_, n)| n)
                .sum()
        })
        .unwrap_or(0);
    #[allow(clippy::cast_precision_loss)]
    let mbps = bytes as f64 * 8.0 / SPEED_WINDOW.as_secs_f64() / 1_000_000.0;
    let port = go.listen.port();
    Ok(Status {
        android_app: android_app(state),
        encryption: settings::flag(&state.store, ENCRYPTION, true).await?,
        security_code: go
            .encryption
            .fingerprint()
            .map(|fingerprint| tls::security_code(&fingerprint)),
        keep: keep_rules_of(&state.store).await?,
        server_name: state
            .store
            .get_setting(SERVER_NAME)
            .await?
            .unwrap_or_default(),
        default_name: go_announce::computer_name(),
        discoverable: settings::flag(&state.store, GO_DISCOVERABLE, true).await?,
        enabled: go.enabled(),
        allow_remote: go.allow_remote(),
        allow_plain_remote: settings::flag(&state.store, GO_ALLOW_PLAIN_REMOTE, false).await?,
        https_hostname: state
            .store
            .get_setting(GO_HTTPS_HOSTNAME)
            .await?
            .unwrap_or_default(),
        upnp: settings::flag(&state.store, GO_UPNP, false).await?,
        upnp_error: go.upnp.lock().ok().and_then(|m| {
            m.as_ref()
                .and_then(|m| m.error.lock().ok().and_then(|e| e.clone()))
        }),
        upnp_external_ip: go.upnp.lock().ok().and_then(|m| {
            m.as_ref().and_then(|m| {
                m.external_ip
                    .lock()
                    .ok()
                    .and_then(|ip| ip.map(|ip| ip.to_string()))
            })
        }),
        upnp_verified: go
            .upnp
            .lock()
            .ok()
            .and_then(|m| m.as_ref().map(|m| m.verified.load(Ordering::Relaxed)))
            .unwrap_or(false),
        running: go.running.lock().is_ok_and(|r| r.is_some()),
        error: go.error.lock().ok().and_then(|e| e.clone()),
        port,
        addresses: crate::banner::urls(go.listen),
        tailscale: tailscale_ip().map(|ip| format!("http://{}", SocketAddr::new(ip, port))),
        trusted_proxies: state
            .store
            .get_setting(TRUSTED_PROXIES)
            .await?
            .unwrap_or_default(),
        admin_two_step: settings::flag(&state.store, ADMIN_TWO_STEP, false).await?,
        listening,
        mbps,
    })
}

#[derive(Deserialize)]
struct HereQuery {
    /// The address the browser has GO open at.
    #[serde(default)]
    here: String,
}

#[derive(Serialize)]
struct Address {
    /// What phones should open: the address in use, unless that's this
    /// computer's own name, then the home network one.
    url: String,
    qr_svg: String,
    /// Other ways in (home network, Tailscale).
    others: Vec<String>,
}

fn qr_svg(text: &str) -> anyhow::Result<String> {
    Ok(qrcode::QrCode::new(text.as_bytes())?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .dark_color(qrcode::render::svg::Color("#14111f"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build())
}

#[derive(Serialize)]
struct AndroidApp {
    #[serde(flatten)]
    release: go_app::AppRelease,
    url: String,
    qr_svg: String,
}

/// For Admin > GO: the app, and a QR code for a phone's camera to fetch it.
fn android_app(state: &AppState) -> Option<AndroidApp> {
    let release = go_app::release()?;
    let base = crate::banner::urls(state.go.listen).into_iter().next()?;
    let url = format!("{base}{}", go_app::APK_PATH);
    let qr_svg = qr_svg(&url).ok()?;
    Some(AndroidApp {
        release,
        url,
        qr_svg,
    })
}

/// What a QR code says: the address, and the certificate to expect. A phone's
/// camera opens the address and browsers keep what's after the # to themselves;
/// the app's scanner remembers the certificate, which makes this the one way
/// of connecting that someone on the network can't get in the middle of.
fn qr_text(url: &str, fingerprint: Option<String>) -> String {
    match fingerprint {
        Some(fingerprint) => format!("{url}#fp={fingerprint}"),
        None => url.to_owned(),
    }
}

/// GO's address and a QR code for it, for opening GO on a phone.
async fn address(
    _: CurrentSession,
    State(state): State<AppState>,
    device: Device,
    Query(query): Query<HereQuery>,
) -> ApiResult<Json<Address>> {
    if !device.go {
        return Err(ApiError::not_found());
    }
    let port = state.go.listen.port();
    let mut urls = crate::banner::urls(state.go.listen);
    if let Some(ip) = tailscale_ip() {
        urls.push(format!("http://{}", SocketAddr::new(ip, port)));
    }
    let here = query.here.trim().trim_end_matches('/');
    let local =
        here.contains("://localhost") || here.contains("://127.") || here.contains("://[::1]");
    let valid = (here.starts_with("http://") || here.starts_with("https://")) && here.len() < 300;
    if valid && !local {
        urls.retain(|u| u != here);
        urls.insert(0, here.to_owned());
    }
    let url = urls.first().cloned().ok_or_else(ApiError::not_found)?;
    let qr_svg = qr_svg(&qr_text(&url, state.go.encryption.fingerprint()))?;
    Ok(Json(Address {
        url,
        qr_svg,
        others: urls.into_iter().skip(1).collect(),
    }))
}

/// This computer's address on Tailscale (100.64.0.0/10), if it has one.
fn tailscale_ip() -> Option<IpAddr> {
    if_addrs::get_if_addrs().ok()?.into_iter().find_map(|i| {
        let ip = i.ip();
        match ip {
            IpAddr::V4(v4) => {
                let [a, b, ..] = v4.octets();
                (a == 100 && (64..128).contains(&b)).then_some(ip)
            }
            IpAddr::V6(_) => None,
        }
    })
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    Ok(Json(status_of(&state).await?))
}

#[derive(Deserialize)]
struct GoSettings {
    #[serde(default = "yes")]
    encryption: bool,
    #[serde(default)]
    keep: KeepRules,
    #[serde(default)]
    server_name: String,
    #[serde(default = "yes")]
    discoverable: bool,
    enabled: bool,
    #[serde(default)]
    allow_remote: bool,
    #[serde(default)]
    allow_plain_remote: bool,
    #[serde(default)]
    https_hostname: String,
    #[serde(default)]
    upnp: bool,
    #[serde(default)]
    trusted_proxies: String,
    #[serde(default)]
    admin_two_step: bool,
}

fn yes() -> bool {
    true
}

/// The admin's rules for songs kept on devices. The app gets them at each
/// contact and follows them while it's away, so they apply from then on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct KeepRules {
    sign_out: String,
    away: String,
    away_days: i64,
}

impl Default for KeepRules {
    fn default() -> Self {
        Self {
            sign_out: "delete".to_owned(),
            away: "lock".to_owned(),
            away_days: DEFAULT_AWAY_DAYS,
        }
    }
}

impl KeepRules {
    fn check(&self) -> Result<(), &'static str> {
        if !matches!(self.sign_out.as_str(), "delete" | "keep") {
            return Err("at sign-out, kept songs are deleted or kept");
        }
        if !matches!(self.away.as_str(), "lock" | "delete" | "off") {
            return Err("after too long away, kept songs are locked, deleted or left alone");
        }
        if !(1..=MAX_AWAY_DAYS).contains(&self.away_days) {
            return Err("the time away must be from 1 to 3650 days");
        }
        Ok(())
    }
}

async fn keep_rules_of(store: &djinn_store::Store) -> anyhow::Result<KeepRules> {
    let defaults = KeepRules::default();
    Ok(KeepRules {
        sign_out: store
            .get_setting(GO_SIGN_OUT_SONGS)
            .await?
            .unwrap_or(defaults.sign_out),
        away: store
            .get_setting(GO_AWAY_ACTION)
            .await?
            .unwrap_or(defaults.away),
        away_days: settings::number(store, GO_AWAY_DAYS, defaults.away_days).await?,
    })
}

async fn keep_rules(
    _: CurrentSession,
    State(state): State<AppState>,
) -> ApiResult<Json<KeepRules>> {
    Ok(Json(keep_rules_of(&state.store).await?))
}

async fn save_settings(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<GoSettings>,
) -> ApiResult<Json<Status>> {
    let server_name = go_announce::clean_name(&body.server_name).map_err(ApiError::bad_request)?;
    body.keep.check().map_err(ApiError::bad_request)?;
    let mut proxies = body.trusted_proxies.trim().to_owned();
    let parsed = security::parse_proxies(&proxies);
    let entries = proxies
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .count();
    if parsed.len() != entries {
        return Err(ApiError::bad_request(
            "trusted proxies must be addresses like 127.0.0.1 or ranges like 10.0.0.0/8",
        ));
    }
    if body.admin_two_step && !admin.two_step {
        return Err(ApiError::bad_request(
            "turn on two-step sign-in for your own account first",
        ));
    }
    let hostname = body.https_hostname.trim().to_ascii_lowercase();
    if !hostname.is_empty() && !go_upnp::valid_hostname(&hostname) {
        return Err(ApiError::bad_request(
            "enter a DNS hostname such as music.example.com",
        ));
    }
    if body.upnp && body.enabled && body.allow_remote && hostname.is_empty() {
        return Err(ApiError::bad_request("turn on GO and outside connections, then enter your HTTPS hostname before enabling UPnP"));
    }
    let upnp = body.upnp && body.enabled && body.allow_remote;
    if upnp
        && !parsed
            .iter()
            .any(|net| net.contains(&IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)))
    {
        if !proxies.is_empty() {
            proxies.push(',');
        }
        proxies.push_str("127.0.0.1");
    }
    settings::set_flag(&state.store, ENCRYPTION, body.encryption).await?;
    state.store.set_setting(SERVER_NAME, &server_name).await?;
    state
        .store
        .set_setting(GO_SIGN_OUT_SONGS, &body.keep.sign_out)
        .await?;
    state
        .store
        .set_setting(GO_AWAY_ACTION, &body.keep.away)
        .await?;
    state
        .store
        .set_setting(GO_AWAY_DAYS, &body.keep.away_days.to_string())
        .await?;
    settings::set_flag(&state.store, GO_DISCOVERABLE, body.discoverable).await?;
    settings::set_flag(&state.store, GO_ENABLED, body.enabled).await?;
    settings::set_flag(&state.store, GO_ALLOW_REMOTE, body.allow_remote).await?;
    settings::set_flag(&state.store, GO_ALLOW_PLAIN_REMOTE, body.allow_plain_remote).await?;
    state
        .store
        .set_setting(GO_HTTPS_HOSTNAME, &hostname)
        .await?;
    settings::set_flag(&state.store, GO_UPNP, upnp).await?;
    state.store.set_setting(TRUSTED_PROXIES, &proxies).await?;
    settings::set_flag(&state.store, ADMIN_TWO_STEP, body.admin_two_step).await?;
    apply(&state).await?;
    tracing::warn!(
        on = body.enabled,
        allow_remote = body.allow_remote,
        "DJINN GO settings changed"
    );
    Ok(Json(status_of(&state).await?))
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    200
}

async fn sign_ins(
    _: AdminUser,
    State(state): State<AppState>,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<Vec<SignIn>>> {
    Ok(Json(
        state.store.sign_ins(query.limit.clamp(1, 1000)).await?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_app_can_be_fetched_before_signing_in() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let (status, body, _) = go(&state, Method::GET, "/api/go/app", None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let (status, _, _) = go(&state, Method::GET, go_app::APK_PATH, None, None).await;
        if go_app::release().is_some() {
            assert_eq!(status, StatusCode::OK);
        } else {
            assert_eq!(body, "null");
            assert_eq!(status, StatusCode::NOT_FOUND);
        }
    }

    #[test]
    fn qr_codes_carry_the_certificate_when_there_is_one() {
        assert_eq!(
            qr_text("http://10.0.0.2:8067", None),
            "http://10.0.0.2:8067"
        );
        assert_eq!(
            qr_text("http://10.0.0.2:8067", Some("abc".to_owned())),
            "http://10.0.0.2:8067#fp=abc"
        );
    }

    #[tokio::test]
    async fn encryption_is_on_unless_turned_off() {
        let dir = std::env::temp_dir().join(format!("djinn-go-tls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let state = AppState::for_tests_in(&dir).await;
        apply_encryption(&state).await.unwrap();
        let fingerprint = state.go.encryption.fingerprint();
        assert!(fingerprint.is_some());
        assert!(dir.join("tls").join("djinn.key").exists());

        state.go.enable_for_tests();
        let (_, body, _) = go(&state, Method::GET, "/api/go/hello", None, None).await;
        let hello: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(hello["encrypted"], true);

        settings::set_flag(&state.store, ENCRYPTION, false)
            .await
            .unwrap();
        apply_encryption(&state).await.unwrap();
        assert!(!state.go.encryption.is_on());
        let (_, body, _) = go(&state, Method::GET, "/api/go/hello", None, None).await;
        let hello: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(hello["encrypted"], false);

        settings::set_flag(&state.store, ENCRYPTION, true)
            .await
            .unwrap();
        apply_encryption(&state).await.unwrap();
        assert_eq!(
            state.go.encryption.fingerprint(),
            fingerprint,
            "the same one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keep_rules_are_checked() {
        let good = KeepRules::default();
        assert_eq!(good.check(), Ok(()));
        assert_eq!(
            good.away, "lock",
            "locked, not deleted, unless the admin says"
        );
        for rules in [
            KeepRules {
                sign_out: "shred".to_owned(),
                ..good.clone()
            },
            KeepRules {
                away: "explode".to_owned(),
                ..good.clone()
            },
            KeepRules {
                away_days: 0,
                ..good.clone()
            },
            KeepRules {
                away_days: MAX_AWAY_DAYS + 1,
                ..good.clone()
            },
        ] {
            assert!(rules.check().is_err(), "{rules:?}");
        }
    }

    #[tokio::test]
    async fn keep_rules_read_what_was_saved() {
        let state = AppState::for_tests().await;
        assert_eq!(
            keep_rules_of(&state.store).await.unwrap(),
            KeepRules::default()
        );
        state
            .store
            .set_setting(GO_AWAY_ACTION, "off")
            .await
            .unwrap();
        state.store.set_setting(GO_AWAY_DAYS, "90").await.unwrap();
        state
            .store
            .set_setting(GO_SIGN_OUT_SONGS, "keep")
            .await
            .unwrap();
        assert_eq!(
            keep_rules_of(&state.store).await.unwrap(),
            KeepRules {
                sign_out: "keep".to_owned(),
                away: "off".to_owned(),
                away_days: 90,
            }
        );
        assert_eq!(
            access(&Method::GET, "/api/go/keep-rules"),
            Some(Access::Listener)
        );
    }

    #[tokio::test]
    async fn a_server_says_who_it_is_before_sign_in() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let (status, body, _) = go(&state, Method::GET, "/api/go/hello", None, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let first: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(first["name"], go_announce::computer_name());
        assert_eq!(first["id"].as_str().map(str::len), Some(16));

        state
            .store
            .set_setting(SERVER_NAME, "Dorm 4")
            .await
            .unwrap();
        let (_, body, _) = go(&state, Method::GET, "/api/go/hello", None, None).await;
        let second: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(second["name"], "Dorm 4");
        assert_eq!(second["id"], first["id"], "the id stays");
    }

    #[test]
    fn route_patterns() {
        assert_eq!(
            access(&Method::GET, "/api/tracks/12"),
            Some(Access::Listener)
        );
        assert_eq!(
            access(&Method::GET, "/api/tracks/12/similar"),
            Some(Access::Listener)
        );
        assert_eq!(access(&Method::GET, "/api/tracks/12/edit"), None);
        assert_eq!(access(&Method::PUT, "/api/tracks/edit"), None);
        assert_eq!(access(&Method::GET, "/api/users"), None);
        assert_eq!(access(&Method::GET, "/api/go/status"), None);
        assert_eq!(access(&Method::POST, "/api/queue"), None);
        assert_eq!(access(&Method::GET, "/api/events"), None);
        assert_eq!(
            access(&Method::POST, "/api/auth/login"),
            Some(Access::Public)
        );
        assert_eq!(access(&Method::POST, "/api/auth/bootstrap"), None);
        assert_eq!(access(&Method::POST, "/api/auth/guest"), None);
        assert_eq!(access(&Method::GET, "/api/me"), Some(Access::Setup));
        assert_eq!(
            access(&Method::POST, "/api/me/username"),
            Some(Access::Setup)
        );
    }

    /// Sends a request to the GO port; returns status, body and any cookie.
    async fn go(
        state: &AppState,
        method: Method,
        path: &str,
        body: Option<&str>,
        cookie: Option<&str>,
    ) -> (StatusCode, String, Option<String>) {
        // Model a remote client reaching GO through a trusted local HTTPS
        // proxy. The server still sees the real address for role checks.
        state.go.trust_local_proxy_for_tests();
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("x-forwarded-for", "198.51.100.42")
            .header("x-forwarded-proto", "https");
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(cookie) = cookie {
            req = req.header(header::COOKIE, cookie);
        }
        let mut req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                50000,
            ))));
        let res = router(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::to_owned);
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&bytes).into_owned(), cookie)
    }

    /// An account whose password is "correct horse", so the real login
    /// path runs.
    async fn account(state: &AppState, name: &str, role_id: i64) {
        use argon2::password_hash::PasswordHasher;
        let hash = argon2::Argon2::default()
            .hash_password(b"correct horse")
            .unwrap()
            .to_string();
        state
            .store
            .create_account(
                name,
                &hash,
                djinn_store::Role::User,
                role_id,
                djinn_store::AccountStatus::Active,
            )
            .await
            .unwrap();
    }

    fn login(name: &str) -> String {
        format!(r#"{{"username":"{name}","password":"correct horse"}}"#)
    }

    fn field(json: &str, key: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(json).unwrap()[key].clone()
    }

    #[tokio::test]
    async fn outside_access_needs_the_switch_and_encryption_unless_allowed() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
        let call = |ip: [u8; 4], proto: Option<&str>| {
            let mut request = Request::builder().uri("/api/auth/mode");
            if let Some(proto) = proto {
                request = request.header("x-forwarded-proto", proto);
            }
            let mut request = request.body(Body::empty()).unwrap();
            request
                .extensions_mut()
                .insert(axum::extract::ConnectInfo(SocketAddr::from((ip, 50000))));
            request
        };
        let res = router(state.clone())
            .oneshot(call([192, 168, 1, 8], None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = router(state.clone())
            .oneshot(call([8, 8, 8, 8], None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        state.go.allow_remote.store(true, Ordering::Relaxed);
        let res = router(state.clone())
            .oneshot(call([8, 8, 8, 8], None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
        let away = djinn_store::Permissions {
            go: true,
            go_remote: true,
            ..djinn_store::Permissions::default()
        };
        let role = state.store.create_role("Remote", &away).await.unwrap();
        account(&state, "alex", role.id).await;
        let plain_login = |tls: bool| {
            let mut request = Request::builder()
                .method(Method::POST)
                .uri("/api/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login("alex")))
                .unwrap();
            request
                .extensions_mut()
                .insert(axum::extract::ConnectInfo(SocketAddr::from((
                    [8, 8, 8, 8],
                    50000,
                ))));
            if tls {
                request.extensions_mut().insert(DirectTls);
            }
            router(state.clone()).oneshot(request)
        };
        let res = plain_login(false).await.unwrap();
        assert_eq!(
            res.status(),
            StatusCode::FORBIDDEN,
            "no password in the clear"
        );
        // DJINN's own encryption counts.
        let res = plain_login(true).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cookie.contains("; Secure"), "{cookie}");
        // The admin can allow it, for a port forward with no HTTPS.
        state.go.allow_plain_remote.store(true, Ordering::Relaxed);
        let res = plain_login(false).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(!cookie.contains("; Secure"), "{cookie}");
        // A direct client can't claim trusted HTTPS by setting a header.
        let res = router(state.clone())
            .oneshot(call([8, 8, 8, 8], Some("https")))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res
            .headers()
            .contains_key(header::STRICT_TRANSPORT_SECURITY));
    }

    #[tokio::test]
    async fn go_port_checks_roles_home_and_two_step() {
        let state = AppState::for_tests().await;
        let (status, _, _) = go(&state, Method::GET, "/api/auth/mode", None, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "off by default");
        state.go.enabled.store(true, Ordering::Relaxed);

        // Without a peer address, requests are remote. The server-wide
        // switch blocks every GO surface before sign-in or song grants.
        for path in [
            "/api/auth/mode",
            "/go/stream/missing",
            "/rest/ping.view",
            "/go",
        ] {
            let (status, _, _) = go(&state, Method::GET, path, None, None).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        }
        state.go.allow_remote.store(true, Ordering::Relaxed);

        // The simulated client is away from home, and Listener can't use
        // GO away from home.
        account(&state, "lee", djinn_store::LISTENER_ROLE).await;
        let (status, text, _) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("lee")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{text}");
        assert!(text.contains("home network"), "{text}");
        assert_eq!(
            state.store.sign_ins(1).await.unwrap()[0].outcome,
            "not_home"
        );

        let away = djinn_store::Permissions {
            go: true,
            go_remote: true,
            ..djinn_store::Permissions::default()
        };
        let travel = state.store.create_role("Travel", &away).await.unwrap();
        account(&state, "sam", travel.id).await;
        let (status, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sam = cookie.unwrap();
        let (status, _, _) = go(&state, Method::GET, "/api/tracks", None, Some(&sam)).await;
        assert_eq!(status, StatusCode::OK);
        // Party and admin routes don't exist on GO.
        for (method, path) in [
            (Method::POST, "/api/queue"),
            (Method::GET, "/api/users"),
            (Method::GET, "/api/events"),
            (Method::GET, "/api/go/status"),
        ] {
            let (status, _, _) = go(&state, method, path, None, Some(&sam)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }
        let (status, _, _) = go(&state, Method::GET, "/api/tracks", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // A role that requires two-step: only setup works until it's on.
        let strict = djinn_store::Permissions {
            require_two_step: true,
            ..away
        };
        state
            .store
            .update_role(travel.id, "Travel", &strict)
            .await
            .unwrap();
        let (status, text, _) = go(&state, Method::GET, "/api/tracks", None, Some(&sam)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{text}");
        let (status, text, _) = go(&state, Method::GET, "/api/me", None, Some(&sam)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(field(&text, "two_step_required"), true);

        // Turn it on, then sign in with a code.
        let (_, setup, _) = go(
            &state,
            Method::POST,
            "/api/me/two-step/start",
            Some(r#"{"password":"correct horse"}"#),
            Some(&sam),
        )
        .await;
        let secret = field(&setup, "secret").as_str().unwrap().to_owned();
        let code = |offset: i64| {
            let key = data_encoding::BASE32_NOPAD
                .decode(secret.as_bytes())
                .unwrap();
            let step = djinn_store::now_ms() / 1000 / 30 + offset;
            format!("{:06}", two_step::code_at(&key, step).unwrap())
        };
        let body = format!(r#"{{"code":"{}"}}"#, code(-1));
        let (status, text, _) = go(
            &state,
            Method::POST,
            "/api/me/two-step/confirm",
            Some(&body),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(field(&text, "recovery_codes").as_array().unwrap().len(), 10);
        let (status, _, _) = go(&state, Method::GET, "/api/tracks", None, Some(&sam)).await;
        assert_eq!(status, StatusCode::OK, "this device stays signed in");

        let (status, text, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        assert_eq!((status, cookie), (StatusCode::OK, None), "a code is needed");
        let ticket = field(&text, "ticket").as_str().unwrap().to_owned();
        let wrong = format!(r#"{{"ticket":"{ticket}","code":"000000"}}"#);
        let (status, _, _) = go(
            &state,
            Method::POST,
            "/api/auth/two-step",
            Some(&wrong),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        let used_code = code(0);
        let right = format!(r#"{{"ticket":"{ticket}","code":"{used_code}"}}"#);
        let (status, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/two-step",
            Some(&right),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(cookie.is_some());

        // The same code doesn't work twice.
        let (_, text, _) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        let ticket = field(&text, "ticket").as_str().unwrap().to_owned();
        let again = format!(r#"{{"ticket":"{ticket}","code":"{used_code}"}}"#);
        let (status, _, _) = go(
            &state,
            Method::POST,
            "/api/auth/two-step",
            Some(&again),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        // An app finishing two-step gets a token instead of a cookie.
        let (_, text, _) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(r#"{"username":"sam","password":"correct horse","device_token":true}"#),
            None,
        )
        .await;
        let ticket = field(&text, "ticket").as_str().unwrap().to_owned();
        let app = format!(
            r#"{{"ticket":"{ticket}","code":"{}","device_token":true}}"#,
            code(1)
        );
        let (status, text, cookie) =
            go(&state, Method::POST, "/api/auth/two-step", Some(&app), None).await;
        assert_eq!((status, cookie), (StatusCode::OK, None));
        assert!(field(&text, "token").is_string());
    }

    /// A role that can use GO from anywhere, as the `go` helper's client is.
    async fn travel_role(state: &AppState) -> i64 {
        state
            .store
            .create_role(
                "Travel",
                &djinn_store::Permissions {
                    go: true,
                    go_remote: true,
                    ..djinn_store::Permissions::default()
                },
            )
            .await
            .unwrap()
            .id
    }

    #[tokio::test]
    async fn a_phone_token_only_works_on_gos_port() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let role = travel_role(&state).await;
        account(&state, "sam", role).await;
        let body = r#"{"username":"sam","password":"correct horse","device_token":true}"#;
        let (status, text, _) = go(&state, Method::POST, "/api/auth/login", Some(body), None).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let token = field(&text, "token").as_str().unwrap().to_owned();
        let cookie = format!("djinn_session={token}");
        let (status, _, _) = go(&state, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK, "GO's port");
        let (status, _) =
            crate::test_util::call(&state, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "the party port");

        // A browser's sign-in still works on both.
        let (_, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        let cookie = cookie.unwrap();
        let (status, _) =
            crate::test_util::call(&state, Method::GET, "/api/me", None, Some(&cookie)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_session_alone_cant_change_who_can_sign_in() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let role = travel_role(&state).await;
        account(&state, "sam", role).await;
        let (_, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        let sam = cookie.unwrap();
        let start = "/api/me/two-step/start";
        for wrong in ["{}", r#"{"password":"not it"}"#] {
            let (status, text, _) = go(&state, Method::POST, start, Some(wrong), Some(&sam)).await;
            assert!(status.is_client_error(), "{status} {text}");
        }
        // The same again once two-step is on: a new phone needs the password.
        let right = r#"{"password":"correct horse"}"#;
        let (_, setup, _) = go(&state, Method::POST, start, Some(right), Some(&sam)).await;
        let secret = field(&setup, "secret").as_str().unwrap().to_owned();
        let key = data_encoding::BASE32_NOPAD
            .decode(secret.as_bytes())
            .unwrap();
        let step = djinn_store::now_ms() / 1000 / 30;
        let code = format!("{:06}", two_step::code_at(&key, step).unwrap());
        let (status, _, _) = go(
            &state,
            Method::POST,
            "/api/me/two-step/confirm",
            Some(&format!(r#"{{"code":"{code}"}}"#)),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _, _) = go(&state, Method::POST, start, Some("{}"), Some(&sam)).await;
        assert!(status.is_client_error());
        let (status, _, _) = go(&state, Method::POST, start, Some(right), Some(&sam)).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn app_passwords_are_shown_once_and_end_with_the_password() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let role = travel_role(&state).await;
        account(&state, "sam", role).await;
        let (_, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        let sam = cookie.unwrap();
        let path = "/api/me/app-password";
        let (status, _, _) = go(&state, Method::POST, path, Some("{}"), Some(&sam)).await;
        assert!(status.is_client_error(), "needs the password");
        let (status, text, _) = go(
            &state,
            Method::POST,
            path,
            Some(r#"{"password":"correct horse"}"#),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let key = field(&text, "password").as_str().unwrap().to_owned();
        let (_, text, _) = go(&state, Method::GET, path, None, Some(&sam)).await;
        assert_eq!(field(&text, "exists"), true);
        assert!(!text.contains(&key), "a session can't read it back");

        let ping = format!("/rest/ping?f=json&u=sam&p={key}");
        let (_, text, _) = go(&state, Method::GET, &ping, None, None).await;
        assert!(text.contains(r#""status":"ok""#), "{text}");

        let change = r#"{"current":"correct horse","new":"a brand new password"}"#;
        let (status, text, _) = go(
            &state,
            Method::POST,
            "/api/me/password",
            Some(change),
            Some(&sam),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
        let (_, text, _) = go(&state, Method::GET, &ping, None, None).await;
        assert!(text.contains(r#""status":"failed""#), "{text}");
    }

    #[tokio::test]
    async fn bad_app_keys_dont_shut_out_good_ones() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let role = travel_role(&state).await;
        account(&state, "sam", role).await;
        let id = state
            .store
            .user_credentials("sam")
            .await
            .unwrap()
            .unwrap()
            .0
            .id;
        state
            .store
            .set_subsonic_key(id, "the-good-key")
            .await
            .unwrap();
        for _ in 0..5 {
            go(
                &state,
                Method::GET,
                "/rest/ping?f=json&apiKey=bad",
                None,
                None,
            )
            .await;
        }
        // Coming from somewhere else, where the bad keys didn't.
        let mut request = Request::builder()
            .uri("/rest/ping?f=json&apiKey=the-good-key")
            .header("x-forwarded-for", "203.0.113.9")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                50000,
            ))));
        let res = router(state.clone()).oneshot(request).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(r#""status":"ok""#), "{text}");
    }

    #[tokio::test]
    async fn wrong_codes_add_up_across_password_sign_ins() {
        let state = AppState::for_tests().await;
        state.go.enable_for_tests();
        let role = travel_role(&state).await;
        account(&state, "sam", role).await;
        let id = state
            .store
            .user_credentials("sam")
            .await
            .unwrap()
            .unwrap()
            .0
            .id;
        state
            .store
            .begin_two_step(id, &two_step::new_secret().unwrap())
            .await
            .unwrap();
        state
            .store
            .confirm_two_step(id, 0, &[], None)
            .await
            .unwrap();
        let mut answers = Vec::new();
        for _ in 0..3 {
            let (_, text, _) = go(
                &state,
                Method::POST,
                "/api/auth/login",
                Some(&login("sam")),
                None,
            )
            .await;
            let ticket = field(&text, "ticket").as_str().unwrap().to_owned();
            let wrong = format!(r#"{{"ticket":"{ticket}","code":"abcdef"}}"#);
            for _ in 0..4 {
                let (status, _, _) = go(
                    &state,
                    Method::POST,
                    "/api/auth/two-step",
                    Some(&wrong),
                    None,
                )
                .await;
                answers.push(status);
            }
        }
        // Five wrong codes, then locked: the ticket that hit it is used up,
        // and the next sign-in's first code is refused too.
        assert!(answers[..5].iter().all(|s| *s == StatusCode::UNAUTHORIZED));
        assert_eq!(answers[5], StatusCode::TOO_MANY_REQUESTS, "{answers:?}");
        assert_eq!(answers[8], StatusCode::TOO_MANY_REQUESTS, "{answers:?}");
    }

    #[tokio::test]
    async fn apps_sign_in_with_a_token() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
        let listener = state.store.role(djinn_store::LISTENER_ROLE).await.unwrap();
        account(&state, "sam", listener.unwrap().id).await;
        let call = |method: Method, path: &str, token: Option<&str>, body: Option<String>| {
            let mut request = Request::builder().method(method).uri(path);
            if let Some(token) = token {
                request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
            }
            if body.is_some() {
                request = request.header(header::CONTENT_TYPE, "application/json");
            }
            let mut request = request.body(Body::from(body.unwrap_or_default())).unwrap();
            request
                .extensions_mut()
                .insert(axum::extract::ConnectInfo(SocketAddr::from((
                    [192, 168, 1, 8],
                    50000,
                ))));
            router(state.clone()).oneshot(request)
        };
        let body = r#"{"username":"sam","password":"correct horse","device_token":true}"#;
        let res = call(Method::POST, "/api/auth/login", None, Some(body.to_owned()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(!res.headers().contains_key(header::SET_COOKIE), "no cookie");
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let token = field(&text, "token").as_str().unwrap().to_owned();
        assert_eq!(field(&text, "me")["username"], "sam");

        let res = call(Method::GET, "/api/tracks", Some(&token), None).await;
        assert_eq!(res.unwrap().status(), StatusCode::OK);
        let res = call(Method::GET, "/api/tracks", Some("not-a-token"), None).await;
        assert_eq!(res.unwrap().status(), StatusCode::UNAUTHORIZED);
        let res = call(Method::GET, "/api/tracks", None, None).await;
        assert_eq!(res.unwrap().status(), StatusCode::UNAUTHORIZED);

        // It's a session like any other, on the list of signed-in devices.
        let res = call(Method::GET, "/api/me/sessions", Some(&token), None).await;
        assert_eq!(res.unwrap().status(), StatusCode::OK);

        // Away from GO's port a token is nothing, even if it's real.
        let (mut parts, _) = Request::builder()
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(())
            .unwrap()
            .into_parts();
        assert!(CurrentSession::from_request_parts(&mut parts, &state)
            .await
            .is_err());

        // Signing out ends it.
        let res = call(Method::POST, "/api/auth/logout", Some(&token), None).await;
        assert_eq!(res.unwrap().status(), StatusCode::NO_CONTENT);
        let res = call(Method::GET, "/api/tracks", Some(&token), None).await;
        assert_eq!(res.unwrap().status(), StatusCode::UNAUTHORIZED);

        // The party's sign-in ignores the request and sets a cookie.
        let plain = Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = crate::api::router()
            .with_state(state.clone())
            .oneshot(plain)
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(res.headers().contains_key(header::SET_COOKIE));
    }

    #[tokio::test]
    async fn offline_copies_need_downloads_and_are_not_streams() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
        state.go.allow_remote.store(true, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("djinn-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("song.mp3");
        std::fs::write(&file, b"not really audio, but served as it is").unwrap();
        state
            .store
            .save_scanned(&[djinn_store::ScannedTrack {
                path: file.to_string_lossy().into_owned(),
                size: 37,
                mtime: 5,
                title: "Song".to_owned(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state
            .store
            .tracks_page(djinn_store::TrackSort::Title, 0, 1)
            .await
            .unwrap()
            .items[0]
            .id;
        let without = djinn_store::Permissions {
            go: true,
            go_remote: true,
            ..djinn_store::Permissions::default()
        };
        let with = djinn_store::Permissions {
            downloads: true,
            ..without.clone()
        };
        let plain = state.store.create_role("Plain", &without).await.unwrap();
        let keeper = state.store.create_role("Keeper", &with).await.unwrap();
        account(&state, "pat", plain.id).await;
        account(&state, "kim", keeper.id).await;
        let sign_in = |name: &'static str| {
            let state = state.clone();
            async move {
                let (_, _, cookie) = go(
                    &state,
                    Method::POST,
                    "/api/auth/login",
                    Some(&login(name)),
                    None,
                )
                .await;
                cookie.unwrap()
            }
        };
        let (pat, kim) = (sign_in("pat").await, sign_in("kim").await);
        let path = format!("/api/go/offline/{id}");

        let (status, _, _) = go(&state, Method::GET, &path, None, Some(&pat)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "no downloads permission");

        let (status, text, _) = go(&state, Method::GET, &path, None, Some(&kim)).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(field(&text, "version"), "37-5");
        assert_eq!(field(&text, "converted"), false);
        let url = field(&text, "url").as_str().unwrap().to_owned();
        let (status, _, _) = go(&state, Method::GET, &url, None, None).await;
        assert_eq!(status, StatusCode::OK);
        let listening = state.go.playing.lock().unwrap().len();
        assert_eq!(listening, 0, "keeping a copy isn't listening");

        // Compressed songs stay as they are, whatever was asked.
        let (_, text, _) = go(
            &state,
            Method::GET,
            &format!("{path}?kbps=128&vbr=true"),
            None,
            Some(&kim),
        )
        .await;
        assert_eq!(field(&text, "converted"), false);
        assert_eq!(field(&text, "convert_refused"), false);

        let versions = format!(r#"{{"ids":[{id},999999]}}"#);
        let (status, text, _) = go(
            &state,
            Method::POST,
            "/api/go/offline/versions",
            Some(&versions),
            Some(&kim),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let found: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            found.as_array().unwrap().len(),
            1,
            "removed songs are left out"
        );
        assert_eq!(found[0]["version"], "37-5");
        assert_eq!(found[0]["size"], 37);

        let (status, _, _) = go(
            &state,
            Method::GET,
            "/api/go/offline-suggested",
            None,
            Some(&pat),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let report = format!(r#"{{"track_id":{id},"ms_played":90000,"at":1000000}}"#);
        let (status, _, _) = go(
            &state,
            Method::POST,
            "/api/go/plays",
            Some(&report),
            Some(&kim),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (status, text, _) = go(
            &state,
            Method::GET,
            "/api/go/offline-suggested",
            None,
            Some(&kim),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let found: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(found[0]["id"], id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn your_own_library_on_go() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
        state.go.allow_remote.store(true, Ordering::Relaxed);
        let songs: Vec<djinn_store::ScannedTrack> = (0..4)
            .map(|i| djinn_store::ScannedTrack {
                path: format!("/m/{i}.flac"),
                title: format!("Song {i}"),
                artist: Some("Band".into()),
                album: Some("Record".into()),
                genre: Some("Rock".into()),
                ..djinn_store::ScannedTrack::default()
            })
            .collect();
        state.store.save_scanned(&songs).await.unwrap();
        let ids: Vec<i64> = state
            .store
            .tracks_page(djinn_store::TrackSort::Title, 0, 10)
            .await
            .unwrap()
            .items
            .iter()
            .map(|t| t.id)
            .collect();
        let away = djinn_store::Permissions {
            go: true,
            go_remote: true,
            ..djinn_store::Permissions::default()
        };
        let role = state.store.create_role("Away", &away).await.unwrap();
        account(&state, "sam", role.id).await;
        let (_, _, cookie) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("sam")),
            None,
        )
        .await;
        let sam = cookie.unwrap();
        let call = |method: Method, path: String, body: Option<String>| {
            let state = state.clone();
            let sam = sam.clone();
            async move { go(&state, method, &path, body.as_deref(), Some(&sam)).await }
        };

        let (status, _, _) =
            call(Method::PUT, format!("/api/go/likes/track/{}", ids[1]), None).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, text, _) = call(Method::GET, "/api/go/likes/ids".into(), None).await;
        assert_eq!(field(&text, "tracks"), serde_json::json!([ids[1]]));

        // What tapping a song does: play it now until changed.
        let (_, text, _) = call(Method::GET, "/api/go/prefs".into(), None).await;
        assert_eq!(field(&text, "tap"), "play");
        let queue = r#"{"tap":"queue"}"#.to_owned();
        let (status, _, _) = call(Method::PUT, "/api/go/prefs".into(), Some(queue)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, text, _) = call(Method::GET, "/api/go/prefs".into(), None).await;
        assert_eq!(field(&text, "tap"), "queue");
        let end = r#"{"tap":"end"}"#.to_owned();
        let (status, _, _) = call(Method::PUT, "/api/go/prefs".into(), Some(end)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let (_, text, _) = call(Method::GET, "/api/go/prefs".into(), None).await;
        assert_eq!(field(&text, "tap"), "end");
        let odd = r#"{"tap":"shout"}"#.to_owned();
        let (status, _, _) = call(Method::PUT, "/api/go/prefs".into(), Some(odd)).await;
        assert!(status.is_client_error());

        let body = format!(r#"{{"name":"Road trip","songs":[{},{}]}}"#, ids[2], ids[0]);
        let (status, text, _) = call(Method::POST, "/api/go/playlists".into(), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        let list = field(&text, "id");
        // The AI reader is off here, so there's no reading, but GO may ask.
        let (status, text, _) = call(
            Method::GET,
            "/api/search/ask?q=music+for+cooking+dinner".into(),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(text, "null");

        // The YouTube key is the server's: GO says whether it's there, and
        // can't change it.
        let key_path = "/api/playlists/youtube-key";
        state
            .store
            .set_setting(crate::youtube::API_KEY, "SECRET-KEY")
            .await
            .unwrap();
        let (status, text, _) = call(Method::GET, key_path.into(), None).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        assert_eq!(text, r#"{"has_key":true}"#);
        let (status, _, _) = call(
            Method::PUT,
            key_path.into(),
            Some(r#"{"key":"other"}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // This role can't share playlists.
        let (status, _, _) = call(
            Method::PATCH,
            format!("/api/go/playlists/{list}"),
            Some(r#"{"shared":true}"#.into()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let report = format!(r#"{{"track_id":{},"ms_played":120000}}"#, ids[0]);
        let (status, _, _) = call(Method::POST, "/api/go/plays".into(), Some(report)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let resume = format!(
            r#"{{"queue":[{},{}],"position":1,"position_ms":5000}}"#,
            ids[0], ids[1]
        );
        call(Method::PUT, "/api/go/resume".into(), Some(resume)).await;
        let (status, text, _) = call(Method::GET, "/api/go/home".into(), None).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let home: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(home["resume"]["position_ms"], 5000);
        assert_eq!(home["recent"][0]["id"], ids[0]);
        assert_eq!(home["playlists"][0]["name"], "Road trip");

        // Radio without sound analysis: the seed, then same artist and genre,
        // leaving out what was just heard.
        // Asking for more leaves out what's already lined up.
        let (status, text, _) = call(
            Method::GET,
            format!("/api/go/radio/{0}?exclude={1},{2}", ids[3], ids[0], ids[2]),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let more: serde_json::Value = serde_json::from_str(&text).unwrap();
        for song in more.as_array().unwrap() {
            assert!(
                song["id"] != ids[0] && song["id"] != ids[2],
                "already lined up"
            );
        }
        let (status, text, _) = call(Method::GET, format!("/api/go/radio/{}", ids[3]), None).await;
        assert_eq!(status, StatusCode::OK, "{text}");
        let radio: Vec<i64> = serde_json::from_str::<Vec<serde_json::Value>>(&text)
            .unwrap()
            .iter()
            .map(|t| t["id"].as_i64().unwrap())
            .collect();
        assert_eq!(radio[0], ids[3]);
        assert!(!radio.contains(&ids[0]), "heard just now");
        assert!(radio.contains(&ids[1]));

        // Away from home (tests have no address), the party is out of reach.
        let (status, _, _) = call(Method::POST, format!("/api/go/party/{}", ids[1]), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);

        // These only exist on the GO port.
        let (status, _) = crate::test_util::get_body(&state, "/api/go/home", &sam).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn wrong_passwords_lock_the_account_for_a_while() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
        state.go.allow_remote.store(true, Ordering::Relaxed);
        account(&state, "kim", djinn_store::LISTENER_ROLE).await;
        let wrong = r#"{"username":"kim","password":"not it at all"}"#;
        for _ in 0..5 {
            let (status, _, _) =
                go(&state, Method::POST, "/api/auth/login", Some(wrong), None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        // Even the right password waits now.
        let (status, text, _) = go(
            &state,
            Method::POST,
            "/api/auth/login",
            Some(&login("kim")),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{text}");
        assert_eq!(
            state.store.sign_ins(1).await.unwrap()[0].outcome,
            "locked_out"
        );
    }
}
