//! Optional AI listening (admin): download the CLAP model, then let it
//! listen to every song in the background, so search can match
//! descriptions against how songs sound. Runs on this computer only; the
//! music folders are never touched.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_ai::{models, Clap};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::state::AppState;

/// Setting key: AI listening is on.
const AI_LISTENING: &str = "ai_listening";
/// Songs listened to per round between progress updates.
const BATCH: i64 = 8;
/// Longest stretch of a song decoded for listening.
const MAX_SECS: u32 = 12 * 60;

/// Genres the model is asked about for songs with no genres of their own.
const GENRES: &[&str] = &[
    "rock",
    "hard rock",
    "heavy metal",
    "glam metal",
    "punk",
    "grunge",
    "alternative rock",
    "indie rock",
    "gothic rock",
    "synth pop",
    "new wave",
    "pop",
    "dance pop",
    "electropop",
    "house",
    "deep house",
    "techno",
    "trance",
    "dubstep",
    "drum and bass",
    "edm",
    "hip hop",
    "trap",
    "r&b",
    "soul",
    "funk",
    "disco",
    "jazz",
    "blues",
    "country",
    "folk",
    "acoustic",
    "classical",
    "ambient",
    "lo fi",
    "reggae",
    "latin",
    "reggaeton",
    "k pop",
    "metalcore",
];

/// Model, progress, and switches, shared by everything.
pub struct AiState {
    dir: PathBuf,
    model: RwLock<Option<Arc<Clap>>>,
    /// Each genre's text embedding, made once the model loads.
    genres: RwLock<Vec<(String, Vec<f32>)>>,
    download: Mutex<Option<(u64, u64)>>,
    error: Mutex<Option<String>>,
    loading: AtomicBool,
    listening: AtomicBool,
}

impl AiState {
    /// `models_dir` is DJINN's models folder (next to the database).
    pub fn new(models_dir: &Path) -> Self {
        Self {
            dir: models_dir.join(models::folder_name()),
            model: RwLock::new(None),
            genres: RwLock::new(Vec::new()),
            download: Mutex::new(None),
            error: Mutex::new(None),
            loading: AtomicBool::new(false),
            listening: AtomicBool::new(false),
        }
    }

    pub fn model(&self) -> Option<Arc<Clap>> {
        self.model.read().ok().and_then(|m| m.clone())
    }

    /// Each genre's embedding, for tagging songs.
    pub fn genre_embeddings(&self) -> Vec<(String, Vec<f32>)> {
        self.genres.read().map(|g| g.clone()).unwrap_or_default()
    }

    fn set_error(&self, error: Option<String>) {
        if let Ok(mut e) = self.error.lock() {
            *e = error;
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai", get(status).put(set_enabled))
        .route("/ai/download", post(download))
        .route("/ai/model", delete(remove_model))
}

#[derive(Clone, Debug, Serialize)]
pub struct AiStatus {
    installed: bool,
    enabled: bool,
    /// Loaded and ready.
    ready: bool,
    download_size: u64,
    /// (done, total) bytes while downloading.
    downloading: Option<(u64, u64)>,
    error: Option<String>,
    heard: i64,
    waiting: i64,
    listening: bool,
}

pub async fn ai_status(state: &AppState) -> ApiResult<AiStatus> {
    let ai = &state.ai;
    let (heard, waiting) = state.store.heard_progress(models::VERSION).await?;
    Ok(AiStatus {
        installed: models::installed(&ai.dir),
        enabled: enabled(state).await?,
        ready: ai.model().is_some(),
        download_size: models::download_size(),
        downloading: ai.download.lock().ok().and_then(|d| *d),
        error: ai.error.lock().ok().and_then(|e| e.clone()),
        heard,
        waiting,
        listening: ai.listening.load(Ordering::Acquire),
    })
}

async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(crate::jukebox::setting(&state.store, AI_LISTENING, 0).await? == 1)
}

async fn broadcast(state: &AppState) {
    if let Ok(status) = ai_status(state).await {
        let _ = state.events.send(ServerEvent::AiStatus(status));
    }
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<AiStatus>> {
    Ok(Json(ai_status(&state).await?))
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

async fn set_enabled(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Enabled>,
) -> ApiResult<Json<AiStatus>> {
    if body.enabled && !models::installed(&state.ai.dir) {
        return Err(ApiError::bad_request("download the model first"));
    }
    state
        .store
        .set_setting(AI_LISTENING, if body.enabled { "1" } else { "0" })
        .await?;
    if body.enabled {
        start(&state);
    } else if let Ok(mut model) = state.ai.model.write() {
        // Free the memory; the listening job stops at its next song.
        *model = None;
    }
    broadcast(&state).await;
    Ok(Json(ai_status(&state).await?))
}

/// Loads the model if AI listening is on, then listens to new songs.
/// Called at startup, when it's switched on, and after library checks.
pub fn start(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        if !enabled(&state).await.unwrap_or(false) || !models::installed(&state.ai.dir) {
            return;
        }
        if state.ai.model().is_none() {
            if state.ai.loading.swap(true, Ordering::AcqRel) {
                return;
            }
            let dir = state.ai.dir.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                let clap = Clap::load(&dir)?;
                let genres: Vec<(String, Vec<f32>)> = GENRES
                    .iter()
                    .filter_map(|g| {
                        clap.embed_text(&format!("This is a {g} song."))
                            .ok()
                            .map(|v| ((*g).to_owned(), v))
                    })
                    .collect();
                Ok::<_, djinn_ai::AiError>((clap, genres))
            })
            .await;
            state.ai.loading.store(false, Ordering::Release);
            match loaded {
                Ok(Ok((clap, genres))) => {
                    tracing::info!("AI model loaded");
                    if let Ok(mut g) = state.ai.genres.write() {
                        *g = genres;
                    }
                    if let Ok(mut m) = state.ai.model.write() {
                        *m = Some(Arc::new(clap));
                    }
                    state.ai.set_error(None);
                    // Heard genres and the description search use it.
                    crate::smart::refresh(&state).await;
                }
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "loading the AI model failed");
                    state.ai.set_error(Some(e.to_string()));
                }
                Err(e) => tracing::error!(error = %e, "loading the AI model failed"),
            }
            broadcast(&state).await;
        }
        listen(&state).await;
    });
}

/// Listens to songs not heard yet, a few at a time.
async fn listen(state: &AppState) {
    if state.ai.listening.swap(true, Ordering::AcqRel) {
        return;
    }
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get() / 4)
        .clamp(1, 2);
    let started = std::time::Instant::now();
    let mut heard = 0usize;
    'rounds: loop {
        let batch = match state.store.tracks_to_hear(models::VERSION, BATCH).await {
            Ok(batch) if !batch.is_empty() => batch,
            Ok(_) => break,
            Err(e) => {
                tracing::error!(error = %e, "reading songs to listen to failed");
                break;
            }
        };
        for chunk in batch.chunks(workers) {
            // Switched off (or the model removed) meanwhile.
            let Some(model) = state.ai.model() else {
                break 'rounds;
            };
            let jobs: Vec<_> = chunk
                .iter()
                .map(|(id, path)| {
                    let (id, path, model) = (*id, PathBuf::from(path), Arc::clone(&model));
                    tokio::task::spawn_blocking(move || {
                        let heard = djinn_engine::decode_mono(
                            &path,
                            djinn_ai::SAMPLE_RATE as u32,
                            MAX_SECS,
                        )
                        .map_err(|e| e.to_string())
                        .and_then(|samples| model.embed_song(&samples).map_err(|e| e.to_string()));
                        (id, heard, path)
                    })
                })
                .collect();
            for job in jobs {
                let Ok((id, result, path)) = job.await else {
                    continue;
                };
                let embedding = match result {
                    Ok(embedding) => embedding,
                    Err(e) => {
                        tracing::debug!(path = %path.display(), error = %e, "cannot listen to song");
                        None
                    }
                };
                if let Err(e) = state
                    .store
                    .save_heard(id, models::VERSION, embedding.as_deref())
                    .await
                {
                    tracing::error!(error = %e, "saving what the AI heard failed");
                }
                heard += 1;
            }
        }
        broadcast(state).await;
    }
    state.ai.listening.store(false, Ordering::Release);
    if heard > 0 {
        tracing::info!(
            songs = heard,
            secs = started.elapsed().as_secs_f32(),
            "AI listened to songs"
        );
        crate::smart::refresh(state).await;
    }
    broadcast(state).await;
}

async fn download(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    if models::installed(&state.ai.dir) {
        return Ok(StatusCode::NO_CONTENT);
    }
    {
        let Ok(mut progress) = state.ai.download.lock() else {
            return Err(anyhow::anyhow!("download state poisoned").into());
        };
        if progress.is_some() {
            return Ok(StatusCode::ACCEPTED);
        }
        *progress = Some((0, models::download_size()));
    }
    state.ai.set_error(None);
    let task_state = state.clone();
    tokio::spawn(async move {
        let result = fetch_all(&task_state).await;
        if let Ok(mut progress) = task_state.ai.download.lock() {
            *progress = None;
        }
        match result {
            Ok(()) => tracing::info!("AI model downloaded"),
            Err(e) => {
                tracing::error!(error = %e, "downloading the AI model failed");
                task_state.ai.set_error(Some(e));
            }
        }
        broadcast(&task_state).await;
    });
    broadcast(&state).await;
    Ok(StatusCode::ACCEPTED)
}

/// Downloads every file, checking sizes and sums, into the model folder.
async fn fetch_all(state: &AppState) -> Result<(), String> {
    let dir = &state.ai.dir;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("DJINN/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut done = 0u64;
    for file in models::FILES {
        let target = dir.join(file.local);
        if tokio::fs::metadata(&target)
            .await
            .is_ok_and(|m| m.len() == file.size)
        {
            done += file.size;
            continue;
        }
        let partial = dir.join(format!("{}.part", file.local));
        let response = client
            .get(models::url(file))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("cannot download {}: {e}", file.local))?;
        let mut out = tokio::fs::File::create(&partial)
            .await
            .map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut written = 0u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("download of {} broke off: {e}", file.local))?;
            hasher.update(&chunk);
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
            written += chunk.len() as u64;
            if let Ok(mut progress) = state.ai.download.lock() {
                *progress = Some((done + written, models::download_size()));
            }
            // About one progress event per megabyte.
            if written % (1 << 20) < chunk.len() as u64 {
                broadcast(state).await;
            }
        }
        out.flush().await.map_err(|e| e.to_string())?;
        drop(out);
        let sum: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if written != file.size || sum != file.sha256 {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err(format!("{} came down damaged; try again", file.local));
        }
        tokio::fs::rename(&partial, &target)
            .await
            .map_err(|e| e.to_string())?;
        done += file.size;
    }
    Ok(())
}

async fn remove_model(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.set_setting(AI_LISTENING, "0").await?;
    if let Ok(mut model) = state.ai.model.write() {
        *model = None;
    }
    if state.ai.dir.exists() {
        tokio::fs::remove_dir_all(&state.ai.dir).await?;
    }
    crate::smart::refresh(&state).await;
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}
