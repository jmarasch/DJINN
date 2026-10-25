//! Startup banner: the address phones should open, as a QR code in the
//! terminal.

use std::io::IsTerminal;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;

/// URLs other devices can reach, best guess first: the listen address, or
/// for 0.0.0.0, this machine's LAN addresses.
pub fn urls(listen: SocketAddr) -> Vec<String> {
    let ips = if listen.ip().is_unspecified() {
        let found = lan_ips();
        if found.is_empty() {
            vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]
        } else {
            found
        }
    } else {
        vec![listen.ip()]
    };
    ips.into_iter()
        .map(|ip| format!("http://{}", SocketAddr::new(ip, listen.port())))
        .collect()
}

/// Private IPv4 addresses on physical-looking interfaces, best first. VPN
/// tunnels, containers, and VM host adapters are skipped: a phone on the
/// Wi-Fi can't reach those.
fn lan_ips() -> Vec<IpAddr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut found: Vec<(u8, IpAddr)> = interfaces
        .iter()
        .filter(|i| !i.is_loopback() && !i.is_link_local() && !i.is_p2p())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => rank(&i.name, ip).map(|r| (r, IpAddr::V4(ip))),
            IpAddr::V6(_) => None,
        })
        .collect();
    found.sort_by_key(|&(rank, _)| rank);
    let mut ips: Vec<IpAddr> = found.into_iter().map(|(_, ip)| ip).collect();
    ips.dedup();
    ips
}

/// Lower is better; `None` means skip. Interface names cover Linux
/// (`wlp6s0`, `docker0`) and Windows friendly names (`Wi-Fi`,
/// `VMware Network Adapter VMnet8`).
fn rank(name: &str, ip: Ipv4Addr) -> Option<u8> {
    const VIRTUAL_PREFIXES: &[&str] = &[
        "br-", "veth", "tun", "tap", "wg", "zt", "utun", "lxc", "cni",
    ];
    const VIRTUAL_NAMES: &[&str] = &[
        "docker",
        "virbr",
        "vmnet",
        "vmware",
        "vbox",
        "virtualbox",
        "vethernet",
        "hyper-v",
        "proton",
        "tailscale",
        "nordlynx",
        "wintun",
        "openvpn",
        "podman",
        "wireguard",
    ];
    const PHYSICAL_PREFIXES: &[&str] = &["en", "eth", "wl", "wi-fi", "wifi", "ethernet", "wlan"];

    let name = name.to_ascii_lowercase();
    if !ip.is_private()
        || VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p))
        || VIRTUAL_NAMES.iter().any(|n| name.contains(n))
    {
        return None;
    }
    let physical = if PHYSICAL_PREFIXES.iter().any(|p| name.starts_with(p)) {
        0
    } else {
        10
    };
    let range = match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        _ => 2,
    };
    Some(physical + range)
}

/// Prints `url` as a scannable QR code with a caption, but only when stdout
/// is a terminal (not under systemd or when piped to a file).
pub fn print_qr(url: &str, caption: &str) {
    if !std::io::stdout().is_terminal() {
        return;
    }
    if let Some(code) = qr_text(url) {
        println!("\n{code}\n  {caption}\n  {url}\n");
    }
}

/// Half-block QR art, light on dark for dark terminal themes.
fn qr_text(url: &str) -> Option<String> {
    let code = QrCode::new(url.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_listen_address_is_used_as_is() {
        let listen: SocketAddr = "192.168.1.20:8066".parse().unwrap();
        assert_eq!(urls(listen), ["http://192.168.1.20:8066"]);
    }

    #[test]
    fn unspecified_listen_address_resolves_to_something_reachable() {
        let urls = urls("0.0.0.0:8066".parse().unwrap());
        assert!(urls
            .iter()
            .all(|u| u.starts_with("http://") && u.ends_with(":8066")));
        assert!(!urls[0].contains("0.0.0.0"));
    }

    #[test]
    fn ranks_physical_lan_above_virtual_adapters() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert_eq!(rank("wlp6s0", ip("192.168.0.81")), Some(0));
        assert_eq!(rank("Wi-Fi", ip("192.168.0.81")), Some(0));
        assert_eq!(rank("eth0", ip("10.0.0.5")), Some(1));
        for name in [
            "proton0",
            "docker0",
            "br-2966094c6203",
            "vmnet8",
            "VMware Network Adapter VMnet8",
            "vEthernet (WSL)",
            "tailscale0",
            "wg0",
        ] {
            assert_eq!(rank(name, ip("192.168.50.1")), None, "{name}");
        }
        assert_eq!(rank("wlan0", ip("8.8.8.8")), None, "public address");
        assert!(rank("bond0", ip("192.168.1.2")) > rank("eno1", ip("10.1.1.1")));
    }

    #[test]
    fn renders_a_code() {
        let art = qr_text("http://192.168.0.81:8066").unwrap();
        assert!(art.lines().count() > 10);
        assert!(art.contains('█'));
    }
}
