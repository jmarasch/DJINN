//! Hardware (MAC) address of a device on the local network, from the
//! operating system's neighbor (ARP) table.
//!
//! This only works for devices on the same network segment as the server,
//! like phones on the same Wi-Fi. Phones usually present a per-network
//! "private" address, which is stable on one network but is not the
//! hardware's real address.

use std::net::{IpAddr, Ipv4Addr};

/// The MAC address for `ip`, as lowercase `aa:bb:cc:dd:ee:ff`.
pub fn mac_for(ip: IpAddr) -> Option<String> {
    let ip = match ip.to_canonical() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => ip,
        // Neighbor lookup for IPv6 isn't supported; the server listens on IPv4.
        _ => return None,
    };
    lookup(ip)
}

#[cfg(target_os = "linux")]
fn lookup(ip: Ipv4Addr) -> Option<String> {
    let table = std::fs::read_to_string("/proc/net/arp").ok()?;
    parse_proc_arp(&table, ip)
}

#[cfg(windows)]
fn lookup(ip: Ipv4Addr) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("arp")
        .args(["-a", &ip.to_string()])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    parse_arp_a(&String::from_utf8_lossy(&output.stdout), ip)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn lookup(ip: Ipv4Addr) -> Option<String> {
    let output = std::process::Command::new("arp")
        .args(["-n", &ip.to_string()])
        .output()
        .ok()?;
    parse_arp_n(&String::from_utf8_lossy(&output.stdout))
}

/// Linux `/proc/net/arp`:
/// `IP address  HW type  Flags  HW address  Mask  Device`
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_proc_arp(table: &str, ip: Ipv4Addr) -> Option<String> {
    let ip = ip.to_string();
    table.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        match cols.as_slice() {
            [addr, _, _, mac, ..] if *addr == ip => normalize(mac),
            _ => None,
        }
    })
}

/// Windows `arp -a <ip>`: `  192.168.0.5    aa-bb-cc-dd-ee-ff     dynamic`
#[cfg_attr(not(windows), allow(dead_code))]
fn parse_arp_a(output: &str, ip: Ipv4Addr) -> Option<String> {
    let ip = ip.to_string();
    output.lines().find_map(|line| {
        let mut cols = line.split_whitespace();
        (cols.next() == Some(ip.as_str()))
            .then(|| cols.next())
            .flatten()
            .and_then(normalize)
    })
}

/// BSD/macOS `arp -n <ip>`: `? (192.168.0.5) at aa:bb:cc:dd:ee:ff on en0 ...`
#[cfg_attr(any(target_os = "linux", windows), allow(dead_code))]
fn parse_arp_n(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .skip_while(|w| *w != "at")
        .nth(1)
        .and_then(normalize)
}

/// Lowercase, colon separated, two digits per byte. Rejects incomplete
/// (all zero) and broadcast entries.
fn normalize(raw: &str) -> Option<String> {
    let bytes: Vec<u8> = raw
        .split([':', '-'])
        .map(|part| u8::from_str_radix(part, 16).ok())
        .collect::<Option<_>>()?;
    if bytes.len() != 6 || bytes.iter().all(|&b| b == 0) || bytes.iter().all(|&b| b == 0xff) {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: Ipv4Addr = Ipv4Addr::new(192, 168, 0, 42);

    #[test]
    fn linux_proc_arp() {
        let table = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.0.1      0x1         0x2         10:20:30:40:50:60     *        wlp6s0
192.168.0.42     0x1         0x2         A4:83:E7:0B:1C:2D     *        wlp6s0
192.168.0.99     0x1         0x0         00:00:00:00:00:00     *        wlp6s0
";
        assert_eq!(
            parse_proc_arp(table, IP).as_deref(),
            Some("a4:83:e7:0b:1c:2d")
        );
        assert_eq!(
            parse_proc_arp(table, Ipv4Addr::new(192, 168, 0, 99)),
            None,
            "incomplete"
        );
        assert_eq!(
            parse_proc_arp(table, Ipv4Addr::new(10, 0, 0, 1)),
            None,
            "absent"
        );
    }

    #[test]
    fn windows_arp_a() {
        let output = "\r\nInterface: 192.168.0.81 --- 0x7\r\n  Internet Address      Physical Address      Type\r\n  192.168.0.42          a4-83-e7-0b-1c-2d     dynamic   \r\n";
        assert_eq!(
            parse_arp_a(output, IP).as_deref(),
            Some("a4:83:e7:0b:1c:2d")
        );
        assert_eq!(parse_arp_a("No ARP Entries Found.", IP), None);
    }

    #[test]
    fn bsd_arp_n() {
        let output = "? (192.168.0.42) at a4:83:e7:b:1c:2d on en0 ifscope [ethernet]";
        assert_eq!(parse_arp_n(output).as_deref(), Some("a4:83:e7:0b:1c:2d"));
        assert_eq!(parse_arp_n("? (192.168.0.42) at (incomplete) on en0"), None);
    }

    #[test]
    fn loopback_has_no_mac() {
        assert_eq!(mac_for(IpAddr::V4(Ipv4Addr::LOCALHOST)), None);
    }
}
