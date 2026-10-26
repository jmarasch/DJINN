//! Decides what plays: requests first, then the active playlist. Feeds the engine
//! and turns its events into history and live updates.
//!
//! Tracks move through three stages: `queued` (sent to the engine as the
//! next track), `in_flight` (the engine is decoding it, but it isn't audible
//! yet), and `playing`. A request stays in the request queue until it
//! becomes audible, so "up next" is always visible.

mod bag;

use std::collections::VecDeque;
use std::path::PathBuf;

use djinn_engine::{Command, EndReason, Event as EngineEvent, PlayState as EngineState, TrackId};
use djinn_store::{
    AddRequestError, PlaySource, QueueItem, RequestLimits, RequestOrigin, Store, Track, User,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use tokio::sync::broadcast;

use self::bag::ShuffleBag;
use crate::events::{
    Entry, NowPlaying, OutputStatus, PlayState, PlayerState, PlaylistRef, ServerEvent,
};

pub const HISTORY_WINDOW: &str = "history_window";
pub const PER_USER_LIMIT: &str = "per_user_limit";
pub const VOLUME: &str = "volume";
/// Most requests the whole queue may hold; 0 means no cap.
pub const MAX_QUEUE: &str = "max_queue";
/// 1 when guests may join with just a name.
pub const GUEST_MODE: &str = "guest_mode";
/// 1 to look up artist photos and bios online.
pub const ONLINE_INFO: &str = "online_artist_info";
/// Crossfade between songs, in seconds (0 joins them gaplessly).
pub const CROSSFADE: &str = "crossfade_secs";
/// Contact (email or website) sent to MusicBrainz; empty turns it off.
pub const MUSICBRAINZ_CONTACT: &str = "musicbrainz_contact";
/// Playlist that plays when nobody has requested anything; 0 or missing
/// means none.
pub const ACTIVE_PLAYLIST: &str = "active_playlist";
/// Last song the active playlist played, so in-order playback resumes.
const PLAYLIST_CURSOR: &str = "playlist_cursor";
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
}

impl Pending {
    fn entry(&self) -> Entry {
        Entry {
            track: self.track.clone(),
            source: self.source,
            requested_by: self.requested_by.as_ref().map(|(_, name)| name.clone()),
        }
    }
}

struct Playing {
    pending: Pending,
    play_event: i64,
    position_ms: u64,
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
    output: OutputStatus,
    last_position_second: Option<u64>,
    /// The playlist feeding the music, if any.
    active: Option<PlaylistRef>,
    /// Last song the active playlist played (for in-order playback).
    cursor: Option<i64>,
    /// Song to play next from the playlist ("play from here").
    start_at: Option<i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error(transparent)]
    Add(#[from] AddRequestError),
    #[error("not found")]
    NotFound,
    #[error("you can only remove your own requests")]
    NotYours,
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
        let volume = setting(&store, VOLUME, DEFAULT_VOLUME).await?.clamp(0, 100) as u8;
        let cursor = setting(&store, PLAYLIST_CURSOR, 0).await?;
        let crossfade = setting(&store, CROSSFADE, 0)
            .await?
            .clamp(0, i64::from(djinn_engine::MAX_CROSSFADE_SECS)) as u8;
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
            output: OutputStatus::default(),
            last_position_second: None,
            active: None,
            cursor: (cursor > 0).then_some(cursor),
            start_at: None,
        };
        (jukebox.engine)(Command::SetVolume(gain(volume)));
        (jukebox.engine)(Command::SetCrossfade(u32::from(crossfade)));
        jukebox.load_active().await?;
        Ok(jukebox)
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
        }
    }

    pub fn active_playlist(&self) -> Option<i64> {
        self.active.as_ref().map(|p| p.id)
    }

    pub fn output_status(&self) -> OutputStatus {
        self.output.clone()
    }

    /// Queues a request. Admins and users marked unlimited skip the
    /// per-person limit and the queue cap.
    pub async fn add_request(
        &mut self,
        user: &User,
        track_id: i64,
        origin: &RequestOrigin,
    ) -> Result<QueueItem, QueueError> {
        let limits = if user.is_admin() || user.unlimited {
            RequestLimits::default()
        } else {
            let cap = setting(&self.store, MAX_QUEUE, 0).await?;
            RequestLimits {
                per_user: Some(setting(&self.store, PER_USER_LIMIT, DEFAULT_PER_USER_LIMIT).await?),
                queue_cap: (cap > 0).then_some(cap),
            }
        };
        let item = self
            .store
            .add_request(track_id, user.id, limits, origin)
            .await?;
        self.queue_changed().await?;
        Ok(item)
    }

    /// Users may remove their own requests; admins may remove any.
    pub async fn remove_request(&mut self, user: &User, id: i64) -> Result<(), QueueError> {
        let item = self
            .store
            .queue_item(id)
            .await?
            .ok_or(QueueError::NotFound)?;
        if item.user_id != user.id && !user.is_admin() {
            return Err(QueueError::NotYours);
        }
        self.store.remove_request(id).await?;
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

    /// Call after the active playlist's contents (or the library) change.
    pub async fn auto_mix_changed(&mut self) -> anyhow::Result<()> {
        self.load_active().await?;
        self.refresh_next().await?;
        self.broadcast_now_playing();
        self.send(ServerEvent::PlayerState(self.player_state()));
        Ok(())
    }

    /// Makes `id` the active playlist and starts it now, optionally from a
    /// given song. Requests still play first, and a request that is playing
    /// is not cut off.
    pub async fn play_playlist(&mut self, id: i64, from_track: Option<i64>) -> anyhow::Result<()> {
        self.store
            .set_setting(ACTIVE_PLAYLIST, &id.to_string())
            .await?;
        self.store.set_setting(PLAYLIST_CURSOR, "0").await?;
        self.bag = ShuffleBag::default();
        self.cursor = None;
        self.start_at = from_track;
        // A pool pick already lined up belongs to the old playlist.
        if let Some(old) = self.queued.take_if(|q| q.source == PlaySource::Pool) {
            self.replaced.push_back(old);
        }
        self.load_active().await?;
        self.refresh_next().await?;
        let request_playing = self
            .playing
            .as_ref()
            .is_some_and(|p| p.pending.source == PlaySource::Request);
        if self.playing.is_some() && !request_playing {
            self.skip_requested_by = None;
            (self.engine)(Command::Skip);
        }
        (self.engine)(Command::Play);
        self.broadcast_now_playing();
        self.send(ServerEvent::PlayerState(self.player_state()));
        Ok(())
    }

    /// No playlist plays when the requests run out.
    pub async fn stop_auto_mix(&mut self) -> anyhow::Result<()> {
        self.store.set_setting(ACTIVE_PLAYLIST, "0").await?;
        self.auto_mix_changed().await
    }

    async fn load_active(&mut self) -> anyhow::Result<()> {
        let id = setting(&self.store, ACTIVE_PLAYLIST, 0).await?;
        self.active = match id {
            0 => None,
            id => self
                .store
                .playlists()
                .await?
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| PlaylistRef {
                    id: p.id,
                    name: p.name,
                    shuffle: p.shuffle,
                }),
        };
        Ok(())
    }

    /// Songs the active playlist covers, in order.
    async fn auto_mix_ids(&self) -> anyhow::Result<Vec<i64>> {
        Ok(match &self.active {
            Some(playlist) => self.store.playlist_track_ids(playlist.id).await?,
            None => Vec::new(),
        })
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
                    playing.position_ms = ms;
                    let second = ms / 1000;
                    if self.last_position_second != Some(second) {
                        self.last_position_second = Some(second);
                        self.send(ServerEvent::Position {
                            track_id: id,
                            position_ms: ms,
                        });
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
            }
            EngineEvent::OutputOpened {
                device,
                sample_rate,
                ..
            } => {
                self.output = OutputStatus {
                    device: Some(device),
                    sample_rate: Some(sample_rate),
                    error: None,
                };
                self.send(ServerEvent::OutputStatus(self.output.clone()));
            }
            EngineEvent::OutputError { error } => {
                self.output = OutputStatus {
                    device: None,
                    sample_rate: None,
                    error: Some(error),
                };
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
        let play_event = self
            .store
            .start_play_event(
                id,
                pending.source,
                pending.requested_by.as_ref().map(|(id, _)| *id),
            )
            .await?;
        // During a crossfade the previous song is still fading out.
        if let Some(previous) = self.playing.take() {
            self.ending.push(previous);
        }
        self.playing = Some(Playing {
            pending,
            play_event,
            position_ms: 0,
        });
        self.last_position_second = None;
        self.broadcast_now_playing();
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
                    i64::try_from(played_ms).unwrap_or(i64::MAX),
                    skipped,
                    skipped_by,
                )
                .await?;
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
                    request_id: Some(head.id),
                    source: PlaySource::Request,
                    track: head.track,
                },
                (None, Some(q)) if q.source == PlaySource::Pool => {
                    let pool = self.auto_mix_ids().await?;
                    if pool.contains(&q.track.id) && self.start_at.is_none() {
                        return Ok(());
                    }
                    match self.draw(&pool).await? {
                        Some(track) => track,
                        None => return self.clear_next(),
                    }
                }
                (None, _) => {
                    let pool = self.auto_mix_ids().await?;
                    match self.draw(&pool).await? {
                        Some(track) => track,
                        None => return self.clear_next(),
                    }
                }
            };

            let Some(path) = self.store.track_path(candidate.track.id).await? else {
                // The file vanished since the last scan.
                if let Some(request) = candidate.request_id {
                    self.store.remove_request(request).await?;
                }
                continue;
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
            (self.engine)(Command::SetNext(Some(djinn_engine::Track {
                id: TrackId(candidate.track.id),
                path: PathBuf::from(path),
                gapless,
            })));
            self.queued = Some(candidate);
            return Ok(());
        }
        Ok(())
    }

    /// The active playlist's next song: shuffled through the bag, or the
    /// one after the last it played.
    async fn draw(&mut self, pool: &[i64]) -> anyhow::Result<Option<Pending>> {
        let id = match self.start_at.take().filter(|id| pool.contains(id)) {
            Some(id) => Some(id),
            None if self.active.as_ref().is_some_and(|p| !p.shuffle) => {
                next_in_order(pool, self.order_anchor())
            }
            None => self.shuffle_draw(pool).await?,
        };
        let Some(id) = id else { return Ok(None) };
        Ok(self.store.track(id).await?.map(|track| Pending {
            track,
            source: PlaySource::Pool,
            request_id: None,
            requested_by: None,
        }))
    }

    /// The latest playlist song on its way or playing, else the last one played.
    fn order_anchor(&self) -> Option<i64> {
        self.in_flight
            .iter()
            .rev()
            .find(|p| p.source == PlaySource::Pool)
            .or(self
                .playing
                .as_ref()
                .map(|p| &p.pending)
                .filter(|p| p.source == PlaySource::Pool))
            .map(|p| p.track.id)
            .or(self.cursor)
    }

    async fn shuffle_draw(&mut self, pool: &[i64]) -> anyhow::Result<Option<i64>> {
        let window = setting(&self.store, HISTORY_WINDOW, DEFAULT_HISTORY_WINDOW)
            .await?
            .max(0);
        // Tracks on their way count as recent too, newest first, on top of
        // the configured window of history.
        let mut recent: Vec<i64> = self
            .in_flight
            .iter()
            .rev()
            .map(|p| p.track.id)
            .chain(self.playing.iter().map(|p| p.pending.track.id))
            .collect();
        let upcoming = recent.len();
        recent.extend(self.store.recent_track_ids(window).await?);
        let window = usize::try_from(window).unwrap_or(0) + upcoming;
        Ok(self.bag.draw(pool, &recent, window, &mut self.rng))
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

/// The song after `anchor` in `ids`, wrapping around; the first song if
/// there's no anchor (or it left the playlist).
fn next_in_order(ids: &[i64], anchor: Option<i64>) -> Option<i64> {
    let first = *ids.first()?;
    match anchor.and_then(|a| ids.iter().position(|&id| id == a)) {
        Some(i) => ids.get(i + 1).copied().or(Some(first)),
        None => Some(first),
    }
}

/// Perceptual volume curve: 0 to 100 maps to gain 0.0 to 1.0, squared.
fn gain(volume: u8) -> f32 {
    let v = f32::from(volume.min(100)) / 100.0;
    v * v
}

pub async fn setting(store: &Store, key: &str, default: i64) -> anyhow::Result<i64> {
    Ok(store
        .get_setting(key)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use djinn_store::{EntryKind, Role, ScannedTrack};
    use std::sync::{Arc, Mutex};

    fn origin() -> RequestOrigin {
        RequestOrigin::default()
    }

    struct Harness {
        jukebox: Jukebox,
        sent: Arc<Mutex<Vec<Command>>>,
        store: Store,
        tracks: Vec<i64>,
        admin: User,
        user: User,
    }

    impl Harness {
        async fn new(pool_size: usize) -> Self {
            let store = Store::open_in_memory().await.unwrap();
            let tracks: Vec<ScannedTrack> = (0..pool_size + 3)
                .map(|i| ScannedTrack {
                    path: format!("/m/{i}.flac"),
                    title: format!("Song {i}"),
                    ..Default::default()
                })
                .collect();
            store.save_scanned(&tracks).await.unwrap();
            let tracks: Vec<i64> = store
                .known_files()
                .await
                .unwrap()
                .iter()
                .map(|k| k.id)
                .collect();
            let admin = store.create_user("admin", "h", Role::Admin).await.unwrap();
            if pool_size > 0 {
                let list = store.create_playlist(admin.id, "Mix").await.unwrap();
                for &id in &tracks[..pool_size] {
                    store
                        .add_playlist_entry(list.id, EntryKind::Track, id)
                        .await
                        .unwrap();
                }
                store
                    .set_setting(ACTIVE_PLAYLIST, &list.id.to_string())
                    .await
                    .unwrap();
            }
            let user = store.create_user("guest", "h", Role::User).await.unwrap();
            let sent = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&sent);
            let (events, _) = broadcast::channel(64);
            let jukebox = Jukebox::new(
                store.clone(),
                Box::new(move |c| sink.lock().unwrap().push(c)),
                events,
            )
            .await
            .unwrap();
            Self {
                jukebox,
                sent,
                store,
                tracks,
                admin,
                user,
            }
        }

        async fn event(&mut self, event: EngineEvent) {
            self.jukebox.on_engine_event(event).await.unwrap();
        }

        /// The last track sent to the engine's next slot.
        fn last_next(&self) -> Option<i64> {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find_map(|c| match c {
                    Command::SetNext(t) => Some(t.as_ref().map(|t| t.id.0)),
                    _ => None,
                })?
        }

        /// Simulates the engine taking the queued track and playing it.
        async fn play_next(&mut self) -> i64 {
            let id = self.last_next().unwrap();
            self.event(EngineEvent::NextNeeded {
                after: Some(TrackId(id)),
            })
            .await;
            self.event(EngineEvent::TrackStarted { id: TrackId(id) })
                .await;
            id
        }

        async fn finish(&mut self, id: i64) {
            self.event(EngineEvent::TrackEnded {
                id: TrackId(id),
                played_ms: 1000,
                reason: EndReason::Finished,
            })
            .await;
        }
    }

    #[tokio::test]
    async fn plays_from_pool_when_queue_empty() {
        let mut h = Harness::new(5).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let first = h.play_next().await;
        assert!(h.tracks[..5].contains(&first));
        assert_eq!(h.jukebox.now_playing().current.unwrap().track.id, first);
        assert_eq!(h.store.recent_track_ids(5).await.unwrap(), [first]);

        // The engine already has the next pool track lined up.
        let second = h.last_next().unwrap();
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn request_preempts_queued_pool_track_and_leaves_queue_when_audible() {
        let mut h = Harness::new(5).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let pool_track = h.last_next().unwrap();

        let requested = h.tracks[6];
        let user = h.user.clone();
        h.jukebox
            .add_request(&user, requested, &origin())
            .await
            .unwrap();
        assert_eq!(h.last_next(), Some(requested));
        assert_eq!(h.jukebox.now_playing().up_next.unwrap().track.id, requested);

        let started = h.play_next().await;
        assert_eq!(started, requested);
        assert!(h.store.queue().await.unwrap().is_empty());
        let now = h.jukebox.now_playing().current.unwrap();
        assert_eq!(now.requested_by.as_deref(), Some("guest"));

        // The displaced pool track comes right back.
        assert_eq!(h.last_next(), Some(pool_track));
    }

    #[tokio::test]
    async fn request_replacing_a_track_the_engine_already_took() {
        let mut h = Harness::new(5).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let pool_track = h.last_next().unwrap();

        // The engine takes the pool track, but before its NextNeeded arrives
        // a request replaces it in the slot.
        let requested = h.tracks[6];
        let user = h.user.clone();
        h.jukebox
            .add_request(&user, requested, &origin())
            .await
            .unwrap();
        h.event(EngineEvent::NextNeeded {
            after: Some(TrackId(pool_track)),
        })
        .await;
        // The request is still the engine's next track; nothing new was sent.
        assert_eq!(h.last_next(), Some(requested));

        h.event(EngineEvent::TrackStarted {
            id: TrackId(pool_track),
        })
        .await;
        assert_eq!(
            h.jukebox.now_playing().current.unwrap().track.id,
            pool_track
        );
        assert_eq!(
            h.store.queue().await.unwrap().len(),
            1,
            "request still waiting"
        );
        h.finish(pool_track).await;
        assert_eq!(h.play_next().await, requested);
        assert!(h.store.queue().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn per_user_limit_spares_admins() {
        let mut h = Harness::new(1).await;
        h.store.set_setting(PER_USER_LIMIT, "1").await.unwrap();
        let (user, admin) = (h.user.clone(), h.admin.clone());
        h.jukebox
            .add_request(&user, h.tracks[1], &origin())
            .await
            .unwrap();
        assert!(matches!(
            h.jukebox.add_request(&user, h.tracks[2], &origin()).await,
            Err(QueueError::Add(AddRequestError::LimitReached(1)))
        ));
        h.jukebox
            .add_request(&admin, h.tracks[2], &origin())
            .await
            .unwrap();
        h.jukebox
            .add_request(&admin, h.tracks[3], &origin())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn queue_cap_spares_unlimited_users() {
        let mut h = Harness::new(1).await;
        h.store.set_setting(MAX_QUEUE, "1").await.unwrap();
        let user = h.user.clone();
        h.jukebox
            .add_request(&user, h.tracks[1], &origin())
            .await
            .unwrap();
        let other = h.store.create_user("other", "h", Role::User).await.unwrap();
        assert!(matches!(
            h.jukebox.add_request(&other, h.tracks[2], &origin()).await,
            Err(QueueError::Add(AddRequestError::QueueFull(1)))
        ));
        h.store.set_unlimited(other.id, true).await.unwrap();
        let other = h.store.user(other.id).await.unwrap().unwrap();
        h.jukebox
            .add_request(&other, h.tracks[2], &origin())
            .await
            .unwrap();
        h.jukebox
            .add_request(&other, h.tracks[3], &origin())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn only_owner_or_admin_removes_requests() {
        let mut h = Harness::new(1).await;
        let (user, admin) = (h.user.clone(), h.admin.clone());
        let item = h
            .jukebox
            .add_request(&admin, h.tracks[1], &origin())
            .await
            .unwrap();
        assert!(matches!(
            h.jukebox.remove_request(&user, item.id).await,
            Err(QueueError::NotYours)
        ));
        let mine = h
            .jukebox
            .add_request(&user, h.tracks[2], &origin())
            .await
            .unwrap();
        h.jukebox.remove_request(&user, mine.id).await.unwrap();
        h.jukebox.remove_request(&admin, item.id).await.unwrap();
        assert!(h.store.queue().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn skip_is_recorded_with_who_skipped() {
        let mut h = Harness::new(3).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let id = h.play_next().await;
        let admin = h.admin.clone();
        h.jukebox.skip(&admin);
        h.event(EngineEvent::TrackEnded {
            id: TrackId(id),
            played_ms: 4200,
            reason: EndReason::Skipped,
        })
        .await;
        let row: (i64, i64, Option<i64>) = djinn_store::sqlx::query_as(
            "SELECT played_ms, skipped, skipped_by FROM play_events WHERE track_id = ?",
        )
        .bind(id)
        .fetch_one(h.store.pool())
        .await
        .unwrap();
        assert_eq!(row, (4200, 1, Some(admin.id)));
    }

    #[tokio::test]
    async fn broken_request_is_dropped() {
        let mut h = Harness::new(2).await;
        let user = h.user.clone();
        let requested = h.tracks[3];
        h.jukebox
            .add_request(&user, requested, &origin())
            .await
            .unwrap();
        h.event(EngineEvent::TrackError {
            id: TrackId(requested),
            error: "corrupt".into(),
        })
        .await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        assert!(h.store.queue().await.unwrap().is_empty());
        assert!(h.tracks[..2].contains(&h.last_next().unwrap()));
    }

    #[tokio::test]
    async fn empty_pool_and_queue_stays_idle() {
        let mut h = Harness::new(0).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        assert_eq!(h.last_next(), None);
        // A request starts playback.
        let user = h.user.clone();
        h.jukebox
            .add_request(&user, h.tracks[0], &origin())
            .await
            .unwrap();
        assert_eq!(h.last_next(), Some(h.tracks[0]));
    }

    #[tokio::test]
    async fn in_order_playlist_plays_in_order_and_loops() {
        let mut h = Harness::new(3).await;
        let list = h.jukebox.active_playlist().unwrap();
        h.store
            .update_playlist(list, None, Some(false))
            .await
            .unwrap();
        h.jukebox.auto_mix_changed().await.unwrap();
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let mut played = Vec::new();
        for _ in 0..4 {
            let id = h.play_next().await;
            played.push(id);
            h.finish(id).await;
        }
        assert_eq!(played, [h.tracks[0], h.tracks[1], h.tracks[2], h.tracks[0]]);
    }

    #[tokio::test]
    async fn play_playlist_from_a_song() {
        let mut h = Harness::new(4).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let first = h.play_next().await;
        let list = h.jukebox.active_playlist().unwrap();
        h.store
            .update_playlist(list, None, Some(false))
            .await
            .unwrap();

        h.jukebox
            .play_playlist(list, Some(h.tracks[2]))
            .await
            .unwrap();
        assert_eq!(h.last_next(), Some(h.tracks[2]), "chosen song is next");
        let skipped = h
            .sent
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, Command::Skip));
        assert!(skipped, "the playing playlist song makes way");

        h.finish(first).await;
        assert_eq!(h.play_next().await, h.tracks[2]);
        assert_eq!(h.last_next(), Some(h.tracks[3]), "then continues in order");

        h.jukebox.stop_auto_mix().await.unwrap();
        assert_eq!(h.last_next(), None, "no auto mix after stopping");
    }

    #[test]
    fn order_wraps_and_restarts() {
        assert_eq!(next_in_order(&[1, 2, 3], Some(1)), Some(2));
        assert_eq!(next_in_order(&[1, 2, 3], Some(3)), Some(1));
        assert_eq!(next_in_order(&[1, 2, 3], Some(9)), Some(1));
        assert_eq!(next_in_order(&[], None), None);
    }

    #[tokio::test]
    async fn crossfade_overlap_records_both_songs() {
        let mut h = Harness::new(3).await;
        h.event(EngineEvent::NextNeeded { after: None }).await;
        let first = h.play_next().await;
        // The next song starts while the first is still fading out.
        let second = h.play_next().await;
        assert_eq!(h.jukebox.now_playing().current.unwrap().track.id, second);
        h.event(EngineEvent::TrackEnded {
            id: TrackId(first),
            played_ms: 3000,
            reason: EndReason::Finished,
        })
        .await;
        assert_eq!(
            h.jukebox.now_playing().current.unwrap().track.id,
            second,
            "still playing"
        );
        let played: Option<i64> =
            djinn_store::sqlx::query_scalar("SELECT played_ms FROM play_events WHERE track_id = ?")
                .bind(first)
                .fetch_one(h.store.pool())
                .await
                .unwrap();
        assert_eq!(played, Some(3000), "the faded-out song's play was recorded");
    }

    #[test]
    fn next_song_on_the_album_joins_gaplessly() {
        let track = |album: Option<i64>, disc: Option<i64>, no: Option<i64>| Track {
            id: 0,
            title: String::new(),
            artist: None,
            album: None,
            album_artist: None,
            artist_id: None,
            album_id: album,
            track_no: no,
            disc_no: disc,
            year: None,
            genre: None,
            duration_ms: None,
            art_hash: None,
        };
        assert!(follows_on_album(
            &track(Some(1), None, Some(3)),
            &track(Some(1), None, Some(4))
        ));
        assert!(follows_on_album(
            &track(Some(1), Some(1), Some(12)),
            &track(Some(1), Some(2), Some(1))
        ));
        assert!(!follows_on_album(
            &track(Some(1), None, Some(3)),
            &track(Some(1), None, Some(5))
        ));
        assert!(!follows_on_album(
            &track(Some(1), None, Some(3)),
            &track(Some(2), None, Some(4))
        ));
        assert!(!follows_on_album(
            &track(None, None, Some(3)),
            &track(None, None, Some(4))
        ));
    }

    #[tokio::test]
    async fn crossfade_setting_reaches_the_engine() {
        let mut h = Harness::new(1).await;
        h.jukebox.set_crossfade(20).await.unwrap();
        assert_eq!(h.jukebox.player_state().crossfade_secs, 12, "capped");
        let sent = h.sent.lock().unwrap();
        assert!(sent.iter().any(|c| matches!(c, Command::SetCrossfade(12))));
    }
}
