//! `djinn`: headless party jukebox server.

// Release builds on Windows run without a console window (the tray icon is
// the interface); see `djinn_platform::console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod auth;
mod banner;
mod client;
mod config;
mod desktop;
mod error;
mod events;
mod folders;
mod jukebox;
mod library_api;
mod player_api;
mod playlist_api;
mod sse;
mod state;
#[cfg(test)]
mod test_util;
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
#[command(version, about)]
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
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Without a console (Windows release build started from Explorer or the
    // Start menu), logs go to a file.
    let has_console = djinn_platform::console::attach_parent_console();
    let log_file = cli
        .log_file
        .clone()
        .or_else(|| (!has_console).then(default_log_file).flatten());
    init_logging(log_file.as_deref())?;

    let (config, source) = Config::load(cli.config.as_deref())?;
    match &source {
        Source::File(path) => tracing::info!(path = %path.display(), "loaded config"),
        Source::Defaults { looked_at } => tracing::info!(
            looked_at = ?looked_at.as_ref().map(|p| p.display().to_string()),
            "no config file found, using defaults"
        ),
    }

    let store = Store::open(&config.database.path)
        .await
        .with_context(|| format!("failed to open database {}", config.database.path.display()))?;
    tracing::info!(path = %config.database.path.display(), "database ready");
    let removed = store.delete_expired_sessions().await?;
    if removed > 0 {
        tracing::debug!(removed, "expired sessions cleaned up");
    }

    let art_dir = config
        .database
        .path
        .parent()
        .map_or_else(|| PathBuf::from("art"), |dir| dir.join("art"));

    // Engine events are handed to the async side through a channel; the
    // callback runs on the engine thread and must not block.
    let (engine_tx, mut engine_rx) = mpsc::unbounded_channel();
    let engine = Engine::start(
        EngineConfig {
            master_rate: config.audio.master_rate,
        },
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
    let base_url = urls.first().cloned().unwrap_or_default();
    let state = AppState::new(
        store.clone(),
        art_dir,
        config.library.roots.clone(),
        jukebox,
        events,
    )
    .with_join_url(base_url.clone());

    let guest_mode = jukebox::setting(&store, jukebox::GUEST_MODE, 0).await? == 1;
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

    let listener = match tokio::net::TcpListener::bind(config.server.listen).await {
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

    // Ready to serve from what the database already knows; look for
    // changes on disk in the background.
    library_api::start_scan(&state);

    let shutdown = std::sync::Arc::clone(&state.shutdown);
    let shutdown_request = std::sync::Arc::clone(&state.shutdown_request);
    axum::serve(
        listener,
        // Connection info gives handlers the client's IP (for MAC lookup).
        app(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal(shutdown_request).await;
        shutdown.send_replace(true);
    })
    .await
    .context("server error")?;

    // Remove the tray icon before exiting (Windows leaves a ghost icon
    // otherwise).
    drop(tray);
    // Stop audio before closing the database.
    drop(engine);
    store.close().await;
    tracing::info!("shut down");
    Ok(())
}

fn init_logging(file: Option<&std::path::Path>) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,symphonia=warn"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    match file {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("cannot open log file {}", path.display()))?;
            builder
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init();
        }
        None => builder.init(),
    }
    Ok(())
}

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
        .fallback(web::static_handler)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

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
