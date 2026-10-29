//! Identifying songs with missing tags (admin): fingerprints go to
//! AcoustID, details come from MusicBrainz, and the admin reviews what was
//! found. Applied matches fill in what the tags lack, in DJINN only.

use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::identify::{Hint, Identifier};
use djinn_store::{Identification, IdentificationRow, IdentifyProgress};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::settings;
use crate::state::AppState;

/// Setting key: the AcoustID application key.
pub const ACOUSTID_KEY: &str = "acoustid_key";
const BATCH: i64 = 10;

/// Whether an identification run is going.
pub static RUNNING: AtomicBool = AtomicBool::new(false);
/// The admin asked the run to stop.
static STOP: AtomicBool = AtomicBool::new(false);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/identify", get(status))
        .route("/identify/start", post(start))
        .route("/identify/stop", post(stop))
        .route("/identify/apply", post(apply))
        .route("/identify/dismiss", post(dismiss))
}

#[derive(Clone, Debug, Serialize)]
pub struct IdentifyStatus {
    #[serde(flatten)]
    progress: IdentifyProgress,
    running: bool,
    /// What's missing before it can run.
    needs_key: bool,
    needs_contact: bool,
}

async fn identify_status(state: &AppState) -> ApiResult<IdentifyStatus> {
    Ok(IdentifyStatus {
        progress: state.store.identify_progress().await?,
        running: RUNNING.load(Ordering::Acquire),
        needs_key: key(state).await?.is_empty(),
        needs_contact: contact(state).await?.is_empty(),
    })
}

async fn key(state: &AppState) -> ApiResult<String> {
    Ok(state
        .store
        .get_setting(ACOUSTID_KEY)
        .await?
        .unwrap_or_default()
        .trim()
        .to_owned())
}

async fn contact(state: &AppState) -> ApiResult<String> {
    Ok(state
        .store
        .get_setting(settings::MUSICBRAINZ_CONTACT)
        .await?
        .unwrap_or_default()
        .trim()
        .to_owned())
}

async fn broadcast(state: &AppState) {
    if let Ok(status) = identify_status(state).await {
        let _ = state.events.send(ServerEvent::IdentifyStatus(status));
    }
}

#[derive(Serialize)]
struct Review {
    #[serde(flatten)]
    status: IdentifyStatus,
    /// Found and waiting for the admin, best matches first.
    found: Vec<IdentificationRow>,
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Review>> {
    Ok(Json(Review {
        status: identify_status(&state).await?,
        found: state.store.identifications("found").await?,
    }))
}

async fn start(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let (key, contact) = (key(&state).await?, contact(&state).await?);
    if key.is_empty() || contact.is_empty() {
        return Err(ApiError::bad_request(
            "add an AcoustID key and a MusicBrainz contact under Rules first",
        ));
    }
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Ok(StatusCode::ACCEPTED);
    }
    STOP.store(false, Ordering::Release);
    let task_state = state.clone();
    tokio::spawn(async move {
        if let Err(e) = run(&task_state, &key, &contact).await {
            tracing::warn!(error = %e, "identifying songs stopped");
        }
        RUNNING.store(false, Ordering::Release);
        broadcast(&task_state).await;
    });
    broadcast(&state).await;
    Ok(StatusCode::ACCEPTED)
}

/// Stops after the song being looked up; what was found so far stays.
async fn stop(_: AdminUser) -> StatusCode {
    STOP.store(true, Ordering::Release);
    StatusCode::NO_CONTENT
}

async fn run(state: &AppState, key: &str, contact: &str) -> Result<(), String> {
    let mut identifier = Identifier::new(key, contact).map_err(|e| e.to_string())?;
    let (mut checked, mut found) = (0usize, 0usize);
    loop {
        let batch = state
            .store
            .tracks_to_identify(BATCH)
            .await
            .map_err(|e| e.to_string())?;
        if batch.is_empty() {
            break;
        }
        for song in batch {
            if STOP.load(Ordering::Acquire) {
                tracing::info!(checked, found, "identifying songs stopped");
                return Ok(());
            }
            let path = std::path::PathBuf::from(&song.path);
            let print = tokio::task::spawn_blocking(move || djinn_engine::fingerprint(&path))
                .await
                .map_err(|e| e.to_string())?;
            let result = match print {
                Ok(Some(print)) => {
                    let duration = song.duration_ms.unwrap_or(0).max(0) as u64 / 1000;
                    let hint = Hint {
                        title: Some(song.title.clone()).filter(|t| !t.trim().is_empty()),
                        artist: song.artist.clone(),
                    };
                    match identifier.identify(&print, duration, &hint).await {
                        Ok(found) => found,
                        // A bad key or a service problem: stop, try later.
                        Err(e) => return Err(e.to_string()),
                    }
                }
                Ok(None) => None,
                Err(e) => {
                    tracing::debug!(path = %song.path, error = %e, "cannot fingerprint song");
                    None
                }
            };
            let saved = result.map(|r| Identification {
                score: r.score,
                recording: r.recording,
                title: r.title,
                artist: r.artist,
                album: r.album,
                year: r.year,
                genres: r.genres,
            });
            found += usize::from(saved.is_some());
            state
                .store
                .save_identification(song.id, saved.as_ref())
                .await
                .map_err(|e| e.to_string())?;
            checked += 1;
        }
        broadcast(state).await;
    }
    tracing::info!(checked, found, "identifying songs finished");
    Ok(())
}

#[derive(Deserialize)]
struct Songs {
    track_ids: Vec<i64>,
}

/// Uses what was found: fills the songs' gaps and re-reads them.
async fn apply(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    let paths = state
        .store
        .set_identification_status(&body.track_ids, "applied")
        .await?;
    crate::library_api::reread(&state, &paths).await?;
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn dismiss(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Songs>,
) -> ApiResult<StatusCode> {
    state
        .store
        .set_identification_status(&body.track_ids, "dismissed")
        .await?;
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}
