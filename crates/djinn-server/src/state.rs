//! State shared by all request handlers.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use djinn_library::ScanProgress;
use djinn_store::Store;
use tokio::sync::{broadcast, watch};

use crate::events::ServerEvent;
use crate::jukebox::Jukebox;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Cover art cache, next to the database.
    pub art_dir: Arc<PathBuf>,
    pub library_roots: Arc<Vec<PathBuf>>,
    /// One-time token for creating the first admin, while none exists.
    pub bootstrap_token: Arc<Mutex<Option<String>>>,
    pub jukebox: Arc<tokio::sync::Mutex<Jukebox>>,
    pub events: broadcast::Sender<ServerEvent>,
    pub scan: Arc<ScanState>,
    /// Set to true on shutdown so long-lived responses (SSE) end and the
    /// graceful shutdown can finish.
    pub shutdown: Arc<watch::Sender<bool>>,
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
        }
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
