//! Finding cast devices on the LAN: Google Cast over mDNS, DLNA renderers
//! and Sonos over SSDP.
//!
//! SSDP goes out on every IPv4 interface explicitly: with a VPN up, the
//! default route (and so default multicast) points into the tunnel, where
//! no TV will ever answer.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::upnp::{self, UpnpControl};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    /// Google Cast: Chromecast, Google TV, Nest speakers and displays.
    Cast,
    /// A DLNA / UPnP media renderer (smart TVs, AV receivers).
    Dlna,
    Sonos,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Device {
    /// Stable across restarts and address changes.
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub model: Option<String>,
    /// A Google Cast speaker group: plays in sync on every speaker in it.
    pub group: bool,
    /// Has a screen, so it can show the TV page (Google Cast only).
    pub screen: bool,
    #[serde(skip)]
    pub addr: SocketAddr,
    #[serde(skip)]
    pub upnp: Option<UpnpControl>,
}

const CAST_SERVICE: &str = "_googlecast._tcp.local.";
const SSDP_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);
const RENDERER: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

/// Looks for devices for about `wait`. Devices found by both protocols
/// (a TV that does Cast and DLNA) are listed once per protocol.
pub async fn discover(wait: Duration) -> Vec<Device> {
    let cast = tokio::task::spawn_blocking(move || discover_cast(wait));
    let (cast, upnp) = tokio::join!(cast, discover_upnp(wait));
    let mut devices = cast.unwrap_or_default();
    devices.extend(upnp);
    devices.sort_by_key(|d| d.name.to_lowercase());
    devices
}

fn discover_cast(wait: Duration) -> Vec<Device> {
    let Ok(daemon) = mdns_sd::ServiceDaemon::new() else {
        tracing::warn!("mDNS unavailable; Cast devices won't be found");
        return Vec::new();
    };
    let Ok(events) = daemon.browse(CAST_SERVICE) else {
        let _ = daemon.shutdown();
        return Vec::new();
    };
    let deadline = Instant::now() + wait;
    let mut found: HashMap<String, Device> = HashMap::new();
    while let Ok(event) = events.recv_deadline(deadline) {
        let mdns_sd::ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        let Some(ip) = service.get_addresses_v4().into_iter().next() else {
            continue;
        };
        let txt = |key: &str| service.get_property_val_str(key).map(str::to_owned);
        let Some(id) = txt("id") else { continue };
        let model = txt("md");
        let name = txt("fn").unwrap_or_else(|| service.fullname.clone());
        // Capability bits: 1 is video out, 32 a multi-room group.
        let caps = txt("ca").and_then(|c| c.parse::<u64>().ok()).unwrap_or(0);
        found.insert(
            id.clone(),
            Device {
                id: format!("cast:{id}"),
                name,
                kind: DeviceKind::Cast,
                group: caps & 32 != 0 || model.as_deref() == Some("Google Cast Group"),
                screen: caps & 1 != 0,
                model,
                addr: SocketAddr::new(IpAddr::V4(ip), service.port),
                upnp: None,
            },
        );
    }
    let _ = daemon.stop_browse(CAST_SERVICE);
    let _ = daemon.shutdown();
    found.into_values().collect()
}

async fn discover_upnp(wait: Duration) -> Vec<Device> {
    let locations = tokio::task::spawn_blocking(move || ssdp_search(wait))
        .await
        .unwrap_or_default();
    let client = upnp::client();
    let mut devices = Vec::new();
    for location in locations {
        match upnp::describe(&client, &location).await {
            Ok(Some(device)) => devices.push(device),
            Ok(None) => {}
            Err(e) => tracing::debug!(%location, error = %e, "skipping UPnP device"),
        }
    }
    devices
}

/// Sends M-SEARCH for media renderers on every IPv4 interface and returns
/// the description URLs that answered.
fn ssdp_search(wait: Duration) -> Vec<String> {
    let request = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {RENDERER}\r\n\r\n"
    );
    let sockets: Vec<std::net::UdpSocket> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => ssdp_socket(ip).ok(),
            IpAddr::V6(_) => None,
        })
        .collect();
    for socket in &sockets {
        // Twice, since UDP gets lost.
        for _ in 0..2 {
            let _ = socket.send_to(request.as_bytes(), SSDP_ADDR);
        }
    }
    let deadline = Instant::now() + wait;
    let mut locations = Vec::new();
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        for socket in &sockets {
            while let Ok((n, _)) = socket.recv_from(&mut buf) {
                if let Some(location) = parse_ssdp_location(&buf[..n]) {
                    if !locations.contains(&location) {
                        locations.push(location);
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    locations
}

fn ssdp_socket(ip: Ipv4Addr) -> std::io::Result<std::net::UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket.set_multicast_if_v4(&ip)?;
    socket.set_multicast_ttl_v4(2)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(ip), 0).into())?;
    Ok(socket.into())
}

/// The LOCATION header of an SSDP reply for a media renderer.
fn parse_ssdp_location(reply: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(reply).ok()?;
    let mut location = None;
    let mut renderer = false;
    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "location" => location = Some(value.trim().to_owned()),
            "st" | "nt" => renderer |= value.trim().eq_ignore_ascii_case(RENDERER),
            _ => {}
        }
    }
    location.filter(|_| renderer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_renderer_replies_only() {
        let reply = b"HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://192.168.0.40:1400/xml/device_description.xml\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:RINCON_000E58::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
        assert_eq!(
            parse_ssdp_location(reply).as_deref(),
            Some("http://192.168.0.40:1400/xml/device_description.xml")
        );
        let router = b"HTTP/1.1 200 OK\r\nLOCATION: http://192.168.0.1/igd.xml\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
        assert_eq!(parse_ssdp_location(router), None);
    }
}
