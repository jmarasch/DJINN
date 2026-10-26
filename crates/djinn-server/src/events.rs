//! Live updates pushed to clients over Server-Sent Events.

use djinn_library::ScanProgress;
use djinn_store::{PlaySource, QueueItem, Track};
use serde::Serialize;

/// The SSE event name is the variant name in snake_case; the data is the
/// variant's JSON.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
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
}

impl ServerEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::NowPlaying(_) => "now_playing",
            Self::Position { .. } => "position",
            Self::QueueChanged(_) => "queue_changed",
            Self::PlayerState(_) => "player_state",
            Self::OutputStatus(_) => "output_status",
            Self::ScanProgress(_) => "scan_progress",
            Self::LibraryChanged => "library_changed",
            Self::ArtistLookup(_) => "artist_lookup",
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
    /// The playlist playing when nobody has requested anything.
    pub playlist: Option<PlaylistRef>,
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

#[derive(Clone, Debug, Default, Serialize)]
pub struct OutputStatus {
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub error: Option<String>,
}
