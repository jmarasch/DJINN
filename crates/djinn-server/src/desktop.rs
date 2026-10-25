//! Desktop integration: the tray icon and opening pages in the browser.

use std::net::SocketAddr;
use std::sync::Arc;

use djinn_tray::{Tray, TrayAction};

use crate::state::AppState;

/// Shows the tray icon if this looks like a desktop session. Returns `None`
/// (after logging why) on headless machines or when the desktop has no tray.
pub fn start_tray(state: &AppState, listen: SocketAddr) -> Option<Tray> {
    if !has_desktop() {
        tracing::info!("no desktop session, running without a tray icon");
        return None;
    }
    let local = local_url(listen);
    let bootstrap = Arc::clone(&state.bootstrap_token);
    let shutdown = Arc::clone(&state.shutdown_request);
    let tooltip = format!("DJINN jukebox\n{}", state.join_url);

    let result = Tray::spawn(tooltip, move |action| match action {
        TrayAction::Open => {
            // Until the admin exists, the useful page is setup.
            let token = bootstrap.lock().ok().and_then(|t| t.clone());
            let url = match token {
                Some(token) => format!("{local}/setup?token={token}"),
                None => format!("{local}/#/admin"),
            };
            open_browser(&url);
        }
        TrayAction::ShowJoinCode => open_browser(&format!("{local}/join")),
        TrayAction::Quit => {
            tracing::info!("shutdown requested from the tray");
            shutdown.notify_one();
        }
    });
    match result {
        Ok(tray) => {
            tracing::info!("tray icon ready");
            Some(tray)
        }
        Err(e) => {
            tracing::info!(error = %e, "running without a tray icon");
            None
        }
    }
}

/// Address for this machine's own browser. `localhost` keeps the admin's
/// session cookie no matter which LAN address guests use.
fn local_url(listen: SocketAddr) -> String {
    let ip = listen.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        format!("http://localhost:{}", listen.port())
    } else {
        format!("http://{listen}")
    }
}

fn has_desktop() -> bool {
    if cfg!(target_os = "linux") {
        ["WAYLAND_DISPLAY", "DISPLAY"]
            .iter()
            .any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty()))
    } else {
        true
    }
}

fn open_browser(url: &str) {
    if let Err(e) = open::that_detached(url) {
        tracing::warn!(error = %e, "could not open the browser");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_url_prefers_localhost() {
        assert_eq!(
            local_url("0.0.0.0:8066".parse().unwrap()),
            "http://localhost:8066"
        );
        assert_eq!(
            local_url("127.0.0.1:9000".parse().unwrap()),
            "http://localhost:9000"
        );
        assert_eq!(
            local_url("192.168.0.81:8066".parse().unwrap()),
            "http://192.168.0.81:8066"
        );
    }
}
