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
/// The least an AI's lyrics must score to be kept, in percent: how well the
/// heard audio matched the words when timing them, or how sure the model was
/// of the words it heard when writing them.
pub const MIN_MATCH: &str = "ai_lyrics_min_match";
pub const MIN_MATCH_DEFAULT: i64 = 85;
/// Genres whose songs the AI doesn't make or time lyrics for (one per line),
/// unless an admin asks for a song.
pub const SKIP_GENRES: &str = "ai_lyrics_skip_genres";
/// Most genres that can be left out.
const SKIP_GENRES_MOST: usize = 100;
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
    asked_at: i64,
    /// What Main last answered a node that asked: "given", "no_songs"
    /// (nothing waiting) or "work_off" (AI work is switched off).
    answer: Option<String>,
    done: u64,
    /// Results under the minimum score: listened to, but not sure enough.
    low: u64,
    /// Songs that failed: an error, or nothing sent back.
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

/// What this computer took each song it holds for (its strippers and its
/// listener share one name), for its status on the Lyrics tab.
static MAIN_DOING: std::sync::LazyLock<Mutex<HashMap<i64, MainWork>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum MainWork {
    /// Separating its vocals.
    Strip,
    /// Listening to it.
    Listen,
    /// Listening to it for a second opinion on a node's low score.
    Second,
}

/// A song this computer is working on, and for what.
#[derive(Serialize)]
struct MainJob {
    what: MainWork,
    #[serde(flatten)]
    song: djinn_store::AiHeld,
}

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

/// Nodes free to help right now: switched on, checked in lately, and with
/// their listener running, not paused for music or a screen. This
/// computer's listener gets songs ready for them while there are any, and
/// does the work itself when there are none.
async fn free_nodes(state: &AppState) -> anyhow::Result<usize> {
    let cut = djinn_store::now_ms() - NODE_QUIET_MS;
    Ok(state
        .store
        .nodes()
        .await?
        .iter()
        .filter(|n| n.status == "approved" && n.ai_help && n.last_seen.is_some_and(|s| s > cut))
        .filter(|n| matches!(NodeAiReport::of(n).phase.as_str(), "working" | "ready"))
        .count())
}

/// How many songs with their vocals separated this computer keeps ready:
/// one for every listener that may take one (each node free to help, while
/// nodes get them, and this computer's own), and one spare.
async fn vocals_wanted(state: &AppState) -> anyhow::Result<i64> {
    let nodes = if settings::flag(&state.store, VOCALS_FOR_NODES, true).await? {
        i64::try_from(free_nodes(state).await?).unwrap_or(0)
    } else {
        0
    };
    Ok(nodes + 2)
}

/// How soon this computer's stripper and listener ask again when there's
/// nothing to do (the other one may make or take a song any moment).
const STRIPPING_WAIT_SECS: u64 = 5;
/// Nodes waiting for work right now: free to help, holding no song, and
/// asking lately (a node free on paper that stopped asking doesn't count).
/// While any wait, this computer's listener leaves new songs to them
/// (`NODES_FIRST`), doing second opinions only.
async fn nodes_waiting(state: &AppState) -> anyhow::Result<usize> {
    let now = djinn_store::now_ms();
    let held = state.store.ai_held(now - LEASE_MS).await?;
    let asked: HashMap<String, i64> = WORKERS
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.asked_at)).collect())
        })
        .unwrap_or_default();
    Ok(state
        .store
        .nodes()
        .await?
        .iter()
        .filter(|n| {
            let worker = node_worker(n);
            n.status == "approved"
                && n.ai_help
                && n.last_seen.is_some_and(|s| s > now - NODE_QUIET_MS)
                && matches!(NodeAiReport::of(n).phase.as_str(), "working" | "ready")
                && asked.get(&worker).is_some_and(|&a| a > now - NODE_ASKED_MS)
                && !held.iter().any(|h| h.worker == worker)
        })
        .count())
}

/// A node that asked for work within this long is still asking.
const NODE_ASKED_MS: i64 = 60 * 1000;
/// A node that hasn't checked in for this long isn't free to help.
const NODE_QUIET_MS: i64 = 30 * 1000;
/// How soon a node that got nothing asks again, so a song made ready for
/// it is picked up quickly.
const NODE_WAIT_SECS: u64 = 15;
/// The largest isolated vocals file taken (16 kHz mono FLAC is a few MB).
const VOCALS_MAX_BYTES: usize = 40 << 20;
/// Setting key: Main isolates vocals for nodes when it can.
pub const VOCALS_FOR_NODES: &str = "ai_vocals_for_nodes";
/// Setting key: while nodes wait for work, this computer's listener leaves
/// new songs to them (doing second opinions only).
pub const NODES_FIRST: &str = "ai_nodes_first";
/// Setting key: a song a node's model wasn't sure enough of goes to this
/// computer's listener (a bigger model) for a second opinion.
pub const SECOND_OPINION: &str = "ai_second_opinion";

/// Whether this computer's listener leaves new songs to nodes waiting for
/// work right now (the setting is on and at least one waits).
async fn leaves_to_nodes(state: &AppState, name: &str) -> anyhow::Result<bool> {
    Ok(name == crate::ai_listener::LOCAL_KEY_NAME
        && settings::flag(&state.store, NODES_FIRST, true).await?
        && nodes_waiting(state).await? > 0)
}

/// Whether songs a node wasn't sure of go to this computer's listener: the
/// setting is on and the listener is installed here to take them.
async fn second_opinions(state: &AppState) -> anyhow::Result<bool> {
    Ok(settings::flag(&state.store, SECOND_OPINION, true).await?
        && crate::ai_listener::installed(&crate::ai_listener::dir(state)))
}

// --- Admin ---

#[derive(Serialize)]
struct Status {
    on: bool,
    new_songs: bool,
    sync_plain: bool,
    vocals_for_nodes: bool,
    second_opinion: bool,
    nodes_first: bool,
    min_match: i64,
    skip_genres: Vec<String>,
    counts: AiCounts,
    /// Songs a node wasn't sure of, waiting for this computer's listener.
    waiting_second: i64,
    /// Why handing out work is paused (music playing, outside the hours).
    held: Option<crate::background::Held>,
    keys: Vec<String>,
    workers: Vec<WorkerView>,
    /// Every node switched on to help, and what it's doing.
    helpers: Vec<Helping>,
    /// What this computer is working on: vocals it's separating, and the
    /// song it's listening to (and whether for a second opinion).
    this_computer: Vec<MainJob>,
}

/// A node switched on to help: what it says about itself, what Main last
/// answered it, and the songs it's working on.
#[derive(Serialize)]
struct Helping {
    node_id: i64,
    name: String,
    /// When the node last checked in; a node gone quiet isn't helping,
    /// whatever its last report said.
    last_seen: Option<i64>,
    report: NodeAiReport,
    asked_at: Option<i64>,
    answer: Option<String>,
    done: u64,
    low: u64,
    failed: u64,
    secs_per_minute: Option<f64>,
    working_on: Vec<djinn_store::AiHeld>,
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
    let held = state
        .store
        .ai_held(djinn_store::now_ms() - LEASE_MS)
        .await?;
    let this_computer: Vec<MainJob> = {
        let mine: Vec<&djinn_store::AiHeld> = held
            .iter()
            .filter(|h| h.worker == crate::ai_listener::LOCAL_KEY_NAME)
            .collect();
        let mut doing = MAIN_DOING
            .lock()
            .map_err(|_| anyhow::anyhow!("status lock"))?;
        // Forget songs it no longer holds.
        doing.retain(|id, _| mine.iter().any(|h| h.track_id == *id));
        let mut jobs: Vec<MainJob> = mine
            .into_iter()
            .map(|h| MainJob {
                what: doing.get(&h.track_id).copied().unwrap_or(MainWork::Listen),
                song: h.clone(),
            })
            .collect();
        // Separating first, then listening; oldest first within each.
        jobs.sort_by_key(|j| j.what != MainWork::Strip);
        jobs
    };
    let helpers = state
        .store
        .nodes()
        .await?
        .into_iter()
        .filter(|n| n.status == "approved" && n.ai_help)
        .map(|node| {
            let worker = node_worker(&node);
            let seen = workers.iter().find(|w| w.name == worker).map(|w| &w.seen);
            Helping {
                node_id: node.id,
                name: node.name.clone(),
                last_seen: node.last_seen,
                report: NodeAiReport::of(&node),
                asked_at: seen.map(|s| s.asked_at).filter(|&a| a > 0),
                answer: seen.and_then(|s| s.answer.clone()),
                done: seen.map_or(0, |s| s.done),
                low: seen.map_or(0, |s| s.low),
                failed: seen.map_or(0, |s| s.failed),
                secs_per_minute: seen.and_then(|s| s.secs_per_minute),
                working_on: held
                    .iter()
                    .filter(|h| h.worker == worker)
                    .cloned()
                    .collect(),
            }
        })
        .collect();
    Ok(Json(Status {
        on: settings::flag(&state.store, WORK_ON, false).await?,
        new_songs: settings::flag(&state.store, WORK_NEW, true).await?,
        sync_plain: settings::flag(&state.store, WORK_SYNC, false).await?,
        vocals_for_nodes: settings::flag(&state.store, VOCALS_FOR_NODES, true).await?,
        second_opinion: settings::flag(&state.store, SECOND_OPINION, true).await?,
        nodes_first: settings::flag(&state.store, NODES_FIRST, true).await?,
        waiting_second: state.store.ai_second_count().await?,
        min_match: min_match(&state).await?,
        skip_genres: skip_genres(&state).await?,
        counts: state
            .store
            .ai_counts(
                lookups_off(&state).await?,
                LEASE_MS,
                &skipped_songs(&state).await?,
            )
            .await?,
        held: crate::background::held(&state).await,
        keys: keys(&state).await?.into_iter().map(|k| k.name).collect(),
        workers,
        helpers,
        this_computer,
    }))
}

#[derive(Deserialize)]
struct Settings {
    on: bool,
    new_songs: bool,
    sync_plain: bool,
    #[serde(default = "yes")]
    vocals_for_nodes: bool,
    #[serde(default = "yes")]
    second_opinion: bool,
    #[serde(default = "yes")]
    nodes_first: bool,
    #[serde(default)]
    min_match: Option<i64>,
    #[serde(default)]
    skip_genres: Option<Vec<String>>,
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
    settings::set_flag(&state.store, SECOND_OPINION, body.second_opinion).await?;
    settings::set_flag(&state.store, NODES_FIRST, body.nodes_first).await?;
    if let Some(pct) = body.min_match {
        state
            .store
            .set_setting(MIN_MATCH, &pct.clamp(0, 100).to_string())
            .await?;
    }
    if let Some(genres) = body.skip_genres {
        let mut kept: Vec<String> = Vec::new();
        for genre in genres {
            let genre = djinn_library::vibe::normalize(&genre);
            if !genre.is_empty() && !kept.contains(&genre) {
                kept.push(genre);
            }
        }
        kept.truncate(SKIP_GENRES_MOST);
        state
            .store
            .set_setting(SKIP_GENRES, &kept.join("\n"))
            .await?;
    }
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
    /// What the worker does; older workers don't say.
    #[serde(default)]
    role: Role,
}

/// This computer runs two workers while it strips vocals: one separating
/// vocals ahead ("prepare"), one listening to songs whose vocals are ready
/// ("listen"), at the same time. Other workers do whole songs ("all").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    #[default]
    All,
    Prepare,
    Listen,
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
    /// The least a result may score, in percent (`MIN_MATCH`): the worker
    /// skips a song that scores lower rather than sending it.
    min_match: i64,
}

async fn skip_genres(state: &AppState) -> anyhow::Result<Vec<String>> {
    Ok(state
        .store
        .get_setting(SKIP_GENRES)
        .await?
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The songs in the genres left out of AI lyrics.
async fn skipped_songs(state: &AppState) -> anyhow::Result<Vec<i64>> {
    let genres = skip_genres(state).await?;
    if genres.is_empty() {
        return Ok(Vec::new());
    }
    Ok(crate::smart::songs_in_genres(state, &genres))
}

async fn min_match(state: &AppState) -> anyhow::Result<i64> {
    Ok(settings::number(&state.store, MIN_MATCH, MIN_MATCH_DEFAULT)
        .await?
        .clamp(0, 100))
}

/// Why an AI's lyrics are refused for scoring under `min`, read from their
/// `[by:...match=NN%]` header. Lyrics that give no score are kept.
fn too_unsure(text: &str, min: i64) -> Option<String> {
    let pct = djinn_library::lyrics_import::header(text)
        .ai_info()?
        .match_pct?;
    (pct < min).then(|| format!("{LOW_SCORE} ({pct}%, under {min}%)"))
}

/// A helping node's result, as a line for the listener's log.
fn node_outcome(
    who: &str,
    song: &str,
    kind: &str,
    body: &Outcome,
    failure: Option<&str>,
    second: bool,
) -> String {
    match failure {
        Some(why) if why.starts_with(LOW_SCORE) && second => {
            format!("{who}: {song} {why}; passed to this computer for a second opinion")
        }
        Some(why) if why.starts_with(LOW_SCORE) => format!("{who}: {song} {why}; given up"),
        Some(why) => format!("{who}: {song} failed: {why}"),
        None => match &body.lrc {
            None => format!("{who}: {song} is an instrumental"),
            Some(text) => {
                let made = if kind == "sync" {
                    "lyrics timed"
                } else {
                    "lyrics made"
                };
                match djinn_library::lyrics_import::header(text)
                    .ai_info()
                    .and_then(|i| i.match_pct)
                {
                    Some(pct) => format!("{who}: {song}, {made} ({pct}% sure)"),
                    None => format!("{who}: {song}, {made}"),
                }
            }
        },
    }
}

/// The start of the reason a result scored too low (also the worker's skip).
const LOW_SCORE: &str = "scored too low";

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

/// Whether any external worker (other worker keys, or approved nodes set to
/// help with AI) exists besides this computer's own listener.
pub async fn has_other_workers(state: &AppState) -> anyhow::Result<bool> {
    if keys(state)
        .await?
        .iter()
        .any(|k| k.name != crate::ai_listener::LOCAL_KEY_NAME)
    {
        return Ok(true);
    }
    Ok(state
        .store
        .nodes()
        .await?
        .into_iter()
        .any(|n| n.status == "approved" && n.ai_help))
}

#[derive(Clone)]
struct ImportLease {
    worker: String,
    kind: String,
    taken_at: i64,
    attempts: i64,
    moved_to: Option<i64>,
}

static IMPORT_JOBS: Mutex<Option<HashMap<(usize, i64), ImportLease>>> = Mutex::new(None);

fn state_key(state: &AppState) -> usize {
    std::sync::Arc::as_ptr(&state.art_dir) as usize
}

fn with_imports<T>(f: impl FnOnce(&mut HashMap<(usize, i64), ImportLease>) -> T) -> T {
    let mut guard = IMPORT_JOBS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// Whether an import file is currently leased to a worker.
pub fn import_working(state: &AppState, file_id: i64) -> bool {
    let key = (state_key(state), file_id);
    let cut = djinn_store::now_ms() - LEASE_MS;
    with_imports(|m| m.get(&key).is_some_and(|l| l.taken_at > cut))
}

/// Clears prior failure counts on import files when asked again.
pub fn clear_import_attempts(state: &AppState, ids: &[i64]) {
    let sk = state_key(state);
    let cut = djinn_store::now_ms() - LEASE_MS;
    with_imports(|m| {
        for &id in ids {
            let key = (sk, id);
            if let Some(lease) = m.get_mut(&key) {
                if lease.taken_at <= cut {
                    m.remove(&key);
                } else {
                    lease.attempts = 0;
                }
            }
        }
    });
}

/// Remembers that an import file was moved into the library while a worker
/// held it, so its result lands on the new track.
pub fn note_import_moved(state: &AppState, file_id: i64, track_id: i64) {
    let key = (state_key(state), file_id);
    with_imports(|m| {
        if let Some(lease) = m.get_mut(&key) {
            lease.moved_to = Some(track_id);
        }
    });
}

async fn worker_busy(state: &AppState, worker: &str) -> bool {
    let sk = state_key(state);
    let cut = djinn_store::now_ms() - LEASE_MS;
    let holding_import = with_imports(|m| {
        m.iter()
            .any(|(&(k, _), l)| k == sk && l.worker == worker && l.taken_at > cut)
    });
    holding_import
        || state
            .store
            .ai_worker_busy(worker, LEASE_MS)
            .await
            .unwrap_or(false)
}

/// Wakes the local listener if installed so on-demand editor work starts
/// straight away even when background listening is off.
pub async fn wake_listener(state: &AppState) {
    if !crate::ai_listener::installed(&crate::ai_listener::dir(state)) {
        return;
    }
    if crate::ai_listener::is_running()
        && !worker_busy(state, crate::ai_listener::LOCAL_KEY_NAME).await
    {
        crate::ai_listener::stop_worker().await;
    }
    if let Err(e) = crate::ai_listener::start_worker(state).await {
        tracing::warn!(error = %e, "couldn't start the lyrics listener");
    }
}

fn codec_for_path(path: &std::path::Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let codec = match ext.as_str() {
        "mp3" => "mpeg",
        "m4a" | "mp4" => "mp4",
        "flac" => "flac",
        "ogg" | "oga" => "vorbis",
        "wav" => "wav",
        "aac" => "aac",
        _ => return None,
    };
    Some(codec.to_owned())
}

async fn claim_imports(
    state: &AppState,
    worker: &str,
    max: usize,
) -> ApiResult<Vec<djinn_store::AiJob>> {
    let wanted = state.store.import_ai_wanted().await?;
    if wanted.is_empty() || max == 0 {
        return Ok(Vec::new());
    }
    let sk = state_key(state);
    let now = djinn_store::now_ms();
    let cut = now - LEASE_MS;
    let candidates: Vec<djinn_store::ImportFile> = with_imports(|m| {
        wanted
            .into_iter()
            .filter(|f| {
                m.get(&(sk, f.id))
                    .is_none_or(|l| l.taken_at <= cut && l.attempts < djinn_store::MAX_ATTEMPTS)
            })
            .collect()
    });
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let original_years = djinn_library::original_years(&state.store).await?;
    let (ready, stale) = tokio::task::spawn_blocking(move || {
        let mut ready: Vec<djinn_store::AiJob> = Vec::new();
        let mut stale: Vec<i64> = Vec::new();
        for file in candidates {
            // Each file is opened (and an MP3's length found by reading all
            // of it), so stop once there's enough; the rest wait for the
            // next ask.
            if ready.len() >= max {
                break;
            }
            let Some(kind) = file.ai_want.clone() else {
                continue;
            };
            let path = std::path::PathBuf::from(&file.path);
            let local = djinn_library::lyrics::local(&path);
            let needs = match kind.as_str() {
                "listen" => local.is_none(),
                "sync" => local
                    .as_ref()
                    .is_some_and(|l| l.synced.is_none() && l.plain.is_some() && !l.instrumental),
                _ => false,
            };
            if !needs {
                stale.push(file.id);
                continue;
            }
            let duration_ms = djinn_library::compare::length_ms(&path);
            let tags = djinn_library::compare::read(&path, original_years).unwrap_or_default();
            let plain = if kind == "sync" {
                local.and_then(|l| l.plain)
            } else {
                None
            };
            ready.push(djinn_store::AiJob {
                track_id: -file.id,
                kind,
                artist: tags.artist.or(tags.album_artist),
                title: tags.title,
                album: tags.album,
                duration_ms,
                codec: codec_for_path(&path),
                plain,
            });
        }
        (ready, stale)
    })
    .await
    .map_err(anyhow::Error::from)?;
    for id in stale {
        state.store.clear_import_ai_want(id).await?;
        with_imports(|m| {
            m.remove(&(sk, id));
        });
    }
    let claimed = with_imports(|m| {
        let mut out = Vec::new();
        for job in ready {
            let file_id = -job.track_id;
            let entry = m.entry((sk, file_id)).or_insert_with(|| ImportLease {
                worker: worker.to_owned(),
                kind: job.kind.clone(),
                taken_at: 0,
                attempts: 0,
                moved_to: None,
            });
            if entry.taken_at > cut || entry.attempts >= djinn_store::MAX_ATTEMPTS {
                continue;
            }
            entry.worker = worker.to_owned();
            entry.kind.clone_from(&job.kind);
            entry.taken_at = now;
            entry.attempts += 1;
            entry.moved_to = None;
            out.push(job);
        }
        out
    });
    Ok(claimed)
}

async fn hand_out(state: &AppState, name: &str, asker: Asker, body: Next) -> ApiResult<Batch> {
    seen(name, |s| {
        s.node = asker == Asker::Node;
        if asker == Asker::Node {
            s.asked_at = djinn_store::now_ms();
        }
    });
    let min_match = min_match(state).await?;
    let nothing = |held| Batch {
        jobs: Vec::new(),
        wait_secs: Some(if asker == Asker::Node {
            NODE_WAIT_SECS
        } else if body.role == Role::All && name != crate::ai_listener::LOCAL_KEY_NAME {
            WAIT_SECS
        } else {
            STRIPPING_WAIT_SECS
        }),
        held,
        min_match,
    };
    let work_on = settings::flag(&state.store, WORK_ON, false).await?;
    // A node keeps its own rules (it asks only while it isn't playing or
    // showing visuals); Main's rules are for Main's own work.
    let held = if asker == Asker::Keyed {
        crate::background::held(state).await
    } else {
        None
    };
    let background = work_on && held.is_none();
    let max_usize = body.max.clamp(1, BATCH_MAX);
    let max = i64::try_from(max_usize).unwrap_or(1);
    let listen = background && settings::flag(&state.store, WORK_NEW, true).await?;
    let sync = background && settings::flag(&state.store, WORK_SYNC, false).await?;
    let lookups_off = lookups_off(state).await?;
    let mut prepare = false;
    let skip = skipped_songs(state).await?;
    // While this computer strips vocals ahead, the listeners (its own and
    // the nodes') take only songs whose vocals are ready, so every song gets
    // the cleaner listen; with no stripper they take whole songs.
    // Nodes left out of the vocals (the setting) keep taking whole songs.
    let shares_vocals =
        asker == Asker::Keyed || settings::flag(&state.store, VOCALS_FOR_NODES, true).await?;
    let stripping = crate::ai_listener::stripper_running() && shares_vocals;
    let role = if asker == Asker::Node {
        Role::Listen
    } else {
        body.role
    };
    // Songs taken for a second opinion, for this computer's status.
    let mut seconds: Vec<i64> = Vec::new();
    let jobs = match role {
        Role::Prepare => {
            if background && body.can_isolate {
                sweep_vocals(state).await?;
                prepare = state.store.ai_prepared_count().await? < vocals_wanted(state).await?;
            }
            if prepare {
                state
                    .store
                    .ai_claim(name, listen, sync, lookups_off, 1, LEASE_MS, &skip)
                    .await?
            } else {
                Vec::new()
            }
        }
        Role::Listen => {
            let mut jobs = if asker == Asker::Keyed {
                claim_imports(state, name, max_usize).await?
            } else {
                Vec::new()
            };
            let left = max - i64::try_from(jobs.len()).unwrap_or(max);
            if background && left > 0 && name == crate::ai_listener::LOCAL_KEY_NAME {
                let claimed = state.store.ai_claim_second(name, left).await?;
                seconds.extend(claimed.iter().map(|j| j.track_id));
                jobs.extend(claimed);
            }
            // Second opinions first; new songs only when no node waits.
            let step_back = jobs.is_empty() && leaves_to_nodes(state, name).await?;
            let left = max - i64::try_from(jobs.len()).unwrap_or(max);
            if background && left > 0 && shares_vocals && !step_back {
                jobs.extend(state.store.ai_claim_prepared(name, left).await?);
            }
            let left = max - i64::try_from(jobs.len()).unwrap_or(max);
            if left > 0 && !stripping && !step_back {
                jobs.extend(
                    state
                        .store
                        .ai_claim(name, listen, sync, lookups_off, left, LEASE_MS, &skip)
                        .await?,
                );
            }
            jobs
        }
        Role::All => {
            let mut jobs = claim_imports(state, name, max_usize).await?;
            let left = max - i64::try_from(jobs.len()).unwrap_or(max);
            if background && left > 0 && name == crate::ai_listener::LOCAL_KEY_NAME {
                let claimed = state.store.ai_claim_second(name, left).await?;
                seconds.extend(claimed.iter().map(|j| j.track_id));
                jobs.extend(claimed);
            }
            let step_back = jobs.is_empty() && leaves_to_nodes(state, name).await?;
            let left = max - i64::try_from(jobs.len()).unwrap_or(max);
            if left > 0 && !step_back {
                jobs.extend(
                    state
                        .store
                        .ai_claim(name, listen, sync, lookups_off, left, LEASE_MS, &skip)
                        .await?,
                );
            }
            jobs
        }
    };
    if name == crate::ai_listener::LOCAL_KEY_NAME && !jobs.is_empty() {
        if let Ok(mut doing) = MAIN_DOING.lock() {
            for job in &jobs {
                let what = if prepare {
                    MainWork::Strip
                } else if seconds.contains(&job.track_id) {
                    MainWork::Second
                } else {
                    MainWork::Listen
                };
                doing.insert(job.track_id, what);
            }
        }
    }
    if asker == Asker::Node {
        let answer = if !jobs.is_empty() {
            "given"
        } else if !work_on {
            "work_off"
        } else if stripping {
            "waiting_vocals"
        } else {
            "no_songs"
        };
        tracing::debug!(
            worker = name,
            answer,
            songs = jobs.len(),
            "a node asked for AI work"
        );
        seen(name, |s| s.answer = Some(answer.to_owned()));
    }
    if jobs.is_empty() {
        if !work_on && name == crate::ai_listener::LOCAL_KEY_NAME && !worker_busy(state, name).await
        {
            tokio::spawn(async {
                crate::ai_listener::stop_worker().await;
            });
        }
        return Ok(nothing(if work_on { held } else { None }));
    }
    let jobs = jobs
        .into_iter()
        .map(|job| {
            let vocals = job.track_id > 0 && !prepare && vocals_file(state, job.track_id).is_file();
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
        min_match,
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
    let path: std::path::PathBuf = if id < 0 {
        let key = (state_key(state), -id);
        let file_id = -id;
        let cut = djinn_store::now_ms() - LEASE_MS;
        let moved_to = with_imports(|m| {
            m.get(&key)
                .filter(|l| l.worker == name && l.taken_at > cut)
                .map(|l| l.moved_to)
        })
        .ok_or_else(ApiError::forbidden)?;
        if let Some(track_id) = moved_to {
            state
                .store
                .track_path(track_id)
                .await?
                .ok_or_else(ApiError::not_found)?
                .into()
        } else {
            state
                .store
                .import_files_by_id(&[file_id])
                .await?
                .into_iter()
                .next()
                .ok_or_else(ApiError::not_found)?
                .path
                .into()
        }
    } else {
        match state.store.ai_holder(id).await? {
            Some((holder, _)) if holder == name => {}
            _ => return Err(ApiError::forbidden()),
        }
        let vocals = vocals_file(state, id);
        if vocals.is_file() {
            vocals
        } else {
            state
                .store
                .track_path(id)
                .await?
                .ok_or_else(ApiError::not_found)?
                .into()
        }
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

/// How a song a worker sent back went, for its counts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tally {
    Done,
    /// Listened to, but under the minimum score.
    Low,
    /// An error, or nothing sent back.
    Failed,
}

impl Tally {
    fn of(failure: Option<&str>) -> Tally {
        match failure {
            None => Tally::Done,
            Some(why) if why.starts_with(LOW_SCORE) => Tally::Low,
            Some(_) => Tally::Failed,
        }
    }
}

fn record_seen(name: &str, tally: Tally, elapsed_ms: Option<u64>, audio_ms: Option<u64>) {
    seen(name, |s| {
        match tally {
            Tally::Done => s.done += 1,
            Tally::Low => s.low += 1,
            Tally::Failed => s.failed += 1,
        }
        // A low score was a whole listen, so it counts toward the speed.
        if let (Some(work), Some(audio)) = (elapsed_ms, audio_ms) {
            if tally != Tally::Failed && audio > 0 {
                s.work_ms += work;
                s.audio_ms += audio;
                #[allow(clippy::cast_precision_loss)]
                {
                    s.secs_per_minute = Some(s.work_ms as f64 / s.audio_ms as f64 * 60.0);
                }
            }
        }
    });
}

async fn finish_track_result(
    state: &AppState,
    name: &str,
    id: i64,
    kind: &str,
    body: &Outcome,
) -> ApiResult<()> {
    let kind_number = if kind == "sync" { 2 } else { 1 };
    let min = min_match(state).await?;
    let failure = match (&body.lrc, &body.error) {
        // An instrumental has no lyrics to make: finished, and marked as one.
        (None, Some(error)) if error == "instrumental" => {
            state.store.mark_instrumental(id).await?;
            None
        }
        (Some(text), _) => match too_unsure(text, min) {
            Some(why) => Some(why),
            None => store_from_worker(state, id, text, kind_number, name)
                .await
                .err(),
        },
        (None, Some(error)) => Some(error.chars().take(200).collect()),
        (None, None) => Some("sent nothing".to_owned()),
    };
    // A low score won't change by asking the same model again, so the song
    // isn't retried (until an admin retries failed songs, say after
    // lowering the minimum); one a node's smaller model wasn't sure of goes
    // to this computer's bigger one first, keeping its separated vocals.
    let low = failure.as_deref().is_some_and(|f| f.starts_with(LOW_SCORE));
    let second = low && name.starts_with("node:") && second_opinions(state).await?;
    if second {
        tracing::debug!(
            worker = name,
            song = id,
            "a second opinion from this computer"
        );
        state
            .store
            .ai_second_opinion(id, failure.as_deref())
            .await?;
    } else if low {
        state.store.ai_give_up(id, failure.as_deref()).await?;
    } else {
        state.store.ai_finish(id, failure.as_deref()).await?;
    }
    if !second {
        let _ = tokio::fs::remove_file(vocals_file(state, id)).await;
    }
    let track = state.store.track(id).await.ok().flatten();
    if name.starts_with("node:") {
        // What a helping node did, in the listener's log beside this
        // computer's own work.
        let who = WORKERS
            .lock()
            .ok()
            .and_then(|g| g.as_ref()?.get(name).map(|s| s.label.clone()))
            .unwrap_or_else(|| name.to_owned());
        let song = track.as_ref().map_or_else(
            || format!("song {id}"),
            |t| match &t.artist {
                Some(artist) => format!("{artist} - {}", t.title),
                None => t.title.clone(),
            },
        );
        crate::ai_listener::log_line(node_outcome(
            &who,
            &song,
            kind,
            body,
            failure.as_deref(),
            second,
        ));
    }
    let audio_ms = track
        .and_then(|t| t.duration_ms)
        .and_then(|d| u64::try_from(d).ok());
    record_seen(
        name,
        Tally::of(failure.as_deref()),
        body.elapsed_ms,
        audio_ms,
    );
    Ok(())
}

async fn store_import_result(
    file: &djinn_store::ImportFile,
    body: &Outcome,
    min: i64,
) -> Option<String> {
    match (&body.lrc, &body.error) {
        (None, Some(error)) if error == "instrumental" => None,
        (Some(text), _) => {
            if let Some(why) = too_unsure(text, min) {
                return Some(why);
            }
            if text.len() as u64 > djinn_library::lyrics_import::MAX_FILE_BYTES {
                return Some("the lyrics are too big".into());
            }
            if let Some(why) = djinn_library::lyrics_import::suspicious(text) {
                return Some(format!("the lyrics look like code or markup ({why})"));
            }
            if !matches!(
                djinn_library::lyrics_import::content(text),
                djinn_library::lyrics_import::Content::Lyrics(_)
            ) {
                return Some("there are no lyrics in it".into());
            }
            let lrc_path = std::path::Path::new(&file.path).with_extension("lrc");
            tokio::fs::write(lrc_path, text)
                .await
                .err()
                .map(|e| e.to_string())
        }
        (None, Some(error)) => Some(error.chars().take(200).collect()),
        (None, None) => Some("sent nothing".to_owned()),
    }
}

async fn take_result(
    state: &AppState,
    name: &str,
    id: i64,
    body: Outcome,
) -> ApiResult<StatusCode> {
    if id < 0 {
        let key = (state_key(state), -id);
        let file_id = -id;
        let lease = with_imports(|m| m.get(&key).cloned())
            .ok_or_else(|| ApiError::conflict("that song isn't held by a worker any more"))?;
        if lease.worker != name {
            return Err(ApiError::forbidden());
        }
        if let Some(track_id) = lease.moved_to {
            with_imports(|m| m.remove(&key));
            finish_track_result(state, name, track_id, &lease.kind, &body).await?;
            return Ok(StatusCode::NO_CONTENT);
        }
        let Some(file) = state
            .store
            .import_files_by_id(&[file_id])
            .await?
            .into_iter()
            .next()
        else {
            with_imports(|m| m.remove(&key));
            return Err(ApiError::conflict(
                "that song isn't held by a worker any more",
            ));
        };
        let failure = store_import_result(&file, &body, min_match(state).await?).await;
        let low = failure.as_deref().is_some_and(|f| f.starts_with(LOW_SCORE))
            || body
                .error
                .as_deref()
                .is_some_and(|e| e.starts_with(LOW_SCORE));
        if failure.is_none() || low || lease.attempts >= djinn_store::MAX_ATTEMPTS {
            with_imports(|m| m.remove(&key));
            state.store.clear_import_ai_want(file_id).await?;
        } else {
            with_imports(|m| {
                if let Some(entry) = m.get_mut(&key) {
                    entry.taken_at = 0;
                }
            });
        }
        let path = std::path::PathBuf::from(&file.path);
        let audio_ms =
            tokio::task::spawn_blocking(move || djinn_library::compare::length_ms(&path))
                .await
                .ok()
                .flatten()
                .and_then(|d| u64::try_from(d).ok());
        record_seen(
            name,
            Tally::of(failure.as_deref()),
            body.elapsed_ms,
            audio_ms,
        );
        return Ok(StatusCode::NO_CONTENT);
    }
    let Some((holder, kind)) = state.store.ai_holder(id).await? else {
        return Err(ApiError::conflict(
            "that song isn't held by a worker any more",
        ));
    };
    if holder != name {
        return Err(ApiError::forbidden());
    }
    finish_track_result(state, name, id, &kind, &body).await?;
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
    /// "linux" or "windows"; nodes before this field are Linux.
    #[serde(default = "linux")]
    os: String,
    python: String,
    arch: String,
    #[serde(default)]
    path: Option<String>,
}

fn linux() -> String {
    "linux".to_owned()
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
        let file = crate::ai_kit::file_of(&state, &ask.os, &ask.python, &ask.arch, &model, &path)
            .ok_or_else(ApiError::not_found)?;
        return match ServeFile::new(file).oneshot(request).await {
            Ok(response) => Ok(response.map(Body::new).into_response()),
            Err(e) => match e {},
        };
    }
    let kit = crate::ai_kit::kit(&state, &ask.os, &ask.python, &ask.arch, &model);
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

    #[test]
    fn a_nodes_results_read_plainly_in_the_log() {
        let outcome = |lrc: Option<&str>, error: Option<&str>| Outcome {
            lrc: lrc.map(str::to_owned),
            error: error.map(str::to_owned),
            elapsed_ms: None,
        };
        let made = "[ar:A]\n[ti:T]\n[by:lyrics-listener (AI-generated; model=base; match=96%)]\n[00:01.00]la\n";
        assert_eq!(
            node_outcome(
                "Kitchen",
                "A - T",
                "listen",
                &outcome(Some(made), None),
                None,
                false
            ),
            "Kitchen: A - T, lyrics made (96% sure)"
        );
        assert_eq!(
            node_outcome(
                "Kitchen",
                "A - T",
                "listen",
                &outcome(None, Some("instrumental")),
                None,
                false
            ),
            "Kitchen: A - T is an instrumental"
        );
        let low = "scored too low (72%, under 85%)";
        assert_eq!(
            node_outcome("Kitchen", "A - T", "listen", &outcome(Some(made), None), Some(low), true),
            "Kitchen: A - T scored too low (72%, under 85%); passed to this computer for a second opinion"
        );
        assert_eq!(
            node_outcome(
                "Kitchen",
                "A - T",
                "listen",
                &outcome(Some(made), None),
                Some(low),
                false
            ),
            "Kitchen: A - T scored too low (72%, under 85%); given up"
        );
        assert_eq!(
            node_outcome(
                "Kitchen",
                "A - T",
                "listen",
                &outcome(None, Some("boom")),
                Some("boom"),
                false
            ),
            "Kitchen: A - T failed: boom"
        );
    }

    #[test]
    fn lyrics_scoring_under_the_minimum_are_refused() {
        let lrc = |by: &str| format!("[ar:A]\n[ti:T]\n[by:{by}]\n[00:01.00]la la\n");
        let made = lrc("lyrics-listener (AI-generated; model=small; match=84%)");
        let why = too_unsure(&made, 85).unwrap_or_default();
        assert!(why.starts_with(LOW_SCORE), "{why}");
        assert_eq!(too_unsure(&made, 84), None);
        let timed = lrc("lyrics-listener (AI-synced; model=small; match=90%)");
        assert_eq!(too_unsure(&timed, 85), None);
        assert!(too_unsure(&timed, 91).is_some());
        // No score given (an older worker, or timing alone): kept.
        let plain = lrc("lyrics-listener (AI-generated; model=small)");
        assert_eq!(too_unsure(&plain, 100), None);
    }

    #[tokio::test]
    async fn a_stripper_prepares_ahead_and_listeners_take_songs() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", djinn_store::Role::Admin).await;
        let songs: Vec<ScannedTrack> = ["One", "Two", "Three"]
            .iter()
            .map(|t| ScannedTrack {
                path: format!("/m/{t}.mp3"),
                title: (*t).into(),
                artist: Some("Band".into()),
                duration_ms: Some(30_000),
                ..Default::default()
            })
            .collect();
        state.store.save_scanned(&songs).await.unwrap();
        for row in state.store.compare_rows().await.unwrap() {
            state
                .store
                .save_read_lyrics(row.id, 1, &NewLyrics::NONE)
                .await
                .unwrap();
            state
                .store
                .save_looked_up_lyrics(row.id, None)
                .await
                .unwrap();
        }
        crate::settings::set_flag(&state.store, WORK_ON, true)
            .await
            .unwrap();
        let made = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/api/ai-work/keys")
            .header("cookie", &admin)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"pc"}"#))
            .unwrap();
        let response = crate::app(state.clone()).oneshot(made).await.unwrap();
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
        let ask = |body: &'static str| {
            let state = state.clone();
            let key = key.clone();
            async move {
                let request = axum::http::Request::builder()
                    .method(Method::POST)
                    .uri("/api/ai-work/next")
                    .header("authorization", format!("Worker {key}"))
                    .header("content-type", "application/json")
                    .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                        [192, 168, 1, 20],
                        4000,
                    ))))
                    .body(Body::from(body))
                    .unwrap();
                let response = crate::app(state).oneshot(request).await.unwrap();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                    .await
                    .unwrap();
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
            }
        };
        // The stripper takes one song at a time to separate its vocals.
        let batch = ask(r#"{"max":1,"can_isolate":true,"role":"prepare"}"#).await;
        assert_eq!(batch["jobs"].as_array().unwrap().len(), 1, "{batch}");
        assert_eq!(batch["jobs"][0]["prepare"], true, "{batch}");
        // A stripper that can't separate vocals gets nothing, and asks again soon.
        let batch = ask(r#"{"max":1,"can_isolate":false,"role":"prepare"}"#).await;
        assert_eq!(batch["jobs"].as_array().unwrap().len(), 0, "{batch}");
        assert_eq!(batch["wait_secs"], STRIPPING_WAIT_SECS, "{batch}");
        // With no stripper running here, a listener takes a whole song.
        let batch = ask(r#"{"max":1,"role":"listen"}"#).await;
        assert_eq!(batch["jobs"].as_array().unwrap().len(), 1, "{batch}");
        assert_eq!(batch["jobs"][0]["prepare"], false, "{batch}");
        assert_eq!(batch["jobs"][0]["vocals"], false, "{batch}");
        // An older worker that doesn't say its role does whole songs.
        let batch = ask(r#"{"max":2}"#).await;
        assert_eq!(batch["jobs"].as_array().unwrap().len(), 1, "{batch}");
    }

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
            state
                .store
                .ai_counts(false, LEASE_MS, &[])
                .await
                .unwrap()
                .done,
            1
        );
    }

    #[tokio::test]
    async fn editor_asked_and_import_jobs_run_even_when_background_work_is_off() {
        let dir = std::env::temp_dir().join(format!("djinn-ai-ondemand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("art")).unwrap();
        let state = AppState::for_tests_in(&dir).await;
        let admin = session(&state, "admin", djinn_store::Role::Admin).await;
        state
            .store
            .set_setting(crate::protected::LEVEL, "off")
            .await
            .unwrap();
        // Background AI work is explicitly off.
        crate::settings::set_flag(&state.store, WORK_ON, false)
            .await
            .unwrap();

        // Add a library song (no lyrics yet) and a staged import file.
        state
            .store
            .save_scanned(&[ScannedTrack {
                path: "/m/Asked.mp3".into(),
                title: "Asked".into(),
                artist: Some("Band".into()),
                duration_ms: Some(30_000),
                ..Default::default()
            }])
            .await
            .unwrap();
        let track_id = state.store.compare_rows().await.unwrap()[0].id;
        let fixture = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../djinn-engine/tests/fixtures/gapless_a.flac"),
        )
        .unwrap();
        let import_path = dir.join("waiting.flac");
        std::fs::write(&import_path, &fixture).unwrap();
        let batch = state.store.next_import_batch().await.unwrap();
        let import_id = state
            .store
            .add_import_file(
                batch,
                "01 Waiting.flac",
                &import_path.to_string_lossy(),
                i64::try_from(fixture.len()).unwrap(),
                None,
            )
            .await
            .unwrap();

        // Create a worker key.
        let made = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/api/ai-work/keys")
            .header("cookie", &admin)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"on-demand-pc"}"#))
            .unwrap();
        let res = crate::app(state.clone()).oneshot(made).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let key = text
            .split("\"key\":\"")
            .nth(1)
            .and_then(|b| b.split('"').next())
            .unwrap()
            .to_owned();

        // Ask the AI for both the staged import song and the library song from the editor.
        for body in [
            format!(r#"{{"ids":[{import_id}],"staged":true}}"#),
            format!(r#"{{"ids":[{track_id}],"staged":false}}"#),
        ] {
            let req = axum::http::Request::builder()
                .method(Method::POST)
                .uri("/api/editor/lyrics/generate")
                .header("cookie", &admin)
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap();
            let res = crate::app(state.clone()).oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }

        let worker_call = |method: Method, path: String, body: String| {
            let state = state.clone();
            let key = key.clone();
            async move {
                let req = axum::http::Request::builder()
                    .method(method)
                    .uri(path.as_str())
                    .header("authorization", format!("Worker {key}"))
                    .header("content-type", "application/json")
                    .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                        [192, 168, 1, 20],
                        4000,
                    ))))
                    .body(Body::from(body))
                    .unwrap();
                let res = crate::app(state).oneshot(req).await.unwrap();
                let status = res.status();
                let bytes = axum::body::to_bytes(res.into_body(), 2 << 20)
                    .await
                    .unwrap();
                (status, bytes)
            }
        };

        // Worker claims both jobs even though WORK_ON is false.
        let (status, bytes) = worker_call(
            Method::POST,
            "/api/ai-work/next".into(),
            r#"{"max":5}"#.into(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let batch_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let jobs = batch_json["jobs"].as_array().unwrap();
        assert_eq!(jobs.len(), 2, "{batch_json}");
        assert!(jobs.iter().any(|j| j["track_id"] == -import_id));
        assert!(jobs.iter().any(|j| j["track_id"] == track_id));
        assert!(import_working(&state, import_id));

        // Worker fetches the import audio via its negative id.
        let (status, audio) = worker_call(
            Method::GET,
            format!("/api/ai-work/song/{}", -import_id),
            String::new(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(audio.as_ref(), fixture.as_slice());

        // Worker posts AI lyrics for the import song -> sidecar .lrc written, ai_want cleared.
        let import_lrc = r#"{"lrc":"[ar:Band]\n[ti:Waiting]\n[by:lyrics-listener (AI-generated; model=m)]\n\n[00:00.00](AI-generated lyrics)\n[00:01.00]waiting words"}"#;
        let (status, _) = worker_call(
            Method::POST,
            format!("/api/ai-work/result/{}", -import_id),
            import_lrc.into(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(import_path.with_extension("lrc").is_file());
        assert!(!import_working(&state, import_id));
        assert!(state.store.import_ai_wanted().await.unwrap().is_empty());

        // Worker posts AI lyrics for the library song -> saved with ai=1.
        let track_lrc = r#"{"lrc":"[ar:Band]\n[ti:Asked]\n[by:lyrics-listener (AI-generated; model=m)]\n\n[00:00.00](AI-generated lyrics)\n[00:01.00]asked words"}"#;
        let (status, _) = worker_call(
            Method::POST,
            format!("/api/ai-work/result/{track_id}"),
            track_lrc.into(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        let got = state.store.lyrics(track_id).await.unwrap().unwrap();
        assert_eq!((got.ai, got.plain.as_deref()), (1, Some("asked words")));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
