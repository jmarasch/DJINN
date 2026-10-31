use super::auto::next_in_order;
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
async fn equalizer_restores_and_reaches_the_engine() {
    let store = Store::open_in_memory().await.unwrap();
    let saved = EqualizerSettings {
        enabled: true,
        bands_db: [6.0, -3.0, 0.0, 2.0, -1.0],
    };
    store
        .set_setting(EQUALIZER_SETTING, &serde_json::to_string(&saved).unwrap())
        .await
        .unwrap();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let commands = Arc::clone(&sent);
    let (events, _) = broadcast::channel(16);
    let mut jukebox = Jukebox::new(
        store,
        Box::new(move |command| commands.lock().unwrap().push(command)),
        events,
    )
    .await
    .unwrap();
    assert_eq!(jukebox.equalizer().bands_db, saved.bands_db);
    assert!(sent.lock().unwrap().iter().any(|command| {
        matches!(command, Command::SetEqualizer(config) if config.enabled && config.bands_db == saved.bands_db)
    }));
    jukebox.set_equalizer(EqualizerSettings::default());
    assert!(
        matches!(sent.lock().unwrap().last(), Some(Command::SetEqualizer(config)) if !config.enabled)
    );
}

#[tokio::test]
async fn skip_votes() {
    let mut h = Harness::new(3).await;
    let (admin, user) = (h.admin.clone(), h.user.clone());
    let other = h.store.create_user("other", "h", Role::User).await.unwrap();
    let skips = |h: &Harness| {
        h.sent
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, Command::Skip))
            .count()
    };
    assert!(matches!(
        h.jukebox.vote_skip(&user),
        Err(QueueError::NothingPlaying)
    ));
    h.event(EngineEvent::NextNeeded { after: None }).await;
    h.play_next().await;

    assert!(matches!(
        h.jukebox.vote_skip(&user),
        Err(QueueError::SkipVotesOff)
    ));
    h.jukebox.set_skip_votes_needed(2);
    assert!(!h.jukebox.vote_skip(&user).unwrap());
    assert!(!h.jukebox.vote_skip(&user).unwrap(), "one vote each");
    assert_eq!(h.jukebox.player_state().skip_votes, 1);
    assert!(h.jukebox.vote_skip(&other).unwrap());
    assert_eq!(skips(&h), 1);

    // A new song starts with no votes, and last song's voters sit it
    // out.
    h.play_next().await;
    assert_eq!(h.jukebox.player_state().skip_votes, 0);
    let mut resting = h.jukebox.player_state().skip_resting;
    resting.sort();
    assert_eq!(resting, [user.id, other.id]);
    assert!(matches!(
        h.jukebox.vote_skip(&user),
        Err(QueueError::SkipResting)
    ));
    // The song after that, they can vote again.
    h.play_next().await;
    assert!(h.jukebox.player_state().skip_resting.is_empty());
    assert!(!h.jukebox.vote_skip(&user).unwrap());
    // Admins skip at once.
    assert!(h.jukebox.vote_skip(&admin).unwrap());
    assert_eq!(skips(&h), 2);
}

#[tokio::test]
async fn requester_can_skip_their_own_song_alone() {
    let mut h = Harness::new(0).await;
    let user = h.user.clone();
    let other = h.store.create_user("other", "h", Role::User).await.unwrap();
    h.jukebox
        .add_request(&user, h.tracks[0], &origin())
        .await
        .unwrap();
    h.event(EngineEvent::NextNeeded { after: None }).await;
    h.play_next().await;
    assert!(matches!(
        h.jukebox.vote_skip(&other),
        Err(QueueError::SkipVotesOff)
    ));
    assert!(h.jukebox.vote_skip(&user).unwrap());
}

#[tokio::test]
async fn replay_gain_reaches_the_engine() {
    let mut h = Harness::new(1).await;
    h.store
        .save_analysis(h.tracks[0], 1, Some((-8.0, 0.5)), None)
        .await
        .unwrap();
    let gain_sent = |h: &Harness| {
        h.sent.lock().unwrap().iter().rev().find_map(|c| match c {
            Command::SetNext(Some(t)) => Some(t.gain),
            _ => None,
        })
    };
    h.event(EngineEvent::NextNeeded { after: None }).await;
    assert_eq!(gain_sent(&h), Some(1.0), "off by default");

    h.jukebox.set_replay_gain(ReplayGain {
        mode: ReplayGainMode::Track,
        preamp_db: 0.0,
    });
    h.event(EngineEvent::NextNeeded {
        after: Some(TrackId(h.tracks[0])),
    })
    .await;
    // -18 LUFS target from -8 LUFS: -10 dB.
    let gain = gain_sent(&h).unwrap();
    assert!((gain - 10f32.powf(-0.5)).abs() < 1e-4, "gain {gain}");
}

#[tokio::test]
async fn wild_play_draws_from_the_whole_library() {
    let mut h = Harness::new(0).await;
    h.event(EngineEvent::NextNeeded { after: None }).await;
    assert_eq!(h.last_next(), None, "nothing to play yet");
    h.jukebox.play_playlist(WILD_PLAY, None).await.unwrap();
    assert_eq!(h.jukebox.player_state().playlist.unwrap().name, "Wild play");
    let mut seen = std::collections::HashSet::new();
    for _ in 0..h.tracks.len() {
        seen.insert(h.play_next().await);
    }
    assert_eq!(seen.len(), h.tracks.len(), "every song, none repeated");
}

/// (played, skipped) as the mixer was told, each time.
type Heard = Arc<Mutex<Vec<(Vec<i64>, Vec<i64>)>>>;

/// A mixer with a fixed pool that notes what it was told.
struct FixedMixer {
    pool: Vec<(i64, f32)>,
    heard: Heard,
}

impl Mixer for FixedMixer {
    fn radio(&self, _: &RadioSeed, played: &[i64], skipped: &[i64]) -> Vec<(i64, f32)> {
        self.heard
            .lock()
            .unwrap()
            .push((played.to_vec(), skipped.to_vec()));
        self.pool.clone()
    }
}

#[tokio::test]
async fn radio_plays_the_seed_then_songs_like_it() {
    let mut h = Harness::new(0).await;
    let pool = vec![(h.tracks[1], 1.0), (h.tracks[2], 1.0)];
    let heard: Heard = Arc::default();
    h.jukebox.set_mixer(Arc::new(FixedMixer {
        pool: pool.clone(),
        heard: Arc::clone(&heard),
    }));
    let seed = h.tracks[0];
    h.jukebox
        .start_radio(RadioSeed::Track { id: seed }, "Song 0".into())
        .await
        .unwrap();
    assert_eq!(
        h.jukebox.player_state().playlist.unwrap().name,
        "Radio: Song 0"
    );
    let first = h.play_next().await;
    assert_eq!(first, seed, "the seed song plays first");
    h.finish(first).await;
    let second = h.play_next().await;
    assert!(pool.iter().any(|(id, _)| *id == second));
    h.event(EngineEvent::TrackEnded {
        id: TrackId(second),
        played_ms: 500,
        reason: EndReason::Skipped,
    })
    .await;
    let third = h.play_next().await;
    assert!(pool.iter().any(|(id, _)| *id == third));
    assert_ne!(third, second, "the skipped song played recently");
    let (played, skipped) = heard.lock().unwrap().last().cloned().unwrap();
    assert_eq!(played, [seed], "what played through steers the radio");
    assert_eq!(skipped, [second], "and what was skipped steers it away");

    // It's still on after a restart.
    let (events, _) = broadcast::channel(4);
    let again = Jukebox::new(h.store.clone(), Box::new(|_| {}), events)
        .await
        .unwrap();
    assert_eq!(again.player_state().playlist.unwrap().name, "Radio: Song 0");
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
    let (item, _) = h
        .jukebox
        .add_request(&admin, h.tracks[1], &origin())
        .await
        .unwrap();
    assert!(matches!(
        h.jukebox.remove_request(&user, item.id).await,
        Err(QueueError::NotYours)
    ));
    let (mine, _) = h
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
        credits: Vec::new(),
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

#[tokio::test]
async fn a_restart_carries_on_with_the_song_playing() {
    let mut h = Harness::new(0).await;
    let user = h.user.clone();
    for &track in &h.tracks[..2] {
        h.jukebox
            .add_request(&user, track, &origin())
            .await
            .unwrap();
    }
    h.event(EngineEvent::NextNeeded { after: None }).await;
    let first = h.play_next().await;
    assert_eq!(first, h.tracks[0]);
    h.event(EngineEvent::Position {
        id: TrackId(first),
        ms: 15_000,
    })
    .await;

    // DJINN restarts on the same database.
    let sent = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&sent);
    let (events, _) = broadcast::channel(64);
    let mut jukebox = Jukebox::new(
        h.store.clone(),
        Box::new(move |c| sink.lock().unwrap().push(c)),
        events,
    )
    .await
    .unwrap();
    jukebox
        .on_engine_event(EngineEvent::NextNeeded { after: None })
        .await
        .unwrap();
    let next = sent.lock().unwrap().iter().rev().find_map(|c| match c {
        Command::SetNext(Some(t)) => Some((t.id.0, t.start_ms)),
        _ => None,
    });
    assert_eq!(next, Some((first, 15_000)), "the same song, where it was");

    for event in [
        EngineEvent::NextNeeded {
            after: Some(TrackId(first)),
        },
        EngineEvent::TrackStarted { id: TrackId(first) },
        EngineEvent::Position {
            id: TrackId(first),
            ms: 1_000,
        },
    ] {
        jukebox.on_engine_event(event).await.unwrap();
    }
    let now = jukebox.now_playing();
    let current = now.current.unwrap();
    assert_eq!(current.requested_by.as_deref(), Some("guest"));
    assert_eq!(
        now.position_ms, 16_000,
        "counted from the start of the song"
    );
    assert_eq!(
        now.up_next.unwrap().track.id,
        h.tracks[1],
        "the queue is still there"
    );

    jukebox
        .on_engine_event(EngineEvent::TrackEnded {
            id: TrackId(first),
            played_ms: 2_000,
            reason: EndReason::Finished,
        })
        .await
        .unwrap();
    assert_eq!(
        h.store.plays_since(0).await.unwrap(),
        [(first, false)],
        "one play, finished"
    );
}
