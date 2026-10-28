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
    /// Windows is signing out or shutting down: close fast, it won't wait.
    pub session_ending: Arc<AtomicBool>,
    /// Notified when library folders are added or removed, so the folder
    /// watcher follows.
    pub roots_changed: Arc<Notify>,
    /// The URL guests open, shown on the join page.
    pub join_url: Arc<String>,
    /// Whether Windows has the guests' network set to Public.
    pub network: Arc<crate::network::Watch>,
    /// Guests may join with just a name. Mirrors the `guest_mode` setting.
    pub guest_mode: Arc<AtomicBool>,
    /// The live stream for casting, when the engine runs.
    pub stream: Option<djinn_cast::stream::LiveStream>,
    /// Secret in the stream URL.
    pub cast_token: Arc<String>,
    pub caster: djinn_cast::Caster,
    /// Songs as points in sound space, for "more like this".
    pub sound: Arc<std::sync::RwLock<djinn_library::sound::SoundIndex>>,
    /// Song facts for search by description.
    pub catalog: Arc<std::sync::RwLock<crate::smart::Catalog>>,
    /// Optional AI listening.
    pub ai: Arc<crate::ai::AiState>,
    /// Version checks and self-updates.
    pub update: Arc<crate::update::UpdateState>,
}

#[derive(Default)]
pub struct ScanState {
    pub running: AtomicBool,
    /// An online artist info lookup is running.
    pub lookup_running: AtomicBool,
    /// "Recheck all" was asked for during a lookup; run it next.
    pub lookup_force_next: AtomicBool,
    /// How the last lookup went, and when it finished (Unix ms).
    pub lookup_last: Mutex<Option<(djinn_library::online::LookupSummary, i64)>>,
    /// Song loudness is being measured for ReplayGain.
    pub loudness_running: AtomicBool,
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
        // Models live next to the art cache, in DJINN's data folder.
        let art_dir_for_models = art_dir
            .parent()
            .map_or_else(|| PathBuf::from("models"), |dir| dir.join("models"));
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
            session_ending: Arc::new(AtomicBool::new(false)),
            roots_changed: Arc::new(Notify::new()),
            join_url: Arc::new(String::new()),
            network: Arc::default(),
            guest_mode: Arc::new(AtomicBool::new(false)),
            stream: None,
            cast_token: Arc::new(String::new()),
            caster: djinn_cast::Caster::new(),
            sound: Arc::default(),
            catalog: Arc::default(),
            ai: Arc::new(crate::ai::AiState::new(&art_dir_for_models)),
            update: Arc::default(),
        }
    }

    pub fn with_stream(mut self, stream: djinn_cast::stream::LiveStream, token: String) -> Self {
        self.stream = Some(stream);
        self.cast_token = Arc::new(token);
        self
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
