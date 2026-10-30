//! Casting: the live stream that cast devices and the TV page play, and
//! the admin API that finds devices and starts and stops casts.
//!
//! The stream URL carries a random token instead of a session cookie,
//! because Chromecasts, TVs, and Sonos speakers fetch it themselves and
//! can't sign in. Only DJINN hands the URL out.

use axum::body::{Body, Bytes};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use djinn_cast::stream::{self, LiveStream};
use djinn_cast::{CastError, CastMedia, CastStatus, Metadata};
use djinn_store::Store;
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
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
        .route("/cast/{id}/start", post(start))
        .route("/cast/{id}/stop", post(stop))
        .route("/cast/{id}/volume", put(volume))
}

#[derive(Serialize)]
struct Listen {
    /// The live stream, or none when the host turned Play here off.
    url: Option<String>,
}

/// Where Play here listens: the same stream casts play.
async fn listen(_: CurrentUser, State(state): State<AppState>) -> ApiResult<Json<Listen>> {
    let on = settings::flag(&state.store, PLAY_HERE, false).await?;
    Ok(Json(Listen {
        url: state
            .stream
            .as_ref()
            .filter(|_| on)
            .map(|_| stream_path(&state.cast_token)),
    }))
}

#[derive(Serialize)]
struct CastList {
    devices: Vec<CastStatus>,
    /// Players connected to the stream (casts and TV pages).
    listeners: usize,
}

fn cast_list(state: &AppState) -> CastList {
    CastList {
        devices: state.caster.status(),
        listeners: state.stream.as_ref().map_or(0, LiveStream::listeners),
    }
}

async fn list(_: AdminUser, State(state): State<AppState>) -> Json<CastList> {
    Json(cast_list(&state))
}

async fn discover(_: AdminUser, State(state): State<AppState>) -> Json<CastList> {
    state.caster.discover(DISCOVERY).await;
    Json(cast_list(&state))
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
    Ok(Json(cast_list(&state)))
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
    Ok(Json(cast_list(&state)))
}

async fn stop_all(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.caster.stop_all().await;
    state.store.set_setting(CAST_TARGETS, "").await?;
    Ok(StatusCode::NO_CONTENT)
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
    if state.stream.is_none() {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "the audio stream isn't running",
        ));
    }
    let base = state.join_url.trim_end_matches('/');
    let token = state.cast_token.as_str();
    let media = match mode {
        Mode::Audio => CastMedia::Stream {
            url: format!("{base}{}", stream_path(token)),
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
                url: format!("{base}/tv?cast=1"),
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
            let (id, mode) = match line.split_once('\t') {
                Some((id, "page")) => (id, Mode::Page),
                Some((id, _)) => (id, Mode::Audio),
                None => (line, Mode::Audio),
            };
            match start_cast(&resume, id, mode).await {
                Ok(()) => tracing::info!(device = id, "resuming cast"),
                Err(_) => tracing::info!(device = id, "cast device not found; not resuming"),
            }
        }
    });
}

/// The join QR code as a PNG: what TVs show while they play the party.
async fn join_png(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    if *state.cast_token != token {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(code) = qrcode::QrCode::new(state.join_url.as_bytes()) else {
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
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    store.set_setting(CAST_TOKEN, &token).await?;
    Ok(token)
}

/// Path of the live stream, relative to the server.
pub fn stream_path(token: &str) -> String {
    format!("/cast/{token}/live.mp3")
}

async fn live(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let Some(stream) = state.stream.as_ref().filter(|_| *state.cast_token == token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Internet radio players (Sonos, many receivers, VLC) ask for song
    // titles inside the stream.
    let wants_titles = headers
        .get("icy-metadata")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim() == "1");
    let title = if wants_titles {
        Some(song_title(&state.jukebox.lock().await.now_playing()))
    } else {
        None
    };
    live_response(
        stream,
        title.map(|t| (t, state.events.subscribe())),
        state.shutdown.subscribe(),
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
}

impl LiveBody {
    async fn next_chunk(&mut self) -> Option<Bytes> {
        loop {
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
    async fn play_here_hands_out_the_stream_once_turned_on() {
        use crate::test_util::{call, get_body, session};
        use axum::http::Method;
        use djinn_store::Role;

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
    }
}
