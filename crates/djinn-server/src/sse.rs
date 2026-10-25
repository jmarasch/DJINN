//! `GET /api/events`: live state over Server-Sent Events.
//!
//! A new connection first receives a snapshot (now playing, player state,
//! queue, output, scan progress) and then every change as it happens.

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

pub async fn events(
    _: CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Subscribe before taking the snapshot so nothing falls in between.
    let updates = BroadcastStream::new(state.events.subscribe());
    let initial = snapshot(&state).await?;

    let resync_state = state.clone();
    let updates = updates.then(move |item| {
        let state = resync_state.clone();
        async move {
            match item {
                Ok(event) => vec![event],
                // This client fell behind; send current state instead of
                // the events it missed.
                Err(BroadcastStreamRecvError::Lagged(_)) => {
                    snapshot(&state).await.unwrap_or_default()
                }
            }
        }
    });
    let mut shutdown = state.shutdown.subscribe();
    let stream = stream::iter(vec![initial])
        .chain(updates)
        .flat_map(stream::iter)
        .map(|event| Ok(to_sse(&event)))
        .take_until(async move {
            let _ = shutdown.wait_for(|&stop| stop).await;
        });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn snapshot(state: &AppState) -> anyhow::Result<Vec<ServerEvent>> {
    let mut events = {
        let jukebox = state.jukebox.lock().await;
        vec![
            ServerEvent::NowPlaying(Box::new(jukebox.now_playing())),
            ServerEvent::PlayerState(jukebox.player_state()),
            ServerEvent::OutputStatus(jukebox.output_status()),
        ]
    };
    events.push(ServerEvent::QueueChanged(state.store.queue().await?));
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
