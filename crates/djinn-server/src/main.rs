//! `djinn`: headless party jukebox server.

// Release builds on Windows run without a console window (the tray icon is
// the interface); see `djinn_platform::console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ai;
mod api;
mod audio_api;
mod auth;
mod backup;
mod banner;
mod cast_api;
mod client;
mod config;
mod desktop;
mod download;
mod error;
mod events;
mod folders;
mod gpu;
mod identify_api;
mod jukebox;
mod library_api;
mod logs;
mod network;
mod party_api;
mod player_api;
mod playlist_api;
mod reader;
mod settings;
mod smart;
mod sse;
mod state;
mod system_api;
#[cfg(test)]
mod test_util;
mod tv_api;
mod update;
mod watcher;
mod web;

use std::path::PathBuf;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use djinn_engine::{Engine, EngineConfig};
use djinn_store::Store;
use tokio::sync::{broadcast, mpsc, Notify};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::{Config, Source};
use crate::jukebox::Jukebox;
use crate::state::AppState;

#[derive(Parser)]
#[command(name = "djinn", version, about)]
struct Cli {
    /// Path to config.toml. Defaults to next to the binary on Windows and
    /// ~/.config/djinn/config.toml elsewhere.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Don't show a system tray icon, even on a desktop.
    #[arg(long)]
    no_tray: bool,
    /// Write logs to this file instead of the terminal.
    #[arg(long)]
    log_file: Option<PathBuf>,
}

#[tokio::main]
async fn main() {
    let code = match run().await {
        Ok(()) => 0,
        Err(e) => {
            // Started from the Start menu or at sign-in there is no console
            // for the error to go to, so put it in the log too.
            tracing::error!(error = %format!("{e:#}"), "DJINN stopped");
            eprintln!("Error: {e:?}");
            1
        }
    };
    // Exit now: background jobs on blocking threads (online lookups,
    // analysis, AI listening) would otherwise keep the process alive after
    // it has shut down, until they finish.
    std::process::exit(code)
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Without a console (Windows release build started from Explorer or the
    // Start menu), logs go to a file.
    let has_console = djinn_platform::console::attach_parent_console();
    let log_file = cli
        .log_file
        .clone()
        .or_else(|| (!has_console).then(default_log_file).flatten());
    init_logging(log_file.as_deref())?;
    if !has_console {
        // Otherwise loading the listening model flashes a console window.
        djinn_ai::settle_cache_probe();
    }

    let (config, source) = Config::load(cli.config.as_deref())?;
    match &source {
        Source::File(path) => tracing::info!(path = %path.display(), "loaded config"),
        Source::Defaults { looked_at } => tracing::info!(
            looked_at = ?looked_at.as_ref().map(|p| p.display().to_string()),
            "no config file found, using defaults"
        ),
    }

    let art_dir = config
        .database
        .path
        .parent()
        .map_or_else(|| PathBuf::from("art"), |dir| dir.join("art"));

    // A full reset, or a backup waiting to be restored, happens before the
    // database opens.
    system_api::reset_if_asked(&config.database.path, &art_dir);
    let restored = backup::put_in_place(&config.database.path, &art_dir);
    let opened = Store::open(&config.database.path).await;
    let store = match (opened, &restored) {
        (Ok(store), _) => store,
        (Err(e), Some(restored)) => {
            tracing::error!(error = %e, "the restored database won't open");
            backup::undo(&config.database.path, restored);
            Store::open(&config.database.path).await?
        }
        (Err(e), None) => {
            return Err(e).with_context(|| {
                format!("failed to open database {}", config.database.path.display())
            })
        }
    };
    if let Some(restored) = &restored {
        backup::finish(&store, &config.library.roots, restored).await?;
    }
    tracing::info!(path = %config.database.path.display(), "database ready");
    let removed = store.delete_expired_sessions().await?;
    if removed > 0 {
        tracing::debug!(removed, "expired sessions cleaned up");
    }

    // The crossfade is a runtime setting; config.toml gives its first value.
    if store.get_setting(jukebox::CROSSFADE).await?.is_none() {
        store
            .set_setting(jukebox::CROSSFADE, &config.audio.crossfade_secs.to_string())
            .await?;
    }

    // Engine events are handed to the async side through a channel; the
    // callback runs on the engine thread and must not block.
    let (engine_tx, mut engine_rx) = mpsc::unbounded_channel();
    let outputs = audio_api::engine_configs(&store.outputs().await?);
    // A copy of the mix for casting, encoded to MP3 while anyone listens.
    let (tap, tap_reader) = djinn_engine::stream_tap(4.0, config.audio.master_rate);
    let stream = djinn_cast::stream::LiveStream::start(tap_reader, config.audio.master_rate)?;
    let engine = Engine::start_with_tap(
        EngineConfig {
            master_rate: config.audio.master_rate,
            outputs,
        },
        Some(tap),
        move |event| {
            let _ = engine_tx.send(event);
        },
    )?;
    let engine = std::sync::Arc::new(engine);
    let engine_sink = std::sync::Arc::clone(&engine);
    let (events, _) = broadcast::channel(256);
    let jukebox = Jukebox::new(
        store.clone(),
        Box::new(move |command| {
            if engine_sink.send(command).is_err() {
                tracing::error!("audio engine stopped");
            }
        }),
        events.clone(),
    )
    .await?;
    let urls = banner::urls(config.server.listen);
    system_api::started(&config.database.path, urls.clone());
    let base_url = urls.first().cloned().unwrap_or_default();
    // Older cache files had no extension; give them one so the folder is
    // easy to browse. Once is enough.
    const ART_NAMED: &str = "art_extensions_named";
    if store.get_setting(ART_NAMED).await?.is_none() {
        let renamed = {
            let art_dir = art_dir.clone();
            tokio::task::spawn_blocking(move || djinn_library::art::add_extensions(&art_dir))
                .await?
        };
        if renamed > 0 {
            tracing::info!(renamed, "named cached images with file extensions");
        }
        store.set_setting(ART_NAMED, "1").await?;
    }
    let state = AppState::new(
        store.clone(),
        art_dir,
        config.library.roots.clone(),
        jukebox,
        events,
    )
    .with_join_url(base_url.clone())
    .with_stream(stream, cast_api::token(&store).await?);

    state
        .jukebox
        .lock()
        .await
        .set_mixer(std::sync::Arc::new(smart::SmartMixer::new(&state)));

    let guest_mode = settings::flag(&store, settings::GUEST_MODE, false).await?;
    state
        .guest_mode
        .store(guest_mode, std::sync::atomic::Ordering::Relaxed);

    let jukebox = std::sync::Arc::clone(&state.jukebox);
    tokio::spawn(async move {
        while let Some(event) = engine_rx.recv().await {
            if let Err(e) = jukebox.lock().await.on_engine_event(event).await {
                tracing::error!(error = %format!("{e:#}"), "handling engine event failed");
            }
        }
    });

    let setup_url = if store.admin_exists().await? {
        None
    } else {
        let token = auth::new_token()?;
        let url = format!("{base_url}/setup?token={token}");
        tracing::warn!("no admin account yet. Create one at {url}");
        if let Ok(mut slot) = state.bootstrap_token.lock() {
            *slot = Some(token);
        }
        Some(url)
    };

    let listener = match bind(config.server.listen).await {
        Ok(listener) => listener,
        // Probably DJINN is already running (opened twice from the app
        // menu): show the running one instead of failing silently.
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && !cli.no_tray => {
            tracing::warn!(addr = %config.server.listen, "already in use, opening the running DJINN");
            desktop::open_existing(config.server.listen);
            return Ok(());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("failed to bind {}", config.server.listen));
        }
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        addr = %config.server.listen,
        "DJINN listening"
    );
    match &setup_url {
        Some(url) => banner::print_qr(url, "Scan to create the admin account:"),
        None => banner::print_qr(&base_url, "Scan to join the jukebox:"),
    }
    if urls.len() > 1 {
        tracing::info!(urls = ?&urls[1..], "also reachable at");
    }
    let tray = if config.server.tray && !cli.no_tray {
        desktop::start_tray(&state, config.server.listen)
    } else {
        None
    };
    // First start on a desktop: go straight to creating the admin.
    if tray.is_some() {
        let token = state.bootstrap_token.lock().ok().and_then(|t| t.clone());
        if let Some(token) = token {
            desktop::open_setup(config.server.listen, &token);
        }
    }

    // Ready to serve from what the database already knows; look for
    // changes on disk in the background.
    library_api::start_scan(&state);
    cast_api::spawn_background(&state);
    {
        // Load what's been analyzed, then analyze anything new.
        let state = state.clone();
        tokio::spawn(async move {
            smart::refresh(&state).await;
            library_api::retry_unfinished_lookups(&state).await;
            audio_api::start_analysis(&state).await;
            ai::start(&state);
            reader::start(&state);
        });
    }
    watcher::start(state.clone());
    update::start(&state);
    network::refresh(&state);
    let update = std::sync::Arc::clone(&state.update);
    let jukebox = std::sync::Arc::clone(&state.jukebox);

    let shutdown = std::sync::Arc::clone(&state.shutdown);
    // Windows is signing out: every shutdown step gets less time.
    let session_ending = std::sync::Arc::clone(&state.session_ending);
    let hurry = move || session_ending.load(std::sync::atomic::Ordering::Acquire);
    let mut stopping = state.shutdown.subscribe();
    let shutdown_request = std::sync::Arc::clone(&state.shutdown_request);
    let server = axum::serve(
        listener,
        // Connection info gives handlers the client's IP (for MAC lookup).
        app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal(shutdown_request).await;
        shutdown.send_replace(true);
    });
    // Long-lived responses end on shutdown; anything still open after a
    // few seconds (one, when Windows is signing out) doesn't get to keep
    // DJINN running.
    let deadline_hurry = hurry.clone();
    let deadline = async move {
        let _ = stopping.wait_for(|stop| *stop).await;
        tracing::info!("stopping the web server");
        let grace = if deadline_hurry() {
            QUICK_STOP_WAIT
        } else {
            SHUTDOWN_GRACE
        };
        tokio::time::sleep(grace).await;
    };
    tokio::select! {
        result = async move { server.await } => result.context("server error")?,
        () = deadline => tracing::warn!("connections still open; shutting down anyway"),
    }

    // Remember how far the song playing got, to carry on from there.
    let saved = tokio::time::timeout(QUICK_STOP_WAIT, async {
        jukebox.lock().await.save_resume().await
    })
    .await;
    if !matches!(saved, Ok(Ok(()))) {
        tracing::warn!("couldn't save where playback was");
    }

    // Stop audio before closing the database. Neither may hold up the end:
    // at sign-out Windows' audio service may be going away too, and Windows
    // only waits a few seconds in all.
    let stop_wait = if hurry() { QUICK_STOP_WAIT } else { STOP_WAIT };
    tracing::info!("stopping audio");
    let (stopped, audio_stopped) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(engine);
        let _ = stopped.send(());
    });
    if audio_stopped.recv_timeout(stop_wait).is_err() {
        tracing::warn!("audio didn't stop in time; carrying on");
    }
    tracing::info!("closing the database");
    if tokio::time::timeout(stop_wait, store.close())
        .await
        .is_err()
    {
        tracing::warn!("the database was still busy; closing anyway");
    }
    tracing::info!("shut down");
    // Remove the tray icon before exiting (Windows leaves a ghost icon
    // otherwise). Last, since on Windows it also lets a waiting sign-out or
    // installer go ahead, and Windows may end DJINN the moment it does.
    drop(tray);
    if update.restart_requested() {
        if let Some(exe) = update.exe() {
            tracing::info!("starting the new version");
            update::restart(exe).context("starting the new version failed")?;
        }
    }
    Ok(())
}

/// Binds the listen address. Right after an update restart the old process
/// may still hold it for a moment, so keep trying for a few seconds.
async fn bind(addr: std::net::SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let restarted = std::env::var_os(update::RESTARTED).is_some();
    let mut tries = 0;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Err(e) if restarted && e.kind() == std::io::ErrorKind::AddrInUse && tries < 40 => {
                tries += 1;
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            result => return result,
        }
    }
}

fn init_logging(file: Option<&std::path::Path>) -> anyhow::Result<()> {
    use tracing_subscriber::fmt::time::ChronoLocal;
    use tracing_subscriber::prelude::*;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(LOG_DEFAULT));
    // Times in this computer's time zone, with the offset.
    let local = ChronoLocal::rfc_3339;
    let out = match file {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("cannot open log file {}", path.display()))?;
            logs::set_file(path);
            tracing_subscriber::fmt::layer()
                .with_timer(local())
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .boxed()
        }
        None => tracing_subscriber::fmt::layer().with_timer(local()).boxed(),
    };
    // The latest lines also stay in memory, for Admin > System.
    let recent = tracing_subscriber::fmt::layer()
        .with_timer(local())
        .with_ansi(false)
        .with_writer(|| logs::Recent);
    tracing_subscriber::registry()
        .with(filter)
        .with(out)
        .with(recent)
        .init();
    Ok(())
}

/// Log levels unless `RUST_LOG` says otherwise. Symphonia warns about junk
/// bytes before the audio of many files, every time one is read, which
/// buried everything else while analysis ran.
const LOG_DEFAULT: &str = "info,symphonia=warn,symphonia_core::formats::probe=error";

/// `djinn.log` next to the binary on Windows (where its config lives too),
/// in `~/.config/djinn/` elsewhere.
fn default_log_file() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(std::env::current_exe().ok()?.parent()?.join("djinn.log"))
    } else {
        Some(dirs::config_dir()?.join("djinn").join("djinn.log"))
    }
}

fn app(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router())
        .merge(cast_api::stream_router())
        .fallback(web::static_handler)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// How long open connections get to finish after a shutdown request.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
/// How long stopping audio, and then closing the database, may each take.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
/// The same three waits while Windows signs out, which shows "an app is
/// preventing sign-out" after a few seconds.
const QUICK_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(1);

/// Resolves on Ctrl+C, SIGTERM on Unix (systemd stop), or a request from
/// the tray menu.
async fn shutdown_signal(request: std::sync::Arc<Notify>) {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(%err, "failed to listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
        () = request.notified() => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn status(path: &str) -> StatusCode {
        app(AppState::for_tests().await)
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn health_ok() {
        assert_eq!(status("/api/health").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_api_path_is_404_not_spa() {
        assert_eq!(status("/api/nope").await, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn join_page_data_is_public() {
        assert_eq!(status("/api/join").await, StatusCode::OK);
        assert_eq!(status("/api/join/qr.svg").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_asset_is_404() {
        assert_eq!(status("/assets/missing.js").await, StatusCode::NOT_FOUND);
    }
}
