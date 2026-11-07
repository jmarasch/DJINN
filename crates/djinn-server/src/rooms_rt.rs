//! Rooms at run time: a player for each room that plays its own music.
//!
//! Main (room 1) keeps the player DJINN always had (`AppState::jukebox`,
//! its engine, its node feed, its events on `AppState::events`). Every other
//! room that isn't linked gets its own engine, jukebox, node feed and event
//! channel here. A linked room has no player: its speakers join the room it
//! follows, its nodes take that room's feed, and its people's requests go to
//! that room's queue. [`sync`] makes this match the database after rooms,
//! links or speaker assignments change.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use djinn_engine::{Engine, EngineConfig};
use djinn_store::{Store, MAIN_ROOM};
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::events::ServerEvent;
use crate::jukebox::Jukebox;
use crate::node_audio::Hub;
use crate::state::AppState;

/// One room's player.
pub struct RoomPlayer {
    pub jukebox: Arc<Mutex<Jukebox>>,
    pub hub: Arc<Hub>,
    pub events: broadcast::Sender<ServerEvent>,
    /// Held so the engine plays while the room exists.
    _engine: Arc<Engine>,
}

/// The players for rooms other than Main, and who follows whom.
#[derive(Default)]
pub struct Rooms {
    players: RwLock<HashMap<i64, Arc<RoomPlayer>>>,
    /// Each linked room and the room it follows.
    links: RwLock<HashMap<i64, i64>>,
    /// Main's master rate, for new engines.
    rate: std::sync::atomic::AtomicU32,
    playback: RwLock<Option<Store>>,
}

impl Rooms {
    pub fn new(rate: u32, playback: Store) -> Rooms {
        Rooms {
            rate: std::sync::atomic::AtomicU32::new(rate),
            playback: RwLock::new(Some(playback)),
            ..Rooms::default()
        }
    }

    /// The room whose player `room` hears: itself, or the one it follows.
    pub fn leader(&self, room: i64) -> i64 {
        self.links
            .read()
            .ok()
            .and_then(|l| l.get(&room).copied())
            .unwrap_or(room)
    }

    fn player(&self, room: i64) -> Option<Arc<RoomPlayer>> {
        self.players.read().ok()?.get(&room).cloned()
    }
}

impl AppState {
    /// The jukebox that plays for `room` (its own, or its leader's).
    pub fn jukebox_for(&self, room: i64) -> Arc<Mutex<Jukebox>> {
        let leader = self.rooms.leader(room);
        match self.rooms.player(leader) {
            Some(player) if leader != MAIN_ROOM => Arc::clone(&player.jukebox),
            _ => Arc::clone(&self.jukebox),
        }
    }

    /// The node feed for `room`.
    pub fn hub_for(&self, room: i64) -> Arc<Hub> {
        let leader = self.rooms.leader(room);
        match self.rooms.player(leader) {
            Some(player) if leader != MAIN_ROOM => Arc::clone(&player.hub),
            _ => Arc::clone(&self.node_audio),
        }
    }

    /// Where `room`'s player announces songs, queue changes and playback;
    /// None for Main, whose events go out on `AppState::events` with
    /// everything else.
    pub fn room_events(&self, room: i64) -> Option<broadcast::Sender<ServerEvent>> {
        let leader = self.rooms.leader(room);
        (leader != MAIN_ROOM)
            .then(|| self.rooms.player(leader).map(|p| p.events.clone()))
            .flatten()
    }

    /// Every room's jukebox, Main's first (for house-wide changes).
    pub fn all_jukeboxes(&self) -> Vec<Arc<Mutex<Jukebox>>> {
        let mut all = vec![Arc::clone(&self.jukebox)];
        if let Ok(players) = self.rooms.players.read() {
            all.extend(players.values().map(|p| Arc::clone(&p.jukebox)));
        }
        all
    }

    /// True if music is playing in any room.
    pub async fn anything_playing(&self) -> bool {
        for jukebox in self.all_jukeboxes() {
            if jukebox.lock().await.player_state().state == crate::events::PlayState::Playing {
                return true;
            }
        }
        false
    }

    /// The room a person is in.
    pub async fn room_of(&self, user_id: i64) -> i64 {
        self.store.user_room(user_id).await.unwrap_or(MAIN_ROOM)
    }

    /// The jukebox for a person's room.
    pub async fn jukebox_of(&self, user_id: i64) -> Arc<Mutex<Jukebox>> {
        self.jukebox_for(self.room_of(user_id).await)
    }
}

/// Starts a player for `room`.
async fn start_player(state: &AppState, room: i64) -> anyhow::Result<RoomPlayer> {
    let rate = state.rooms.rate.load(std::sync::atomic::Ordering::Acquire);
    let rate = if rate == 0 { 48_000 } else { rate };
    let store = state
        .rooms
        .playback
        .read()
        .ok()
        .and_then(|p| p.clone())
        .unwrap_or_else(|| state.store.clone());
    let (engine_tx, mut engine_rx) = mpsc::unbounded_channel();
    let (timed, timed_reader) = djinn_engine::timed_tap(4.0, rate);
    let hub = Hub::start(timed_reader, rate)?;
    let engine = Arc::new(Engine::start_with_taps(
        EngineConfig {
            master_rate: rate,
            outputs: Vec::new(),
        },
        None,
        Some(timed),
        move |event| {
            let _ = engine_tx.send(event);
        },
    )?);
    let sink = Arc::clone(&engine);
    let (events, _) = broadcast::channel(256);
    let mut jukebox = Jukebox::for_room(
        store,
        room,
        Box::new(move |command| {
            if sink.send(command).is_err() {
                tracing::error!(room, "a room's audio engine stopped");
            }
        }),
        events.clone(),
    )
    .await?;
    jukebox.set_mixer(Arc::new(crate::smart::SmartMixer::new(state)));
    jukebox.set_copies(Arc::new(crate::duplicates::KnownCopies::new(state)));
    let jukebox = Arc::new(Mutex::new(jukebox));
    let pump = Arc::downgrade(&jukebox);
    tokio::spawn(async move {
        while let Some(event) = engine_rx.recv().await {
            let Some(jukebox) = pump.upgrade() else { break };
            let result = jukebox.lock().await.on_engine_event(event).await;
            if let Err(e) = result {
                tracing::error!(room, error = %format!("{e:#}"), "handling a room's engine event failed");
            }
        }
    });
    tracing::info!(room, "room player started");
    Ok(RoomPlayer {
        jukebox,
        hub: Arc::new(hub),
        events,
        _engine: engine,
    })
}

/// Makes the players, links and speaker assignments match the database.
pub async fn sync(state: &AppState) -> anyhow::Result<()> {
    let rooms = state.store.rooms().await?;
    let links: HashMap<i64, i64> = rooms
        .iter()
        .filter_map(|r| r.linked_to.map(|to| (r.id, to)))
        .collect();
    if let Ok(mut l) = state.rooms.links.write() {
        *l = links.clone();
    }
    // A player for each room playing its own music, besides Main.
    let wanted: Vec<i64> = rooms
        .iter()
        .filter(|r| r.id != MAIN_ROOM && r.linked_to.is_none())
        .map(|r| r.id)
        .collect();
    let stale: Vec<Arc<RoomPlayer>> = match state.rooms.players.write() {
        Ok(mut players) => {
            let gone: Vec<i64> = players
                .keys()
                .filter(|id| !wanted.contains(id))
                .copied()
                .collect();
            gone.into_iter()
                .filter_map(|id| players.remove(&id))
                .collect()
        }
        Err(_) => Vec::new(),
    };
    for player in stale {
        let _ = player.jukebox.lock().await.save_resume().await;
    }
    for room in &wanted {
        if state.rooms.player(*room).is_none() {
            let player = start_player(state, *room).await?;
            if let Ok(mut players) = state.rooms.players.write() {
                players.insert(*room, Arc::new(player));
            }
        }
    }
    apply_outputs(state).await
}

/// Gives each player the speakers of its room and of the rooms that follow
/// it.
pub async fn apply_outputs(state: &AppState) -> anyhow::Result<()> {
    let rows = state.store.outputs().await?;
    let mut leaders: Vec<i64> = vec![MAIN_ROOM];
    if let Ok(players) = state.rooms.players.read() {
        leaders.extend(players.keys().copied());
    }
    for leader in leaders {
        let mine: Vec<_> = rows
            .iter()
            .filter(|row| state.rooms.leader(row.room_id) == leader)
            .cloned()
            .collect();
        state
            .jukebox_for(leader)
            .lock()
            .await
            .set_outputs(crate::audio_api::engine_configs(&mine));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rooms_get_players_and_links_share_them() {
        let state = AppState::for_tests().await;
        let patio = state.store.create_room("Patio").await.unwrap().id;
        let deck = state.store.create_room("Deck").await.unwrap().id;
        sync(&state).await.unwrap();
        assert!(
            !Arc::ptr_eq(&state.jukebox_for(patio), &state.jukebox),
            "its own player"
        );
        assert!(Arc::ptr_eq(&state.jukebox_for(MAIN_ROOM), &state.jukebox));
        assert!(state.room_events(MAIN_ROOM).is_none());
        assert!(state.room_events(patio).is_some());

        // The deck follows the patio: same player, no player of its own.
        state.store.link_room(deck, Some(patio)).await.unwrap();
        sync(&state).await.unwrap();
        assert!(Arc::ptr_eq(
            &state.jukebox_for(deck),
            &state.jukebox_for(patio)
        ));
        assert!(state.rooms.player(deck).is_none());

        // Linked to Main: Main's player.
        state.store.link_room(patio, Some(MAIN_ROOM)).await.unwrap();
        sync(&state).await.unwrap();
        assert!(Arc::ptr_eq(&state.jukebox_for(patio), &state.jukebox));
        assert!(
            Arc::ptr_eq(&state.jukebox_for(deck), &state.jukebox),
            "its followers too"
        );

        // A person's requests go to their room's player.
        let sam = state
            .store
            .create_user("sam", "h", djinn_store::Role::User)
            .await
            .unwrap()
            .id;
        state.store.link_room(patio, None).await.unwrap();
        sync(&state).await.unwrap();
        state.store.set_user_room(sam, patio).await.unwrap();
        assert!(Arc::ptr_eq(
            &state.jukebox_of(sam).await,
            &state.jukebox_for(patio)
        ));

        // Deleting a room stops its player.
        state.store.delete_room(patio).await.unwrap();
        sync(&state).await.unwrap();
        assert!(state.rooms.player(patio).is_none());
        assert_eq!(state.room_of(sam).await, MAIN_ROOM);
    }
}
