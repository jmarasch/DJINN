//! State shared by all request handlers.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use djinn_store::Store;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    /// Cover art cache, next to the database.
    pub art_dir: Arc<PathBuf>,
    /// One-time token for creating the first admin, while none exists.
    pub bootstrap_token: Arc<Mutex<Option<String>>>,
}

impl AppState {
    pub fn new(store: Store, art_dir: PathBuf) -> Self {
        Self {
            store,
            art_dir: Arc::new(art_dir),
            bootstrap_token: Arc::new(Mutex::new(None)),
        }
    }

    /// In-memory state for tests.
    #[cfg(test)]
    pub async fn for_tests() -> Self {
        let store = Store::open_in_memory().await.unwrap();
        let art_dir = std::env::temp_dir().join(format!("djinn-test-art-{}", std::process::id()));
        Self::new(store, art_dir)
    }
}
