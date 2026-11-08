//! Main's audio for nodes: the engine's timed copy of the mix (see
//! `djinn_engine::timed`), framed and sent to every joined node that's
//! listening, plus the clock nodes match theirs to.
//!
//! One thread drains the engine's tap while any node listens and hands the
//! same frames to all of them. A node that joins late first hears the
//! current volume and whether playback is paused.
//!
//! Frames: a little-endian u32 length, then a type byte and its payload:
//! `A` audio (i64 play-at µs, u32 frames, then frames × 2 i16 samples), `P`
//! pause, `R` resume, `F` flush, `V` volume (f32).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use djinn_engine::{Mark, Piece, TimedReader};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::client::DirectTls;
use crate::error::{ApiError, ApiResult};
use crate::nodes::NodeAuth;
use crate::state::AppState;

/// Frames waiting per node before it counts as fallen behind (chunks are
/// about 10 ms, so a few seconds).
const BACKLOG: usize = 512;
const POLL: Duration = Duration::from_millis(5);

/// Encodes one piece of the stream as a frame.
/// How often a quiet stream says it's still there (well inside the node's
/// 10 second read timeout).
const STILL_HERE_EVERY: std::time::Duration = std::time::Duration::from_secs(3);

pub fn encode(piece: &Piece) -> Bytes {
    let mut body = Vec::with_capacity(1 + 12 + piece.samples.len() * 2);
    match piece.mark {
        Mark::Audio { play_at_us, .. } => {
            body.push(b'A');
            body.extend_from_slice(&play_at_us.to_le_bytes());
            body.extend_from_slice(&((piece.samples.len() / 2) as u32).to_le_bytes());
            for sample in &piece.samples {
                let s = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
                body.extend_from_slice(&s.to_le_bytes());
            }
        }
        Mark::Pause => body.push(b'P'),
        Mark::Resume => body.push(b'R'),
        Mark::Flush => body.push(b'F'),
        Mark::Volume(volume) => {
            body.push(b'V');
            body.extend_from_slice(&volume.to_le_bytes());
        }
    }
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    Bytes::from(frame)
}

/// Splits whole frames off the front of `buffer` and decodes them; a
/// partial frame stays for the next read. None on a malformed frame.
pub fn decode(buffer: &mut Vec<u8>) -> Option<Vec<Piece>> {
    let mut pieces = Vec::new();
    let mut at = 0;
    while buffer.len() - at >= 4 {
        let len = u32::from_le_bytes(buffer[at..at + 4].try_into().ok()?) as usize;
        if len == 0 || len > 16 * 1024 * 1024 {
            return None;
        }
        if buffer.len() - at - 4 < len {
            break;
        }
        let body = &buffer[at + 4..at + 4 + len];
        let piece = match body[0] {
            b'A' if body.len() >= 13 => {
                let play_at_us = i64::from_le_bytes(body[1..9].try_into().ok()?);
                let frames = u32::from_le_bytes(body[9..13].try_into().ok()?);
                let data = &body[13..];
                if data.len() != frames as usize * 4 {
                    return None;
                }
                let samples = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| f32::from(i16::from_le_bytes(*b)) / f32::from(i16::MAX))
                    .collect();
                Piece {
                    mark: Mark::Audio { frames, play_at_us },
                    samples,
                }
            }
            b'P' => Piece {
                mark: Mark::Pause,
                samples: Vec::new(),
            },
            b'R' => Piece {
                mark: Mark::Resume,
                samples: Vec::new(),
            },
            b'F' => Piece {
                mark: Mark::Flush,
                samples: Vec::new(),
            },
            b'V' if body.len() >= 5 => Piece {
                mark: Mark::Volume(f32::from_le_bytes(body[1..5].try_into().ok()?)),
                samples: Vec::new(),
            },
            _ => return None,
        };
        pieces.push(piece);
        at += 4 + len;
    }
    buffer.drain(..at);
    Some(pieces)
}

/// Hands the engine's timed copy to every listening node.
pub struct Hub {
    tx: broadcast::Sender<Bytes>,
    listeners: Arc<AtomicUsize>,
    /// The last volume and pause state seen, for nodes that join late.
    state: Arc<Mutex<(f32, bool)>>,
    rate: u32,
}

impl Default for Hub {
    /// A hub with no engine behind it (tests): nothing is ever sent.
    fn default() -> Self {
        Hub {
            tx: broadcast::channel(BACKLOG).0,
            listeners: Arc::default(),
            state: Arc::new(Mutex::new((1.0, false))),
            rate: 48_000,
        }
    }
}

/// A node listening; counts while it lives.
pub struct Listening {
    pub frames: broadcast::Receiver<Bytes>,
    listeners: Arc<AtomicUsize>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.listeners.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Hub {
    /// Starts draining `reader` (the engine's timed tap) whenever a node
    /// listens.
    pub fn start(mut reader: TimedReader, rate: u32) -> anyhow::Result<Hub> {
        let hub = Hub {
            rate,
            ..Hub::default()
        };
        let (tx, listeners, state) = (
            hub.tx.clone(),
            Arc::clone(&hub.listeners),
            Arc::clone(&hub.state),
        );
        std::thread::Builder::new()
            .name("djinn-node-audio".to_owned())
            .spawn(move || loop {
                let wanted = listeners.load(Ordering::Acquire) > 0;
                if reader.is_active() != wanted {
                    reader.set_active(wanted);
                }
                if wanted {
                    for piece in reader.read() {
                        if let Ok(mut s) = state.lock() {
                            match piece.mark {
                                Mark::Volume(v) => s.0 = v,
                                Mark::Pause => s.1 = true,
                                Mark::Resume => s.1 = false,
                                _ => {}
                            }
                        }
                        let _ = tx.send(encode(&piece));
                    }
                }
                std::thread::sleep(POLL);
            })?;
        Ok(hub)
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Starts listening: the current volume and pause state, then the live
    /// frames.
    pub fn listen(&self) -> (Vec<Bytes>, Listening) {
        let frames = self.tx.subscribe();
        self.listeners.fetch_add(1, Ordering::AcqRel);
        let (volume, paused) = self.state.lock().map(|s| *s).unwrap_or((1.0, false));
        let mut first = vec![encode(&Piece {
            mark: Mark::Volume(volume),
            samples: Vec::new(),
        })];
        if paused {
            first.push(encode(&Piece {
                mark: Mark::Pause,
                samples: Vec::new(),
            }));
        }
        (
            first,
            Listening {
                frames,
                listeners: Arc::clone(&self.listeners),
            },
        )
    }
}

#[derive(Serialize)]
pub struct ClockAnswer {
    now_us: i64,
}

/// Main's clock, for nodes to match theirs to. Public but encrypted-only:
/// a count of microseconds says nothing, and skipping the sign-in keeps the
/// answer quick, which is what makes it accurate.
pub async fn clock(tls: Option<axum::Extension<DirectTls>>) -> ApiResult<Json<ClockAnswer>> {
    let now_us = djinn_engine::clock::now_micros();
    if tls.is_none() {
        return Err(ApiError::new(
            StatusCode::UPGRADE_REQUIRED,
            "nodes connect over an encrypted connection",
        ));
    }
    Ok(Json(ClockAnswer { now_us }))
}

/// The live audio for a joined node, as frames, until it disconnects.
pub async fn stream(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
) -> ApiResult<Response> {
    if node.status != "approved" {
        return Err(ApiError::forbidden());
    }
    let hub = state.hub_for(node.room_id);
    let (first, listening) = hub.listen();
    let rate = hub.rate();
    tracing::info!(node = %node.name, "node listening");
    let live = frames_for(first, listening, hub);
    let body = Body::from_stream(futures_util::StreamExt::map(
        live,
        Ok::<_, std::convert::Infallible>,
    ));
    Ok((
        [
            (header::CONTENT_TYPE, "application/x-djinn-audio".to_owned()),
            (header::CACHE_CONTROL, "no-store".to_owned()),
            (
                header::HeaderName::from_static("x-djinn-rate"),
                rate.to_string(),
            ),
        ],
        body,
    )
        .into_response())
}

/// The first frames, then each live one; a node that falls behind skips
/// ahead (the next chunk's time says where it belongs). While nothing plays,
/// the volume goes out every few seconds so the node knows Main is still
/// there and doesn't take a quiet room for a lost server.
fn frames_for(
    first: Vec<Bytes>,
    listening: Listening,
    hub: std::sync::Arc<Hub>,
) -> impl futures_util::Stream<Item = Bytes> {
    let first = futures_util::stream::iter(first);
    // The stream owns `listening`, so the node counts until it's gone.
    let live = futures_util::stream::unfold((listening, hub), |(mut listening, hub)| async move {
        loop {
            match tokio::time::timeout(STILL_HERE_EVERY, listening.frames.recv()).await {
                Ok(Ok(frame)) => return Some((frame, (listening, hub))),
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(broadcast::error::RecvError::Closed)) => return None,
                Err(_) => {
                    let volume = hub.state.lock().map(|s| s.0).unwrap_or(1.0);
                    let frame = encode(&Piece {
                        mark: Mark::Volume(volume),
                        samples: Vec::new(),
                    });
                    return Some((frame, (listening, hub)));
                }
            }
        }
    });
    futures_util::StreamExt::chain(first, live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let pieces = [
            Piece {
                mark: Mark::Volume(0.5),
                samples: vec![],
            },
            Piece {
                mark: Mark::Audio {
                    frames: 2,
                    play_at_us: -42,
                },
                samples: vec![0.0, 0.5, -0.5, 1.0],
            },
            Piece {
                mark: Mark::Pause,
                samples: vec![],
            },
            Piece {
                mark: Mark::Resume,
                samples: vec![],
            },
            Piece {
                mark: Mark::Flush,
                samples: vec![],
            },
        ];
        let mut bytes: Vec<u8> = pieces.iter().flat_map(|p| encode(p).to_vec()).collect();
        // Arrives in two parts, split mid-frame.
        let rest = bytes.split_off(9);
        let mut buffer = bytes;
        let mut got = decode(&mut buffer).unwrap();
        buffer.extend_from_slice(&rest);
        got.extend(decode(&mut buffer).unwrap());
        assert!(buffer.is_empty());
        assert_eq!(got.len(), 5);
        assert_eq!(got[0].mark, Mark::Volume(0.5));
        assert_eq!(
            got[1].mark,
            Mark::Audio {
                frames: 2,
                play_at_us: -42
            }
        );
        assert!((got[1].samples[1] - 0.5).abs() < 1e-4 && (got[1].samples[3] - 1.0).abs() < 1e-4);
        assert_eq!(got[4].mark, Mark::Flush);
    }

    #[test]
    fn garbage_is_refused() {
        let mut buffer = vec![5, 0, 0, 0, b'Z', 1, 2, 3, 4];
        assert!(decode(&mut buffer).is_none());
        let mut buffer = vec![0, 0, 0, 0];
        assert!(decode(&mut buffer).is_none());
    }

    #[test]
    fn late_listeners_hear_volume_and_pause_first() {
        let hub = Hub::default();
        if let Ok(mut s) = hub.state.lock() {
            *s = (0.3, true);
        }
        let (first, _listening) = hub.listen();
        let mut buffer: Vec<u8> = first.iter().flat_map(|b| b.to_vec()).collect();
        let pieces = decode(&mut buffer).unwrap();
        assert_eq!(pieces[0].mark, Mark::Volume(0.3));
        assert_eq!(pieces[1].mark, Mark::Pause);
        assert_eq!(hub.listeners.load(Ordering::Acquire), 1);
        drop(_listening);
        assert_eq!(hub.listeners.load(Ordering::Acquire), 0);
    }
}
