//! Casting: the live stream that cast devices and the TV page play, and
//! the admin API that finds devices and starts and stops casts.
//!
//! The stream URL carries a random token instead of a session cookie,
//! because Chromecasts, TVs, and Sonos speakers fetch it themselves and
//! can't sign in. Only DJINN hands the URL out: to whoever Play here is on for,
//! to the TV page when the admin allows sound there, and to the devices it
//! casts to. An admin can change it, which ends what's listening with the old
//! one.

use axum::body::{Body, Bytes};
use std::net::IpAddr;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_cast::stream::{self, LiveStream};
use djinn_cast::{CastError, CastMedia, CastStatus, Metadata};
use djinn_store::Store;
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser, Device};
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::settings;
use crate::state::AppState;

/// Setting key holding the stream token.
const CAST_TOKEN: &str = "cast_token";
/// Setting key: anyone signed in can play the party in their own browser
/// (Play here). Off unless set to "1".
pub const PLAY_HERE: &str = "play_here";
/// Setting key: devices being cast to, so casts resume after a restart.
const CAST_TARGETS: &str = "cast_targets";
/// Setting key: the room each cast device belongs to, as "id<TAB>room" lines.
/// A device with no line (or whose room is gone) plays Main's music.
const CAST_ROOMS: &str = "cast_rooms";
/// How long to listen for devices.
const DISCOVERY: Duration = Duration::from_secs(4);

/// Routes outside /api: devices fetch these without cookies.
pub fn stream_router() -> Router<AppState> {
    Router::new()
        .route("/cast/{token}/live.mp3", get(live))
        .route("/cast/{token}/join.png", get(join_png))
}

/// Routes under /api: Play here for everyone signed in, the rest for admins.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/listen", get(listen))
        .route("/cast", get(list))
        .route("/cast/discover", post(discover))
        .route("/cast/stop-all", post(stop_all))
        .route("/cast/new-link", post(new_link))
        .route("/cast/{id}/start", post(start))
        .route("/cast/{id}/stop", post(stop))
        .route("/cast/{id}/volume", put(volume))
        .route("/cast/{id}/room", put(set_room))
}

#[derive(Serialize)]
struct Listen {
    /// The live stream, or none when the host turned Play here off.
    url: Option<String>,
}

/// The host's own browser would play a delayed copy beside its speakers.
/// Compare the resolved client address with every local interface, including
/// loopback and VPN adapters; another device on the LAN remains eligible.
fn is_server_address(ip: IpAddr, local: &[IpAddr]) -> bool {
    let ip = ip.to_canonical();
    ip.is_loopback() || local.iter().any(|address| address.to_canonical() == ip)
}

fn on_server(ip: Option<IpAddr>) -> bool {
    let Some(ip) = ip.map(|address| address.to_canonical()) else {
        return false;
    };
    if ip.is_loopback() {
        return true;
    }
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return false;
    };
    let local: Vec<IpAddr> = interfaces.iter().map(|interface| interface.ip()).collect();
    is_server_address(ip, &local)
}

/// Where Play here listens: the same stream casts play.
async fn listen(
    _: CurrentUser,
    device: Device,
    State(state): State<AppState>,
) -> ApiResult<Json<Listen>> {
    let on = settings::flag(&state.store, PLAY_HERE, false).await?;
    Ok(Json(Listen {
        url: state
            .stream
            .as_ref()
            .filter(|_| on && !on_server(device.addr))
            .map(|_| stream_path(&state.stream_token())),
    }))
}

#[derive(Serialize)]
struct CastDevice {
    #[serde(flatten)]
    status: CastStatus,
    /// The room it plays; Main until it's put in another.
    room_id: i64,
    /// Someone put it in a room (the Rooms page lists those as endpoints).
    placed: bool,
}

#[derive(Serialize)]
struct CastList {
    devices: Vec<CastDevice>,
    /// Players connected to the stream (casts and TV pages).
    listeners: usize,
}

/// Each device's room, from the saved lines; rooms that are gone count as Main.
async fn cast_rooms(state: &AppState) -> ApiResult<std::collections::HashMap<String, i64>> {
    let known: Vec<i64> = state.store.rooms().await?.iter().map(|r| r.id).collect();
    let saved = state
        .store
        .get_setting(CAST_ROOMS)
        .await?
        .unwrap_or_default();
    Ok(saved
        .lines()
        .filter_map(|line| {
            let (id, room) = line.split_once('\t')?;
            let room: i64 = room.trim().parse().ok()?;
            known.contains(&room).then(|| (id.to_owned(), room))
        })
        .collect())
}

async fn room_of_cast(state: &AppState, id: &str) -> ApiResult<i64> {
    Ok(cast_rooms(state)
        .await?
        .get(id)
        .copied()
        .unwrap_or(djinn_store::MAIN_ROOM))
}

async fn cast_list(state: &AppState) -> ApiResult<CastList> {
    let rooms = cast_rooms(state).await?;
    Ok(CastList {
        devices: state
            .caster
            .status()
            .into_iter()
            .map(|status| CastDevice {
                room_id: rooms
                    .get(&status.device.id)
                    .copied()
                    .unwrap_or(djinn_store::MAIN_ROOM),
                placed: rooms.contains_key(&status.device.id),
                status,
            })
            .collect(),
        listeners: state.stream.as_ref().map_or(0, LiveStream::listeners),
    })
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<CastList>> {
    Ok(Json(cast_list(&state).await?))
}

async fn discover(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<CastList>> {
    state.caster.discover(DISCOVERY).await;
    Ok(Json(cast_list(&state).await?))
}

#[derive(Deserialize)]
struct CastRoom {
    room_id: i64,
}

/// Puts a cast device in a room: it plays that room's music from now on (a
/// cast already running is started again on the new stream).
async fn set_room(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CastRoom>,
) -> ApiResult<Json<CastList>> {
    if state.store.room(body.room_id).await?.is_none() {
        return Err(ApiError::bad_request("there's no such room"));
    }
    let mut rooms = cast_rooms(&state).await?;
    rooms.insert(id.clone(), body.room_id);
    let lines: Vec<String> = rooms
        .iter()
        .map(|(id, room)| format!("{id}\t{room}"))
        .collect();
    state
        .store
        .set_setting(CAST_ROOMS, &lines.join("\n"))
        .await?;
    if let Some(running) = state
        .caster
        .status()
        .into_iter()
        .find(|s| s.device.id == id && s.active)
    {
        let mode = if running.page {
            Mode::Page
        } else {
            Mode::Audio
        };
        state.caster.stop(&id).await;
        for _ in 0..50 {
            if !state.caster.active().contains(&id) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        start_cast(&state, &id, mode).await?;
        save_targets(&state).await?;
    }
    Ok(Json(cast_list(&state).await?))
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    /// Just the music (any device).
    #[default]
    Audio,
    /// The TV page with sound (Google Cast devices with a screen).
    Page,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Page => "page",
        }
    }
}

#[derive(Default, Deserialize)]
struct StartCast {
    #[serde(default)]
    mode: Mode,
}

async fn start(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<StartCast>>,
) -> ApiResult<Json<CastList>> {
    let mode = body.map(|b| b.0.mode).unwrap_or_default();
    start_cast(&state, &id, mode).await?;
    save_targets(&state).await?;
    Ok(Json(cast_list(&state).await?))
}

async fn stop(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<CastList>> {
    state.caster.stop(&id).await;
    // The session ends a moment later; save the list without it now.
    let targets: Vec<String> = state
        .caster
        .active()
        .into_iter()
        .filter(|t| *t != id)
        .collect();
    state
        .store
        .set_setting(CAST_TARGETS, &targets.join("\n"))
        .await?;
    Ok(Json(cast_list(&state).await?))
}

async fn stop_all(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.caster.stop_all().await;
    state.store.set_setting(CAST_TARGETS, "").await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Changes the secret in the stream's address, for when it has got around.
/// Casts carry on with the new one; browsers playing Play here ask for it.
async fn new_link(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<CastList>> {
    let token = new_stream_token()?;
    state.store.set_setting(CAST_TOKEN, &token).await?;
    if let Ok(mut current) = state.cast_token.write() {
        *current = token;
    }
    tracing::warn!("the live stream has a new address");
    recast(&state).await;
    Ok(Json(cast_list(&state).await?))
}

/// Starts every running cast again, for when its address changed (a new
/// secret, or this computer's own address).
pub async fn recast(state: &AppState) {
    let targets = cast_targets(state);
    if targets.is_empty() {
        return;
    }
    state.caster.stop_all().await;
    for _ in 0..50 {
        if state.caster.active().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    for line in &targets {
        let (id, mode) = parse_target(line);
        if let Err(e) = start_cast(state, id, mode).await {
            tracing::warn!(device = id, error = ?e, "couldn't start the cast again");
        }
    }
}

#[derive(Deserialize)]
struct Volume {
    /// 0.0 to 1.0.
    level: f32,
}

async fn volume(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Volume>,
) -> ApiResult<StatusCode> {
    if !(0.0..=1.0).contains(&body.level) {
        return Err(ApiError::bad_request("volume must be 0 to 1"));
    }
    state.caster.set_volume(&id, body.level).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn start_cast(state: &AppState, id: &str, mode: Mode) -> ApiResult<()> {
    let room = room_of_cast(state, id).await?;
    if state.stream_for(room).is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the audio stream isn't running",
        ));
    }
    let base = state.join_url.base();
    let base = base.as_str();
    let token = state.stream_token();
    let media = match mode {
        Mode::Audio => CastMedia::Stream {
            url: format!("{base}{}", stream_path_in(&token, room)),
            meta: Metadata {
                title: "DJINN".to_owned(),
                subtitle: "Scan the code to add songs".to_owned(),
                image_url: Some(format!("{base}/cast/{token}/join.png")),
            },
        },
        Mode::Page => {
            if !crate::tv_api::enabled(state).await? {
                return Err(ApiError::bad_request(
                    "turn on the TV page under Rules first",
                ));
            }
            CastMedia::Page {
                // The page plays the stream itself, so it gets the secret.
                url: if room == djinn_store::MAIN_ROOM {
                    format!("{base}/tv?cast={token}")
                } else {
                    format!("{base}/tv?cast={token}&room={room}")
                },
            }
        }
    };
    state.caster.start(id, media).await.map_err(|e| match e {
        CastError::NoScreen => ApiError::bad_request(e.to_string()),
        _ => ApiError::not_found_with(e.to_string()),
    })
}

/// Saved casts as "id<TAB>mode" lines.
fn cast_targets(state: &AppState) -> Vec<String> {
    state
        .caster
        .status()
        .into_iter()
        .filter(|s| s.active)
        .map(|s| {
            let mode = if s.page { Mode::Page } else { Mode::Audio };
            format!("{}\t{}", s.device.id, mode.as_str())
        })
        .collect()
}

/// One line of `cast_targets`.
fn parse_target(line: &str) -> (&str, Mode) {
    match line.split_once('\t') {
        Some((id, "page")) => (id, Mode::Page),
        Some((id, _)) => (id, Mode::Audio),
        None => (line, Mode::Audio),
    }
}

async fn save_targets(state: &AppState) -> ApiResult<()> {
    state
        .store
        .set_setting(CAST_TARGETS, &cast_targets(state).join("\n"))
        .await?;
    Ok(())
}

/// Background work at startup: forwards cast status to clients, and picks
/// up the casts that were running when DJINN last stopped.
pub fn spawn_background(state: &AppState) {
    let forward = state.clone();
    tokio::spawn(async move {
        let mut changes = forward.caster.subscribe();
        while changes.changed().await.is_ok() {
            let _ = forward
                .events
                .send(ServerEvent::CastStatus(forward.caster.status()));
            // Coalesce bursts of changes.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    let resume = state.clone();
    tokio::spawn(async move {
        let targets = match resume.store.get_setting(CAST_TARGETS).await {
            Ok(Some(targets)) if !targets.trim().is_empty() => targets,
            _ => return,
        };
        resume.caster.discover(DISCOVERY).await;
        for line in targets.lines().map(str::trim).filter(|t| !t.is_empty()) {
            let (id, mode) = parse_target(line);
            match start_cast(&resume, id, mode).await {
                Ok(()) => tracing::info!(device = id, "resuming cast"),
                Err(_) => tracing::info!(device = id, "cast device not found; not resuming"),
            }
        }
    });
}

/// The join QR code as a PNG: what TVs show while they play the party.
async fn join_png(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    if !crate::security::same_secret(state.stream_token().as_bytes(), token.as_bytes()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(code) = qrcode::QrCode::new(state.join_url.get().as_bytes()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let image = code
        .render::<image::Luma<u8>>()
        .min_dimensions(720, 720)
        .build();
    let mut png = std::io::Cursor::new(Vec::new());
    if image.write_to(&mut png, image::ImageFormat::Png).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], png.into_inner()).into_response()
}

/// The stream token, made on first use.
pub async fn token(store: &Store) -> anyhow::Result<String> {
    if let Some(token) = store.get_setting(CAST_TOKEN).await? {
        return Ok(token);
    }
    let token = new_stream_token()?;
    store.set_setting(CAST_TOKEN, &token).await?;
    Ok(token)
}

fn new_stream_token() -> anyhow::Result<String> {
    crate::security::random_hex(16)
}

/// Path of the live stream, relative to the server.
pub fn stream_path(token: &str) -> String {
    format!("/cast/{token}/live.mp3")
}

/// The stream a room's casts play: Main's, or `?room=` for another's.
pub fn stream_path_in(token: &str, room: i64) -> String {
    if room == djinn_store::MAIN_ROOM {
        stream_path(token)
    } else {
        format!("{}?room={room}", stream_path(token))
    }
}

#[derive(Deserialize)]
struct LiveQuery {
    /// Which room's music (Main's when missing).
    room: Option<i64>,
}

async fn live(
    State(state): State<AppState>,
    Path(token): Path<String>,
    Query(query): Query<LiveQuery>,
    headers: axum::http::HeaderMap,
) -> Response {
    let room = query.room.unwrap_or(djinn_store::MAIN_ROOM);
    let stream = state.stream_for(room);
    let Some(stream) = stream.as_ref().filter(|_| {
        crate::security::same_secret(state.stream_token().as_bytes(), token.as_bytes())
    }) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Internet radio players (Sonos, many receivers, VLC) ask for song
    // titles inside the stream.
    let wants_titles = headers
        .get("icy-metadata")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "1");
    let title = if wants_titles {
        Some(song_title(
            &state.sound_jukebox_for(room).lock().await.now_playing(),
        ))
    } else {
        None
    };
    let events = state
        .room_events(room)
        .unwrap_or_else(|| state.events.clone());
    live_response(
        stream,
        title.map(|t| (t, events.subscribe())),
        state.shutdown.subscribe(),
        (token, std::sync::Arc::clone(&state.cast_token)),
    )
}

/// How many audio bytes go between title blocks.
const ICY_METAINT: usize = 16_000;

/// "Artist - Title" for the song playing.
fn song_title(now: &crate::events::NowPlaying) -> String {
    match &now.current {
        Some(entry) => match &entry.track.artist {
            Some(artist) => format!("{artist} - {}", entry.track.title),
            None => entry.track.title.clone(),
        },
        None => "DJINN".to_owned(),
    }
}

/// One ICY metadata block: a length byte (in 16-byte units), then the
/// text padded to it. An unchanged title is a lone zero byte.
fn icy_block(title: Option<&str>) -> Vec<u8> {
    let Some(title) = title else {
        return vec![0];
    };
    // Quotes end the field; keep it simple and drop them.
    let clean: String = title.chars().filter(|c| *c != '\'').collect();
    let mut text = format!("StreamTitle='{clean}';").into_bytes();
    text.truncate(255 * 16);
    let blocks = text.len().div_ceil(16);
    text.resize(blocks * 16, 0);
    let mut out = Vec::with_capacity(text.len() + 1);
    out.push(u8::try_from(blocks).unwrap_or(u8::MAX));
    out.extend_from_slice(&text);
    out
}

struct LiveBody {
    backlog: std::collections::VecDeque<Bytes>,
    live: tokio::sync::broadcast::Receiver<Bytes>,
    _guard: stream::ListenerGuard,
    /// Titles for players that asked: (current title, sent yet, events).
    titles: Option<(String, bool, tokio::sync::broadcast::Receiver<ServerEvent>)>,
    until_title: usize,
    pending: Bytes,
    /// Ends the stream when DJINN shuts down, so it can exit.
    shutdown: tokio::sync::watch::Receiver<bool>,
    /// The secret this listener came with, and the one in use: when an admin
    /// changes it, whoever has the old one stops getting music.
    token: String,
    current: std::sync::Arc<std::sync::RwLock<String>>,
}

impl LiveBody {
    async fn next_chunk(&mut self) -> Option<Bytes> {
        loop {
            if self.current.read().map_or(true, |now| *now != self.token) {
                return None;
            }
            if !self.pending.is_empty() {
                return Some(std::mem::take(&mut self.pending));
            }
            if let Some(frame) = self.backlog.pop_front() {
                return Some(frame);
            }
            let frame = tokio::select! {
                frame = self.live.recv() => frame,
                _ = self.shutdown.wait_for(|stop| *stop) => return None,
            };
            match frame {
                Ok(frame) => return Some(frame),
                // A slow client skipped frames; MP3 picks up at the next.
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// The next piece of the response, with title blocks spliced in every
    /// `ICY_METAINT` audio bytes when asked for.
    async fn next(&mut self) -> Option<Bytes> {
        let chunk = self.next_chunk().await?;
        let Some((title, sent, events)) = self.titles.as_mut() else {
            return Some(chunk);
        };
        while let Ok(event) = events.try_recv() {
            if let ServerEvent::NowPlaying(now) = event {
                let latest = song_title(&now);
                if latest != *title {
                    *title = latest;
                    *sent = false;
                }
            }
        }
        if chunk.len() < self.until_title {
            self.until_title -= chunk.len();
            return Some(chunk);
        }
        let (audio, rest) = chunk.split_at(self.until_title);
        let mut out = Vec::with_capacity(audio.len() + 64);
        out.extend_from_slice(audio);
        out.extend_from_slice(&icy_block((!*sent).then_some(title.as_str())));
        *sent = true;
        self.pending = Bytes::copy_from_slice(rest);
        self.until_title = ICY_METAINT;
        Some(Bytes::from(out))
    }
}

fn live_response(
    stream: &LiveStream,
    titles: Option<(String, tokio::sync::broadcast::Receiver<ServerEvent>)>,
    shutdown: tokio::sync::watch::Receiver<bool>,
    (token, current): (String, std::sync::Arc<std::sync::RwLock<String>>),
) -> Response {
    let stream::Listener {
        backlog,
        live,
        guard,
    } = stream.subscribe();
    let icy = titles.is_some();
    let state = LiveBody {
        backlog: backlog.into(),
        live,
        _guard: guard,
        titles: titles.map(|(title, events)| (title, false, events)),
        until_title: ICY_METAINT,
        pending: Bytes::new(),
        shutdown,
        token,
        current,
    };
    let body = futures_util::stream::unfold(state, |mut state| async move {
        let chunk = state.next().await?;
        Some((Ok::<_, std::convert::Infallible>(chunk), state))
    });
    let mut response = (
        [
            (header::CONTENT_TYPE, stream::CONTENT_TYPE),
            (header::CACHE_CONTROL, "no-cache, no-store"),
            // DLNA renderers look for these before playing a live stream.
            (
                header::HeaderName::from_static("transfermode.dlna.org"),
                "Streaming",
            ),
            (
                header::HeaderName::from_static("contentfeatures.dlna.org"),
                "DLNA.ORG_PN=MP3;DLNA.ORG_OP=00;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
            ),
            (header::HeaderName::from_static("icy-name"), "DJINN"),
        ],
        Body::from_stream(body),
    )
        .into_response();
    if icy {
        response.headers_mut().insert(
            header::HeaderName::from_static("icy-metaint"),
            header::HeaderValue::from(ICY_METAINT),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_addresses_are_distinct_from_other_lan_devices() {
        let local: Vec<IpAddr> = ["192.168.1.20", "fd7a:115c:a1e0::1"]
            .into_iter()
            .map(|ip| ip.parse().unwrap())
            .collect();
        assert!(is_server_address("127.0.0.1".parse().unwrap(), &local));
        assert!(is_server_address("::1".parse().unwrap(), &local));
        assert!(is_server_address("192.168.1.20".parse().unwrap(), &local));
        assert!(is_server_address(
            "::ffff:192.168.1.20".parse().unwrap(),
            &local
        ));
        assert!(is_server_address(
            "fd7a:115c:a1e0::1".parse().unwrap(),
            &local
        ));
        assert!(!is_server_address("192.168.1.21".parse().unwrap(), &local));
    }

    #[test]
    fn title_blocks_follow_the_icy_format() {
        assert_eq!(icy_block(None), [0]);
        let block = icy_block(Some("Zara Larsson - Lush Life"));
        assert_eq!(block.len(), 1 + usize::from(block[0]) * 16);
        let text = String::from_utf8_lossy(&block[1..]);
        assert!(text.starts_with("StreamTitle='Zara Larsson - Lush Life';"));
        assert!(text.trim_end_matches('\0').ends_with(';'));
        // Quotes would end the field early.
        let quoted = icy_block(Some("Don't Stop"));
        assert!(String::from_utf8_lossy(&quoted).contains("'Dont Stop';"));
    }

    #[tokio::test]
    async fn a_cast_device_belongs_to_a_room_and_goes_back_to_main_without_it() {
        use crate::test_util::{call, session};
        use djinn_store::Role;
        let state = crate::state::AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let patio = state.store.create_room("Patio").await.unwrap();
        // Not a room.
        let (status, _) = call(
            &state,
            axum::http::Method::PUT,
            "/api/cast/tv-1/room",
            Some(r#"{"room_id":999}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &state,
            axum::http::Method::PUT,
            "/api/cast/tv-1/room",
            Some(&format!(r#"{{"room_id":{}}}"#, patio.id)),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(room_of_cast(&state, "tv-1").await.unwrap(), patio.id);
        assert_eq!(
            room_of_cast(&state, "other").await.unwrap(),
            djinn_store::MAIN_ROOM,
            "devices with no room play Main's music"
        );
        // Main's stream keeps its old address; another room's names it.
        assert_eq!(
            stream_path_in("t", djinn_store::MAIN_ROOM),
            "/cast/t/live.mp3"
        );
        assert_eq!(
            stream_path_in("t", patio.id),
            format!("/cast/t/live.mp3?room={}", patio.id)
        );
        // A room that's deleted sends its casts back to Main.
        state.store.delete_room(patio.id).await.unwrap();
        assert_eq!(
            room_of_cast(&state, "tv-1").await.unwrap(),
            djinn_store::MAIN_ROOM
        );
    }

    #[tokio::test]
    async fn play_here_hands_out_the_stream_once_turned_on() {
        use crate::test_util::{call, get_body, session};
        use axum::body::to_bytes;
        use axum::extract::ConnectInfo;
        use axum::http::header::COOKIE;
        use axum::http::Method;
        use axum::http::Request;
        use djinn_store::Role;
        use tower::ServiceExt;

        let (_tap, reader) = djinn_engine::stream_tap(1.0, 48_000);
        let stream = LiveStream::start(reader, 48_000).unwrap();
        let state = AppState::for_tests()
            .await
            .with_stream(stream, "secret".into());
        state
            .guest_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let guest = session(&state, "gus", Role::Guest).await;

        let (status, _) = call(&state, Method::GET, "/api/listen", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            get_body(&state, "/api/listen", &guest).await,
            (StatusCode::OK, r#"{"url":null}"#.into()),
            "off until the host turns it on"
        );

        state.store.set_setting(PLAY_HERE, "1").await.unwrap();
        assert_eq!(
            get_body(&state, "/api/listen", &guest).await,
            (StatusCode::OK, r#"{"url":"/cast/secret/live.mp3"}"#.into())
        );

        for (address, expected) in [
            ("127.0.0.1:4321", r#"{"url":null}"#),
            ("198.51.100.7:4321", r#"{"url":"/cast/secret/live.mp3"}"#),
        ] {
            let mut request = Request::builder()
                .method(Method::GET)
                .uri("/api/listen")
                .header(COOKIE, &guest)
                .body(Body::empty())
                .unwrap();
            request.extensions_mut().insert(ConnectInfo(
                address.parse::<std::net::SocketAddr>().unwrap(),
            ));
            let response = crate::app(state.clone()).oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), 1024).await.unwrap();
            assert_eq!(body.as_ref(), expected.as_bytes(), "client {address}");
        }
    }

    #[tokio::test]
    async fn the_stream_stays_private_until_allowed() {
        use crate::test_util::{call, get_body, session};
        use djinn_store::Role;

        let (_tap, reader) = djinn_engine::stream_tap(1.0, 48_000);
        let stream = LiveStream::start(reader, 48_000).unwrap();
        let state = AppState::for_tests()
            .await
            .with_stream(stream, "secret".into());
        let admin = session(&state, "ana", Role::Admin).await;
        let snapshot = |path: &'static str| {
            let state = state.clone();
            let admin = admin.clone();
            async move {
                let (_, text) = get_body(&state, path, &admin).await;
                let json: serde_json::Value = serde_json::from_str(&text).unwrap();
                json["stream_url"].clone()
            }
        };
        assert!(snapshot("/api/tv").await.is_null(), "the TV feed is public");
        assert!(snapshot("/api/tv?key=wrong").await.is_null());
        assert_eq!(
            snapshot("/api/tv?key=secret").await,
            "/cast/secret/live.mp3"
        );
        state
            .store
            .set_setting(crate::tv_api::TV_SOUND, "1")
            .await
            .unwrap();
        assert_eq!(snapshot("/api/tv").await, "/cast/secret/live.mp3");

        let (status, _) = call(
            &state,
            axum::http::Method::POST,
            "/api/cast/new-link",
            None,
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let token = state.stream_token();
        assert!(token.len() == 32 && token != "secret");
        assert_eq!(
            state
                .store
                .get_setting(CAST_TOKEN)
                .await
                .unwrap()
                .as_deref(),
            Some(token.as_str())
        );
        assert_eq!(snapshot("/api/tv").await, stream_path(&token).as_str());
        let (status, _) = call(
            &state,
            axum::http::Method::GET,
            "/cast/secret/live.mp3",
            None,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "the old address");
    }
}
