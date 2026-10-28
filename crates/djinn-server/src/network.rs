//! Watches whether Windows has the guests' network set to Public, where
//! phones can't reach DJINN, so the join card and Admin can say so.

use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::state::AppState;

/// People switch networks or fix the setting while DJINN runs.
const EVERY: Duration = Duration::from_secs(60);

pub fn start(state: &AppState) {
    if !cfg!(windows) {
        return;
    }
    let Some(ip) = join_ip(&state.join_url) else {
        return;
    };
    let public_network = std::sync::Arc::clone(&state.public_network);
    tokio::spawn(async move {
        loop {
            let found = tokio::task::spawn_blocking(move || djinn_platform::network::is_public(ip))
                .await
                .ok()
                .flatten();
            if let Some(public) = found {
                let was = public_network.swap(public, Ordering::Relaxed);
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
            tokio::time::sleep(EVERY).await;
        }
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
