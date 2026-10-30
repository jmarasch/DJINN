//! DJINN GO's optional converting: off unless the admin turns it on. Songs
//! become MP3s for devices that can't play the original format, for people
//! saving phone data, and for Subsonic apps asking for a lower bit rate.
//!
//! Each converted song is kept in `<db dir>/converted/` (oldest removed
//! past the size limit) so it's only converted once, and only a set number
//! convert at a time so the party never stutters.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use md5::Digest as _;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::auth::AdminUser;
use crate::backup::data_dir;
use crate::error::{ApiError, ApiResult};
use crate::settings;
use crate::state::AppState;

pub const CONVERT_ENABLED: &str = "convert_enabled";
pub const CONVERT_KBPS: &str = "convert_kbps";
pub const CONVERT_JOBS: &str = "convert_jobs";
pub const CONVERT_CACHE_MB: &str = "convert_cache_mb";
const DEFAULT_KBPS: i64 = 192;
const DEFAULT_JOBS: i64 = 1;
const DEFAULT_CACHE_MB: i64 = 2048;
const CACHE_DIR: &str = "converted";

pub fn router() -> Router<AppState> {
    Router::new().route("/go/convert", get(get_settings).put(put_settings))
}

/// Converting's running state.
pub struct Converter {
    /// Conversions allowed at once; replaced when the setting changes.
    slots: Mutex<Arc<Semaphore>>,
    /// One conversion per file and bit rate at a time; others wait for it.
    busy: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
}

impl Default for Converter {
    fn default() -> Self {
        Self {
            slots: Mutex::new(Arc::new(Semaphore::new(1))),
            busy: Mutex::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConvertSettings {
    pub enabled: bool,
    /// MP3 bit rate, kbit/s.
    pub kbps: i64,
    /// Conversions at once.
    pub jobs: i64,
    /// Space for converted songs.
    pub cache_mb: i64,
}

pub async fn settings_of(state: &AppState) -> anyhow::Result<ConvertSettings> {
    Ok(ConvertSettings {
        enabled: settings::flag(&state.store, CONVERT_ENABLED, false).await?,
        kbps: settings::number(&state.store, CONVERT_KBPS, DEFAULT_KBPS).await?,
        jobs: settings::number(&state.store, CONVERT_JOBS, DEFAULT_JOBS).await?,
        cache_mb: settings::number(&state.store, CONVERT_CACHE_MB, DEFAULT_CACHE_MB).await?,
    })
}

/// The bit rate to convert at for `user`, if converting is on and their
/// role allows it.
pub async fn available(state: &AppState, user: &djinn_store::User) -> anyhow::Result<Option<u32>> {
    let s = settings_of(state).await?;
    if !s.enabled || !state.store.permissions(user).await?.convert {
        return Ok(None);
    }
    Ok(u32::try_from(s.kbps).ok())
}

/// The nearest offered bit rate at or under `kbps` (the lowest if none).
pub fn snap(kbps: u32) -> u32 {
    let rates = djinn_cast::convert::BITRATES;
    rates
        .iter()
        .rev()
        .copied()
        .find(|r| *r <= kbps)
        .unwrap_or(rates[0])
}

fn cache_dir(state: &AppState) -> PathBuf {
    data_dir(state).join(CACHE_DIR)
}

/// The MP3 of `path` at `kbps`, converting it first unless it's cached.
/// The cache key includes the file's size and change time, so a changed
/// file is converted again.
pub async fn converted(state: &AppState, path: &Path, kbps: u32) -> ApiResult<PathBuf> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|_| ApiError::not_found())?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let key: String =
        md5::Md5::digest(format!("{}|{}|{modified}|{kbps}", path.display(), meta.len()).as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    let dir = cache_dir(state);
    let target = dir.join(format!("{key}.mp3"));
    // Someone else converting the same file: wait for them.
    let lock = {
        let mut busy = state
            .converter
            .busy
            .lock()
            .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        Arc::clone(busy.entry(target.clone()).or_default())
    };
    let _guard = lock.lock().await;
    if target.exists() {
        touch(&target);
        return Ok(target);
    }
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(anyhow::Error::from)?;
    let slots = state
        .converter
        .slots
        .lock()
        .map(|s| Arc::clone(&s))
        .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
    let _slot = slots.acquire_owned().await.map_err(anyhow::Error::from)?;
    let partial = dir.join(format!("{key}.part"));
    let started = std::time::Instant::now();
    let result = tokio::task::spawn_blocking({
        let (input, output) = (path.to_path_buf(), partial.clone());
        move || djinn_cast::convert::to_mp3(&input, &output, kbps)
    })
    .await
    .map_err(anyhow::Error::from)?;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        tracing::warn!(path = %path.display(), error = %e, "couldn't convert a song");
        return Err(ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            "couldn't convert that song",
        ));
    }
    tokio::fs::rename(&partial, &target)
        .await
        .map_err(anyhow::Error::from)?;
    tracing::info!(path = %path.display(), kbps, secs = started.elapsed().as_secs(), "song converted");
    if let Ok(mut busy) = state.converter.busy.lock() {
        busy.remove(&target);
    }
    let limit = settings::number(&state.store, CONVERT_CACHE_MB, DEFAULT_CACHE_MB).await?;
    let dir = cache_dir(state);
    tokio::task::spawn_blocking(move || {
        trim(&dir, u64::try_from(limit).unwrap_or(0) * 1024 * 1024)
    });
    Ok(target)
}

/// Marks a cached file as just used, so it's kept longest.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Removes the least recently used converted songs until the folder fits.
fn trim(dir: &Path, limit: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "mp3"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    files.sort_by_key(|(at, _, _)| *at);
    for (_, len, path) in files {
        if total <= limit {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

fn cache_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

#[derive(Serialize)]
struct SettingsView {
    #[serde(flatten)]
    settings: ConvertSettings,
    /// Space the converted songs take now.
    cache_used_mb: u64,
}

async fn view(state: &AppState) -> anyhow::Result<SettingsView> {
    let dir = cache_dir(state);
    let used = tokio::task::spawn_blocking(move || cache_bytes(&dir)).await?;
    Ok(SettingsView {
        settings: settings_of(state).await?,
        cache_used_mb: used / 1024 / 1024,
    })
}

async fn get_settings(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<SettingsView>> {
    Ok(Json(view(&state).await?))
}

async fn put_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<ConvertSettings>,
) -> ApiResult<Json<SettingsView>> {
    let kbps = u32::try_from(body.kbps).unwrap_or(0);
    if !djinn_cast::convert::BITRATES.contains(&kbps) {
        return Err(ApiError::bad_request("pick one of the offered bit rates"));
    }
    if !(1..=8).contains(&body.jobs) {
        return Err(ApiError::bad_request("1 to 8 conversions at once"));
    }
    if !(100..=1_000_000).contains(&body.cache_mb) {
        return Err(ApiError::bad_request(
            "keep 100 MB to 1000 GB of converted songs",
        ));
    }
    settings::set_flag(&state.store, CONVERT_ENABLED, body.enabled).await?;
    state
        .store
        .set_setting(CONVERT_KBPS, &body.kbps.to_string())
        .await?;
    state
        .store
        .set_setting(CONVERT_JOBS, &body.jobs.to_string())
        .await?;
    state
        .store
        .set_setting(CONVERT_CACHE_MB, &body.cache_mb.to_string())
        .await?;
    apply(&state).await?;
    let dir = cache_dir(&state);
    let limit = u64::try_from(body.cache_mb).unwrap_or(0) * 1024 * 1024;
    tokio::task::spawn_blocking(move || trim(&dir, limit));
    tracing::info!(
        on = body.enabled,
        kbps = body.kbps,
        jobs = body.jobs,
        "converting settings changed"
    );
    Ok(Json(view(&state).await?))
}

/// Sizes the conversion slots from the setting. At startup and on change.
pub async fn apply(state: &AppState) -> anyhow::Result<()> {
    let jobs = settings::number(&state.store, CONVERT_JOBS, DEFAULT_JOBS).await?;
    let permits = usize::try_from(jobs.clamp(1, 8)).unwrap_or(1);
    if let Ok(mut slots) = state.converter.slots.lock() {
        *slots = Arc::new(Semaphore::new(permits));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_rates_snap_down() {
        assert_eq!(snap(320), 320);
        assert_eq!(snap(200), 192);
        assert_eq!(snap(64), 96);
    }

    #[tokio::test]
    async fn converts_once_and_keeps_the_copy() {
        let dir = std::env::temp_dir().join(format!("djinn-convert-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("art")).unwrap();
        let state = AppState::for_tests_in(&dir).await;
        let song = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures/gapless_a.flac");
        let first = converted(&state, &song, 128).await.unwrap();
        let made = std::fs::metadata(&first).unwrap().len();
        assert!(made > 1000);
        let again = converted(&state, &song, 128).await.unwrap();
        assert_eq!(first, again, "cached");
        let other = converted(&state, &song, 192).await.unwrap();
        assert_ne!(first, other, "each bit rate is its own copy");
        trim(&cache_dir(&state), 0);
        assert!(!first.exists() && !other.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
