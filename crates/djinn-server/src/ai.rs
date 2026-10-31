//! Optional AI listening (admin): download the CLAP model, then let it
//! listen to every song in the background, so search can match
//! descriptions against how songs sound. Runs on this computer only; the
//! music folders are never touched.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use djinn_ai::{models, Clap};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::download::CheckedFile;
use crate::error::{ApiError, ApiResult};
use crate::events::ServerEvent;
use crate::settings;
use crate::state::AppState;

/// Setting key: AI listening is on.
const AI_LISTENING: &str = "ai_listening";
/// Setting key: how many songs are listened to at once: "gentle" (one),
/// "normal", or "fast" (half the processor cores).
const LISTEN_SPEED: &str = "ai_listen_speed";
/// Setting key: songs with no genres get the ones the AI hears.
pub const GENRE_TAGS: &str = "ai_genre_tags";
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

/// A model's download progress and last error.
#[derive(Default)]
pub struct Fetch {
    /// (done, total) bytes while downloading.
    progress: Mutex<Option<(u64, u64)>>,
    error: Mutex<Option<String>>,
    /// The admin stopped the download.
    cancel: AtomicBool,
}

impl Fetch {
    pub fn progress(&self) -> Option<(u64, u64)> {
        self.progress.lock().ok().and_then(|p| *p)
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    /// Stops a download in progress.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Marks a download as starting; false if one is already going.
    pub fn begin(&self, total: u64) -> bool {
        let Ok(mut progress) = self.progress.lock() else {
            return false;
        };
        if progress.is_some() {
            return false;
        }
        *progress = Some((0, total));
        self.cancel.store(false, Ordering::Release);
        drop(progress);
        self.set_error(None);
        true
    }

    pub fn set_progress(&self, progress: Option<(u64, u64)>) {
        if let Ok(mut p) = self.progress.lock() {
            *p = progress;
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    pub fn set_error(&self, error: Option<String>) {
        if let Ok(mut e) = self.error.lock() {
            *e = error;
        }
    }
}

/// Model, progress, and switches, shared by everything.
pub struct AiState {
    dir: PathBuf,
    model: RwLock<Option<Arc<Clap>>>,
    /// Each genre's text embedding, made once the model loads.
    genres: RwLock<Vec<(String, Vec<f32>)>>,
    fetch: Fetch,
    loading: AtomicBool,
    listening: AtomicBool,
    /// The admin stopped listening for now.
    stop_listening: AtomicBool,
    /// The AI reader for free-form requests.
    pub reader: crate::reader::ReaderState,
}

impl AiState {
    /// `models_dir` is DJINN's models folder (next to the database).
    pub fn new(models_dir: &Path) -> Self {
        Self {
            dir: models_dir.join(models::CLAP.folder),
            model: RwLock::new(None),
            genres: RwLock::new(Vec::new()),
            fetch: Fetch::default(),
            loading: AtomicBool::new(false),
            listening: AtomicBool::new(false),
            stop_listening: AtomicBool::new(false),
            reader: crate::reader::ReaderState::new(
                &models_dir.join(models::READER.folder),
                &crate::gpu::pack_dir(models_dir),
            ),
        }
    }

    pub fn model(&self) -> Option<Arc<Clap>> {
        self.model.read().ok().and_then(|m| m.clone())
    }

    /// Each genre's embedding, for tagging songs.
    pub fn genre_embeddings(&self) -> Vec<(String, Vec<f32>)> {
        self.genres.read().map(|g| g.clone()).unwrap_or_default()
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai", get(status).put(set_enabled))
        .route("/ai/download", post(download))
        .route("/ai/download/cancel", post(cancel_download))
        .route("/ai/model", delete(remove_model))
        .route("/ai/listen/stop", post(stop_listening))
        .route("/ai/listen/start", post(continue_listening))
        .route("/ai/options", put(set_options))
        .merge(crate::reader::router())
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
    reader: crate::reader::ReaderStatus,
    options: AiOptions,
    system: SystemInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiOptions {
    /// "gentle", "normal", or "fast".
    listen_speed: String,
    genre_tags: bool,
    /// Who the AI reader reads searches for: "everyone", "users" (not
    /// guests), or "admins".
    reader_for: String,
}

/// What this computer has, to compare with what the models need.
#[derive(Clone, Debug, Serialize)]
pub struct SystemInfo {
    cores: usize,
    /// Bytes; `None` where DJINN can't tell.
    memory_total: Option<u64>,
    memory_available: Option<u64>,
}

fn system_info() -> SystemInfo {
    let memory = djinn_platform::system::memory();
    SystemInfo {
        cores: crate::cpu::threads(),
        memory_total: memory.map(|m| m.total),
        memory_available: memory.map(|m| m.available),
    }
}

pub async fn options(state: &AppState) -> ApiResult<AiOptions> {
    let text = |key: &'static str, default: &'static str| async move {
        Ok::<_, ApiError>(
            state
                .store
                .get_setting(key)
                .await?
                .unwrap_or_else(|| default.to_owned()),
        )
    };
    Ok(AiOptions {
        listen_speed: text(LISTEN_SPEED, "normal").await?,
        genre_tags: settings::flag(&state.store, GENRE_TAGS, true).await?,
        reader_for: text(crate::reader::READER_FOR, "everyone").await?,
    })
}

async fn set_options(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<AiOptions>,
) -> ApiResult<StatusCode> {
    if !["gentle", "normal", "fast"].contains(&body.listen_speed.as_str()) {
        return Err(ApiError::bad_request("unknown listening speed"));
    }
    if !["everyone", "users", "admins"].contains(&body.reader_for.as_str()) {
        return Err(ApiError::bad_request("unknown choice for the AI reader"));
    }
    let tags_changed = options(&state).await?.genre_tags != body.genre_tags;
    state
        .store
        .set_setting(LISTEN_SPEED, &body.listen_speed)
        .await?;
    settings::set_flag(&state.store, GENRE_TAGS, body.genre_tags).await?;
    state
        .store
        .set_setting(crate::reader::READER_FOR, &body.reader_for)
        .await?;
    if tags_changed {
        crate::smart::refresh(&state).await;
    }
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ai_status(state: &AppState) -> ApiResult<AiStatus> {
    let ai = &state.ai;
    let (heard, waiting) = state.store.heard_progress(models::VERSION).await?;
    Ok(AiStatus {
        installed: models::CLAP.installed(&ai.dir),
        enabled: enabled(state).await?,
        ready: ai.model().is_some(),
        download_size: models::CLAP.download_size(),
        downloading: ai.fetch.progress(),
        error: ai.fetch.error(),
        heard,
        waiting,
        listening: ai.listening.load(Ordering::Acquire),
        reader: crate::reader::status(state).await?,
        options: options(state).await?,
        system: system_info(),
    })
}

/// AI listening, for the library's progress bar.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Listening {
    /// Switched on with the model installed.
    on: bool,
    running: bool,
    heard: i64,
    waiting: i64,
}

pub async fn listening(state: &AppState) -> ApiResult<Listening> {
    let (heard, waiting) = state.store.heard_progress(models::VERSION).await?;
    Ok(Listening {
        on: enabled(state).await? && models::CLAP.installed(&state.ai.dir),
        running: state.ai.listening.load(Ordering::Acquire),
        heard,
        waiting,
    })
}

async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(settings::flag(&state.store, AI_LISTENING, false).await?)
}

pub async fn broadcast(state: &AppState) {
    if let Ok(status) = ai_status(state).await {
        let _ = state.events.send(ServerEvent::AiStatus(Box::new(status)));
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
    if body.enabled && !models::CLAP.installed(&state.ai.dir) {
        return Err(ApiError::bad_request("download the model first"));
    }
    settings::set_flag(&state.store, AI_LISTENING, body.enabled).await?;
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
        if !enabled(&state).await.unwrap_or(false) || !models::CLAP.installed(&state.ai.dir) {
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
                    state.ai.fetch.set_error(None);
                    // Heard genres and the description search use it.
                    crate::smart::refresh(&state).await;
                }
                Ok(Err(e)) => {
                    tracing::error!(error = %e, "loading the AI model failed");
                    state.ai.fetch.set_error(Some(e.to_string()));
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
    // One at a time: both decode every song. The analysis starts listening
    // when it finishes.
    if state.scan.loudness_running.load(Ordering::Acquire) {
        return;
    }
    if state.ai.listening.swap(true, Ordering::AcqRel) {
        return;
    }
    state.ai.stop_listening.store(false, Ordering::Release);
    let cores = crate::cpu::threads();
    let speed = state.store.get_setting(LISTEN_SPEED).await.ok().flatten();
    let workers = match speed.as_deref() {
        Some("gentle") => 1,
        Some("fast") => (cores / 2).clamp(1, 8),
        _ => (cores / 4).clamp(1, 2),
    };
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
            // Switched off, stopped, or the model removed meanwhile.
            let Some(model) = state.ai.model() else {
                break 'rounds;
            };
            if state.ai.stop_listening.load(Ordering::Acquire) {
                break 'rounds;
            }
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

/// Stops listening for now; it carries on when DJINN starts again, new
/// music arrives, or the admin continues.
async fn stop_listening(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.ai.stop_listening.store(true, Ordering::Release);
    StatusCode::NO_CONTENT
}

async fn continue_listening(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    start(&state);
    StatusCode::NO_CONTENT
}

async fn cancel_download(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.ai.fetch.cancel();
    StatusCode::NO_CONTENT
}

async fn download(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let dir = state.ai.dir.clone();
    start_download(&state, &models::CLAP, dir, |ai| &ai.fetch)
}

/// Downloads `model` into `dir` in the background, unless it's there or
/// already coming down. `fetch` picks where its progress goes.
pub fn start_download(
    state: &AppState,
    model: &'static models::Model,
    dir: PathBuf,
    fetch: fn(&AiState) -> &Fetch,
) -> ApiResult<StatusCode> {
    if model.installed(&dir) {
        return Ok(StatusCode::NO_CONTENT);
    }
    {
        let Ok(mut progress) = fetch(&state.ai).progress.lock() else {
            return Err(anyhow::anyhow!("download state poisoned").into());
        };
        if progress.is_some() {
            return Ok(StatusCode::ACCEPTED);
        }
        *progress = Some((0, model.download_size()));
    }
    fetch(&state.ai).set_error(None);
    fetch(&state.ai).cancel.store(false, Ordering::Release);
    let task_state = state.clone();
    tokio::spawn(async move {
        let result = fetch_all(&task_state, model, &dir, fetch).await;
        let progress = fetch(&task_state.ai);
        if let Ok(mut p) = progress.progress.lock() {
            *p = None;
        }
        match result {
            Ok(()) => tracing::info!(model = model.folder, "AI model downloaded"),
            Err(_) if progress.cancel.load(Ordering::Acquire) => {
                tracing::info!(model = model.folder, "AI model download stopped");
            }
            Err(e) => {
                tracing::error!(model = model.folder, error = %e, "downloading an AI model failed");
                progress.set_error(Some(e));
            }
        }
        broadcast(&task_state).await;
    });
    let state = state.clone();
    tokio::spawn(async move { broadcast(&state).await });
    Ok(StatusCode::ACCEPTED)
}

/// Downloads every file, checking sizes and sums, into `dir`.
async fn fetch_all(
    state: &AppState,
    model: &models::Model,
    dir: &Path,
    fetch: fn(&AiState) -> &Fetch,
) -> Result<(), String> {
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("DJINN/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let total = model.download_size();
    let mut done = 0u64;
    for file in model.files {
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
            .get(file.url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("cannot download {}: {e}", file.local))?;
        let mut out = CheckedFile::create(&partial)
            .await
            .map_err(|e| e.to_string())?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            if fetch(&state.ai).cancel.load(Ordering::Acquire) {
                out.discard().await;
                return Err("stopped".into());
            }
            let chunk = chunk.map_err(|e| format!("download of {} broke off: {e}", file.local))?;
            let written = out.write(&chunk).await?;
            if let Ok(mut progress) = fetch(&state.ai).progress.lock() {
                *progress = Some((done + written, total));
            }
            // About one progress event per megabyte.
            if written % (1 << 20) < chunk.len() as u64 {
                broadcast(state).await;
            }
        }
        out.finish(file.size, file.sha256, file.local).await?;
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
