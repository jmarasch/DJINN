//! State shared by all request handlers.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use djinn_library::ScanProgress;
use djinn_store::Store;
use tokio::sync::{broadcast, watch, Notify};

use crate::events::ServerEvent;
use crate::jukebox::Jukebox;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Cover art cache, next to the database.
    pub art_dir: Arc<PathBuf>,
    /// Library folders from config.toml. Folders added in the app are in
    /// the database; see `folders::library_roots`.
    pub library_roots: Arc<Vec<PathBuf>>,
    /// One-time token for creating the first admin, while none exists.
    pub bootstrap_token: Arc<Mutex<Option<String>>>,
    pub jukebox: Arc<tokio::sync::Mutex<Jukebox>>,
    pub events: broadcast::Sender<ServerEvent>,
    pub scan: Arc<ScanState>,
    /// Set to true on shutdown so long-lived responses (SSE) end and the
    /// graceful shutdown can finish.
    pub shutdown: Arc<watch::Sender<bool>>,
    /// Notified to request a shutdown (from the tray menu).
    pub shutdown_request: Arc<Notify>,
    /// The URL guests open, shown on the join page.
    pub join_url: Arc<String>,
    /// Guests may join with just a name. Mirrors the `guest_mode` setting.
    pub guest_mode: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct ScanState {
    pub running: AtomicBool,
    pub last: Mutex<Option<ScanProgress>>,
}

impl AppState {
    pub fn new(
        store: Store,
        art_dir: PathBuf,
        library_roots: Vec<PathBuf>,
        jukebox: Jukebox,
        events: broadcast::Sender<ServerEvent>,
    ) -> Self {
        Self {
            store,
            art_dir: Arc::new(art_dir),
            library_roots: Arc::new(library_roots),
            bootstrap_token: Arc::new(Mutex::new(None)),
            jukebox: Arc::new(tokio::sync::Mutex::new(jukebox)),
            events,
            scan: Arc::default(),
            shutdown: Arc::new(watch::Sender::new(false)),
            shutdown_request: Arc::new(Notify::new()),
            join_url: Arc::new(String::new()),
            guest_mode: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn guest_mode(&self) -> bool {
        self.guest_mode.load(Ordering::Relaxed)
    }

    pub fn with_join_url(mut self, url: String) -> Self {
        self.join_url = Arc::new(url);
        self
    }

    /// In-memory state with no audio engine, for tests.
    #[cfg(test)]
    pub async fn for_tests() -> Self {
        let store = Store::open_in_memory().await.unwrap();
        let art_dir = std::env::temp_dir().join(format!("djinn-test-art-{}", std::process::id()));
        let (events, _) = broadcast::channel(64);
        let jukebox = Jukebox::new(store.clone(), Box::new(|_| {}), events.clone())
            .await
            .unwrap();
        Self::new(store, art_dir, Vec::new(), jukebox, events)
    }
}
