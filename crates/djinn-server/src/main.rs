//! `djinn`: headless party jukebox server.

mod api;
mod config;
mod web;

use std::path::PathBuf;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use djinn_store::Store;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::{Config, Source};

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

    let listener = tokio::net::TcpListener::bind(config.server.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.server.listen))?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        addr = %config.server.listen,
        "DJINN listening"
    );

    axum::serve(listener, app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    store.close().await;
    tracing::info!("shut down");
    Ok(())
}

fn app() -> Router {
    Router::new()
        .nest("/api", api::router())
        .fallback(web::static_handler)
        .layer(TraceLayer::new_for_http())
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
        app()
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
