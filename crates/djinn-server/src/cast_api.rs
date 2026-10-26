//! Casting: the live stream that cast devices and the TV page play.
//!
//! The stream URL carries a random token instead of a session cookie,
//! because Chromecasts, TVs, and Sonos speakers fetch it themselves and
//! can't sign in. Only DJINN hands the URL out.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use djinn_cast::stream::{self, LiveStream};
use djinn_store::Store;
use futures_util::stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::AppState;

/// Setting key holding the stream token.
const CAST_TOKEN: &str = "cast_token";

/// Routes outside /api: devices fetch these without cookies.
pub fn stream_router() -> Router<AppState> {
    Router::new().route("/cast/{token}/live.mp3", get(live))
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

async fn live(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    let Some(stream) = state.stream.as_ref().filter(|_| *state.cast_token == token) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    live_response(stream)
}

fn live_response(stream: &LiveStream) -> Response {
    let stream::Listener {
        backlog,
        live,
        guard,
    } = stream.subscribe();
    let live = BroadcastStream::new(live).filter_map(move |frame| {
        // The guard counts this listener for as long as the response
        // streams.
        let _ = &guard;
        // A slow client that skipped frames (lagged) picks up again at the
        // next one; MP3 resyncs on its own.
        std::future::ready(frame.ok())
    });
    let backlog = futures_util::stream::iter(backlog);
    let body = backlog.chain(live).map(Ok::<_, std::convert::Infallible>);
    (
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
        .into_response()
}
