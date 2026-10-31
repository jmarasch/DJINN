//! Live updates pushed to clients over Server-Sent Events.

use djinn_library::ScanProgress;
use djinn_store::{PlaySource, QueueItem, Track};
use serde::Serialize;

/// The SSE event name is the variant name in snake_case; the data is the
/// variant's JSON.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
    /// First on every connection: the web app build the server has, so an
    /// out-of-date page reloads.
    Hello {
        web_build: Option<String>,
    },
    NowPlaying(Box<NowPlaying>),
    Position {
        track_id: i64,
        position_ms: u64,
    },
    QueueChanged(Vec<QueueItem>),
    PlayerState(PlayerState),
    OutputStatus(OutputStatus),
    ScanProgress(ScanProgress),
    /// Artists, albums, or pictures changed; cached lists are stale.
    LibraryChanged,
    ArtistLookup(LookupStatus),
    /// Progress measuring song loudness for ReplayGain.
    Loudness(LoudnessStatus),
    /// Cast devices and how their casts are going.
    CastStatus(Vec<djinn_cast::CastStatus>),
    /// AI listening: download, model, and progress.
    AiStatus(Box<crate::ai::AiStatus>),
    /// Identifying songs by fingerprint: progress.
    IdentifyStatus(crate::identify_api::IdentifyStatus),
}

impl ServerEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "hello",
            Self::NowPlaying(_) => "now_playing",
            Self::Position { .. } => "position",
            Self::QueueChanged(_) => "queue_changed",
            Self::PlayerState(_) => "player_state",
            Self::OutputStatus(_) => "output_status",
            Self::ScanProgress(_) => "scan_progress",
            Self::LibraryChanged => "library_changed",
            Self::ArtistLookup(_) => "artist_lookup",
            Self::Loudness(_) => "loudness",
            Self::CastStatus(_) => "cast_status",
            Self::AiStatus(_) => "ai_status",
            Self::IdentifyStatus(_) => "identify_status",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct NowPlaying {
    pub current: Option<Entry>,
    pub position_ms: u64,
    pub up_next: Option<Entry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub track: Track,
    pub source: PlaySource,
    /// Username of whoever requested it.
    pub requested_by: Option<String>,
    pub requested_by_id: Option<i64>,
    /// Others who asked for it too.
    pub backers: Vec<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayState {
    Idle,
    Playing,
    Paused,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlayerState {
    pub state: PlayState,
    /// Master volume, 0 to 100.
    pub volume: u8,
    /// Crossfade between songs, in seconds; 0 is gapless.
    pub crossfade_secs: u8,
    /// The playlist playing when nobody has requested anything.
    pub playlist: Option<PlaylistRef>,
    /// Votes to skip the song playing.
    pub skip_votes: u32,
    /// Who voted, so each person's screen knows whether they did.
    pub skip_voters: Vec<i64>,
    /// Votes that skip it; 0 when skip voting is off.
    pub skip_votes_needed: u32,
    /// People who voted to skip the previous song, so sit this one out.
    pub skip_resting: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlaylistRef {
    pub id: i64,
    pub name: String,
    pub shuffle: bool,
}

/// Online artist photo and bio lookups.
#[derive(Clone, Debug, Default, Serialize)]
pub struct LookupStatus {
    pub running: bool,
    pub last: Option<djinn_library::online::LookupSummary>,
    /// When the last lookup finished (Unix ms).
    pub finished_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct LoudnessStatus {
    #[serde(flatten)]
    pub progress: djinn_store::LoudnessProgress,
    pub running: bool,
    /// Sound analysis is switched on.
    pub on: bool,
    /// What it's waiting for (music playing, or outside its hours).
    pub held: Option<crate::background::Held>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct OutputStatus {
    /// The devices playing, for the status line.
    pub device: Option<String>,
    /// The clock output's sample rate.
    pub sample_rate: Option<u32>,
    /// Why nothing plays, when no output does.
    pub error: Option<String>,
    /// Each output's live state, for the admin page.
    pub outputs: Vec<LiveOutput>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveOutput {
    pub id: i64,
    /// "disabled", "waiting", or "playing".
    pub state: &'static str,
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub error: Option<String>,
    /// The output the others are kept in time with.
    pub clock: bool,
    pub drift_ppm: f64,
    pub offset_ms: f64,
}

impl OutputStatus {
    pub fn from_engine(list: &[djinn_engine::OutputStatus]) -> Self {
        let playing: Vec<_> = list
            .iter()
            .filter(|o| o.state == djinn_engine::OutputState::Playing)
            .collect();
        let names: Vec<&str> = playing.iter().filter_map(|o| o.device.as_deref()).collect();
        let round = |x: f64| (x * 10.0).round() / 10.0;
        Self {
            device: (!names.is_empty()).then(|| names.join(" + ")),
            sample_rate: playing.iter().find(|o| o.clock).and_then(|o| o.sample_rate),
            error: if playing.is_empty() {
                list.iter().find_map(|o| o.error.clone())
            } else {
                None
            },
            outputs: list
                .iter()
                .map(|o| LiveOutput {
                    id: o.id,
                    state: match o.state {
                        djinn_engine::OutputState::Disabled => "disabled",
                        djinn_engine::OutputState::Waiting => "waiting",
                        djinn_engine::OutputState::Playing => "playing",
                    },
                    device: o.device.clone(),
                    sample_rate: o.sample_rate,
                    error: o.error.clone(),
                    clock: o.clock,
                    drift_ppm: round(o.drift_ppm),
                    offset_ms: round(o.offset_ms),
                })
                .collect(),
        }
    }
}
