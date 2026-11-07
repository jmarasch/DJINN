//! `GET /api/events`: live state over Server-Sent Events.
//!
//! A new connection first receives a snapshot (now playing, player state,
//! queue, output, scan progress) and then every change as it happens.
//! `GET /api/tv/events` is the public, read-only version for the TV page:
//! only what's playing and the queue.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, Stream, StreamExt};
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;

use crate::auth::CurrentUser;
use crate::error::ApiResult;
use crate::events::ServerEvent;
use crate::state::AppState;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Audience {
    /// Signed-in people: everything.
    Member,
    /// The TV page: playback and queue only.
    Tv,
}

impl Audience {
    fn wants(self, event: &ServerEvent) -> bool {
        match self {
            Self::Member => true,
            Self::Tv => matches!(
                event,
                ServerEvent::Hello { .. }
                    | ServerEvent::NowPlaying(_)
                    | ServerEvent::Position { .. }
                    | ServerEvent::QueueChanged(_)
                    | ServerEvent::PlayerState(_)
            ),
        }
    }
}

/// What a room's player says (the rest is about the whole house).
fn from_a_room(event: &ServerEvent) -> bool {
    matches!(
        event,
        ServerEvent::NowPlaying(_)
            | ServerEvent::Position { .. }
            | ServerEvent::QueueChanged(_)
            | ServerEvent::PlayerState(_)
            | ServerEvent::OutputStatus(_)
    )
}

/// Whether a room's page gets an event: house-wide news from the house
/// channel; the queue and speakers from the room's own; the song and
/// playback from the room's own, or Main's (the house channel) while the
/// room follows Main. Main's pages get everything on the house channel.
fn wanted(event: &ServerEvent, has_own: bool, from_own: bool, follows: bool) -> bool {
    if !has_own {
        return true;
    }
    if !from_a_room(event) {
        return !from_own;
    }
    match event {
        ServerEvent::QueueChanged(_) | ServerEvent::OutputStatus(_) => from_own,
        _ => from_own != follows,
    }
}

/// A page's room, from `?room=` (Main when missing).
#[derive(serde::Deserialize)]
pub struct RoomQuery {
    pub room: Option<i64>,
}

pub async fn events(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let room = state.room_of(user.id).await;
    live(state, Audience::Member, room).await
}

/// The TV page's feed. Public while the TV page is on.
pub async fn tv_events(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<RoomQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    crate::tv_api::check_enabled(&state).await?;
    live(
        state,
        Audience::Tv,
        query.room.unwrap_or(djinn_store::MAIN_ROOM),
    )
    .await
}

pub async fn lyrics_events(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<RoomQuery>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    crate::lyrics::check_page(&state).await?;
    live(
        state,
        Audience::Tv,
        query.room.unwrap_or(djinn_store::MAIN_ROOM),
    )
    .await
}

async fn live(
    state: AppState,
    audience: Audience,
    room: i64,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let room = state.rooms.leader(room);
    // Subscribe before taking the snapshot so nothing falls in between. A
    // room other than Main has its own channel for what its player says;
    // the house-wide channel brings the rest, and Main's song while the
    // room is quiet and follows Main. Items are (from the room's own
    // channel, event).
    let own = state.room_events(room);
    let has_own = own.is_some();
    let house = BroadcastStream::new(state.events.subscribe()).map(|item| (false, item));
    let updates: std::pin::Pin<Box<dyn Stream<Item = _> + Send>> = match own {
        Some(own) => Box::pin(stream::select(
            house,
            BroadcastStream::new(own.subscribe()).map(|item| (true, item)),
        )),
        None => Box::pin(house),
    };
    let initial = snapshot(&state, room).await?;

    let resync_state = state.clone();
    let following =
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(state.follows_main(room)));
    let updates = updates.then(move |(from_own, item)| {
        let state = resync_state.clone();
        let following = std::sync::Arc::clone(&following);
        async move {
            match item {
                Ok(event) => {
                    let follows = state.follows_main(room);
                    if following.swap(follows, std::sync::atomic::Ordering::Relaxed) != follows {
                        // Started or stopped following Main: show what the
                        // room hears now.
                        return snapshot(&state, room).await.unwrap_or_default();
                    }
                    if wanted(&event, has_own, from_own, follows) {
                        vec![event]
                    } else {
                        Vec::new()
                    }
                }
                // This client fell behind; send current state instead of
                // the events it missed.
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    snapshot(&state, room).await.unwrap_or_default()
                }
            }
        }
    });
    let mut shutdown = state.shutdown.subscribe();
    let stream = stream::iter(vec![initial])
        .chain(updates)
        .flat_map(stream::iter)
        .filter(move |event| std::future::ready(audience.wants(event)))
        .map(|event| Ok(to_sse(&event)))
        .take_until(async move {
            let _ = shutdown.wait_for(|&stop| stop).await;
        });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn snapshot(state: &AppState, room: i64) -> anyhow::Result<Vec<ServerEvent>> {
    let mut events = {
        // The song from whichever jukebox the room hears; the speakers are
        // the room's own.
        let sound = state.sound_jukebox_for(room);
        let sound = sound.lock().await;
        vec![
            ServerEvent::Hello {
                web_build: crate::web::build_id(),
            },
            ServerEvent::NowPlaying(Box::new(sound.now_playing())),
            ServerEvent::PlayerState(sound.player_state()),
        ]
    };
    let output = state.jukebox_for(room).lock().await.output_status();
    events.push(ServerEvent::OutputStatus(output));
    events.push(ServerEvent::QueueChanged(state.store.queue(room).await?));
    events.push(ServerEvent::ArtistLookup(
        crate::library_api::lookup_status(state),
    ));
    let scan = state.scan.last.lock().ok().and_then(|last| last.clone());
    if let Some(progress) = scan {
        events.push(ServerEvent::ScanProgress(progress));
    }
    Ok(events)
}

fn to_sse(event: &ServerEvent) -> Event {
    let data = serde_json::to_value(event)
        .map(|mut v| v["data"].take())
        .unwrap_or_default();
    Event::default().event(event.name()).data(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_room_following_main_shows_mains_song_but_keeps_its_queue() {
        let song = ServerEvent::Position {
            track_id: 1,
            position_ms: 0,
        };
        let queue = ServerEvent::QueueChanged(Vec::new());
        let news = ServerEvent::LibraryChanged;
        // Main's own pages: everything from the house channel.
        assert!(wanted(&song, false, false, false));
        // Another room playing its own music.
        assert!(wanted(&song, true, true, false));
        assert!(!wanted(&song, true, false, false), "not Main's song");
        // Following Main: Main's song, not its own (idle) player's.
        assert!(wanted(&song, true, false, true));
        assert!(!wanted(&song, true, true, true));
        // Its own queue either way; never Main's.
        assert!(wanted(&queue, true, true, true));
        assert!(!wanted(&queue, true, false, true));
        // House news only once, from the house channel.
        assert!(wanted(&news, true, false, false));
        assert!(!wanted(&news, true, true, false));
    }
}
