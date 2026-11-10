//! This computer's address on the home network, kept current. Routers can
//! hand out a new one (after a restart, a lease running out, or moving from
//! Wi-Fi to a cable), and the join link, its QR codes, casts and nodes'
//! screens all carry it, so a change reaches them without restarting DJINN.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::RwLock;
use std::time::Duration;

use crate::events::ServerEvent;
use crate::state::AppState;

/// How often the network is looked at. Cheap: one list of interfaces.
const CHECK_EVERY: Duration = Duration::from_secs(10);

/// After a new address, how long this computer announces itself even when
/// set to stay hidden, so phones and nodes that knew the old one find it.
pub const ANNOUNCE_AFTER_MOVE: Duration = Duration::from_secs(60 * 60);

/// The address guests open (`http://192.168.1.20:8066`), and when it last
/// changed (ms; 0 never).
#[derive(Default)]
pub struct JoinUrl(RwLock<String>, AtomicI64);

impl JoinUrl {
    pub fn new(url: String) -> Self {
        Self(RwLock::new(url), AtomicI64::new(0))
    }

    pub fn get(&self) -> String {
        self.0.read().map(|u| u.clone()).unwrap_or_default()
    }

    /// The address changed within ANNOUNCE_AFTER_MOVE.
    pub fn moved_lately(&self) -> bool {
        let at = self.1.load(Ordering::Relaxed);
        at > 0
            && djinn_store::now_ms() - at
                < i64::try_from(ANNOUNCE_AFTER_MOVE.as_millis()).unwrap_or(i64::MAX)
    }

    /// Without the trailing slash, for adding a path.
    pub fn base(&self) -> String {
        self.get().trim_end_matches('/').to_owned()
    }

    fn set(&self, url: String) {
        if let Ok(mut current) = self.0.write() {
            *current = url;
        }
        self.1.store(djinn_store::now_ms(), Ordering::Relaxed);
    }
}

/// The address to switch to, if any: the current one stays while this
/// computer still has it, so two good ones never take turns; otherwise the
/// best one found. Nothing found (no network for a moment) changes nothing.
pub fn next_url(current: &str, found: &[String]) -> Option<String> {
    // banner::urls falls back to this computer itself when it finds no
    // network; that's no address for a phone.
    let found: Vec<&String> = found.iter().filter(|u| !u.contains("://127.")).collect();
    let best = found.first()?;
    if found.iter().any(|u| u.as_str() == current) {
        return None;
    }
    Some((*best).clone())
}

/// What to do once the address changed, besides telling open pages.
pub type OnChange = Box<dyn Fn(&str) + Send + Sync>;

/// Watches for a new address while DJINN runs. Only for a listen address
/// of 0.0.0.0: one set in the config is what it is.
pub fn watch(state: AppState, listen: SocketAddr, on_change: OnChange) {
    if !listen.ip().is_unspecified() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(CHECK_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await;
        loop {
            tick.tick().await;
            let found = tokio::task::spawn_blocking(move || crate::banner::urls(listen))
                .await
                .unwrap_or_default();
            let current = state.join_url.get();
            let Some(url) = next_url(&current, &found) else {
                continue;
            };
            tracing::info!(from = %current, to = %url, "this computer's address changed");
            state.join_url.set(url.clone());
            on_change(&url);
            let _ = state.events.send(ServerEvent::Address { url: url.clone() });
            crate::network::refresh(&state);
            // Phones find it by its announcement (even if hidden, for a
            // while: `moved_lately`); stop that again afterwards.
            if let Err(e) = crate::go::apply(&state).await {
                tracing::warn!(error = %format!("{e:#}"), "couldn't announce the new address");
            }
            let later = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(ANNOUNCE_AFTER_MOVE + Duration::from_secs(5)).await;
                let _ = crate::go::apply(&later).await;
            });
            // Cast devices were sent the old address.
            crate::cast_api::recast(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(list: &[&str]) -> Vec<String> {
        list.iter().map(|u| (*u).to_owned()).collect()
    }

    #[test]
    fn keeps_an_address_this_computer_still_has() {
        let found = urls(&["http://192.168.1.5:8066", "http://10.0.0.7:8066"]);
        assert_eq!(next_url("http://10.0.0.7:8066", &found), None);
    }

    #[test]
    fn moves_to_the_best_one_when_the_old_one_is_gone() {
        let found = urls(&["http://192.168.1.9:8066", "http://10.0.0.7:8066"]);
        assert_eq!(
            next_url("http://192.168.1.5:8066", &found).as_deref(),
            Some("http://192.168.1.9:8066")
        );
    }

    #[test]
    fn leaves_localhost_once_a_network_appears() {
        let found = urls(&["http://192.168.1.9:8066"]);
        assert_eq!(
            next_url("http://127.0.0.1:8066", &found).as_deref(),
            Some("http://192.168.1.9:8066")
        );
    }

    #[test]
    fn no_network_for_a_moment_changes_nothing() {
        assert_eq!(next_url("http://192.168.1.5:8066", &[]), None);
        let only_itself = urls(&["http://127.0.0.1:8066"]);
        assert_eq!(next_url("http://192.168.1.5:8066", &only_itself), None);
    }

    #[test]
    fn the_shared_address_changes_for_everyone() {
        let join = JoinUrl::new("http://192.168.1.5:8066/".into());
        assert_eq!(join.base(), "http://192.168.1.5:8066");
        assert!(!join.moved_lately());
        join.set("http://192.168.1.9:8066".into());
        assert_eq!(join.get(), "http://192.168.1.9:8066");
        assert!(join.moved_lately(), "announced for a while after");
    }
}
