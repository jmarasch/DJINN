//! The AI reader (admin switch; everyone's searches use it): a small local
//! language model that reads requests the phrase reader can't place
//! ("something for a first dance at a wedding, no rap"). Optional and off
//! until downloaded and switched on. Runs on this computer only: on the
//! graphics card through the GPU pack when it's installed and works (see
//! `gpu.rs`), else on the processor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, post, put};
use axum::{Json, Router};
use djinn_ai::{models, Reader, Reading};
use serde::{Deserialize, Serialize};

use crate::ai::{broadcast, start_download, Fetch};
use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Setting key: the AI reader is on.
const AI_READER: &str = "ai_reader";
/// Setting key: whose searches it reads: "everyone", "users" (not
/// guests), or "admins".
pub const READER_FOR: &str = "ai_reader_for";
/// Answers remembered (forgotten all at once when full).
const MAX_ANSWERS: usize = 500;
/// How long a request waits for the reader before giving up.
const WAIT: Duration = Duration::from_secs(20);
/// Setting key: use the graphics card when the GPU pack is installed.
const READER_GPU: &str = "ai_reader_gpu";

/// Where the reader runs.
enum Backend {
    Cpu(Box<Reader>),
    Gpu(crate::gpu::Helper),
}

impl Backend {
    fn read(&mut self, query: &str) -> Result<Reading, String> {
        match self {
            Self::Cpu(reader) => reader.read(query).map_err(|e| e.to_string()),
            Self::Gpu(helper) => helper.read(query),
        }
    }

    fn on_gpu(&self) -> bool {
        match self {
            Self::Cpu(reader) => reader.on_gpu(),
            Self::Gpu(_) => true,
        }
    }
}

pub struct ReaderState {
    dir: PathBuf,
    model: RwLock<Option<Arc<tokio::sync::Mutex<Backend>>>>,
    fetch: Fetch,
    loading: AtomicBool,
    /// Loaded onto a GPU.
    on_gpu: AtomicBool,
    /// Requests read so far, by their normalized text.
    answers: Mutex<HashMap<String, Reading>>,
    /// This version's GPU pack.
    gpu_dir: PathBuf,
    gpu_fetch: Fetch,
    /// The graphics card failed this session; the processor took over.
    gpu_failed: AtomicBool,
    /// Why the reader isn't on the graphics card, when it could be.
    gpu_note: Mutex<Option<String>>,
}

impl ReaderState {
    pub fn new(dir: &Path, gpu_dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            model: RwLock::new(None),
            fetch: Fetch::default(),
            loading: AtomicBool::new(false),
            on_gpu: AtomicBool::new(false),
            answers: Mutex::new(HashMap::new()),
            gpu_dir: gpu_dir.to_owned(),
            gpu_fetch: Fetch::default(),
            gpu_failed: AtomicBool::new(false),
            gpu_note: Mutex::new(None),
        }
    }

    fn model(&self) -> Option<Arc<tokio::sync::Mutex<Backend>>> {
        self.model.read().ok().and_then(|m| m.clone())
    }

    fn set_model(&self, model: Option<Backend>) {
        if let Ok(mut m) = self.model.write() {
            *m = model.map(|r| Arc::new(tokio::sync::Mutex::new(r)));
        }
    }

    fn set_gpu_note(&self, note: Option<String>) {
        if let Ok(mut n) = self.gpu_note.lock() {
            *n = note;
        }
    }

    /// Loaded and ready to read.
    pub fn ready(&self) -> bool {
        self.model().is_some()
    }

    /// What `query` was read as before, if it was.
    pub fn cached(&self, query: &str) -> Option<Reading> {
        let key = djinn_library::vibe::normalize(query);
        self.answers.lock().ok()?.get(&key).cloned()
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai/reader", put(set_enabled))
        .route("/ai/reader/download", post(download))
        .route("/ai/reader/download/cancel", post(cancel_download))
        .route("/ai/reader/model", delete(remove_model))
        .route("/ai/reader/gpu", put(set_gpu).delete(remove_gpu))
        .route("/ai/reader/gpu/download", post(download_gpu))
        .route("/ai/reader/gpu/download/cancel", post(cancel_gpu_download))
}

#[derive(Clone, Debug, Serialize)]
pub struct ReaderStatus {
    installed: bool,
    enabled: bool,
    ready: bool,
    download_size: u64,
    downloading: Option<(u64, u64)>,
    error: Option<String>,
    /// Built to use a GPU when there is one.
    gpu_build: bool,
    /// Loaded onto a GPU.
    on_gpu: bool,
    /// An NVIDIA graphics driver is installed, so the GPU pack can help.
    nvidia: bool,
    gpu_installed: bool,
    /// Use the graphics card when the pack is installed.
    gpu_enabled: bool,
    /// Rough download size of the pack.
    gpu_pack_size: u64,
    gpu_downloading: Option<(u64, u64)>,
    gpu_error: Option<String>,
    /// Why the reader runs on the processor although it could use the
    /// graphics card.
    gpu_note: Option<String>,
}

pub async fn status(state: &AppState) -> ApiResult<ReaderStatus> {
    let reader = &state.ai.reader;
    Ok(ReaderStatus {
        installed: models::READER.installed(&reader.dir),
        enabled: enabled(state).await?,
        ready: reader.ready(),
        download_size: models::READER.download_size(),
        downloading: reader.fetch.progress(),
        error: reader.fetch.error(),
        gpu_build: cfg!(any(feature = "cuda", feature = "metal")),
        on_gpu: reader.ready() && reader.on_gpu.load(Ordering::Acquire),
        nvidia: crate::gpu::nvidia_present(),
        gpu_installed: crate::gpu::installed(&reader.gpu_dir),
        gpu_enabled: gpu_enabled(state).await?,
        gpu_pack_size: crate::gpu::PACK_ESTIMATE,
        gpu_downloading: reader.gpu_fetch.progress(),
        gpu_error: reader.gpu_fetch.error(),
        gpu_note: reader.gpu_note.lock().ok().and_then(|n| n.clone()),
    })
}

/// True if the reader may read searches for `user` (see [`READER_FOR`]).
pub async fn allowed_for(state: &AppState, user: &djinn_store::User) -> bool {
    use djinn_store::Role;
    let setting = state.store.get_setting(READER_FOR).await.ok().flatten();
    match setting.as_deref() {
        Some("admins") => user.role == Role::Admin,
        Some("users") => user.role != Role::Guest,
        _ => true,
    }
}

async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(crate::jukebox::setting(&state.store, AI_READER, 0).await? == 1)
}

async fn gpu_enabled(state: &AppState) -> ApiResult<bool> {
    Ok(crate::jukebox::setting(&state.store, READER_GPU, 1).await? == 1)
}

/// Loads the reader again (after a change to where it should run).
fn reload(state: &AppState) {
    state.ai.reader.set_model(None);
    state.ai.reader.gpu_failed.store(false, Ordering::Release);
    start(state);
}

async fn set_gpu(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Enabled>,
) -> ApiResult<StatusCode> {
    state
        .store
        .set_setting(READER_GPU, if body.enabled { "1" } else { "0" })
        .await?;
    reload(&state);
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn download_gpu(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let reader = &state.ai.reader;
    if crate::gpu::installed(&reader.gpu_dir) {
        return Ok(StatusCode::NO_CONTENT);
    }
    if !reader.gpu_fetch.begin(crate::gpu::PACK_ESTIMATE) {
        return Ok(StatusCode::ACCEPTED);
    }
    let task_state = state.clone();
    tokio::spawn(async move {
        let reader = &task_state.ai.reader;
        let progress_state = task_state.clone();
        let result = crate::gpu::download(&reader.gpu_dir, &reader.gpu_fetch, move || {
            let state = progress_state.clone();
            Box::pin(async move { broadcast(&state).await })
        })
        .await;
        reader.gpu_fetch.set_progress(None);
        if result.is_err() {
            // Nothing half done stays behind.
            let _ = tokio::fs::remove_dir_all(&reader.gpu_dir).await;
        }
        match result {
            Ok(()) => {
                tracing::info!("GPU pack installed");
                reload(&task_state);
            }
            Err(_) if reader.gpu_fetch.cancelled() => tracing::info!("GPU pack download stopped"),
            Err(e) => {
                tracing::warn!(error = %e, "GPU pack download failed");
                reader.gpu_fetch.set_error(Some(e));
            }
        }
        broadcast(&task_state).await;
    });
    broadcast(&state).await;
    Ok(StatusCode::ACCEPTED)
}

async fn cancel_gpu_download(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.ai.reader.gpu_fetch.cancel();
    StatusCode::NO_CONTENT
}

async fn remove_gpu(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let reader = &state.ai.reader;
    // Stop the helper before deleting what it runs from.
    reader.set_model(None);
    if reader.gpu_dir.exists() {
        tokio::fs::remove_dir_all(&reader.gpu_dir).await?;
    }
    reader.set_gpu_note(None);
    reload(&state);
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

async fn set_enabled(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Enabled>,
) -> ApiResult<StatusCode> {
    if body.enabled && !models::READER.installed(&state.ai.reader.dir) {
        return Err(ApiError::bad_request("download the model first"));
    }
    state
        .store
        .set_setting(AI_READER, if body.enabled { "1" } else { "0" })
        .await?;
    if body.enabled {
        start(&state);
    } else {
        // Frees the memory once a request in progress is done.
        state.ai.reader.set_model(None);
    }
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn download(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    let dir = state.ai.reader.dir.clone();
    start_download(&state, &models::READER, dir, |ai| &ai.reader.fetch)
}

async fn cancel_download(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    state.ai.reader.fetch.cancel();
    StatusCode::NO_CONTENT
}

async fn remove_model(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.set_setting(AI_READER, "0").await?;
    state.ai.reader.set_model(None);
    let dir = &state.ai.reader.dir;
    if dir.exists() {
        tokio::fs::remove_dir_all(dir).await?;
    }
    broadcast(&state).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Loads the reader if it's on. Called at startup and when switched on.
pub fn start(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let reader = &state.ai.reader;
        if !enabled(&state).await.unwrap_or(false)
            || !models::READER.installed(&reader.dir)
            || reader.ready()
            || reader.loading.swap(true, Ordering::AcqRel)
        {
            return;
        }
        broadcast(&state).await;
        let dir = reader.dir.clone();
        let gpu_dir = reader.gpu_dir.clone();
        // The graphics card first, if the pack is there and it's allowed.
        let try_gpu = gpu_enabled(&state).await.unwrap_or(true)
            && crate::gpu::installed(&gpu_dir)
            && crate::gpu::nvidia_present()
            && !reader.gpu_failed.load(Ordering::Acquire);
        let started = std::time::Instant::now();
        let loaded = tokio::task::spawn_blocking(move || {
            let mut note = None;
            if try_gpu {
                match crate::gpu::Helper::start(&gpu_dir, &dir) {
                    Ok(helper) => return Ok((Backend::Gpu(helper), None)),
                    Err(e) => note = Some(e),
                }
            }
            Reader::load(&dir).map(|r| (Backend::Cpu(Box::new(r)), note))
        })
        .await;
        reader.loading.store(false, Ordering::Release);
        match loaded {
            Ok(Ok((model, note))) => {
                tracing::info!(
                    secs = started.elapsed().as_secs_f32(),
                    gpu = model.on_gpu(),
                    "AI reader loaded"
                );
                if let Some(note) = &note {
                    tracing::warn!(reason = %note, "the AI reader is on the processor instead of the graphics card");
                    reader.gpu_failed.store(true, Ordering::Release);
                }
                // A note from a failure earlier this session stays.
                if try_gpu {
                    reader.set_gpu_note(note);
                }
                reader.fetch.set_error(None);
                reader.on_gpu.store(model.on_gpu(), Ordering::Release);
                // Switched off while loading: drop it.
                if enabled(&state).await.unwrap_or(false) {
                    reader.set_model(Some(model));
                }
            }
            Ok(Err(e)) => {
                tracing::error!(error = %e, "loading the AI reader failed");
                reader.fetch.set_error(Some(e.to_string()));
            }
            Err(e) => tracing::error!(error = %e, "loading the AI reader failed"),
        }
        broadcast(&state).await;
    });
}

/// Reads `query` with the model (or remembers it). `None` if the reader
/// is off, busy for too long, or failed.
pub async fn read(state: &AppState, query: &str) -> Option<Reading> {
    let reader = &state.ai.reader;
    if let Some(reading) = reader.cached(query) {
        return Some(reading);
    }
    let model = reader.model()?;
    let mut turn = tokio::time::timeout(WAIT, model.lock_owned()).await.ok()?;
    // Someone asked the same while this one waited.
    if let Some(reading) = reader.cached(query) {
        return Some(reading);
    }
    let text = query.to_owned();
    let started = std::time::Instant::now();
    let read = tokio::task::spawn_blocking(move || {
        let on_gpu = turn.on_gpu();
        (turn.read(&text), on_gpu)
    })
    .await;
    let reading = match read {
        Ok((Ok(reading), _)) => reading,
        Ok((Err(e), on_gpu)) => {
            tracing::warn!(error = %e, "the AI reader failed");
            if on_gpu {
                // The graphics card let us down: carry on on the processor.
                reader.gpu_failed.store(true, Ordering::Release);
                reader.set_gpu_note(Some(e));
                reader.set_model(None);
                start(state);
            }
            return None;
        }
        Err(_) => return None,
    };
    tracing::debug!(
        query,
        secs = started.elapsed().as_secs_f32(),
        ?reading,
        "AI reader read a request"
    );
    if let Ok(mut answers) = reader.answers.lock() {
        if answers.len() >= MAX_ANSWERS {
            answers.clear();
        }
        answers.insert(djinn_library::vibe::normalize(query), reading.clone());
    }
    Some(reading)
}
