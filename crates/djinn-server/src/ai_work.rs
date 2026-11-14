//! Work for AI workers: computers running the lyrics listener (local models,
//! nothing leaves the network) that make lyrics for songs that have none and
//! time plain lyrics. DJINN keeps the queue and hands out batches, but only
//! when background work may run (no music playing, or the hours the admin
//! set); the worker pulls a batch, streams each song, and posts back the
//! `.lrc` it made, which is stored like an import with the AI's mark.
//! Workers sign in with a key an admin makes (only its hash is kept), and
//! only from the home network.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::body::Body;
use axum::extract::{FromRequestParts, Path as UrlPath, Request, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use djinn_store::AiCounts;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::auth::{AdminUser, Device};
use crate::error::{ApiError, ApiResult};
use crate::lyrics_import::store_from_worker;
use crate::nodes::NodeAuth;
use crate::security::{hex, random_hex, same_secret};
use crate::settings;
use crate::state::AppState;

/// Setting keys.
pub const WORK_ON: &str = "ai_work_on";
const WORK_NEW: &str = "ai_work_new";
const WORK_SYNC: &str = "ai_work_sync";
const KEYS: &str = "ai_worker_keys";
const BATCH_MAX: usize = 10;
/// A song held this long by a worker that never answers goes back to waiting.
const LEASE_MS: i64 = 30 * 60 * 1000;
/// Seconds a worker is told to wait when there's nothing to do or work is held.
const WAIT_SECS: u64 = 120;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ai-work", get(status))
        .route("/ai-work/settings", axum::routing::put(put_settings))
        .route("/ai-work/keys", post(make_key))
        .route("/ai-work/keys/{name}", delete(remove_key))
        .route("/ai-work/retry", post(retry))
        .route("/ai-work/next", post(next))
        .route("/ai-work/song/{id}", get(song))
        .route("/ai-work/result/{id}", post(result))
        .route(
            "/ai-work/vocals/{id}",
            post(vocals).layer(axum::extract::DefaultBodyLimit::max(VOCALS_MAX_BYTES)),
        )
        .route("/nodes/ai/next", post(node_next))
        .route("/nodes/ai/song/{id}", get(node_song))
        .route("/nodes/ai/result/{id}", post(node_result))
        .route("/nodes/ai/kit", get(node_kit))
        .route("/nodes/ai/report", axum::routing::put(node_report))
}

// --- Worker keys ---

#[derive(Clone, Serialize, Deserialize)]
struct Key {
    name: String,
    /// SHA-256 of the key, hex.
    hash: String,
}

async fn keys(state: &AppState) -> anyhow::Result<Vec<Key>> {
    Ok(match state.store.get_setting(KEYS).await? {
        Some(json) => serde_json::from_str(&json)?,
        None => Vec::new(),
    })
}

async fn save_keys(state: &AppState, keys: &[Key]) -> anyhow::Result<()> {
    state
        .store
        .set_setting(KEYS, &serde_json::to_string(keys)?)
        .await?;
    Ok(())
}

fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// Whether `key` is the live key of the worker called `name`.
pub async fn key_is_valid(state: &AppState, name: &str, key: &str) -> anyhow::Result<bool> {
    let hash = hash_key(key);
    Ok(keys(state)
        .await?
        .iter()
        .any(|k| k.name == name && same_secret(k.hash.as_bytes(), hash.as_bytes())))
}

/// Makes a new key for `name` (replacing an old one) and returns it.
pub async fn replace_key(state: &AppState, name: &str) -> anyhow::Result<String> {
    let mut all = keys(state).await?;
    all.retain(|k| k.name != name);
    let key = random_hex(24)?;
    all.push(Key {
        name: name.to_owned(),
        hash: hash_key(&key),
    });
    save_keys(state, &all).await?;
    Ok(key)
}

/// A worker that signed in with a key: `Authorization: Worker <key>`, from
/// the home network.
pub struct Worker(pub String);

impl FromRequestParts<AppState> for Worker {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let Ok(device) =
            <Device as FromRequestParts<AppState>>::from_request_parts(parts, state).await;
        if !device.home {
            return Err(ApiError::forbidden());
        }
        let key = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Worker "))
            .ok_or_else(ApiError::unauthorized)?;
        let hash = hash_key(key.trim());
        keys(state)
            .await?
            .into_iter()
            .find(|k| same_secret(k.hash.as_bytes(), hash.as_bytes()))
            .map(|k| Worker(k.name))
            .ok_or_else(ApiError::unauthorized)
    }
}

// --- What workers have done, for the status page ---

#[derive(Clone, Default, Serialize)]
struct Seen {
    /// The name to show (a node's own name).
    label: String,
    node: bool,
    last_seen: i64,
    /// When it last asked for work while free to do it (a node that's
    /// playing doesn't ask).
    #[serde(skip)]
    asked_at: i64,
    done: u64,
    failed: u64,
    /// Time spent listening and the length of the songs it listened to,
    /// for its speed.
    #[serde(skip)]
    work_ms: u64,
    #[serde(skip)]
    audio_ms: u64,
    /// Seconds of work per minute of song, once it has done some.
    secs_per_minute: Option<f64>,
}

static WORKERS: Mutex<Option<HashMap<String, Seen>>> = Mutex::new(None);

fn seen(name: &str, f: impl FnOnce(&mut Seen)) {
    if let Ok(mut guard) = WORKERS.lock() {
        let entry = guard
            .get_or_insert_with(HashMap::new)
            .entry(name.to_owned())
            .or_insert_with(|| Seen {
                label: name.to_owned(),
                ..Seen::default()
            });
        entry.last_seen = djinn_store::now_ms();
        f(entry);
    }
}

/// A worker's measured speed, in seconds of work per minute of song.
pub fn speed_of(name: &str) -> Option<f64> {
    WORKERS.lock().ok()?.as_ref()?.get(name)?.secs_per_minute
}

/// Nodes that asked for work lately, while free to do it.
fn helping_nodes() -> usize {
    let cut = djinn_store::now_ms() - HELPER_FRESH_MS;
    WORKERS
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .map(|m| m.values().filter(|s| s.node && s.asked_at > cut).count())
        })
        .unwrap_or(0)
}

/// A node that asked within this long counts as helping.
const HELPER_FRESH_MS: i64 = 10 * 60 * 1000;
/// Songs with isolated vocals kept waiting per helping node, and at most.
const PREPARED_PER_NODE: i64 = 2;
const PREPARED_MAX: i64 = 8;
/// The largest isolated vocals file taken (16 kHz mono FLAC is a few MB).
const VOCALS_MAX_BYTES: usize = 40 << 20;
/// Setting key: Main isolates vocals for nodes when it can.
pub const VOCALS_FOR_NODES: &str = "ai_vocals_for_nodes";

// --- Admin ---

#[derive(Serialize)]
struct Status {
    on: bool,
    new_songs: bool,
    sync_plain: bool,
    vocals_for_nodes: bool,
    counts: AiCounts,
    /// Why handing out work is paused (music playing, outside the hours).
    held: Option<crate::background::Held>,
    keys: Vec<String>,
    workers: Vec<WorkerView>,
}

#[derive(Serialize)]
struct WorkerView {
    name: String,
    #[serde(flatten)]
    seen: Seen,
}

async fn lookups_off(state: &AppState) -> anyhow::Result<bool> {
    Ok(!settings::flag(&state.store, crate::lyrics::LYRICS_LOOKUP, true).await?)
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    let mut workers: Vec<WorkerView> = WORKERS
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|(name, seen)| WorkerView { name, seen })
        .collect();
    workers.sort_by_key(|w| std::cmp::Reverse(w.seen.last_seen));
    Ok(Json(Status {
        on: settings::flag(&state.store, WORK_ON, false).await?,
        new_songs: settings::flag(&state.store, WORK_NEW, true).await?,
        sync_plain: settings::flag(&state.store, WORK_SYNC, false).await?,
        vocals_for_nodes: settings::flag(&state.store, VOCALS_FOR_NODES, true).await?,
        counts: state
            .store
            .ai_counts(lookups_off(&state).await?, LEASE_MS)
            .await?,
        held: crate::background::held(&state).await,
        keys: keys(&state).await?.into_iter().map(|k| k.name).collect(),
        workers,
    }))
}

#[derive(Deserialize)]
struct Settings {
    on: bool,
    new_songs: bool,
    sync_plain: bool,
    #[serde(default = "yes")]
    vocals_for_nodes: bool,
}

fn yes() -> bool {
    true
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Settings>,
) -> ApiResult<StatusCode> {
    settings::set_flag(&state.store, WORK_ON, body.on).await?;
    settings::set_flag(&state.store, WORK_NEW, body.new_songs).await?;
    settings::set_flag(&state.store, WORK_SYNC, body.sync_plain).await?;
    settings::set_flag(&state.store, VOCALS_FOR_NODES, body.vocals_for_nodes).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct NewKey {
    name: String,
}

#[derive(Serialize)]
struct MadeKey {
    name: String,
    /// Shown once; only its hash is kept.
    key: String,
}

async fn make_key(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<NewKey>,
) -> ApiResult<Json<MadeKey>> {
    let name: String = body.name.trim().chars().take(40).collect();
    if name.is_empty() {
        return Err(ApiError::bad_request("give the worker a name"));
    }
    if name.starts_with("node:") {
        return Err(ApiError::bad_request("that name is kept for nodes"));
    }
    let mut all = keys(&state).await?;
    if all.iter().any(|k| k.name == name) {
        return Err(ApiError::conflict("a worker with that name exists"));
    }
    let key = random_hex(24)?;
    all.push(Key {
        name: name.clone(),
        hash: hash_key(&key),
    });
    save_keys(&state, &all).await?;
    Ok(Json(MadeKey { name, key }))
}

async fn remove_key(
    _: AdminUser,
    State(state): State<AppState>,
    UrlPath(name): UrlPath<String>,
) -> ApiResult<StatusCode> {
    let mut all = keys(&state).await?;
    all.retain(|k| k.name != name);
    save_keys(&state, &all).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn retry(_: AdminUser, State(state): State<AppState>) -> ApiResult<StatusCode> {
    state.store.ai_retry_failed().await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Workers ---

#[derive(Deserialize)]
struct Next {
    /// Songs wanted at once.
    #[serde(default)]
    max: usize,
    /// This worker has vocal isolation (Demucs) and may isolate vocals for
    /// nodes.
    #[serde(default)]
    can_isolate: bool,
}

/// A song for a worker, and what to do with it.
#[derive(Serialize)]
struct Job {
    #[serde(flatten)]
    job: djinn_store::AiJob,
    /// Only isolate the vocals and send them back (`/ai-work/vocals/{id}`),
    /// for a node to listen to.
    prepare: bool,
    /// The song's audio is its isolated vocals already: listen as it is.
    vocals: bool,
}

#[derive(Serialize)]
struct Batch {
    jobs: Vec<Job>,
    /// Nothing handed out: ask again after this many seconds.
    wait_secs: Option<u64>,
    /// Why, when it's paused.
    held: Option<crate::background::Held>,
}

fn vocals_dir(state: &AppState) -> std::path::PathBuf {
    crate::ai_listener::dir(state).join("vocals")
}

fn vocals_file(state: &AppState, id: i64) -> std::path::PathBuf {
    vocals_dir(state).join(format!("{id}.flac"))
}

/// Removes isolated vocals no job wants any more.
async fn sweep_vocals(state: &AppState) -> anyhow::Result<()> {
    let Ok(mut entries) = tokio::fs::read_dir(vocals_dir(state)).await else {
        return Ok(());
    };
    let open: std::collections::HashSet<i64> =
        state.store.ai_open_jobs().await?.into_iter().collect();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name
            .strip_suffix(".flac")
            .and_then(|n| n.parse::<i64>().ok());
        if id.is_none_or(|id| !open.contains(&id)) {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    Ok(())
}

/// Who is asking for work.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asker {
    /// A computer with a worker key (this one's own listener among them).
    Keyed,
    /// A node: its own rules (not playing, no visuals) decided it may ask.
    Node,
}

async fn hand_out(state: &AppState, name: &str, asker: Asker, body: Next) -> ApiResult<Batch> {
    seen(name, |s| {
        s.node = asker == Asker::Node;
        if asker == Asker::Node {
            s.asked_at = djinn_store::now_ms();
        }
    });
    let nothing = |held| Batch {
        jobs: Vec::new(),
        wait_secs: Some(WAIT_SECS),
        held,
    };
    if !settings::flag(&state.store, WORK_ON, false).await? {
        return Ok(nothing(None));
    }
    // A node keeps its own rules (it asks only while it isn't playing or
    // showing visuals); Main's rules are for Main's own work.
    if asker == Asker::Keyed {
        if let Some(held) = crate::background::held(state).await {
            return Ok(nothing(Some(held)));
        }
    }
    let max = i64::try_from(body.max.clamp(1, BATCH_MAX)).unwrap_or(1);
    let listen = settings::flag(&state.store, WORK_NEW, true).await?;
    let sync = settings::flag(&state.store, WORK_SYNC, false).await?;
    let lookups_off = lookups_off(state).await?;
    let mut prepare = false;
    let jobs = if asker == Asker::Node {
        let mut jobs = state.store.ai_claim_prepared(name, max).await?;
        if jobs.is_empty() {
            jobs = state
                .store
                .ai_claim(name, listen, sync, lookups_off, max, LEASE_MS)
                .await?;
        }
        jobs
    } else {
        // This computer's own listener isolates vocals for helping nodes
        // first, keeping a few songs ready for each.
        if name == crate::ai_listener::LOCAL_KEY_NAME && body.can_isolate {
            sweep_vocals(state).await?;
            let nodes = i64::try_from(helping_nodes()).unwrap_or(0);
            prepare = nodes > 0
                && settings::flag(&state.store, VOCALS_FOR_NODES, true).await?
                && state.store.ai_prepared_count().await?
                    < (nodes * PREPARED_PER_NODE).min(PREPARED_MAX);
        }
        state
            .store
            .ai_claim(
                name,
                listen,
                sync,
                lookups_off,
                if prepare { 1 } else { max },
                LEASE_MS,
            )
            .await?
    };
    if jobs.is_empty() {
        return Ok(nothing(None));
    }
    let jobs = jobs
        .into_iter()
        .map(|job| {
            let vocals = !prepare && vocals_file(state, job.track_id).is_file();
            Job {
                job,
                prepare,
                vocals,
            }
        })
        .collect();
    Ok(Batch {
        jobs,
        wait_secs: None,
        held: None,
    })
}

async fn next(
    Worker(name): Worker,
    State(state): State<AppState>,
    Json(body): Json<Next>,
) -> ApiResult<Json<Batch>> {
    Ok(Json(hand_out(&state, &name, Asker::Keyed, body).await?))
}

/// A song's audio (or its isolated vocals), only to the worker that holds it.
async fn send_song(state: &AppState, name: &str, id: i64, request: Request) -> ApiResult<Response> {
    match state.store.ai_holder(id).await? {
        Some((holder, _)) if holder == name => {}
        _ => return Err(ApiError::forbidden()),
    }
    let vocals = vocals_file(state, id);
    let path = if vocals.is_file() {
        vocals
    } else {
        state
            .store
            .track_path(id)
            .await?
            .ok_or_else(ApiError::not_found)?
            .into()
    };
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => Ok(response.map(Body::new).into_response()),
        Err(e) => match e {},
    }
}

async fn song(
    Worker(name): Worker,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    request: Request,
) -> ApiResult<Response> {
    send_song(&state, &name, id, request).await
}

#[derive(Deserialize)]
struct Outcome {
    /// The `.lrc` text.
    lrc: Option<String>,
    /// Why there isn't one.
    error: Option<String>,
    /// How long the listening took, for the worker's speed.
    #[serde(default)]
    elapsed_ms: Option<u64>,
}

async fn take_result(
    state: &AppState,
    name: &str,
    id: i64,
    body: Outcome,
) -> ApiResult<StatusCode> {
    let Some((holder, kind)) = state.store.ai_holder(id).await? else {
        return Err(ApiError::conflict(
            "that song isn't held by a worker any more",
        ));
    };
    if holder != name {
        return Err(ApiError::forbidden());
    }
    let kind_number = if kind == "sync" { 2 } else { 1 };
    let failure = match (&body.lrc, &body.error) {
        // An instrumental has no lyrics to make: finished, nothing stored.
        (None, Some(error)) if error == "instrumental" => None,
        (Some(text), _) => store_from_worker(state, id, text, kind_number, name)
            .await
            .err(),
        (None, Some(error)) => Some(error.chars().take(200).collect()),
        (None, None) => Some("sent nothing".to_owned()),
    };
    state.store.ai_finish(id, failure.as_deref()).await?;
    let _ = tokio::fs::remove_file(vocals_file(state, id)).await;
    let audio_ms = state
        .store
        .track(id)
        .await
        .ok()
        .flatten()
        .and_then(|t| t.duration_ms)
        .and_then(|d| u64::try_from(d).ok());
    seen(name, |s| {
        if failure.is_some() {
            s.failed += 1;
        } else {
            s.done += 1;
        }
        if let (Some(work), Some(audio)) = (body.elapsed_ms, audio_ms) {
            if failure.is_none() && audio > 0 {
                s.work_ms += work;
                s.audio_ms += audio;
                #[allow(clippy::cast_precision_loss)]
                {
                    s.secs_per_minute = Some(s.work_ms as f64 / s.audio_ms as f64 * 60.0);
                }
            }
        }
    });
    Ok(StatusCode::NO_CONTENT)
}

async fn result(
    Worker(name): Worker,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    Json(body): Json<Outcome>,
) -> ApiResult<StatusCode> {
    take_result(&state, &name, id, body).await
}

/// This computer's listener sends a song's isolated vocals (16 kHz mono
/// FLAC), which then waits for a node.
async fn vocals(
    Worker(name): Worker,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    body: axum::body::Bytes,
) -> ApiResult<StatusCode> {
    match state.store.ai_holder(id).await? {
        Some((holder, _)) if holder == name => {}
        _ => return Err(ApiError::forbidden()),
    }
    if body.len() > VOCALS_MAX_BYTES || !body.starts_with(b"fLaC") {
        return Err(ApiError::bad_request("that isn't FLAC vocals"));
    }
    tokio::fs::create_dir_all(vocals_dir(&state))
        .await
        .map_err(anyhow::Error::from)?;
    tokio::fs::write(vocals_file(&state, id), &body)
        .await
        .map_err(anyhow::Error::from)?;
    state.store.ai_prepared(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- Nodes helping ---

/// The name a node's work goes under.
pub fn node_worker(node: &djinn_store::Node) -> String {
    format!("node:{}", node.node_id)
}

/// A node that may help: accepted and switched on by the admin.
fn helper(node: &djinn_store::Node) -> ApiResult<String> {
    if node.status != "approved" || !node.ai_help {
        return Err(ApiError::forbidden());
    }
    let name = node_worker(node);
    seen(&name, |s| {
        s.label.clone_from(&node.name);
        s.node = true;
    });
    Ok(name)
}

async fn node_next(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    Json(body): Json<Next>,
) -> ApiResult<Json<Batch>> {
    let name = helper(&node)?;
    Ok(Json(hand_out(&state, &name, Asker::Node, body).await?))
}

async fn node_song(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    UrlPath(id): UrlPath<i64>,
    request: Request,
) -> ApiResult<Response> {
    let name = helper(&node)?;
    send_song(&state, &name, id, request).await
}

async fn node_result(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    UrlPath(id): UrlPath<i64>,
    Json(body): Json<Outcome>,
) -> ApiResult<StatusCode> {
    let name = helper(&node)?;
    take_result(&state, &name, id, body).await
}

#[derive(Deserialize)]
struct KitAsk {
    python: String,
    arch: String,
    #[serde(default)]
    path: Option<String>,
}

/// The node's model: the admin's choice, else the one suggested for it.
pub fn model_for(node: &djinn_store::Node) -> String {
    node.ai_model
        .clone()
        .filter(|m| crate::ai_listener::MODELS.contains(&m.as_str()))
        .unwrap_or_else(|| {
            suggested_model(&NodeAiReport::of(node), speed_of(&node_worker(node))).to_owned()
        })
}

/// What the node needs: the list (ready, or being put together), or one
/// file of it with `path`.
async fn node_kit(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    axum::extract::Query(ask): axum::extract::Query<KitAsk>,
    request: Request,
) -> ApiResult<Response> {
    helper(&node)?;
    let model = model_for(&node);
    if let Some(path) = ask.path {
        if !crate::ai_kit::safe_path(&path) {
            return Err(ApiError::bad_request("not a kit file"));
        }
        let file = crate::ai_kit::file_of(&state, &ask.python, &ask.arch, &model, &path)
            .ok_or_else(ApiError::not_found)?;
        return match ServeFile::new(file).oneshot(request).await {
            Ok(response) => Ok(response.map(Body::new).into_response()),
            Err(e) => match e {},
        };
    }
    let kit = crate::ai_kit::kit(&state, &ask.python, &ask.arch, &model);
    let code = match kit {
        crate::ai_kit::Kit::Ready { .. } => StatusCode::OK,
        crate::ai_kit::Kit::Preparing => StatusCode::ACCEPTED,
        crate::ai_kit::Kit::Failed { .. } => StatusCode::CONFLICT,
    };
    Ok((
        code,
        Json(serde_json::json!({ "model": model, "kit": kit })),
    )
        .into_response())
}

/// What a node says about its helper: how far the install got, what it's
/// doing, its processor and memory, and how fast it listens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeAiReport {
    /// "off", "installing", "ready", "working", "paused" or "failed".
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub python: Option<String>,
    #[serde(default)]
    pub threads: Option<u32>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
    /// The model installed.
    #[serde(default)]
    pub model: Option<String>,
    /// The speed test: milliseconds the model takes to hear 30 seconds.
    #[serde(default)]
    pub bench_ms: Option<u64>,
}

impl NodeAiReport {
    pub fn of(node: &djinn_store::Node) -> Self {
        node.ai_report
            .as_deref()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default()
    }
}

/// The Whisper model to suggest for a node: by its memory and processor,
/// one size smaller when it has proved slow (more than 1.5 minutes of work
/// per minute of song, measured or from the speed test).
pub fn suggested_model(report: &NodeAiReport, secs_per_minute: Option<f64>) -> &'static str {
    const SIZES: [&str; 3] = ["tiny", "base", "small"];
    let memory = report.memory_mb.unwrap_or(0);
    let threads = report.threads.unwrap_or(0);
    let mut size: usize = if memory < 6_000 || threads < 4 {
        0
    } else if memory < 12_000 {
        1
    } else {
        2
    };
    // The speed test hears 30 s; a whole song takes about three times its
    // listening (finding the voice, writing the words) on top.
    #[allow(clippy::cast_precision_loss)]
    let tested = report.bench_ms.map(|ms| ms as f64 / 1000.0 * 2.0 * 3.0);
    if secs_per_minute.or(tested).is_some_and(|s| s > 90.0) {
        size = size.saturating_sub(1);
    }
    SIZES[size]
}

async fn node_report(
    State(state): State<AppState>,
    NodeAuth(node): NodeAuth,
    Json(body): Json<NodeAiReport>,
) -> ApiResult<StatusCode> {
    if node.status != "approved" {
        return Err(ApiError::forbidden());
    }
    let mut report = body;
    report.detail = report.detail.map(|d| d.chars().take(300).collect());
    report.phase = report.phase.chars().take(20).collect();
    state
        .store
        .set_node_ai_report(
            node.id,
            &serde_json::to_string(&report).map_err(anyhow::Error::from)?,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use djinn_store::{NewLyrics, ScannedTrack};

    use super::*;
    use crate::test_util::session;
    use tower::ServiceExt;

    #[tokio::test]
    async fn a_worker_takes_a_song_and_sends_lyrics_back() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", djinn_store::Role::Admin).await;
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: "/m/Skit.mp3".into(),
                title: "Skit".into(),
                artist: Some("Band".into()),
                duration_ms: Some(30_000),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = state.store.compare_rows().await.unwrap()[0].id;
        state
            .store
            .save_read_lyrics(id, 1, &NewLyrics::NONE)
            .await
            .unwrap();
        state.store.save_looked_up_lyrics(id, None).await.unwrap();
        // An admin makes a key; a worker needs it, and work is off until switched on.
        let made = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/api/ai-work/keys")
            .header("cookie", &admin)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"pc"}"#))
            .unwrap();
        let response = crate::app(state.clone()).oneshot(made).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let key = text
            .split("\"key\":\"")
            .nth(1)
            .and_then(|b| b.split('"').next())
            .unwrap()
            .to_owned();
        let worker = |path: String, body: String, key: String| {
            let state = state.clone();
            async move {
                let request = axum::http::Request::builder()
                    .method(Method::POST)
                    .uri(path.as_str())
                    .header("authorization", format!("Worker {key}"))
                    .header("content-type", "application/json")
                    .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                        [192, 168, 1, 20],
                        4000,
                    ))))
                    .body(Body::from(body))
                    .unwrap();
                let response = crate::app(state.clone()).oneshot(request).await.unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                    .await
                    .unwrap();
                (status, String::from_utf8_lossy(&bytes).into_owned())
            }
        };
        let (status, text) = worker(
            "/api/ai-work/next".into(),
            r#"{"max":2}"#.into(),
            "wrong".into(),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{text}");
        let (_, text) = worker(
            "/api/ai-work/next".into(),
            r#"{"max":2}"#.into(),
            key.clone(),
        )
        .await;
        assert!(text.contains("\"jobs\":[]"), "{text}");
        crate::settings::set_flag(&state.store, WORK_ON, true)
            .await
            .unwrap();
        crate::settings::set_flag(&state.store, crate::background::WHEN, false)
            .await
            .ok();
        let (_, text) = worker(
            "/api/ai-work/next".into(),
            r#"{"max":2}"#.into(),
            key.clone(),
        )
        .await;
        assert!(
            text.contains("\"kind\":\"listen\"") && text.contains("Skit"),
            "{text}"
        );
        // It sends the lyrics back, marked as the AI's.
        let lrc = r#"{"lrc":"[ar:Band]\n[ti:Skit]\n[by:lyrics-listener (AI-generated; model=m)]\n\n[00:00.00](AI-generated lyrics)\n[00:01.00]hello there"}"#;
        let (status, text) = worker(format!("/api/ai-work/result/{id}"), lrc.into(), key).await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{text}");
        let got = state.store.lyrics(id).await.unwrap().unwrap();
        assert_eq!((got.ai, got.plain.as_deref()), (1, Some("hello there")));
        assert_eq!(
            state.store.ai_counts(false, LEASE_MS).await.unwrap().done,
            1
        );
    }
}
