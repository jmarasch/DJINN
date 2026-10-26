//! Keeps the library current: watches the music folders and checks for
//! changes once things go quiet, plus an occasional check as a fallback
//! (network shares don't report changes, and watch limits can run out).

use std::path::PathBuf;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::folders::library_roots;
use crate::library_api::start_scan;
use crate::state::AppState;

/// Check once changes have stopped for this long (a copy in progress keeps
/// producing events).
#[cfg(not(test))]
const QUIET: Duration = Duration::from_secs(5);
#[cfg(test)]
const QUIET: Duration = Duration::from_millis(200);
/// Check at least this often regardless of events.
const FALLBACK_EVERY: Duration = Duration::from_secs(30 * 60);

/// Starts watching in the background. Call after the startup scan began.
pub fn start(state: AppState) {
    tokio::spawn(run(state));
}

async fn run(state: AppState) {
    let (tx, mut changes) = mpsc::unbounded_channel();
    let mut watcher = match notify::recommended_watcher(
        move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                if !matches!(event.kind, EventKind::Access(_)) {
                    let _ = tx.send(());
                }
            }
        },
    ) {
        Ok(watcher) => Some(watcher),
        Err(e) => {
            tracing::warn!(error = %e, "can't watch music folders; checking every 30 minutes instead");
            None
        }
    };
    let mut watched: Vec<PathBuf> = Vec::new();
    if let Some(watcher) = watcher.as_mut() {
        watched = rewatch(&state, watcher, &watched).await;
    }

    let mut fallback = tokio::time::interval(FALLBACK_EVERY);
    fallback.tick().await; // The first tick fires immediately; skip it.
    loop {
        tokio::select! {
            change = changes.recv() => {
                if change.is_none() {
                    return;
                }
                // Wait for things to settle, then check once.
                while tokio::time::timeout(QUIET, changes.recv()).await.is_ok() {}
                tracing::debug!("music folders changed");
                check_now(&state).await;
            }
            () = state.roots_changed.notified() => {
                if let Some(watcher) = watcher.as_mut() {
                    watched = rewatch(&state, watcher, &watched).await;
                }
            }
            _ = fallback.tick() => check_now(&state).await,
        }
    }
}

/// Watches the current library folders, dropping ones no longer in it.
async fn rewatch(
    state: &AppState,
    watcher: &mut RecommendedWatcher,
    watched: &[PathBuf],
) -> Vec<PathBuf> {
    let roots = library_roots(state).await.unwrap_or_default();
    for old in watched.iter().filter(|w| !roots.contains(w)) {
        let _ = watcher.unwatch(old);
    }
    let mut now_watched = Vec::new();
    for root in roots {
        if watched.contains(&root) {
            now_watched.push(root);
            continue;
        }
        match watcher.watch(&root, RecursiveMode::Recursive) {
            Ok(()) => {
                tracing::info!(path = %root.display(), "watching for new music");
                now_watched.push(root);
            }
            // Missing (unplugged) or too many folders to watch; the
            // fallback check still covers it.
            Err(e) => tracing::warn!(path = %root.display(), error = %e, "can't watch folder"),
        }
    }
    now_watched
}

/// Starts a check for changes, or one right after the check in progress.
async fn check_now(state: &AppState) {
    while !start_scan(state) {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_music_appears_without_a_rescan() {
        let state = AppState::for_tests().await;
        let root = std::env::temp_dir().join(format!("djinn-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::folders::add_root_for_tests(&state, &root).await;
        start(state.clone());
        tokio::time::sleep(Duration::from_millis(300)).await;

        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures/gapless_a.flac");
        std::fs::copy(fixture, root.join("new song.flac")).unwrap();

        let mut found = 0;
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            found = state.store.track_count().await.unwrap();
            if found == 1 {
                break;
            }
        }
        assert_eq!(found, 1, "the new file was picked up");
        std::fs::remove_dir_all(root).unwrap();
    }
}
