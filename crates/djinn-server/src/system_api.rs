//! Admin > System: how DJINN is doing, its log, restarting or quitting it,
//! and clearing things out.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axum::extract::{Path as RoutePath, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_library::ScanProgress;
use djinn_store::Store;
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::config::Config;
use crate::error::{ApiError, ApiResult};
use crate::events::{OutputStatus, PlayState};
use crate::state::AppState;
use crate::{folders, library_api, logs, settings};

/// Settings a reset puts back to their defaults: the party rules, sound,
/// smart play, AI and update options. Accounts, music folders, speakers,
/// casts, the AI switches, and the AcoustID key and MusicBrainz contact
/// stay.
const RESET_SETTINGS: &[&str] = &[
    "active_playlist",
    "ai_genre_tags",
    "ai_listen_speed",
    "ai_reader_for",
    "artist_pictures",
    "artist_picture_count",
    "artist_picture_per_artist_mb",
    "artist_picture_max_edge",
    "artist_picture_min_width",
    "artist_picture_min_height",
    "art_cache_max_mb",
    "artist_source_commons",
    "artist_source_deezer",
    "artist_source_wikipedia",
    "artist_source_fanart",
    "fanart_api_key",
    "artist_source_audiodb",
    "audiodb_api_key",
    "avoid_copies",
    "artist_source_unsplash",
    "unsplash_access_key",
    "artist_remote_images",
    "background_from",
    "background_until",
    "background_when",
    "crossfade_secs",
    "guest_mode",
    "history_window",
    "max_queue",
    "musicbrainz",
    "online_artist_info",
    "per_user_limit",
    "play_here",
    "playlist_cursor",
    "radio",
    "replaygain_mode",
    "replaygain_preamp_db",
    "scheduled_check",
    "scheduled_check_hour",
    "skip_votes",
    "smart_play",
    "smart_playlists",
    "smart_strength",
    "sound_analysis",
    "tv_page",
    "tv_sound",
    "update_auto",
    "update_check",
    "volume",
];

/// Next to the database: DJINN starts over from scratch at the next start.
const RESET_MARKER: &str = "djinn.reset";

/// Most log lines one request returns.
const LOG_LINES: usize = 1000;
/// Most of the log file a download sends, from its end.
const LOG_DOWNLOAD_BYTES: u64 = 5_000_000;

/// Facts known once at startup.
struct Startup {
    started_ms: i64,
    database: PathBuf,
    addresses: Vec<String>,
    party_listen: SocketAddr,
}

static STARTUP: OnceLock<Startup> = OnceLock::new();

/// Records when DJINN started, its database, and the addresses guests use.
pub fn started(database: &Path, addresses: Vec<String>, party_listen: SocketAddr) {
    let _ = STARTUP.set(Startup {
        started_ms: djinn_store::now_ms(),
        database: database.to_path_buf(),
        addresses,
        party_listen,
    });
}

const PARTY_PORT_SETTING: &str = "party_port";
const GO_PORT_SETTING: &str = "go_port";

/// Admin-selected ports override the config file after the database opens.
/// They take effect together at the next process start.
pub async fn apply_ports(store: &Store, config: &mut Config) -> anyhow::Result<()> {
    let party = saved_port(store, PARTY_PORT_SETTING, config.server.listen.port()).await?;
    let go = saved_port(store, GO_PORT_SETTING, config.go.listen.port()).await?;
    anyhow::ensure!(party != go, "party and GO ports must be different");
    config.server.listen.set_port(party);
    config.go.listen.set_port(go);
    Ok(())
}

async fn saved_port(store: &Store, key: &str, default: u16) -> anyhow::Result<u16> {
    Ok(store
        .get_setting(key)
        .await?
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(default))
}

/// At start, before the database opens: if a full reset is waiting,
/// deletes the database (and any copy kept from before a restore), the
/// artwork cache and a waiting restore, so DJINN starts as if just
/// installed. The music, the AI models and the log stay.
pub fn reset_if_asked(db: &Path, art_dir: &Path) {
    let Some(dir) = db.parent() else { return };
    let marker = dir.join(RESET_MARKER);
    if !marker.exists() {
        return;
    }
    let name = db.file_name().unwrap_or_default().to_string_lossy();
    for copy in [String::new(), ".before-restore".to_owned()] {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(dir.join(format!("{name}{copy}{suffix}")));
        }
    }
    let _ = std::fs::remove_dir_all(art_dir);
    let _ = std::fs::remove_dir_all(dir.join(crate::backup::RESTORE_DIR));
    match std::fs::remove_file(&marker) {
        Ok(()) => tracing::warn!("DJINN was reset: starting over from scratch"),
        Err(e) => tracing::error!(error = %e, "couldn't remove the reset marker"),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/settings/changed", get(changed_settings))
        .route("/system/status", get(status))
        .route("/system/ports", get(ports).put(save_ports))
        .route("/system/log", get(log))
        .route("/system/log/file", get(log_file))
        .route("/system/log/files", get(log_files))
        .route("/system/log/files/{day}", get(log_file_for_day))
        .route(
            "/system/log/settings",
            get(log_settings).put(save_log_settings),
        )
        .route("/system/restart", post(restart))
        .route("/system/quit", post(quit))
        .route("/system/reset", post(reset))
}

#[derive(Serialize)]
struct Ports {
    party_port: u16,
    go_port: u16,
    pending_party_port: u16,
    pending_go_port: u16,
}

#[derive(Deserialize)]
struct PortChange {
    party_port: u16,
    go_port: u16,
}

async fn port_status(state: &AppState) -> anyhow::Result<Ports> {
    let party_port = STARTUP
        .get()
        .map_or(8066, |startup| startup.party_listen.port());
    let go_port = state.go.port();
    Ok(Ports {
        party_port,
        go_port,
        pending_party_port: saved_port(&state.store, PARTY_PORT_SETTING, party_port).await?,
        pending_go_port: saved_port(&state.store, GO_PORT_SETTING, go_port).await?,
    })
}

async fn ports(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Ports>> {
    Ok(Json(port_status(&state).await?))
}

async fn save_ports(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<PortChange>,
) -> ApiResult<Json<Ports>> {
    if body.party_port == 0 || body.go_port == 0 || body.party_port == body.go_port {
        return Err(ApiError::bad_request(
            "choose two different ports from 1 to 65535",
        ));
    }
    let active = port_status(&state).await?;
    let party_ip = STARTUP
        .get()
        .map_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED), |startup| {
            startup.party_listen.ip()
        });
    if body.party_port != active.party_port && body.party_port != active.go_port {
        check_port(party_ip, body.party_port).await?;
    }
    if body.go_port != active.go_port && body.go_port != active.party_port {
        check_port(IpAddr::V4(Ipv4Addr::UNSPECIFIED), body.go_port).await?;
    }
    state
        .store
        .set_setting(PARTY_PORT_SETTING, &body.party_port.to_string())
        .await?;
    state
        .store
        .set_setting(GO_PORT_SETTING, &body.go_port.to_string())
        .await?;
    tracing::warn!(
        party_port = body.party_port,
        go_port = body.go_port,
        "DJINN ports saved; restart to apply"
    );
    Ok(Json(port_status(&state).await?))
}

async fn check_port(ip: IpAddr, port: u16) -> ApiResult<()> {
    let address = SocketAddr::new(ip, port);
    tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| ApiError::conflict(format!("port {port} is unavailable: {error}")))?;
    Ok(())
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
    /// How busy the processor is, 0 to 1.
    cpu: Option<f32>,
    memory_total: Option<u64>,
    memory_available: Option<u64>,
    /// The disk holding DJINN's data.
    disk_total: Option<u64>,
    disk_available: Option<u64>,
    /// When a backup was last downloaded (Unix ms).
    last_backup_ms: Option<i64>,
}

/// Settings set to something other than their default, by key.
async fn changed_settings(
    _: AdminUser,
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<&'static str>>> {
    Ok(Json(settings::changed(&state.store).await?))
}

/// The processor reading before the last, to measure how busy it's been
/// since.
static CPU: std::sync::Mutex<Option<(std::time::Instant, djinn_platform::system::CpuTimes)>> =
    std::sync::Mutex::new(None);

/// How busy the processor has been since the last status, or over a
/// quarter second if that was long ago or never.
async fn cpu_busy() -> Option<f32> {
    use djinn_platform::system::cpu_times;
    let earlier = CPU
        .lock()
        .ok()
        .and_then(|c| *c)
        .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(60))
        .map(|(_, t)| t);
    let earlier = match earlier {
        Some(t) => t,
        None => {
            let first = cpu_times()?;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            first
        }
    };
    let now = cpu_times()?;
    if let Ok(mut last) = CPU.lock() {
        *last = Some((std::time::Instant::now(), now));
    }
    now.busy_since(&earlier)
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
    let memory = djinn_platform::system::memory();
    let disk = data_dir.as_deref().and_then(djinn_platform::system::disk);
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
        version: env!("DJINN_RELEASE_VERSION"),
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
        cpu: cpu_busy().await,
        memory_total: memory.map(|m| m.total),
        memory_available: memory.map(|m| m.available),
        disk_total: disk.map(|d| d.total),
        disk_available: disk.map(|d| d.available),
        last_backup_ms: settings::number(&state.store, crate::backup::LAST_BACKUP, 0)
            .await
            .ok()
            .filter(|&ms| ms > 0),
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

#[derive(Deserialize, Serialize)]
struct LogSettings {
    level: String,
    keep_days: u16,
}

async fn log_settings(_: AdminUser) -> Json<LogSettings> {
    Json(LogSettings {
        level: logs::record_level().to_owned(),
        keep_days: logs::keep_days(),
    })
}

async fn save_log_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<LogSettings>,
) -> ApiResult<Json<LogSettings>> {
    if !logs::valid_level(&body.level) {
        return Err(ApiError::bad_request(
            "choose full tracing, warnings and errors, or errors only",
        ));
    }
    if !(1..=logs::MAX_DAYS).contains(&body.keep_days) {
        return Err(ApiError::bad_request("keep logs for 1 to 365 days"));
    }
    state
        .store
        .set_setting(logs::LEVEL_SETTING, &body.level)
        .await?;
    state
        .store
        .set_setting(logs::DAYS_SETTING, &body.keep_days.to_string())
        .await?;
    logs::set_record_level(&body.level);
    logs::set_keep_days(body.keep_days)?;
    tracing::warn!(level = %body.level, keep_days = body.keep_days, "log settings changed");
    Ok(Json(body))
}

/// The log file (its last few megabytes), or the lines kept in memory.
async fn log_file(_: AdminUser) -> ApiResult<impl IntoResponse> {
    let text = match logs::file() {
        Some(path) => tokio::task::spawn_blocking(move || file_tail(&path, LOG_DOWNLOAD_BYTES))
            .await
            .map_err(anyhow::Error::from)??,
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

async fn log_files(_: AdminUser) -> ApiResult<Json<Vec<String>>> {
    Ok(Json(logs::available_days()?))
}

async fn log_file_for_day(
    _: AdminUser,
    RoutePath(day): RoutePath<String>,
) -> ApiResult<impl IntoResponse> {
    let path = logs::file_for_day(&day).ok_or_else(ApiError::not_found)?;
    let text = tokio::task::spawn_blocking(move || file_tail(&path, LOG_DOWNLOAD_BYTES))
        .await
        .map_err(anyhow::Error::from)??;
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"djinn-{day}.log\""),
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
    /// Everything DJINN knows, as if just installed.
    Everything,
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
        // Done at the next start, before the database opens.
        Reset::Everything => {
            let dir = STARTUP
                .get()
                .and_then(|s| s.database.parent().map(Path::to_path_buf))
                .ok_or_else(|| anyhow::anyhow!("DJINN's data folder is unknown"))?;
            tokio::fs::write(dir.join(RESET_MARKER), b"").await?;
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
    use std::path::Path;

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
        let (status, _) = get_body(&state, "/api/system/ports", &user).await;
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
            post(r#"{"what":"music"}"#, admin.clone())
                .await
                .is_client_error(),
            "unknown resets are refused; the music is never one"
        );
        let (status, _) = get_body(&state, "/api/system/log?level=warn", &admin).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn changed_settings_are_only_those_off_their_default() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        state.store.set_setting("guest_mode", "1").await.unwrap();
        // Saved, but at its default.
        state
            .store
            .set_setting("per_user_limit", "3")
            .await
            .unwrap();
        let (status, body) = get_body(&state, "/api/settings/changed", &admin).await;
        assert_eq!(status, StatusCode::OK);
        let changed: Vec<String> = serde_json::from_str(&body).unwrap();
        assert_eq!(changed, ["guest_mode"]);
        let (status, body) = get_body(&state, "/api/system/status", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"disk_available\""), "{body}");
    }

    #[tokio::test]
    async fn admin_ports_apply_after_restart() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "admin", Role::Admin).await;
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/system/ports",
            Some(r#"{"party_port":8068,"go_port":8068}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/system/ports",
            Some(r#"{"party_port":18066,"go_port":18067}"#),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = get_body(&state, "/api/system/ports", &admin).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"pending_party_port\":18066"), "{body}");
        assert!(body.contains("\"pending_go_port\":18067"), "{body}");

        let mut config = crate::config::Config::default();
        super::apply_ports(&state.store, &mut config).await.unwrap();
        assert_eq!(config.server.listen.port(), 18066);
        assert_eq!(config.go.listen.port(), 18067);
    }

    #[test]
    fn a_full_reset_clears_djinns_data_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("djinn-reset-{}", std::process::id()));
        let art = dir.join("art");
        std::fs::create_dir_all(&art).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let db = dir.join("djinn.db");
        for file in [
            "djinn.db",
            "djinn.db-wal",
            "djinn.db.before-restore",
            "djinn.log",
        ] {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        std::fs::write(art.join("cover.jpg"), b"x").unwrap();

        super::reset_if_asked(&db, &art);
        assert!(db.exists(), "nothing happens without the marker");

        std::fs::write(dir.join(super::RESET_MARKER), b"").unwrap();
        super::reset_if_asked(&db, &art);
        let left = |p: &str| Path::new(&dir).join(p).exists();
        assert!(!left("djinn.db") && !left("djinn.db-wal") && !left("djinn.db.before-restore"));
        assert!(!left("art") && !left(super::RESET_MARKER));
        assert!(
            left("models") && left("djinn.log"),
            "models and the log stay"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
