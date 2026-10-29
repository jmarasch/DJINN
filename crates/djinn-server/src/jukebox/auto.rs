//! The auto mix: what plays when nobody has requested anything. The
//! active playlist (shuffled through the bag or in order), wild play
//! (the whole library) and radio, the last two weighted by what parties
//! did with each song.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use djinn_engine::Command;
use djinn_store::PlaySource;

use super::bag::ShuffleBag;
use super::mix::{self, Learned, Mixer, Radio, RadioSeed, SongLinks};
use super::{
    Jukebox, Pending, ACTIVE_PLAYLIST, DEFAULT_HISTORY_WINDOW, HISTORY_WINDOW, PLAYLIST_CURSOR,
    RADIO, WILD_PLAY,
};
use crate::events::{PlaylistRef, ServerEvent};
use crate::settings;

/// Setting: the radio playing, as JSON ([`Radio`]).
const RADIO_SETTING: &str = "radio";
/// Radio draws from this many songs at a time.
const RADIO_POOL: usize = 150;
/// Only this much of the radio's history steers it.
const RADIO_MEMORY_MS: i64 = 2 * 60 * 60 * 1000;
/// How long party weights are reused before being read again.
const PARTY_REFRESH: Duration = Duration::from_secs(120);
/// Setting: 1 to weigh wild play and radio by what parties did.
pub const SMART_PLAY: &str = "smart_play";
/// Setting: 1 to also order shuffled playlists by it.
pub const SMART_PLAYLISTS: &str = "smart_playlists";
/// Setting: how much it counts, 0 to 100 (percent).
pub const SMART_STRENGTH: &str = "smart_strength";

impl Jukebox {
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

    pub fn set_mixer(&mut self, mixer: Arc<dyn Mixer>) {
        self.mixer = Some(mixer);
    }

    /// Starts a radio from `seed` (named `name`), like a playlist: a song
    /// seed plays first, then songs like it.
    pub async fn start_radio(&mut self, seed: RadioSeed, name: String) -> anyhow::Result<()> {
        let first = match &seed {
            RadioSeed::Track { id } => Some(*id),
            _ => None,
        };
        let radio = Radio {
            seed,
            name,
            started_at: djinn_store::now_ms(),
        };
        self.store
            .set_setting(RADIO_SETTING, &serde_json::to_string(&radio)?)
            .await?;
        self.play_playlist(RADIO, first).await
    }

    /// No playlist plays when the requests run out.
    pub async fn stop_auto_mix(&mut self) -> anyhow::Result<()> {
        self.store.set_setting(ACTIVE_PLAYLIST, "0").await?;
        self.auto_mix_changed().await
    }

    pub(super) async fn load_active(&mut self) -> anyhow::Result<()> {
        let id = settings::number(&self.store, ACTIVE_PLAYLIST, 0).await?;
        self.radio = None;
        self.active = match id {
            0 => None,
            RADIO => {
                let radio: Option<Radio> = self
                    .store
                    .get_setting(RADIO_SETTING)
                    .await?
                    .and_then(|json| serde_json::from_str(&json).ok());
                self.radio = radio;
                self.radio.as_ref().map(|r| PlaylistRef {
                    id: RADIO,
                    name: format!("Radio: {}", r.name),
                    shuffle: true,
                })
            }
            WILD_PLAY => Some(PlaylistRef {
                id: WILD_PLAY,
                name: "Wild play".to_owned(),
                shuffle: true,
            }),
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

    /// Songs the active playlist covers, in order, with weights (1 for a
    /// playlist's songs; radio's from the mixer).
    pub(super) async fn auto_mix(&self) -> anyhow::Result<Vec<(i64, f32)>> {
        let even = |ids: Vec<i64>| ids.into_iter().map(|id| (id, 1.0)).collect();
        Ok(match &self.active {
            Some(playlist) if playlist.id == WILD_PLAY => {
                even(self.store.library_track_ids().await?)
            }
            Some(playlist) if playlist.id == RADIO => self.radio_pool().await?,
            Some(playlist) => even(self.store.playlist_track_ids(playlist.id).await?),
            None => Vec::new(),
        })
    }

    async fn radio_pool(&self) -> anyhow::Result<Vec<(i64, f32)>> {
        let (Some(radio), Some(mixer)) = (&self.radio, &self.mixer) else {
            return Ok(Vec::new());
        };
        let since = radio
            .started_at
            .max(djinn_store::now_ms() - RADIO_MEMORY_MS);
        let plays = self.store.plays_since(since).await?;
        let played: Vec<i64> = plays.iter().filter(|p| !p.1).map(|p| p.0).collect();
        let skipped: Vec<i64> = plays.iter().filter(|p| p.1).map(|p| p.0).collect();
        let mut pool = mixer.radio(&radio.seed, &played, &skipped);
        pool.truncate(RADIO_POOL);
        tracing::debug!(
            radio = %radio.name,
            played = played.len(),
            skipped = skipped.len(),
            top = ?&pool[..pool.len().min(8)],
            "radio pool"
        );
        Ok(pool)
    }

    /// True if the active playlist draws by weight (wild play and radio).
    pub(super) fn weighted(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|p| p.id == WILD_PLAY || p.id == RADIO)
    }

    /// Each song's weight after what parties of the style in use did with
    /// it (and with its artists and genres), read again every couple of
    /// minutes. Empty with smart play off.
    pub(super) async fn party_weights(&mut self) -> anyhow::Result<HashMap<i64, f32>> {
        if let Some((at, weights)) = &self.party {
            if at.elapsed() < PARTY_REFRESH {
                return Ok(weights.clone());
            }
        }
        let style = self.store.active_party_style().await?;
        let weights = match style {
            Some(style) if settings::flag(&self.store, SMART_PLAY, true).await? => {
                let strength = settings::number(&self.store, SMART_STRENGTH, 100)
                    .await?
                    .clamp(0, 100) as f32
                    / 100.0;
                let history = self.store.party_history(style).await?;
                let links = self.links();
                Learned::new(&history, &links).weights(&links, strength)
            }
            _ => HashMap::new(),
        };
        self.party = Some((Instant::now(), weights.clone()));
        Ok(weights)
    }

    /// Each song's artists and genres, from the mixer (none without one).
    pub fn links(&self) -> Arc<SongLinks> {
        self.mixer
            .as_ref()
            .map_or_else(Arc::default, |mixer| mixer.links())
    }

    /// Call after the style in use or the smart play settings change.
    pub fn party_changed(&mut self) {
        self.party = None;
    }

    /// The active playlist's next song: shuffled through the bag, or the
    /// one after the last it played.
    pub(super) async fn draw(&mut self, pool: &[(i64, f32)]) -> anyhow::Result<Option<Pending>> {
        let ids: Vec<i64> = pool.iter().map(|(id, _)| *id).collect();
        // A radio's first song needn't be in its pool.
        let radio = self.active.as_ref().is_some_and(|p| p.id == RADIO);
        let id = match self.start_at.take().filter(|id| radio || ids.contains(id)) {
            Some(id) => Some(id),
            None if self.weighted() => self.weighted_draw(pool).await?,
            None if self.active.as_ref().is_some_and(|p| !p.shuffle) => {
                next_in_order(&ids, self.order_anchor())
            }
            None => self.shuffle_draw(&ids).await?,
        };
        let Some(id) = id else { return Ok(None) };
        Ok(self.store.track(id).await?.map(|track| Pending {
            track,
            source: PlaySource::Pool,
            request_id: None,
            requested_by: None,
            backers: Vec::new(),
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
        let (recent, window) = self.recent().await?;
        let weights = if settings::flag(&self.store, SMART_PLAYLISTS, false).await? {
            self.party_weights().await?
        } else {
            HashMap::new()
        };
        self.bag.set_weights(weights);
        Ok(self.bag.draw(pool, &recent, window, &mut self.rng))
    }

    /// Wild play and radio: by weight, after the party's history, never a
    /// song that played recently.
    async fn weighted_draw(&mut self, pool: &[(i64, f32)]) -> anyhow::Result<Option<i64>> {
        let (recent, window) = self.recent().await?;
        let recent: HashSet<i64> = recent.into_iter().take(window).collect();
        let party = self.party_weights().await?;
        Ok(mix::weighted_draw(pool, &recent, &party, &mut self.rng))
    }

    /// Songs played recently, newest first, and how many of them count:
    /// those on their way, on top of the configured window of history.
    async fn recent(&self) -> anyhow::Result<(Vec<i64>, usize)> {
        let window = settings::number(&self.store, HISTORY_WINDOW, DEFAULT_HISTORY_WINDOW)
            .await?
            .max(0);
        let mut recent: Vec<i64> = self
            .in_flight
            .iter()
            .rev()
            .map(|p| p.track.id)
            .chain(self.playing.iter().map(|p| p.pending.track.id))
            .collect();
        let upcoming = recent.len();
        recent.extend(self.store.recent_track_ids(window).await?);
        Ok((recent, usize::try_from(window).unwrap_or(0) + upcoming))
    }
}

/// The song after `anchor` in `ids`, wrapping around; the first song if
/// there's no anchor (or it left the playlist).
pub(super) fn next_in_order(ids: &[i64], anchor: Option<i64>) -> Option<i64> {
    let first = *ids.first()?;
    match anchor.and_then(|a| ids.iter().position(|&id| id == a)) {
        Some(i) => ids.get(i + 1).copied().or(Some(first)),
        None => Some(first),
    }
}
