//! DJINN audio engine: decode, mix, and fan out to output devices.
//!
//! This crate must stay free of web, database, and async runtime
//! dependencies. The server talks to it through [`Command`]s and receives
//! [`Event`]s through a callback.
//!
//! The engine never decides what plays next. It emits [`Event::NextNeeded`]
//! whenever the slot after the track being decoded is empty, and the owner
//! answers with [`Command::SetNext`].

mod decoder;
mod engine;
mod error;
mod mixdown;
mod output;
mod player;
mod resample;

use std::path::PathBuf;

pub use engine::{Engine, EngineConfig};
pub use error::EngineError;

/// Opaque track identifier chosen by the engine's owner.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub i64);

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Sets or replaces the track that follows the one being decoded. If the
    /// engine is idle, it starts immediately (unless paused).
    SetNext(Option<Track>),
    /// Resume playback.
    Play,
    Pause,
    /// End the audible track now and move to the next one.
    Skip,
    /// Master volume as linear gain, clamped to 0.0..=1.0.
    SetVolume(f32),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EndReason {
    Finished,
    Skipped,
    Error,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlayState {
    /// Nothing to play.
    Idle,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The slot after the track being decoded is empty. Answer with
    /// [`Command::SetNext`]. `after` is the track that just left the slot
    /// and started decoding, or `None` if the slot emptied otherwise (at
    /// startup, or after a track failed to open). Because decoding runs
    /// ahead of playback, this arrives shortly before `after` is audible.
    NextNeeded {
        after: Option<TrackId>,
    },
    /// The track's first sample reached the output.
    TrackStarted {
        id: TrackId,
    },
    TrackEnded {
        id: TrackId,
        played_ms: u64,
        reason: EndReason,
    },
    /// A track could not be opened or failed mid-stream. A failure to open
    /// is followed by [`Event::NextNeeded`] with `after: None`.
    TrackError {
        id: TrackId,
        error: String,
    },
    /// Playback position of the audible track, roughly twice a second.
    Position {
        id: TrackId,
        ms: u64,
    },
    StateChanged(PlayState),
    OutputOpened {
        device: String,
        sample_rate: u32,
        channels: u16,
    },
    /// The output failed. Playback stalls and the engine retries.
    OutputError {
        error: String,
    },
}
