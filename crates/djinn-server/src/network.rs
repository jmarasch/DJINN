//! Whether Windows has the guests' network set to Public, where phones
//! can't reach DJINN, so the join card and Admin can say so. Checked at
//! start and then when someone opens one of those pages, at most once a
//! minute, since each check starts PowerShell.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::state::AppState;

const FRESH: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Watch {
    public: AtomicBool,
    checking: AtomicBool,
    checked_at: Mutex<Option<Instant>>,
}

impl Watch {
    /// The network was Public at the last check.
    pub fn public(&self) -> bool {
        self.public.load(Ordering::Relaxed)
    }
}

/// Checks in the background unless a check is running or recent.
pub fn refresh(state: &AppState) {
    if !cfg!(windows) {
        return;
    }
    let Some(ip) = join_ip(&state.join_url) else {
        return;
    };
    let watch = Arc::clone(&state.network);
    let recent = watch
        .checked_at
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < FRESH);
    if recent || watch.checking.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async move {
        let found = tokio::task::spawn_blocking(move || djinn_platform::network::is_public(ip))
            .await
            .ok()
            .flatten();
        if let Some(public) = found {
            let was = watch.public.swap(public, Ordering::Relaxed);
            if public && !was {
                tracing::warn!(
                    %ip,
                    "Windows has this network set to Public, so phones can't reach DJINN; \
                     set it to Private in Windows settings"
                );
            } else if was && !public {
                tracing::info!(%ip, "the network is no longer set to Public");
            }
        }
        if let Ok(mut at) = watch.checked_at.lock() {
            *at = Some(Instant::now());
        }
        watch.checking.store(false, Ordering::Release);
    });
}

/// The address in the join URL (`http://192.168.0.10:8066`).
fn join_ip(url: &str) -> Option<IpAddr> {
    let host = url.strip_prefix("http://")?.split('/').next()?;
    let (ip, _port) = host.rsplit_once(':')?;
    ip.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_ip_from_the_join_url() {
        assert_eq!(
            join_ip("http://192.168.0.10:8066"),
            Some(IpAddr::from([192, 168, 0, 10]))
        );
        assert_eq!(join_ip("http://localhost:8066"), None);
    }
}
