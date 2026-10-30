//! Decides what plays: requests first, then the active playlist. Feeds the engine
//! and turns its events into history and live updates.
//!
//! Tracks move through three stages: `queued` (sent to the engine as the
//! next track), `in_flight` (the engine is decoding it, but it isn't audible
//! yet), and `playing`. A request stays in the request queue until it
//! becomes audible, so "up next" is always visible. The song playing, and
//! how far it got, is saved as it goes, so after a restart it carries on
//! where it was.

mod auto;
mod bag;
mod gain;
mod mix;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use djinn_engine::{Command, EndReason, Event as EngineEvent, PlayState as EngineState, TrackId};
use djinn_store::{
    AddRequestError, Backer, PlaySource, QueueItem, RequestLimits, RequestOrigin, RequestOutcome,
    Store, Track, User, Withdrawal,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

pub use self::auto::{SMART_PLAY, SMART_PLAYLISTS, SMART_STRENGTH};
use self::bag::ShuffleBag;
pub use self::gain::{
    ReplayGain, ReplayGainMode, MAX_PREAMP_DB, REPLAY_GAIN_MODE, REPLAY_GAIN_PREAMP,
};
pub use self::mix::{Learned, Mixer, Radio, RadioSeed, SongLinks};
use crate::events::{
    Entry, NowPlaying, OutputStatus, PlayState, PlayerState, PlaylistRef, ServerEvent,
};
use crate::settings;

pub const HISTORY_WINDOW: &str = "history_window";
pub const PER_USER_LIMIT: &str = "per_user_limit";
pub const VOLUME: &str = "volume";
/// Most requests the whole queue may hold; 0 means no cap.
pub const MAX_QUEUE: &str = "max_queue";
/// Crossfade between songs, in seconds (0 joins them gaplessly).
pub const CROSSFADE: &str = "crossfade_secs";
/// Playlist that plays when nobody has requested anything; 0 or missing
/// means none.
pub const ACTIVE_PLAYLIST: &str = "active_playlist";
/// The active "playlist" id for wild play: random songs from the whole
/// library.
pub const WILD_PLAY: i64 = -1;
/// The active "playlist" id for radio (see the `radio` setting).
pub const RADIO: i64 = -2;
/// Votes that skip a song; 0 turns skip voting off.
pub const SKIP_VOTES: &str = "skip_votes";
/// Last song the active playlist played, so in-order playback resumes.
const PLAYLIST_CURSOR: &str = "playlist_cursor";
/// The song playing and how far it got (a [`Resume`]), for carrying on
/// after a restart.
const RESUME: &str = "resume";
/// How often the position playing is saved, in seconds.
const RESUME_EVERY_SECS: u64 = 5;
pub const DEFAULT_HISTORY_WINDOW: i64 = 25;
pub const DEFAULT_PER_USER_LIMIT: i64 = 3;
const DEFAULT_VOLUME: i64 = 80;
/// Tracks replaced in the engine's slot that it may already have taken.
const MAX_REPLACED: usize = 8;

pub type EngineSink = Box<dyn Fn(Command) + Send + Sync>;

#[derive(Clone, Debug)]
struct Pending {
    track: Track,
    source: PlaySource,
    request_id: Option<i64>,
    requested_by: Option<(i64, String)>,
    /// Others who asked for it too.
    backers: Vec<Backer>,
    /// Carrying on with a song cut off by a restart: its play and where it
    /// got to, in milliseconds.
    resume: Option<(i64, u64)>,
}

/// What [`RESUME`] holds.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Resume {
    track_id: i64,
    source: PlaySource,
    requested_by: Option<(i64, String)>,
    #[serde(default)]
    backers: Vec<Backer>,
    play_event: i64,
    position_ms: u64,
    paused: bool,
}

impl Pending {
    fn entry(&self) -> Entry {
        Entry {
            track: self.track.clone(),
            source: self.source,
            requested_by: self.requested_by.as_ref().map(|(_, name)| name.clone()),
            requested_by_id: self.requested_by.as_ref().map(|(id, _)| *id),
            backers: self.backers.iter().map(|b| b.name.clone()).collect(),
        }
    }
}

struct Playing {
    pending: Pending,
    play_event: i64,
    position_ms: u64,
    /// Where it started, when it carried on after a restart; the engine
    /// counts from there.
    offset_ms: u64,
}

pub struct Jukebox {
    store: Store,
    engine: EngineSink,
    events: broadcast::Sender<ServerEvent>,
    bag: ShuffleBag,
    rng: StdRng,
    queued: Option<Pending>,
    replaced: VecDeque<Pending>,
    in_flight: VecDeque<Pending>,
    playing: Option<Playing>,
    /// Songs fading out under the playing one during a crossfade.
    ending: Vec<Playing>,
    state: PlayState,
    volume: u8,
    crossfade: u8,
    skip_requested_by: Option<i64>,
    /// People who voted to skip the song playing.
    skip_votes: HashSet<i64>,
    /// Votes that skip a song; 0 when skip voting is off.
    skip_votes_needed: u32,
    /// Songs started so far, to count songs since someone's skip vote.
    songs_started: u64,
    /// The song (by `songs_started`) each person last voted to skip. They
    /// sit out the next one, so one guest can't vote down every song.
    skip_voted_on: HashMap<i64, u64>,
    output: OutputStatus,
    last_position_second: Option<u64>,
    /// The playlist feeding the music, if any.
    active: Option<PlaylistRef>,
    /// Last song the active playlist played (for in-order playback).
    cursor: Option<i64>,
    /// Song to play next from the playlist ("play from here").
    start_at: Option<i64>,
    replay_gain: ReplayGain,
    /// Picks radio songs (set once the server's sound index exists).
    mixer: Option<Arc<dyn Mixer>>,
    /// The radio, while it's the active "playlist".
    radio: Option<Radio>,
    /// Each song's party weight, and when it was read.
    party: Option<(Instant, HashMap<i64, f32>)>,
    /// The song to carry on with after a restart, until it's sent.
    resuming: Option<Pending>,
    /// Last position saved for the song playing, in whole seconds.
    saved_second: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error(transparent)]
    Add(#[from] AddRequestError),
    #[error("not found")]
    NotFound,
    #[error("you can only remove your own requests")]
    NotYours,
    #[error("nothing is playing")]
    NothingPlaying,
    #[error("skip voting is off")]
    SkipVotesOff,
    #[error("you can vote to skip again after this song")]
    SkipResting,
    #[error(transparent)]
    Store(#[from] djinn_store::StoreError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Jukebox {
    pub async fn new(
        store: Store,
        engine: EngineSink,
        events: broadcast::Sender<ServerEvent>,
    ) -> anyhow::Result<Self> {
        let volume = settings::number(&store, VOLUME, DEFAULT_VOLUME)
            .await?
            .clamp(0, 100) as u8;
        let cursor = settings::number(&store, PLAYLIST_CURSOR, 0).await?;
        let crossfade = settings::number(&store, CROSSFADE, 0)
            .await?
            .clamp(0, i64::from(djinn_engine::MAX_CROSSFADE_SECS)) as u8;
        let skip_votes_needed = settings::number(&store, SKIP_VOTES, 0).await?.clamp(0, 100) as u32;
        let replay_gain = ReplayGain {
            mode: ReplayGainMode::parse(
                store
                    .get_setting(REPLAY_GAIN_MODE)
                    .await?
                    .as_deref()
                    .unwrap_or_default(),
            ),
            preamp_db: store
                .get_setting(REPLAY_GAIN_PREAMP)
                .await?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
        };
        let mut seed = [0u8; 8];
        getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
        let mut jukebox = Self {
            store,
            engine,
            events,
            bag: ShuffleBag::default(),
            rng: StdRng::seed_from_u64(u64::from_le_bytes(seed)),
            queued: None,
            replaced: VecDeque::new(),
            in_flight: VecDeque::new(),
            playing: None,
            ending: Vec::new(),
            state: PlayState::Idle,
            volume,
            crossfade,
            skip_requested_by: None,
            skip_votes: HashSet::new(),
            skip_votes_needed,
            songs_started: 0,
            skip_voted_on: HashMap::new(),
            output: OutputStatus::default(),
            last_position_second: None,
            active: None,
            cursor: (cursor > 0).then_some(cursor),
            start_at: None,
            replay_gain,
            mixer: None,
            radio: None,
            party: None,
            resuming: None,
            saved_second: None,
        };
        (jukebox.engine)(Command::SetVolume(gain(volume)));
        (jukebox.engine)(Command::SetCrossfade(u32::from(crossfade)));
        jukebox.load_active().await?;
        jukebox.load_resume().await?;
        Ok(jukebox)
    }

    /// Picks up the song a restart cut off; it goes to the engine first.
    async fn load_resume(&mut self) -> anyhow::Result<()> {
        let saved = self.store.get_setting(RESUME).await?;
        let Some(resume) = saved.and_then(|s| serde_json::from_str::<Resume>(&s).ok()) else {
            return Ok(());
        };
        let Some(track) = self.store.track(resume.track_id).await? else {
            return Ok(());
        };
        if resume.paused {
            (self.engine)(Command::Pause);
        }
        tracing::info!(
            track = resume.track_id,
            position_ms = resume.position_ms,
            "carrying on where playback was"
        );
        self.resuming = Some(Pending {
            track,
            source: resume.source,
            request_id: None,
            requested_by: resume.requested_by,
            backers: resume.backers,
            resume: Some((resume.play_event, resume.position_ms)),
        });
        Ok(())
    }

    /// The song a restart cut off hasn't started again yet.
    fn carrying_on(&self) -> bool {
        self.resuming.is_some()
            || self.queued.as_ref().is_some_and(|q| q.resume.is_some())
            || self.in_flight.iter().any(|p| p.resume.is_some())
    }

    /// Saves the song playing and how far it got, or that nothing is.
    pub async fn save_resume(&mut self) -> anyhow::Result<()> {
        let resume = self.playing.as_ref().map(|p| Resume {
            track_id: p.pending.track.id,
            source: p.pending.source,
            requested_by: p.pending.requested_by.clone(),
            backers: p.pending.backers.clone(),
            play_event: p.play_event,
            position_ms: p.position_ms,
            paused: self.state == PlayState::Paused,
        });
        match resume {
            Some(resume) => {
                self.store
                    .set_setting(RESUME, &serde_json::to_string(&resume)?)
                    .await?
            }
            // Until it plays again, keep what a restart cut off.
            None if self.carrying_on() => {}
            None => self.store.set_setting(RESUME, "").await?,
        }
        Ok(())
    }

    pub fn now_playing(&self) -> NowPlaying {
        NowPlaying {
            current: self.playing.as_ref().map(|p| p.pending.entry()),
            position_ms: self.playing.as_ref().map_or(0, |p| p.position_ms),
            up_next: self
                .in_flight
                .front()
                .or(self.queued.as_ref())
                .map(Pending::entry),
        }
    }

    pub fn player_state(&self) -> PlayerState {
        PlayerState {
            state: self.state,
            volume: self.volume,
            crossfade_secs: self.crossfade,
            playlist: self.active.clone(),
            skip_votes: u32::try_from(self.skip_votes.len()).unwrap_or(u32::MAX),
            skip_voters: self.skip_votes.iter().copied().collect(),
            skip_votes_needed: self.skip_votes_needed,
            skip_resting: self
                .skip_voted_on
                .iter()
                .filter(|&(_, &song)| song + 1 == self.songs_started)
                .map(|(&user, _)| user)
                .collect(),
        }
    }

    pub fn active_playlist(&self) -> Option<i64> {
        self.active.as_ref().map(|p| p.id)
    }

    pub fn output_status(&self) -> OutputStatus {
        self.output.clone()
    }

    pub fn replay_gain(&self) -> ReplayGain {
        self.replay_gain
    }

    /// Applies from the next song on.
    pub fn set_replay_gain(&mut self, replay_gain: ReplayGain) {
        self.replay_gain = replay_gain;
    }

    /// Replaces the engine's outputs.
    pub fn set_outputs(&self, outputs: Vec<djinn_engine::OutputConfig>) {
        (self.engine)(Command::SetOutputs(outputs));
    }

    /// Queues a request. Admins and users marked unlimited skip the
    /// per-person limit and the queue cap.
    pub async fn add_request(
        &mut self,
        user: &User,
        track_id: i64,
        origin: &RequestOrigin,
    ) -> Result<(QueueItem, RequestOutcome), QueueError> {
        let limits = if user.is_admin() || user.unlimited {
            RequestLimits::default()
        } else {
            let cap = settings::number(&self.store, MAX_QUEUE, 0).await?;
            RequestLimits {
                per_user: Some(
                    settings::number(&self.store, PER_USER_LIMIT, DEFAULT_PER_USER_LIMIT).await?,
                ),
                queue_cap: (cap > 0).then_some(cap),
            }
        };
        let added = self
            .store
            .add_request(track_id, user.id, limits, origin)
            .await?;
        self.queue_changed().await?;
        Ok(added)
    }

    /// Queues several songs in order (admin: no limits). Songs already in
    /// the queue are skipped. Returns how many were added.
    pub async fn add_requests(
        &mut self,
        admin: &User,
        track_ids: &[i64],
        origin: &RequestOrigin,
    ) -> Result<usize, QueueError> {
        let mut added = 0;
        for &id in track_ids {
            match self
                .store
                .add_request(id, admin.id, RequestLimits::default(), origin)
                .await
            {
                Ok(_) => added += 1,
                Err(
                    AddRequestError::AlreadyQueued
                    | AddRequestError::AlreadyYours
                    | AddRequestError::NotFound,
                ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if added > 0 {
            self.queue_changed().await?;
        }
        Ok(added)
    }

    /// People take themselves off a request: a backer steps back, and the
    /// requester hands it to the first backer (or it goes, if nobody else
    /// wanted it). Admins remove anyone's request outright.
    pub async fn remove_request(&mut self, user: &User, id: i64) -> Result<(), QueueError> {
        if self.store.queue_item(id).await?.is_none() {
            return Err(QueueError::NotFound);
        }
        match self.store.withdraw_request(id, user.id).await? {
            Withdrawal::NotInvolved if user.is_admin() => {
                self.store.remove_request(id).await?;
            }
            Withdrawal::NotInvolved => return Err(QueueError::NotYours),
            Withdrawal::Unbacked | Withdrawal::HandedOver | Withdrawal::Removed => {}
        }
        self.queue_changed().await?;
        Ok(())
    }

    pub async fn move_request(&mut self, id: i64, index: usize) -> Result<(), QueueError> {
        if !self.store.move_request(id, index).await? {
            return Err(QueueError::NotFound);
        }
        self.queue_changed().await?;
        Ok(())
    }

    /// Empties the request queue (Admin > System).
    pub async fn clear_queue(&mut self) -> anyhow::Result<()> {
        self.store.clear_queue().await?;
        self.queue_changed().await
    }

    pub fn play(&self) {
        (self.engine)(Command::Play);
    }

    pub fn pause(&self) {
        (self.engine)(Command::Pause);
    }

    pub fn skip(&mut self, by: &User) {
        self.skip_requested_by = Some(by.id);
        (self.engine)(Command::Skip);
    }

    pub fn set_skip_votes_needed(&mut self, needed: u32) {
        self.skip_votes_needed = needed;
        self.send(ServerEvent::PlayerState(self.player_state()));
    }

    /// A vote to skip the song playing. Admins skip at once, and so does
    /// whoever requested it if nobody else asked for it too. Returns
    /// whether it skipped.
    pub fn vote_skip(&mut self, user: &User) -> Result<bool, QueueError> {
        let Some(playing) = self.playing.as_ref() else {
            return Err(QueueError::NothingPlaying);
        };
        let own = playing
            .pending
            .requested_by
            .as_ref()
            .is_some_and(|(id, _)| *id == user.id)
            && playing.pending.backers.is_empty();
        if user.is_admin() || own {
            self.skip(user);
            return Ok(true);
        }
        if self.skip_votes_needed == 0 {
            return Err(QueueError::SkipVotesOff);
        }
        if self.skip_votes.contains(&user.id) {
            return Ok(false);
        }
        if self
            .skip_voted_on
            .get(&user.id)
            .is_some_and(|&song| song + 1 == self.songs_started)
        {
            return Err(QueueError::SkipResting);
        }
        self.skip_voted_on.insert(user.id, self.songs_started);
        self.skip_votes.insert(user.id);
        let skipped = self.skip_votes.len() >= self.skip_votes_needed as usize;
        if skipped {
            self.skip(user);
        }
        self.send(ServerEvent::PlayerState(self.player_state()));
        Ok(skipped)
    }

    /// Crossfade for upcoming transitions, in seconds (0 for gapless).
    pub async fn set_crossfade(&mut self, secs: u8) -> anyhow::Result<()> {
        self.crossfade = secs.min(djinn_engine::MAX_CROSSFADE_SECS as u8);
        (self.engine)(Command::SetCrossfade(u32::from(self.crossfade)));
        self.store
            .set_setting(CROSSFADE, &self.crossfade.to_string())
            .await?;
        self.send(ServerEvent::PlayerState(self.player_state()));
        Ok(())
    }

    pub async fn set_volume(&mut self, volume: u8) -> anyhow::Result<()> {
        self.volume = volume.min(100);
        (self.engine)(Command::SetVolume(gain(self.volume)));
        self.store
            .set_setting(VOLUME, &self.volume.to_string())
            .await?;
        self.send(ServerEvent::PlayerState(self.player_state()));
        Ok(())
    }

    pub async fn on_engine_event(&mut self, event: EngineEvent) -> anyhow::Result<()> {
        match event {
            EngineEvent::NextNeeded { after } => {
                if let Some(TrackId(id)) = after {
                    let taken = if self.queued.as_ref().is_some_and(|q| q.track.id == id) {
                        self.queued.take()
                    } else {
                        // Replaced after the engine had already taken it.
                        self.replaced
                            .iter()
                            .position(|p| p.track.id == id)
                            .and_then(|i| self.replaced.remove(i))
                    };
                    if let Some(pending) = taken {
                        self.in_flight.push_back(pending);
                    }
                }
                self.refresh_next().await?;
                self.broadcast_now_playing();
            }
            EngineEvent::TrackStarted { id: TrackId(id) } => self.track_started(id).await?,
            EngineEvent::TrackEnded {
                id: TrackId(id),
                played_ms,
                reason,
            } => self.track_ended(id, played_ms, reason).await?,
            EngineEvent::TrackError {
                id: TrackId(id),
                error,
            } => {
                tracing::warn!(track = id, %error, "track failed");
                if let Some(pending) = self.queued.take_if(|q| q.track.id == id) {
                    // Failed to open; drop a broken request so it isn't retried.
                    if let Some(request) = pending.request_id {
                        self.store.remove_request(request).await?;
                        self.broadcast_queue().await?;
                    }
                }
            }
            EngineEvent::Position {
                id: TrackId(id),
                ms,
            } => {
                if let Some(playing) = self.playing.as_mut().filter(|p| p.pending.track.id == id) {
                    let ms = playing.offset_ms + ms;
                    playing.position_ms = ms;
                    let second = ms / 1000;
                    if self.last_position_second != Some(second) {
                        self.last_position_second = Some(second);
                        self.send(ServerEvent::Position {
                            track_id: id,
                            position_ms: ms,
                        });
                    }
                    if second % RESUME_EVERY_SECS == 0 && self.saved_second != Some(second) {
                        self.saved_second = Some(second);
                        self.save_resume().await?;
                    }
                }
            }
            EngineEvent::StateChanged(state) => {
                self.state = match state {
                    EngineState::Idle => PlayState::Idle,
                    EngineState::Playing => PlayState::Playing,
                    EngineState::Paused => PlayState::Paused,
                };
                self.send(ServerEvent::PlayerState(self.player_state()));
                self.save_resume().await?;
            }
            EngineEvent::Outputs(list) => {
                self.output = OutputStatus::from_engine(&list);
                self.send(ServerEvent::OutputStatus(self.output.clone()));
            }
        }
        Ok(())
    }

    async fn track_started(&mut self, id: i64) -> anyhow::Result<()> {
        // Anything ahead of it was skipped before it became audible.
        while self.in_flight.front().is_some_and(|p| p.track.id != id) {
            self.in_flight.pop_front();
        }
        let Some(pending) = self.in_flight.pop_front() else {
            tracing::warn!(track = id, "unexpected track started");
            return Ok(());
        };
        if let Some(request) = pending.request_id {
            self.store.remove_request(request).await?;
            self.broadcast_queue().await?;
        }
        if pending.source == PlaySource::Pool {
            self.cursor = Some(id);
            self.store
                .set_setting(PLAYLIST_CURSOR, &id.to_string())
                .await?;
        }
        // A song carried on after a restart keeps its play.
        let (play_event, offset_ms) = match pending.resume {
            Some(resumed) => resumed,
            None => (
                self.store
                    .start_play_event(
                        id,
                        pending.source,
                        pending.requested_by.as_ref().map(|(id, _)| *id),
                    )
                    .await?,
                0,
            ),
        };
        // During a crossfade the previous song is still fading out.
        if let Some(previous) = self.playing.take() {
            self.ending.push(previous);
        }
        self.playing = Some(Playing {
            pending,
            play_event,
            position_ms: offset_ms,
            offset_ms,
        });
        self.last_position_second = None;
        self.saved_second = None;
        self.save_resume().await?;
        self.broadcast_now_playing();
        self.songs_started += 1;
        let songs = self.songs_started;
        // Only last song's voters still sit out (this one).
        let resting = self.skip_voted_on.len();
        self.skip_voted_on.retain(|_, song| *song + 1 == songs);
        if !self.skip_votes.is_empty() || resting > 0 {
            self.skip_votes.clear();
            self.send(ServerEvent::PlayerState(self.player_state()));
        }
        Ok(())
    }

    async fn track_ended(
        &mut self,
        id: i64,
        played_ms: u64,
        reason: EndReason,
    ) -> anyhow::Result<()> {
        let skipped = reason == EndReason::Skipped;
        let skipped_by = if skipped {
            self.skip_requested_by.take()
        } else {
            None
        };
        let finished = match self.playing.take_if(|p| p.pending.track.id == id) {
            Some(playing) => Some(playing),
            None => self
                .ending
                .iter()
                .position(|p| p.pending.track.id == id)
                .map(|i| self.ending.remove(i)),
        };
        if let Some(playing) = finished {
            self.store
                .finish_play_event(
                    playing.play_event,
                    i64::try_from(playing.offset_ms + played_ms).unwrap_or(i64::MAX),
                    skipped,
                    skipped_by,
                )
                .await?;
            self.save_resume().await?;
            self.broadcast_now_playing();
        } else if let Some(i) = self.in_flight.iter().position(|p| p.track.id == id) {
            // Skipped before it became audible.
            if let Some(pending) = self.in_flight.remove(i) {
                if let Some(request) = pending.request_id {
                    self.store.remove_request(request).await?;
                    self.broadcast_queue().await?;
                }
            }
            self.broadcast_now_playing();
        }
        Ok(())
    }

    async fn queue_changed(&mut self) -> anyhow::Result<()> {
        self.refresh_next().await?;
        self.broadcast_queue().await?;
        self.broadcast_now_playing();
        Ok(())
    }

    /// Makes sure the engine's next slot holds the right track: the first
    /// request not already on its way, otherwise the active playlist's next
    /// song.
    async fn refresh_next(&mut self) -> anyhow::Result<()> {
        // The song a restart cut off goes first, whatever else is waiting.
        if self.queued.as_ref().is_some_and(|q| q.resume.is_some()) {
            return Ok(());
        }
        if self.queued.is_none() {
            if let Some(resumed) = self.resuming.take() {
                if self.send_next(resumed).await? {
                    return Ok(());
                }
            }
        }
        for _ in 0..5 {
            let in_flight: Vec<i64> = self.in_flight.iter().filter_map(|p| p.request_id).collect();
            let head = self
                .store
                .queue()
                .await?
                .into_iter()
                .find(|q| !in_flight.contains(&q.id));

            let candidate = match (head, &self.queued) {
                (Some(head), Some(q)) if q.request_id == Some(head.id) => return Ok(()),
                (Some(head), _) => Pending {
                    requested_by: Some((head.user_id, head.username.clone())),
                    backers: head.backers.clone(),
                    request_id: Some(head.id),
                    source: PlaySource::Request,
                    track: head.track,
                    resume: None,
                },
                (None, Some(q)) if q.source == PlaySource::Pool => {
                    // Radio's pool shifts as it goes; what it drew stays.
                    if self.weighted() && self.start_at.is_none() {
                        return Ok(());
                    }
                    let pool = self.auto_mix().await?;
                    if pool.iter().any(|(id, _)| *id == q.track.id) && self.start_at.is_none() {
                        return Ok(());
                    }
                    match self.draw(&pool).await? {
                        Some(track) => track,
                        None => return self.clear_next(),
                    }
                }
                (None, _) => {
                    let pool = self.auto_mix().await?;
                    match self.draw(&pool).await? {
                        Some(track) => track,
                        None => return self.clear_next(),
                    }
                }
            };

            if self.send_next(candidate).await? {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Puts `candidate` in the engine's next slot. Returns false if its file
    /// has gone (a request for it is dropped).
    async fn send_next(&mut self, candidate: Pending) -> anyhow::Result<bool> {
        let Some(path) = self.store.track_path(candidate.track.id).await? else {
            // The file vanished since the last scan.
            if let Some(request) = candidate.request_id {
                self.store.remove_request(request).await?;
            }
            return Ok(false);
        };
        if let Some(old) = self.queued.take() {
            if old.source == PlaySource::Pool && candidate.source == PlaySource::Request {
                self.bag.put_back(old.track.id);
            }
            self.replaced.push_back(old);
            if self.replaced.len() > MAX_REPLACED {
                self.replaced.pop_front();
            }
        }
        // The song the engine plays before this one.
        let previous = self
            .in_flight
            .back()
            .or(self.playing.as_ref().map(|p| &p.pending));
        let gapless = previous.is_some_and(|p| follows_on_album(&p.track, &candidate.track));
        let gain = match self.replay_gain.mode {
            ReplayGainMode::Off => 1.0,
            _ => self
                .store
                .gain_info(candidate.track.id)
                .await?
                .map_or(1.0, |info| self.replay_gain.linear(&info)),
        };
        (self.engine)(Command::SetNext(Some(djinn_engine::Track {
            id: TrackId(candidate.track.id),
            path: PathBuf::from(path),
            gapless: gapless && candidate.resume.is_none(),
            gain,
            start_ms: candidate.resume.map_or(0, |(_, ms)| ms),
        })));
        self.queued = Some(candidate);
        Ok(true)
    }

    fn clear_next(&mut self) -> anyhow::Result<()> {
        if let Some(old) = self.queued.take() {
            self.replaced.push_back(old);
            (self.engine)(Command::SetNext(None));
        }
        Ok(())
    }

    async fn broadcast_queue(&self) -> anyhow::Result<()> {
        self.send(ServerEvent::QueueChanged(self.store.queue().await?));
        Ok(())
    }

    fn broadcast_now_playing(&self) {
        self.send(ServerEvent::NowPlaying(Box::new(self.now_playing())));
    }

    fn send(&self, event: ServerEvent) {
        // No subscribers is fine.
        let _ = self.events.send(event);
    }
}

/// Whether `next` is the following song on `previous`'s album (including
/// the first song of the next disc), so it should join without a
/// crossfade, as live albums and mixes are meant to.
fn follows_on_album(previous: &Track, next: &Track) -> bool {
    if previous.album_id.is_none() || previous.album_id != next.album_id {
        return false;
    }
    let (Some(prev_no), Some(next_no)) = (previous.track_no, next.track_no) else {
        return false;
    };
    let (prev_disc, next_disc) = (previous.disc_no.unwrap_or(1), next.disc_no.unwrap_or(1));
    (next_disc == prev_disc && next_no == prev_no + 1)
        || (next_disc == prev_disc + 1 && next_no == 1)
}

/// Perceptual volume curve: 0 to 100 maps to gain 0.0 to 1.0, squared.
fn gain(volume: u8) -> f32 {
    let v = f32::from(volume.min(100)) / 100.0;
    v * v
}

#[cfg(test)]
mod tests;
