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
    last_seen: i64,
    done: u64,
    failed: u64,
}

static WORKERS: Mutex<Option<HashMap<String, Seen>>> = Mutex::new(None);

fn seen(name: &str, f: impl FnOnce(&mut Seen)) {
    if let Ok(mut guard) = WORKERS.lock() {
        let entry = guard
            .get_or_insert_with(HashMap::new)
            .entry(name.to_owned())
            .or_default();
        entry.last_seen = djinn_store::now_ms();
        f(entry);
    }
}

// --- Admin ---

#[derive(Serialize)]
struct Status {
    on: bool,
    new_songs: bool,
    sync_plain: bool,
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
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<Settings>,
) -> ApiResult<StatusCode> {
    settings::set_flag(&state.store, WORK_ON, body.on).await?;
    settings::set_flag(&state.store, WORK_NEW, body.new_songs).await?;
    settings::set_flag(&state.store, WORK_SYNC, body.sync_plain).await?;
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
}

#[derive(Serialize)]
struct Batch {
    jobs: Vec<djinn_store::AiJob>,
    /// Nothing handed out: ask again after this many seconds.
    wait_secs: Option<u64>,
    /// Why, when it's paused.
    held: Option<crate::background::Held>,
}

async fn next(
    Worker(name): Worker,
    State(state): State<AppState>,
    Json(body): Json<Next>,
) -> ApiResult<Json<Batch>> {
    seen(&name, |_| {});
    let nothing = |held| Batch {
        jobs: Vec::new(),
        wait_secs: Some(WAIT_SECS),
        held,
    };
    if !settings::flag(&state.store, WORK_ON, false).await? {
        return Ok(Json(nothing(None)));
    }
    if let Some(held) = crate::background::held(&state).await {
        return Ok(Json(nothing(Some(held))));
    }
    let max = i64::try_from(body.max.clamp(1, BATCH_MAX)).unwrap_or(1);
    let jobs = state
        .store
        .ai_claim(
            &name,
            settings::flag(&state.store, WORK_NEW, true).await?,
            settings::flag(&state.store, WORK_SYNC, false).await?,
            lookups_off(&state).await?,
            max,
            LEASE_MS,
        )
        .await?;
    Ok(Json(if jobs.is_empty() {
        nothing(None)
    } else {
        Batch {
            jobs,
            wait_secs: None,
            held: None,
        }
    }))
}

/// A song's audio, only to the worker that holds it.
async fn song(
    Worker(name): Worker,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    request: Request,
) -> ApiResult<Response> {
    match state.store.ai_holder(id).await? {
        Some((holder, _)) if holder == name => {}
        _ => return Err(ApiError::forbidden()),
    }
    let path = state
        .store
        .track_path(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    match ServeFile::new(path).oneshot(request).await {
        Ok(response) => Ok(response.map(Body::new).into_response()),
        Err(e) => match e {},
    }
}

#[derive(Deserialize)]
struct Outcome {
    /// The `.lrc` text.
    lrc: Option<String>,
    /// Why there isn't one.
    error: Option<String>,
}

async fn result(
    Worker(name): Worker,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<i64>,
    Json(body): Json<Outcome>,
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
        (Some(text), _) => store_from_worker(&state, id, text, kind_number, &name)
            .await
            .err(),
        (None, Some(error)) => Some(error.chars().take(200).collect()),
        (None, None) => Some("sent nothing".to_owned()),
    };
    state.store.ai_finish(id, failure.as_deref()).await?;
    seen(&name, |s| {
        if failure.is_some() {
            s.failed += 1;
        } else {
            s.done += 1;
        }
    });
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
