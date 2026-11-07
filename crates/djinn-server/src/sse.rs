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
    // the house-wide channel then only brings the rest.
    let own = state.room_events(room);
    let house = BroadcastStream::new(state.events.subscribe());
    let updates: std::pin::Pin<Box<dyn Stream<Item = _> + Send>> = match own {
        Some(own) => {
            let house = house.filter(|item| {
                std::future::ready(!matches!(item, Ok(event) if from_a_room(event)))
            });
            let own = BroadcastStream::new(own.subscribe()).filter(|item| {
                std::future::ready(!matches!(item, Ok(event) if !from_a_room(event)))
            });
            Box::pin(stream::select(house, own))
        }
        None => Box::pin(house),
    };
    let initial = snapshot(&state, room).await?;

    let resync_state = state.clone();
    let updates = updates.then(move |item| {
        let state = resync_state.clone();
        async move {
            match item {
                Ok(event) => vec![event],
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
        let jukebox = state.jukebox_for(room);
        let jukebox = jukebox.lock().await;
        vec![
            ServerEvent::Hello {
                web_build: crate::web::build_id(),
            },
            ServerEvent::NowPlaying(Box::new(jukebox.now_playing())),
            ServerEvent::PlayerState(jukebox.player_state()),
            ServerEvent::OutputStatus(jukebox.output_status()),
        ]
    };
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
