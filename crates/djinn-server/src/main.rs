//! `djinn`: headless party jukebox server.

// Release builds on Windows run without a console window (the tray icon is
// the interface); see `djinn_platform::console`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod accounts;
mod ai;
mod api;
mod audio_api;
mod auth;
mod background;
mod backup;
mod banner;
mod cast_api;
mod client;
mod config;
mod convert;
mod cpu;
mod desktop;
mod download;
mod duplicates;
mod error;
mod events;
mod folders;
mod go;
mod go_announce;
mod go_app;
mod go_library;
mod go_upnp;
mod go_year;
mod gpu;
mod identify_api;
mod jukebox;
mod library_api;
mod likes_api;
mod logs;
mod lyrics;
mod network;
mod node_audio;
mod node_mode;
mod node_offline;
mod nodes;
mod party_api;
mod player_api;
mod playlist_api;
mod playlist_import;
mod problems_api;
mod reader;
mod rooms_api;
mod rooms_rt;
mod scheduled_check;
mod security;
mod settings;
mod smart;
mod sse;
mod state;
mod stats_api;
mod submissions;
mod subsonic;
mod sync_tone;
mod system_api;
#[cfg(test)]
mod test_util;
mod tls;
mod tv_api;
mod two_step;
mod update;
mod watcher;
mod web;
mod wishes_api;
mod youtube;

use std::path::PathBuf;

use anyhow::Context;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use clap::Parser;
use djinn_engine::{Engine, EngineConfig};
use djinn_store::Store;
use tokio::sync::{broadcast, mpsc, Notify};
use tower_http::trace::TraceLayer;

use crate::config::{Config, Source};
use crate::jukebox::Jukebox;
use crate::state::AppState;

#[derive(Parser)]
#[command(name = "djinn", version = env!("DJINN_RELEASE_VERSION"), about)]
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

fn main() {
    // Before the runtime starts any threads: it sets environment variables.
    cpu::init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("Error: couldn't start: {e}");
            std::process::exit(1)
        }
    };
    let code = match runtime.block_on(run()) {
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
    // Attached first, so --version, --help and argument errors reach the
    // terminal that started a Windows release build.
    let has_console = djinn_platform::console::attach_parent_console();
    let cli = Cli::parse();

    // Always keep dated files; also show lines in an attached terminal.
    let log_file = cli.log_file.clone().or_else(default_log_file);
    init_logging(log_file.as_deref(), has_console && cli.log_file.is_none())?;
    cpu::log();
    if !has_console {
        // Otherwise loading the listening model flashes a console window.
        djinn_ai::settle_cache_probe();
    }

    let (mut config, source) = Config::load(cli.config.as_deref())?;
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

    backup::protect_data(&config.database.path);

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
    logs::apply(&store).await?;
    system_api::apply_ports(&store, &mut config).await?;
    // A node plays a room for a main DJINN: no library, no party app.
    if node_mode::is_node(&store).await? {
        if let Some(banner) = &config.server.console_banner {
            let url = banner::urls(config.server.listen)
                .first()
                .cloned()
                .unwrap_or_default();
            write_banner(
                banner,
                &format!(
                    "DJINN node: {url}/node (or http://{}.local:8066/node)\n",
                    go_announce::computer_name()
                ),
            );
        }
        return node_mode::serve(store, &config.database.path, config.server.listen).await;
    }
    tokio::spawn(async {
        let mut hourly = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        loop {
            hourly.tick().await;
            if let Err(error) = logs::rotate_today() {
                tracing::error!(%error, "couldn't rotate the daily log");
            }
        }
    });
    let picture_settings = library_api::PictureSettings::load(&store).await?;
    djinn_library::art::set_cache_limit(
        (picture_settings.cache_mb as u64).saturating_mul(1024 * 1024),
    );
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
    // The mix again, stamped with when it's heard, for nodes.
    let (timed, timed_reader) = djinn_engine::timed_tap(4.0, config.audio.master_rate);
    let node_audio = node_audio::Hub::start(timed_reader, config.audio.master_rate)?;
    let engine = Engine::start_with_taps(
        EngineConfig {
            master_rate: config.audio.master_rate,
            outputs,
        },
        Some(tap),
        Some(timed),
        move |event| {
            let _ = engine_tx.send(event);
        },
    )?;
    let engine = std::sync::Arc::new(engine);
    let engine_sink = std::sync::Arc::clone(&engine);
    let (events, _) = broadcast::channel(256);
    let playback_store = Store::open_playback(&config.database.path).await?;
    let jukebox = Jukebox::new(
        playback_store.clone(),
        Box::new(move |command| {
            if engine_sink.send(command).is_err() {
                tracing::error!("audio engine stopped");
            }
        }),
        events.clone(),
    )
    .await?;
    let urls = banner::urls(config.server.listen);
    system_api::started(&config.database.path, urls.clone(), config.server.listen);
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
    .with_node_audio(node_audio)
    .with_rooms(rooms_rt::Rooms::new(
        config.audio.master_rate,
        playback_store.clone(),
    ))
    .with_open_setup(config.server.open_setup)
    .with_stream(stream, cast_api::token(&store).await?)
    .with_go_listen(config.go.listen);

    state
        .jukebox
        .lock()
        .await
        .set_mixer(std::sync::Arc::new(smart::SmartMixer::new(&state)));
    state
        .jukebox
        .lock()
        .await
        .set_copies(std::sync::Arc::new(duplicates::KnownCopies::new(&state)));
    duplicates::refresh(&state);
    // A player for each room playing its own music.
    if let Err(e) = rooms_rt::sync(&state).await {
        tracing::error!(error = %format!("{e:#}"), "starting the rooms failed");
    }
    rooms_rt::watch_quiet(state.clone());
    nodes::watch_joining(state.clone(), config.server.listen.port());
    node_mode::find_mains_during_setup(state.clone());

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
    // A box without a screen of its own shows where to go on whatever
    // screen is plugged in.
    if let Some(banner) = &config.server.console_banner {
        let text = match &setup_url {
            Some(_) if config.server.open_setup => {
                format!(
                    "DJINN: set it up at {base_url}/setup (or http://{}.local:8066/setup)\n",
                    go_announce::computer_name()
                )
            }
            Some(url) => format!("DJINN: set it up at {url}\n"),
            None => format!(
                "DJINN is running: {base_url} (or http://{}.local:8066)\n",
                go_announce::computer_name()
            ),
        };
        write_banner(banner, &text);
    }

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
        version = env!("DJINN_RELEASE_VERSION"),
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
    scheduled_check::start(state.clone());
    lyrics::start(&state);
    update::start(&state);
    if let Err(e) = convert::apply(&state).await {
        tracing::error!(error = %format!("{e:#}"), "couldn't read the converting settings");
    }
    if let Err(e) = go::apply(&state).await {
        tracing::error!(error = %format!("{e:#}"), "couldn't start DJINN GO");
    }
    network::refresh(&state);
    let update = std::sync::Arc::clone(&state.update);
    let jukebox = std::sync::Arc::clone(&state.jukebox);

    let shutdown = std::sync::Arc::clone(&state.shutdown);
    // Windows is signing out: every shutdown step gets less time.
    let session_ending = std::sync::Arc::clone(&state.session_ending);
    let hurry = move || session_ending.load(std::sync::atomic::Ordering::Acquire);
    let mut stopping = state.shutdown.subscribe();
    let shutdown_request = std::sync::Arc::clone(&state.shutdown_request);
    let listener = tls::listen(listener, std::sync::Arc::clone(&state.go.encryption))?;
    let server = axum::serve(
        listener,
        // Connection info gives handlers the client's IP (for MAC lookup).
        app(state).into_make_service_with_connect_info::<tls::Peer>(),
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
    if tokio::time::timeout(stop_wait, playback_store.close())
        .await
        .is_err()
    {
        tracing::warn!("the playback database was still busy; closing anyway");
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

/// Writes the address to show on the box's own screen (a login banner).
pub fn write_banner(path: &std::path::Path, text: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, text) {
        tracing::warn!(path = %path.display(), error = %e, "couldn't write the console banner");
    }
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

/// Logs to `file` and/or the `console`, and always to memory.
fn init_logging(file: Option<&std::path::Path>, console: bool) -> anyhow::Result<()> {
    use tracing_subscriber::filter::dynamic_filter_fn;
    use tracing_subscriber::fmt::time::ChronoLocal;
    use tracing_subscriber::prelude::*;
    // Times in this computer's time zone, with the offset.
    let local = ChronoLocal::rfc_3339;
    let to_file = match file {
        Some(path) => {
            logs::open_file(path)
                .with_context(|| format!("cannot open log file {}", path.display()))?;
            let layer = tracing_subscriber::fmt::layer()
                .with_timer(local())
                .with_ansi(false)
                .with_writer(logs::writer)
                .with_filter(dynamic_filter_fn(|meta, _| logs::enabled(meta)))
                .boxed();
            Some(layer)
        }
        None => None,
    };
    let to_console = (console || to_file.is_none()).then(|| {
        tracing_subscriber::fmt::layer()
            .with_timer(local())
            .with_filter(dynamic_filter_fn(|meta, _| logs::enabled(meta)))
            .boxed()
    });
    // The latest lines also stay in memory, for Admin > System.
    let recent = tracing_subscriber::fmt::layer()
        .with_timer(local())
        .with_ansi(false)
        .with_writer(|| logs::Recent)
        .with_filter(dynamic_filter_fn(|meta, _| logs::enabled(meta)));
    let subscriber = tracing_subscriber::registry()
        .with(to_file)
        .with(to_console)
        .with(recent);
    tracing::subscriber::set_global_default(subscriber)?;
    logs::pass_on_log_crate()?;
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
        .merge(cast_api::stream_router())
        // What a screen plugged into this box shows (the kiosk opens it).
        .route("/screen", axum::routing::get(auth::screen))
        .fallback(web::static_handler)
        .layer(middleware::from_fn(security::same_origin))
        .layer(middleware::from_fn_with_state(state.clone(), party_headers))
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn(client::note_peer))
        .with_state(state)
}

/// Security headers on everything the party port sends.
async fn party_headers(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let origin = security::origin(
        security::peer(request.extensions()),
        request.headers(),
        &state.go.proxies(),
    );
    let mut response = next.run(request).await;
    security::set_headers(response.headers_mut(), security::PARTY_CSP, origin.https);
    response
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
    async fn pages_get_security_headers() {
        let response = app(AppState::for_tests().await)
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        for name in [
            "content-security-policy",
            "x-content-type-options",
            "x-frame-options",
            "referrer-policy",
        ] {
            assert!(response.headers().contains_key(name), "{name}");
        }
        assert!(!response.headers().contains_key("strict-transport-security"));
    }

    #[tokio::test]
    async fn pages_from_elsewhere_cant_press_the_buttons() {
        let state = AppState::for_tests().await;
        let admin = test_util::session(&state, "ana", djinn_store::Role::Admin).await;
        let quit = |origin: &'static str| {
            app(state.clone()).oneshot(
                Request::post("/api/system/quit")
                    .header("host", "10.0.0.2:8066")
                    .header("origin", origin)
                    .header("cookie", &admin)
                    .body(Body::empty())
                    .unwrap(),
            )
        };
        let refused = quit("http://10.0.0.2:9000").await.unwrap();
        assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn missing_asset_is_404() {
        assert_eq!(status("/assets/missing.js").await, StatusCode::NOT_FOUND);
    }
}
