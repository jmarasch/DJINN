//! What this server calls itself, and letting phones on the same network find
//! it: GO's address goes out over mDNS while GO is on, with the server's name.
//! The sign-in screen lists what it hears, so nobody has to type an address,
//! and a network with several servers (a campus, say) can tell them apart.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use djinn_store::Store;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::Serialize;

use crate::error::ApiResult;
use crate::state::AppState;

/// What the admin named this server. Empty means this computer's name.
pub const SERVER_NAME: &str = "server_name";
/// Tells servers with the same name apart. Made once, never shown to people.
const SERVER_ID: &str = "server_id";
/// 1 to announce GO on the network, which is the default.
pub const GO_DISCOVERABLE: &str = "go_discoverable";
const SERVICE: &str = "_djinn-go._tcp.local.";
/// What nodes being set up look for: main DJINNs letting nodes join.
pub const MAIN_SERVICE: &str = "_djinn-main._tcp.local.";
const MAX_NAME: usize = 40;

/// This computer's own name, or something readable when it has none.
pub fn computer_name() -> String {
    let found = if cfg!(windows) {
        std::env::var("COMPUTERNAME").ok()
    } else {
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .or_else(|| std::env::var("HOSTNAME").ok())
    };
    found
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "DJINN".to_owned())
}

/// A name as the admin typed it, ready to save, or what's wrong with it.
pub fn clean_name(typed: &str) -> Result<String, &'static str> {
    let name = typed.trim();
    if name.chars().any(char::is_control) {
        return Err("the server name can't have line breaks or control characters");
    }
    if name.chars().count() > MAX_NAME {
        return Err("keep the server name to 40 characters");
    }
    Ok(name.to_owned())
}

/// The name people see: the admin's, or this computer's.
pub async fn server_name(store: &Store) -> anyhow::Result<String> {
    let saved = store.get_setting(SERVER_NAME).await?.unwrap_or_default();
    let saved = saved.trim();
    Ok(if saved.is_empty() {
        computer_name()
    } else {
        saved.to_owned()
    })
}

pub async fn server_id(store: &Store) -> anyhow::Result<String> {
    if let Some(id) = store.get_setting(SERVER_ID).await? {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    store.set_setting(SERVER_ID, &id).await?;
    Ok(id)
}

#[derive(Serialize)]
pub struct Hello {
    name: String,
    id: String,
    /// Encrypted connections are offered on this port.
    encrypted: bool,
}

/// Who this is, before anyone has signed in: the app shows the name above the
/// sign-in form, and on its settings page.
pub async fn hello(State(state): State<AppState>) -> ApiResult<Json<Hello>> {
    Ok(Json(Hello {
        name: server_name(&state.store).await?,
        id: server_id(&state.store).await?,
        encrypted: state.go.encryption.is_on(),
    }))
}

/// This computer's name as a DNS label, which is much fussier than a name.
fn host_label(name: &str) -> String {
    let label: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .take(40)
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "djinn".to_owned()
    } else {
        label.to_owned()
    }
}

/// Announcing GO on the network. Stops when told to.
pub struct Announcer {
    daemon: ServiceDaemon,
    fullname: String,
    /// What's being announced, to tell when it needs doing again.
    pub name: String,
    pub fingerprint: Option<String>,
}

impl Announcer {
    pub async fn start(
        store: &Store,
        port: u16,
        fingerprint: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::start_as(SERVICE, store, port, fingerprint).await
    }

    /// Announces this main DJINN to nodes being set up.
    pub async fn start_main(store: &Store, port: u16) -> anyhow::Result<Self> {
        Self::start_as(MAIN_SERVICE, store, port, None).await
    }

    async fn start_as(
        service: &str,
        store: &Store,
        port: u16,
        fingerprint: Option<String>,
    ) -> anyhow::Result<Self> {
        let name = server_name(store).await?;
        let id = server_id(store).await?;
        let host = format!("{}.local.", host_label(&computer_name()));
        let mut properties = HashMap::from([
            ("name".to_owned(), name.clone()),
            ("id".to_owned(), id.clone()),
        ]);
        if let Some(fingerprint) = &fingerprint {
            properties.insert("fp".to_owned(), fingerprint.clone());
        }
        // Named by id, since two servers may share a name and names can have
        // characters that don't survive every phone's mDNS.
        let info = ServiceInfo::new(service, &format!("djinn-{id}"), &host, (), port, properties)?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_owned();
        let daemon = ServiceDaemon::new()?;
        daemon.register(info)?;
        Ok(Self {
            daemon,
            fullname,
            name,
            fingerprint,
        })
    }

    /// Says goodbye, so phones drop it from their lists straight away.
    pub fn stop(self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(
            clean_name("  Dorm 4 Jukebox "),
            Ok("Dorm 4 Jukebox".to_owned())
        );
        assert_eq!(clean_name(""), Ok(String::new()));
        assert_eq!(clean_name("Ünï códe ♫"), Ok("Ünï códe ♫".to_owned()));
        assert!(clean_name("two\nlines").is_err());
        assert!(clean_name(&"x".repeat(41)).is_err());
        assert!(clean_name(&"é".repeat(40)).is_ok());
    }

    #[test]
    fn host_names_are_safe() {
        assert_eq!(host_label("my-pc"), "my-pc");
        assert_eq!(host_label("Jakob's PC"), "Jakob-s-PC");
        assert_eq!(host_label("♫♫"), "djinn");
    }

    #[test]
    fn a_computer_has_some_name() {
        assert!(!computer_name().is_empty());
    }

    /// Uses the real network, so it's run by hand.
    #[tokio::test]
    #[ignore = "needs multicast on the local network"]
    async fn the_announcement_can_be_heard() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .set_setting(SERVER_NAME, "Test ♫ server")
            .await
            .unwrap();
        let announcer = Announcer::start(&store, 18067, None).await.unwrap();
        let listener = ServiceDaemon::new().unwrap();
        let heard = listener.browse(SERVICE).unwrap();
        let found = tokio::task::spawn_blocking(move || {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
            while std::time::Instant::now() < until {
                if let Ok(mdns_sd::ServiceEvent::ServiceResolved(service)) =
                    heard.recv_timeout(std::time::Duration::from_secs(1))
                {
                    if service.get_port() == 18067 {
                        return service.get_property_val_str("name").map(str::to_owned);
                    }
                }
            }
            None
        })
        .await
        .unwrap();
        announcer.stop();
        let _ = listener.shutdown();
        assert_eq!(found.as_deref(), Some("Test ♫ server"));
    }
}
