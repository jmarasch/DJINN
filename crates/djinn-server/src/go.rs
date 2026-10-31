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
use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_store::SignIn;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use tower_http::trace::TraceLayer;

use crate::auth::{go_refusal, new_token, AdminUser, CurrentSession, Device};
use crate::error::{ApiError, ApiResult};
use crate::security::{self, TRUSTED_PROXIES};
use crate::state::AppState;
use crate::two_step::{self, ADMIN_TWO_STEP};
use crate::{api, settings, web};

/// 1 while DJINN GO is on.
pub const GO_ENABLED: &str = "go_enabled";
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

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Turns GO on without a listener, for tests of its router.
    #[cfg(test)]
    pub fn enable_for_tests(&self) {
        self.enabled.store(true, Ordering::Relaxed);
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
    if on {
        start(state).await;
    } else {
        stop(state);
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
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
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
        .merge(crate::subsonic::router())
        .fallback(web::go_handler)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(middleware::from_fn_with_state(state.clone(), headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Admin > GO, on the main port; and the song links, which only work on GO.
pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/go/status", get(status))
        .route("/go/settings", put(save_settings))
        .route("/go/sign-ins", get(sign_ins))
        .route("/go/play/{id}", get(play))
        .route("/go/address", get(address))
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
    (Method::GET, "/api/go/home", Access::Listener),
    (Method::GET, "/api/go/address", Access::Setup),
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
    let origin = security::origin(
        security::peer(request.extensions()),
        request.headers(),
        &state.go.proxies(),
    );
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
             object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    if origin.https {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
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
    if query.download && !grant.downloads {
        return ApiError::forbidden().into_response();
    }
    // Downloads are always the original file.
    let path = match grant.convert_kbps.filter(|_| !query.download) {
        Some(kbps) => match crate::convert::converted(&state, &grant.path, kbps).await {
            Ok(path) => path,
            Err(e) => return e.into_response(),
        },
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
    if let Ok(mut playing) = state.go.playing.lock() {
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
    response
}

#[derive(Serialize)]
struct Listener {
    username: String,
    title: String,
    seconds_ago: u64,
}

#[derive(Serialize)]
struct Status {
    enabled: bool,
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
        enabled: go.enabled(),
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
    let qr_svg = qrcode::QrCode::new(url.as_bytes())
        .map_err(anyhow::Error::from)?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .dark_color(qrcode::render::svg::Color("#14111f"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
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
    enabled: bool,
    #[serde(default)]
    trusted_proxies: String,
    #[serde(default)]
    admin_two_step: bool,
}

async fn save_settings(
    AdminUser(admin): AdminUser,
    State(state): State<AppState>,
    Json(body): Json<GoSettings>,
) -> ApiResult<Json<Status>> {
    let proxies = body.trusted_proxies.trim();
    let parsed = security::parse_proxies(proxies);
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
    settings::set_flag(&state.store, GO_ENABLED, body.enabled).await?;
    state.store.set_setting(TRUSTED_PROXIES, proxies).await?;
    settings::set_flag(&state.store, ADMIN_TWO_STEP, body.admin_two_step).await?;
    apply(&state).await?;
    tracing::info!(on = body.enabled, "DJINN GO settings changed");
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
        let mut req = Request::builder().method(method).uri(path);
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(cookie) = cookie {
            req = req.header(header::COOKIE, cookie);
        }
        let req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .unwrap();
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
    async fn go_port_checks_roles_home_and_two_step() {
        let state = AppState::for_tests().await;
        let (status, _, _) = go(&state, Method::GET, "/api/auth/mode", None, None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "off by default");
        state.go.enabled.store(true, Ordering::Relaxed);

        // Tests have no peer address, so every request is "away from home",
        // and Listener can't use GO away from home.
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
            Some("{}"),
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
    }

    #[tokio::test]
    async fn your_own_library_on_go() {
        let state = AppState::for_tests().await;
        state.go.enabled.store(true, Ordering::Relaxed);
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
        let odd = r#"{"tap":"shout"}"#.to_owned();
        let (status, _, _) = call(Method::PUT, "/api/go/prefs".into(), Some(odd)).await;
        assert!(status.is_client_error());

        let body = format!(r#"{{"name":"Road trip","songs":[{},{}]}}"#, ids[2], ids[0]);
        let (status, text, _) = call(Method::POST, "/api/go/playlists".into(), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "{text}");
        let list = field(&text, "id");
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
