//! The live stream: the engine's master mix as MP3, shared by every
//! listener (cast devices, TV page browsers).
//!
//! An encoder thread drains the engine's [`TapReader`], encodes 320 kbps
//! CBR MP3, keeps a few seconds of recent frames for new listeners, and
//! broadcasts the rest. It never lets the stream fall behind real time:
//! while nothing plays (paused, idle) it sends silence, because network
//! players treat a stalled stream as a broken one. The tap is only active
//! while someone listens.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use djinn_engine::TapReader;
use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig};
use tokio::sync::broadcast;

use crate::CastError;

pub const BITRATE_KBPS: u32 = 320;
pub const CONTENT_TYPE: &str = "audio/mpeg";
const TICK: Duration = Duration::from_millis(20);
/// Recent audio sent to a new listener first, so its player starts at
/// once instead of waiting to fill its buffer.
const BACKLOG_SECS: u64 = 3;
/// How far behind real time the stream may fall before silence fills in.
const MAX_LAG_SECS: f64 = 0.1;

struct Shared {
    listeners: AtomicUsize,
    backlog: Mutex<VecDeque<Bytes>>,
    tx: broadcast::Sender<Bytes>,
}

/// Handle to the stream. Cheap to clone; the encoder thread stops when the
/// last handle goes.
#[derive(Clone)]
pub struct LiveStream {
    shared: Arc<Shared>,
}

/// One listener: the backlog to send first, then live frames.
pub struct Listener {
    pub backlog: Vec<Bytes>,
    pub live: broadcast::Receiver<Bytes>,
    /// Counts this listener until dropped.
    pub guard: ListenerGuard,
}

pub struct ListenerGuard(Arc<Shared>);

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.0.listeners.fetch_sub(1, Ordering::AcqRel);
    }
}

impl LiveStream {
    pub fn start(reader: TapReader, rate: u32) -> Result<Self, CastError> {
        let (tx, _) = broadcast::channel(1024);
        let shared = Arc::new(Shared {
            listeners: AtomicUsize::new(0),
            backlog: Mutex::new(VecDeque::new()),
            tx,
        });
        let weak = Arc::downgrade(&shared);
        std::thread::Builder::new()
            .name("djinn-stream".to_owned())
            .spawn(move || run(&weak, reader, rate))
            .map_err(|e| CastError::Encoder(e.to_string()))?;
        Ok(Self { shared })
    }

    pub fn subscribe(&self) -> Listener {
        self.shared.listeners.fetch_add(1, Ordering::AcqRel);
        // Subscribe before copying the backlog so no frame falls between.
        let live = self.shared.tx.subscribe();
        let backlog = self
            .shared
            .backlog
            .lock()
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default();
        Listener {
            backlog,
            live,
            guard: ListenerGuard(Arc::clone(&self.shared)),
        }
    }

    pub fn listeners(&self) -> usize {
        self.shared.listeners.load(Ordering::Acquire)
    }
}

/// Encoder state for one listening session.
struct Session {
    encoder: Mp3Encoder,
    started: Instant,
    /// Frames encoded since `started`.
    sent: u64,
    /// The encoder's first packet is a file header for gapless playback
    /// of a finished file; a live stream doesn't want it.
    header_skipped: bool,
}

impl Session {
    fn new() -> Self {
        Self {
            encoder: Mp3Encoder::new(Mp3EncoderConfig {
                bitrate_kbps: BITRATE_KBPS,
                vbr_quality: None,
            }),
            started: Instant::now(),
            sent: 0,
            header_skipped: false,
        }
    }
}

fn run(shared: &std::sync::Weak<Shared>, mut reader: TapReader, rate: u32) {
    let mut pcm = Vec::new();
    let mut session: Option<Session> = None;
    let backlog_bytes = (BACKLOG_SECS * u64::from(BITRATE_KBPS) * 1000 / 8) as usize;
    loop {
        std::thread::sleep(TICK);
        let Some(shared) = shared.upgrade() else {
            reader.set_active(false);
            return;
        };
        let listening = shared.listeners.load(Ordering::Acquire) > 0;
        reader.set_active(listening);
        pcm.clear();
        reader.read(&mut pcm);
        if !listening {
            if session.take().is_some() {
                if let Ok(mut backlog) = shared.backlog.lock() {
                    backlog.clear();
                }
            }
            continue;
        }
        let s = session.get_or_insert_with(Session::new);

        // Keep up with real time: fill any shortfall with silence.
        let due = (s.started.elapsed().as_secs_f64() * f64::from(rate)) as u64;
        let have = s.sent + (pcm.len() / 2) as u64;
        let lag = (MAX_LAG_SECS * f64::from(rate)) as u64;
        if have + lag < due {
            let missing = usize::try_from(due - have).unwrap_or(0);
            pcm.resize(pcm.len() + missing * 2, 0.0);
        }
        if pcm.is_empty() {
            continue;
        }
        s.sent += (pcm.len() / 2) as u64;
        if let Err(e) = s.encoder.push_pcm_f32(&pcm, 2, rate) {
            tracing::warn!(error = %e, "stream encoder failed, restarting it");
            session = None;
            continue;
        }
        while let Ok(packet) = s.encoder.next_packet() {
            if !s.header_skipped {
                s.header_skipped = true;
                continue;
            }
            let packet = Bytes::from(packet);
            if let Ok(mut backlog) = shared.backlog.lock() {
                backlog.push_back(packet.clone());
                let mut total: usize = backlog.iter().map(Bytes::len).sum();
                while total > backlog_bytes {
                    match backlog.pop_front() {
                        Some(old) => total -= old.len(),
                        None => break,
                    }
                }
            }
            let _ = shared.tx.send(packet);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use djinn_engine::stream_tap;

    #[test]
    fn streams_mp3_and_fills_gaps_with_silence() {
        let (_tap, reader) = stream_tap(2.0, 48_000);
        let stream = LiveStream::start(reader, 48_000).unwrap();
        let mut listener = stream.subscribe();
        assert_eq!(stream.listeners(), 1);
        // Nothing plays, yet frames keep coming (silence) at about the
        // bitrate.
        std::thread::sleep(Duration::from_millis(600));
        let mut bytes = Vec::new();
        while let Ok(frame) = listener.live.try_recv() {
            bytes.extend_from_slice(&frame);
        }
        assert!(bytes.len() > 10_000, "got {} bytes", bytes.len());
        // MPEG-1 Layer III frames: sync word, then 320 kbps at 48 kHz.
        assert_eq!(bytes[0], 0xFF);
        assert_eq!(bytes[1] & 0xE0, 0xE0);
        drop(listener);
        assert_eq!(stream.listeners(), 0);
    }
}
