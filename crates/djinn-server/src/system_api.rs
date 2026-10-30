//! Admin > System: how DJINN is doing, its log, restarting or quitting it,
//! and clearing things out.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::ScanProgress;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::ApiResult;
use crate::events::{OutputStatus, PlayState};
use crate::state::AppState;
use crate::{folders, library_api, logs};

/// Settings a reset puts back to their defaults: the party rules, sound,
/// smart play, AI and update options. Accounts, music folders, speakers,
/// casts, the AI switches, and the AcoustID key and MusicBrainz contact
/// stay.
const RESET_SETTINGS: &[&str] = &[
    "active_playlist",
    "ai_genre_tags",
    "ai_listen_speed",
    "ai_reader_for",
    "crossfade_secs",
    "guest_mode",
    "history_window",
    "max_queue",
    "online_artist_info",
    "per_user_limit",
    "play_here",
    "playlist_cursor",
    "radio",
    "replaygain_mode",
    "replaygain_preamp_db",
    "skip_votes",
    "smart_play",
    "smart_playlists",
    "smart_strength",
    "tv_page",
    "update_auto",
    "update_check",
    "volume",
];

/// Most log lines one request returns.
const LOG_LINES: usize = 1000;
/// Most of the log file a download sends, from its end.
const LOG_DOWNLOAD_BYTES: u64 = 5_000_000;

/// Facts known once at startup.
struct Startup {
    started_ms: i64,
    database: PathBuf,
    addresses: Vec<String>,
}

static STARTUP: OnceLock<Startup> = OnceLock::new();

/// Records when DJINN started, its database, and the addresses guests use.
pub fn started(database: &Path, addresses: Vec<String>) {
    let _ = STARTUP.set(Startup {
        started_ms: djinn_store::now_ms(),
        database: database.to_path_buf(),
        addresses,
    });
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/system/status", get(status))
        .route("/system/log", get(log))
        .route("/system/log/file", get(log_file))
        .route("/system/restart", post(restart))
        .route("/system/quit", post(quit))
        .route("/system/reset", post(reset))
}

#[derive(Serialize)]
struct Status {
    version: &'static str,
    started_ms: Option<i64>,
    /// Where phones reach DJINN.
    addresses: Vec<String>,
    /// Windows has the network set to Public, which blocks phones.
    public_network: bool,
    songs: i64,
    library_folders: usize,
    scanning: bool,
    last_scan: Option<ScanProgress>,
    state: PlayState,
    outputs: OutputStatus,
    casts: Vec<CastLine>,
    ai_listening: bool,
    ai_reader: bool,
    data_dir: Option<String>,
    database_bytes: u64,
    art_bytes: u64,
    models_bytes: u64,
    log_file: Option<String>,
}

#[derive(Serialize)]
struct CastLine {
    name: String,
    playing: bool,
    error: Option<String>,
}

async fn status(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Status>> {
    let startup = STARTUP.get();
    let (play_state, outputs) = {
        let jukebox = state.jukebox.lock().await;
        (jukebox.player_state().state, jukebox.output_status())
    };
    let casts = state
        .caster
        .status()
        .into_iter()
        .filter(|c| c.active)
        .map(|c| CastLine {
            name: c.device.name,
            playing: c.playing,
            error: c.error,
        })
        .collect();
    let database = startup.map(|s| s.database.clone());
    let data_dir = state.art_dir.parent().map(Path::to_path_buf);
    let (database_bytes, art_bytes, models_bytes) = {
        let art = state.art_dir.as_ref().clone();
        tokio::task::spawn_blocking(move || {
            let db = database.as_deref().map_or(0, database_size);
            let models = art.parent().map_or(0, |d| folder_size(&d.join("models")));
            (db, folder_size(&art), models)
        })
        .await
        .unwrap_or_default()
    };
    Ok(Json(Status {
        version: env!("CARGO_PKG_VERSION"),
        started_ms: startup.map(|s| s.started_ms),
        addresses: startup.map(|s| s.addresses.clone()).unwrap_or_default(),
        public_network: state.network.public(),
        songs: state.store.track_count().await?,
        library_folders: folders::library_roots(&state).await?.len(),
        scanning: state
            .scan
            .running
            .load(std::sync::atomic::Ordering::Acquire),
        last_scan: state.scan.last.lock().ok().and_then(|l| l.clone()),
        state: play_state,
        outputs,
        casts,
        ai_listening: state.ai.model().is_some(),
        ai_reader: state.ai.reader.ready(),
        data_dir: data_dir.map(|d| d.display().to_string()),
        database_bytes,
        art_bytes,
        models_bytes,
        log_file: logs::file().map(|f| f.display().to_string()),
    }))
}

/// The database with its write-ahead log.
fn database_size(path: &Path) -> u64 {
    let name = path.as_os_str().to_string_lossy();
    ["", "-wal", "-shm"]
        .iter()
        .filter_map(|ext| std::fs::metadata(format!("{name}{ext}")).ok())
        .map(|m| m.len())
        .sum()
}

fn folder_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => folder_size(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

#[derive(Deserialize)]
struct LogQuery {
    /// Least serious level to show: trace, debug, info, warn or error.
    level: Option<String>,
    /// Only lines containing this.
    q: Option<String>,
}

#[derive(Serialize)]
struct LogLines {
    lines: Vec<String>,
}

async fn log(_: AdminUser, Query(query): Query<LogQuery>) -> Json<LogLines> {
    Json(LogLines {
        lines: logs::recent(query.level.as_deref(), query.q.as_deref(), LOG_LINES),
    })
}

/// The log file (its last few megabytes), or the lines kept in memory.
async fn log_file(_: AdminUser) -> ApiResult<impl IntoResponse> {
    let text = match logs::file() {
        Some(path) => {
            let path = path.to_path_buf();
            tokio::task::spawn_blocking(move || file_tail(&path, LOG_DOWNLOAD_BYTES))
                .await
                .map_err(anyhow::Error::from)??
        }
        None => logs::all(),
    };
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"djinn.log\"",
            ),
        ],
        text,
    ))
}

/// The last `bytes` of a file, from the start of a line.
fn file_tail(path: &Path, bytes: u64) -> anyhow::Result<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(match (start > 0, text.find('\n')) {
        (true, Some(i)) => text[i + 1..].to_owned(),
        _ => text,
    })
}

async fn restart(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    tracing::info!("restart requested from Admin > System");
    state.update.restart(&state.shutdown_request);
    StatusCode::ACCEPTED
}

async fn quit(_: AdminUser, State(state): State<AppState>) -> StatusCode {
    tracing::info!("quit requested from Admin > System");
    state.shutdown_request.notify_one();
    StatusCode::ACCEPTED
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Reset {
    Queue,
    TagEdits,
    Analysis,
    Listening,
    History,
    Settings,
}

#[derive(Deserialize)]
struct ResetRequest {
    what: Reset,
}

#[derive(Serialize)]
struct ResetDone {
    /// DJINN restarts to start over cleanly.
    restarting: bool,
}

async fn reset(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<ResetRequest>,
) -> ApiResult<Json<ResetDone>> {
    tracing::info!(what = ?body.what, "reset from Admin > System");
    let restarting = match body.what {
        Reset::Queue => {
            state.jukebox.lock().await.clear_queue().await?;
            false
        }
        Reset::TagEdits => {
            let paths = state.store.clear_tag_edits().await?;
            if !paths.is_empty() {
                library_api::reread(&state, &paths).await?;
            }
            false
        }
        // These change what's loaded in memory all over; a restart picks
        // everything up afresh (and the music carries on where it was).
        Reset::Analysis => {
            state.store.clear_analysis().await?;
            true
        }
        Reset::Listening => {
            state.store.clear_listening().await?;
            true
        }
        Reset::History => {
            state.store.clear_history().await?;
            true
        }
        Reset::Settings => {
            state.store.reset_settings(RESET_SETTINGS).await?;
            true
        }
    };
    if restarting {
        state.update.restart(&state.shutdown_request);
    }
    Ok(Json(ResetDone { restarting }))
}

#[cfg(test)]
mod tests {
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    use crate::state::AppState;
    use crate::test_util::{call, get_body, session};

    #[tokio::test]
    async fn only_admins_use_system_tools() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let user = session(&state, "sam", Role::User).await;
        let post = |body: &'static str, who: String| {
            let state = state.clone();
            async move {
                call(
                    &state,
                    Method::POST,
                    "/api/system/reset",
                    Some(body),
                    Some(&who),
                )
                .await
                .0
            }
        };

        let (status, _) = get_body(&state, "/api/system/status", &user).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            post(r#"{"what":"queue"}"#, user.clone()).await,
            StatusCode::FORBIDDEN
        );

        let (status, body) = get_body(&state, "/api/system/status", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"songs\":0"), "{body}");
        assert_eq!(
            post(r#"{"what":"queue"}"#, admin.clone()).await,
            StatusCode::OK
        );
        assert!(
            post(r#"{"what":"everything"}"#, admin.clone())
                .await
                .is_client_error(),
            "unknown resets are refused"
        );
        let (status, _) = get_body(&state, "/api/system/log?level=warn", &admin).await;
        assert_eq!(status, StatusCode::OK);
    }
}
