//! The AI reader (admin switch; everyone's searches use it): a small local
//! language model that reads requests the phrase reader can't place
//! ("something for a first dance at a wedding, no rap"). Optional and off
//! until downloaded and switched on. Runs on this computer only.

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
/// Answers remembered (forgotten all at once when full).
const MAX_ANSWERS: usize = 500;
/// How long a request waits for the reader before giving up.
const WAIT: Duration = Duration::from_secs(20);

pub struct ReaderState {
    dir: PathBuf,
    model: RwLock<Option<Arc<tokio::sync::Mutex<Reader>>>>,
    fetch: Fetch,
    loading: AtomicBool,
    /// Requests read so far, by their normalized text.
    answers: Mutex<HashMap<String, Reading>>,
}

impl ReaderState {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            model: RwLock::new(None),
            fetch: Fetch::default(),
            loading: AtomicBool::new(false),
            answers: Mutex::new(HashMap::new()),
        }
    }

    fn model(&self) -> Option<Arc<tokio::sync::Mutex<Reader>>> {
        self.model.read().ok().and_then(|m| m.clone())
    }

    fn set_model(&self, model: Option<Reader>) {
        if let Ok(mut m) = self.model.write() {
            *m = model.map(|r| Arc::new(tokio::sync::Mutex::new(r)));
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
        .route("/ai/reader/model", delete(remove_model))
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
    })
}

async fn enabled(state: &AppState) -> ApiResult<bool> {
    Ok(crate::jukebox::setting(&state.store, AI_READER, 0).await? == 1)
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
        let started = std::time::Instant::now();
        let loaded = tokio::task::spawn_blocking(move || Reader::load(&dir)).await;
        reader.loading.store(false, Ordering::Release);
        match loaded {
            Ok(Ok(model)) => {
                tracing::info!(secs = started.elapsed().as_secs_f32(), "AI reader loaded");
                reader.fetch.set_error(None);
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
    let read = tokio::task::spawn_blocking(move || turn.read(&text)).await;
    let reading = match read {
        Ok(Ok(reading)) => reading,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "the AI reader failed");
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
