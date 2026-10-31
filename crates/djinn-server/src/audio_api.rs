//! Audio settings (admin): outputs (play through several devices at once,
//! each with its own volume trim and delay) and ReplayGain.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use std::sync::atomic::Ordering;

use axum::routing::{get, put};
use axum::{Json, Router};
use djinn_engine::{OutputConfig, OutputDevice, MAX_DELAY_MS};
use djinn_store::{OutputRow, Store};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::events::{LoudnessStatus, OutputStatus, ServerEvent};
use crate::jukebox::{ReplayGain, MAX_PREAMP_DB, REPLAY_GAIN_MODE, REPLAY_GAIN_PREAMP};
use crate::state::AppState;

const MAX_LABEL: usize = 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/outputs", get(list).post(add))
        .route("/outputs/devices", get(devices))
        .route("/outputs/{id}", put(update).delete(remove))
        .route("/audio/replaygain", get(replay_gain).put(set_replay_gain))
}

/// The engine's view of the configured outputs.
pub fn engine_configs(rows: &[OutputRow]) -> Vec<OutputConfig> {
    rows.iter()
        .map(|row| OutputConfig {
            id: row.id,
            device: if row.is_system_default() {
                OutputDevice::Default
            } else {
                OutputDevice::Id(row.device_name.clone())
            },
            enabled: row.enabled,
            trim: row.volume_trim.clamp(0.0, 1.0) as f32,
            delay_ms: u32::try_from(row.delay_ms.clamp(0, i64::from(MAX_DELAY_MS))).unwrap_or(0),
        })
        .collect()
}

/// Sends the stored outputs to the engine.
async fn apply(state: &AppState) -> ApiResult<Vec<OutputRow>> {
    let rows = state.store.outputs().await?;
    state
        .jukebox
        .lock()
        .await
        .set_outputs(engine_configs(&rows));
    Ok(rows)
}

#[derive(Serialize)]
struct OutputList {
    outputs: Vec<OutputRow>,
    status: OutputStatus,
    max_delay_ms: u32,
}

async fn list(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<OutputList>> {
    let outputs = state.store.outputs().await?;
    let status = state.jukebox.lock().await.output_status();
    Ok(Json(OutputList {
        outputs,
        status,
        max_delay_ms: MAX_DELAY_MS,
    }))
}

#[derive(Serialize)]
struct Device {
    id: String,
    name: String,
    /// The OS default right now (what "System default" plays through).
    is_default: bool,
    /// Already set up as an output.
    added: bool,
}

async fn devices(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Vec<Device>>> {
    // Listing talks to the audio server and blocks.
    let found = tokio::task::spawn_blocking(djinn_engine::output_devices)
        .await
        .map_err(anyhow::Error::from)?
        .map_err(|e| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    let outputs = state.store.outputs().await?;
    Ok(Json(
        found
            .into_iter()
            .map(|d| Device {
                added: outputs.iter().any(|o| o.device_name == d.id),
                id: d.id,
                name: d.name,
                is_default: d.is_default,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
struct NewOutput {
    device_id: String,
    label: String,
}

async fn add(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewOutput>,
) -> ApiResult<(StatusCode, Json<OutputRow>)> {
    let device_id = body.device_id.trim();
    if device_id.is_empty() {
        return Err(ApiError::bad_request("pick a device"));
    }
    let label = clean_label(&body.label, device_id);
    let row = state.store.add_output(device_id, &label).await?;
    apply(&state).await?;
    Ok((StatusCode::CREATED, Json(row)))
}

#[derive(Deserialize)]
struct OutputUpdate {
    enabled: bool,
    volume_trim: f64,
    delay_ms: i64,
    /// A new name, if renamed.
    #[serde(default)]
    label: Option<String>,
}

async fn update(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<OutputUpdate>,
) -> ApiResult<Json<OutputRow>> {
    if !(0.0..=1.0).contains(&body.volume_trim) {
        return Err(ApiError::bad_request("volume trim must be 0 to 1"));
    }
    if !(0..=i64::from(MAX_DELAY_MS)).contains(&body.delay_ms) {
        return Err(ApiError::bad_request(format!(
            "delay must be 0 to {MAX_DELAY_MS} ms"
        )));
    }
    if !state
        .store
        .update_output(id, body.enabled, body.volume_trim, body.delay_ms)
        .await?
    {
        return Err(ApiError::not_found());
    }
    if let Some(label) = body.label.as_deref().map(str::trim) {
        if label.is_empty() {
            return Err(ApiError::bad_request("give the output a name"));
        }
        let label: String = label.chars().take(MAX_LABEL).collect();
        state.store.rename_output(id, &label).await?;
    }
    apply(&state).await?;
    output(&state.store, id).await.map(Json)
}

async fn remove(
    _: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let row = output(&state.store, id).await?;
    if row.is_system_default() {
        return Err(ApiError::bad_request(
            "the system default output can't be removed; turn it off instead",
        ));
    }
    state.store.delete_output(id).await?;
    apply(&state).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn output(store: &Store, id: i64) -> ApiResult<OutputRow> {
    store.output(id).await?.ok_or_else(ApiError::not_found)
}

#[derive(Serialize)]
struct ReplayGainResponse {
    #[serde(flatten)]
    settings: ReplayGain,
    max_preamp_db: f64,
    loudness: LoudnessStatus,
}

async fn replay_gain(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<ReplayGainResponse>> {
    let settings = state.jukebox.lock().await.replay_gain();
    Ok(Json(ReplayGainResponse {
        settings,
        max_preamp_db: MAX_PREAMP_DB,
        loudness: loudness_status(&state).await?,
    }))
}

async fn set_replay_gain(
    admin: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<ReplayGain>,
) -> ApiResult<Json<ReplayGainResponse>> {
    if !body.preamp_db.is_finite() || body.preamp_db.abs() > MAX_PREAMP_DB {
        return Err(ApiError::bad_request(format!(
            "preamp must be -{MAX_PREAMP_DB} to {MAX_PREAMP_DB} dB"
        )));
    }
    state
        .store
        .set_setting(REPLAY_GAIN_MODE, body.mode.as_str())
        .await?;
    state
        .store
        .set_setting(REPLAY_GAIN_PREAMP, &body.preamp_db.to_string())
        .await?;
    state.jukebox.lock().await.set_replay_gain(body);
    replay_gain(admin, State(state)).await
}

pub async fn loudness_status(state: &AppState) -> ApiResult<LoudnessStatus> {
    Ok(LoudnessStatus {
        progress: state
            .store
            .loudness_progress(djinn_engine::FEATURES_VERSION)
            .await?,
        running: state.scan.loudness_running.load(Ordering::Acquire),
    })
}

/// Songs analyzed per round between progress updates.
const ANALYSIS_BATCH: i64 = 16;

/// Analyzes songs not analyzed yet in the background: loudness (for
/// ReplayGain) and sound (for smart playlists), one decode each. Does
/// nothing if it already runs.
pub async fn start_analysis(state: &AppState) {
    if state.scan.loudness_running.swap(true, Ordering::AcqRel) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        let workers = crate::cpu::analysis();
        let version = djinn_engine::FEATURES_VERSION;
        let started = std::time::Instant::now();
        let mut analyzed = 0usize;
        // Songs that couldn't be decoded (or are too short); they're marked
        // done too, so they aren't tried again.
        let mut unreadable = 0usize;
        loop {
            let batch = match state.store.tracks_to_analyze(version, ANALYSIS_BATCH).await {
                Ok(batch) if !batch.is_empty() => batch,
                Ok(_) => break,
                Err(e) => {
                    tracing::error!(error = %e, "reading songs to analyze failed");
                    break;
                }
            };
            // A few at a time on blocking threads; decoding is CPU-heavy,
            // and this leaves cores for playback.
            for chunk in batch.chunks(workers) {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|(id, path)| {
                        let (id, path) = (*id, std::path::PathBuf::from(path));
                        tokio::task::spawn_blocking(move || {
                            (id, djinn_engine::analyze(&path), path)
                        })
                    })
                    .collect();
                for job in jobs {
                    let Ok((id, result, path)) = job.await else {
                        continue;
                    };
                    let (loudness, features) = match result {
                        Ok(analysis) => (
                            analysis.loudness.map(|l| (l.lufs, l.peak)),
                            analysis.features,
                        ),
                        Err(e) => {
                            tracing::debug!(path = %path.display(), error = %e, "cannot analyze song");
                            unreadable += 1;
                            (None, None)
                        }
                    };
                    let saved = state
                        .store
                        .save_analysis(
                            id,
                            version,
                            loudness,
                            features.as_ref().map(|f| (f.values.as_slice(), f.bpm)),
                        )
                        .await;
                    if let Err(e) = saved {
                        tracing::error!(error = %e, "saving analysis failed");
                    }
                    analyzed += 1;
                }
            }
            if let Ok(status) = loudness_status(&state).await {
                let _ = state.events.send(ServerEvent::Loudness(status));
            }
        }
        state.scan.loudness_running.store(false, Ordering::Release);
        if analyzed > 0 {
            tracing::info!(
                songs = analyzed - unreadable,
                unreadable,
                secs = started.elapsed().as_secs_f32(),
                "songs analyzed"
            );
            crate::smart::refresh(&state).await;
        }
        if let Ok(status) = loudness_status(&state).await {
            let _ = state.events.send(ServerEvent::Loudness(status));
        }
        // Next step: AI listening, if it's on.
        crate::ai::start(&state);
    });
}

fn clean_label(label: &str, device_id: &str) -> String {
    let label = label.trim();
    let label = if label.is_empty() { device_id } else { label };
    label.chars().take(MAX_LABEL).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_engine_configs() {
        let rows = [
            OutputRow {
                id: 1,
                device_name: String::new(),
                label: "System default".into(),
                enabled: true,
                volume_trim: 1.0,
                delay_ms: 120,
            },
            OutputRow {
                id: 2,
                device_name: "pulseaudio:bluez_output.AA".into(),
                label: "Speaker".into(),
                enabled: false,
                volume_trim: 1.5,
                delay_ms: 5000,
            },
        ];
        let configs = engine_configs(&rows);
        assert_eq!(configs[0].device, OutputDevice::Default);
        assert_eq!(configs[0].delay_ms, 120);
        assert_eq!(
            configs[1].device,
            OutputDevice::Id("pulseaudio:bluez_output.AA".into())
        );
        assert!(!configs[1].enabled);
        assert_eq!(configs[1].trim, 1.0, "trim is clamped");
        assert_eq!(configs[1].delay_ms, MAX_DELAY_MS, "delay is clamped");
    }
}
