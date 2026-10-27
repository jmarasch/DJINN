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
mod devices;
mod drift;
mod engine;
mod error;
mod features;
mod loudness;
mod mixdown;
mod output;
mod outputs;
mod player;
mod resample;
mod tap;

use std::path::PathBuf;

pub use devices::{output_devices, DeviceInfo};
pub use engine::{Engine, EngineConfig};
pub use error::EngineError;
pub use features::{analyze, describe, Analysis, Features, FEATURES_VERSION, FEATURE_COUNT};
pub use loudness::{measure_loudness, Loudness, REFERENCE_LUFS};
pub use player::MAX_CROSSFADE_SECS;
pub use tap::{stream_tap, StreamTap, TapReader};

/// Opaque track identifier chosen by the engine's owner.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub i64);

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
    /// Join to the track before it without a crossfade, sample-exactly
    /// (for example, the next song on the same album).
    pub gapless: bool,
    /// Linear gain for this track (ReplayGain), 1.0 for none.
    pub gain: f32,
}

/// Longest per-output delay, in milliseconds.
pub const MAX_DELAY_MS: u32 = 1000;

/// Which device an output plays through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputDevice {
    /// The OS default device; follows it when it changes.
    Default,
    /// A device by id, as listed by [`output_devices`].
    Id(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputConfig {
    /// Chosen by the owner and reported back in [`OutputStatus`].
    pub id: i64,
    pub device: OutputDevice,
    pub enabled: bool,
    /// Gain on top of the master volume, 0.0..=1.0.
    pub trim: f32,
    /// Holds this output back by up to [`MAX_DELAY_MS`], to line it up
    /// with slower ones (Bluetooth lags by a few hundred milliseconds).
    pub delay_ms: u32,
}

impl OutputConfig {
    /// The OS default device at full trim: what the engine starts with.
    pub fn system_default(id: i64) -> Self {
        Self {
            id,
            device: OutputDevice::Default,
            enabled: true,
            trim: 1.0,
            delay_ms: 0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutputState {
    Disabled,
    /// Not open yet, or lost and waiting to retry.
    Waiting,
    Playing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputStatus {
    pub id: i64,
    pub state: OutputState,
    /// Device name while open.
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    /// Why it isn't playing, if it failed.
    pub error: Option<String>,
    /// The output the others are kept in time with.
    pub clock: bool,
    /// Speed correction in parts per million (followers only).
    pub drift_ppm: f64,
    /// How far it is from where it should be, in milliseconds. Correction
    /// keeps this near zero.
    pub offset_ms: f64,
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
    /// Crossfade length in seconds for upcoming transitions, 0 to 12; 0
    /// joins tracks gaplessly.
    SetCrossfade(u32),
    /// Replaces the output list. Outputs that keep their id and device keep
    /// playing; trim and delay changes apply right away.
    SetOutputs(Vec<OutputConfig>),
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
    /// Output status: sent when an output opens, fails, or changes, and
    /// every few seconds while more than one plays (for sync figures).
    Outputs(Vec<OutputStatus>),
}
