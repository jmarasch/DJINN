//! `djinn`: headless party jukebox server.

mod api;
mod auth;
mod banner;
mod config;
mod error;
mod events;
mod jukebox;
mod library_api;
mod player_api;
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
use tokio::sync::{broadcast, mpsc};
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,symphonia=warn")),
        )
        .init();

    let (config, source) = Config::load(cli.config.as_deref())?;
    match &source {
        Source::File(path) => tracing::info!(path = %path.display(), "loaded config"),
        Source::Defaults { looked_at } => tracing::info!(
            looked_at = ?looked_at.as_ref().map(|p| p.display().to_string()),
            "no config file found, using defaults"
        ),
    }
    if config.library.roots.is_empty() {
        tracing::warn!("no library roots configured");
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
    let state = AppState::new(
        store.clone(),
        art_dir,
        config.library.roots.clone(),
        jukebox,
        events,
    );

    let jukebox = std::sync::Arc::clone(&state.jukebox);
    tokio::spawn(async move {
        while let Some(event) = engine_rx.recv().await {
            if let Err(e) = jukebox.lock().await.on_engine_event(event).await {
                tracing::error!(error = %format!("{e:#}"), "handling engine event failed");
            }
        }
    });
    library_api::start_scan(&state);

    let urls = banner::urls(config.server.listen);
    let base_url = urls.first().cloned().unwrap_or_default();
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

    let listener = tokio::net::TcpListener::bind(config.server.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.server.listen))?;
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

    let shutdown = std::sync::Arc::clone(&state.shutdown);
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown.send_replace(true);
        })
        .await
        .context("server error")?;

    // Stop audio before closing the database.
    drop(engine);
    store.close().await;
    tracing::info!("shut down");
    Ok(())
}

fn app(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router())
        .fallback(web::static_handler)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Resolves on Ctrl+C, or SIGTERM on Unix (systemd stop).
async fn shutdown_signal() {
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
    async fn missing_asset_is_404() {
        assert_eq!(status("/assets/missing.js").await, StatusCode::NOT_FOUND);
    }
}
