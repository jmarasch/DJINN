//! The thorough check: reads the start of every MP3 in the library to find
//! variable bit rate files with no length header, which players other than
//! DJINN show with the wrong length. Runs in the background after startup
//! and after library checks, taking its turn like other heavy work; a file
//! is read again only when it changes. Found ones are a Library Problems
//! kind, fixed with the editor's Fix MP3 lengths.

use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use djinn_store::{Mp3CheckCounts, Mp3ToCheck};
use serde::Serialize;

use crate::auth::AdminUser;
use crate::error::ApiResult;
use crate::state::AppState;

/// MP3s read per batch (the check waits its turn between batches).
const BATCH: i64 = 400;
/// Files read at once.
const READERS: usize = 4;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Asked to start while running: go round again when done.
static AGAIN: AtomicBool = AtomicBool::new(false);

pub fn router() -> Router<AppState> {
    Router::new().route("/library/mp3-check", get(status).post(again))
}

/// Checks the MP3s not yet checked (or changed since), in the background.
pub fn start(state: &AppState) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        AGAIN.store(true, Ordering::Release);
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            AGAIN.store(false, Ordering::Release);
            match run(&state).await {
                Ok(0) => {}
                Ok(found) => {
                    tracing::info!(found, "thorough check: MP3s with no length header");
                }
                Err(e) => tracing::warn!(error = %format!("{e:#}"), "thorough MP3 check failed"),
            }
            if !AGAIN.load(Ordering::Acquire) || *state.shutdown.borrow() {
                break;
            }
        }
        RUNNING.store(false, Ordering::Release);
    });
}

/// Works through every MP3 waiting; how many lack a header.
async fn run(state: &AppState) -> anyhow::Result<usize> {
    let mut found = 0;
    loop {
        let batch = state.store.mp3s_to_check(BATCH).await?;
        if batch.is_empty() {
            return Ok(found);
        }
        let go_on = crate::background::wait_turn(state, || async { true }, || async {}).await;
        if !go_on || *state.shutdown.borrow() {
            return Ok(found);
        }
        let results = tokio::task::spawn_blocking(move || read_all(batch)).await?;
        found += results.iter().filter(|(_, missing)| *missing).count();
        state.store.save_mp3_checks(&results).await?;
        // The Problems page counts again.
        *state.problem_counts.lock().await = None;
    }
}

/// Reads each file's header, a few files at a time.
fn read_all(batch: Vec<Mp3ToCheck>) -> Vec<(Mp3ToCheck, bool)> {
    let chunk = batch.len().div_ceil(READERS).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> = batch
            .chunks(chunk)
            .map(|part| {
                scope.spawn(move || {
                    part.iter()
                        .map(|song| {
                            let missing = djinn_library::mpeg::needs_length_header(
                                std::path::Path::new(&song.path),
                            );
                            (song.clone(), missing)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

#[derive(Serialize)]
struct Status {
    running: bool,
    #[serde(flatten)]
    counts: Mp3CheckCounts,
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    Ok(Json(Status {
        running: RUNNING.load(Ordering::Acquire),
        counts: state.store.mp3_check_counts().await?,
    }))
}

/// Forgets every result and reads every MP3 again.
async fn again(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.forget_mp3_checks().await?;
    *state.problem_counts.lock().await = None;
    start(&state);
    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_that_cannot_be_read_are_not_listed() {
        let batch: Vec<Mp3ToCheck> = (0..9)
            .map(|id| Mp3ToCheck {
                id,
                path: format!("/no/such/file-{id}.mp3"),
                size: 1,
                mtime: 1,
            })
            .collect();
        let out = read_all(batch);
        assert_eq!(out.len(), 9);
        assert!(out.iter().all(|(_, missing)| !missing));
    }
}
