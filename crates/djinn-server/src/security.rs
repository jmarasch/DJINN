//! Who's really asking and from where (behind trusted proxies), whether
//! that's the home network, and slowing down password guessing.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use ipnet::IpNet;

/// Proxies (or tunnels like cloudflared) whose forwarding headers DJINN
/// believes, one address or range per line or comma.
pub const TRUSTED_PROXIES: &str = "trusted_proxies";

/// Parses the `trusted_proxies` setting; bad entries are skipped.
pub fn parse_proxies(text: &str) -> Vec<IpNet> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            s.parse::<IpNet>()
                .ok()
                .or_else(|| s.parse::<IpAddr>().ok().map(IpNet::from))
        })
        .collect()
}

/// Where a request came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    /// The client, as best DJINN can tell: the peer, or what a trusted
    /// proxy says it forwarded for.
    pub ip: Option<IpAddr>,
    /// On the home network (or Tailscale).
    pub home: bool,
    /// Reached over HTTPS through a trusted proxy.
    pub https: bool,
}

/// Works out the client behind `peer`. Forwarding headers count only from
/// a trusted proxy; from anyone else they mark the request as not from
/// home, since a tunnel on this computer would otherwise look local.
pub fn origin(peer: Option<IpAddr>, headers: &HeaderMap, proxies: &[IpNet]) -> Origin {
    let forwarded = forwarded_for(headers);
    let trusted = peer.is_some_and(|p| proxies.iter().any(|n| n.contains(&p)));
    if !trusted {
        return Origin {
            ip: peer,
            home: forwarded.is_empty() && peer.is_some_and(is_home),
            https: false,
        };
    }
    // The nearest address that isn't one of our proxies is the client.
    let ip = forwarded
        .iter()
        .rev()
        .copied()
        .find(|ip| !proxies.iter().any(|n| n.contains(ip)))
        .or(peer);
    let https = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"));
    Origin {
        ip,
        home: ip.is_some_and(is_home),
        https,
    }
}

/// The peer address from axum's connection info.
pub fn peer(extensions: &axum::http::Extensions) -> Option<IpAddr> {
    extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_canonical())
}

/// Addresses from `CF-Connecting-IP` and `X-Forwarded-For`, nearest last.
fn forwarded_for(headers: &HeaderMap) -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = headers
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|s| s.trim().parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
        .collect();
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<IpAddr>().ok())
    {
        ips.insert(0, ip.to_canonical());
    }
    ips
}

/// This computer, private networks, link-local addresses, and the range
/// Tailscale uses (100.64.0.0/10, also carrier NAT, which the internet
/// can't reach in from).
pub fn is_home(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                // Unique local (fc00::/7, includes Tailscale's) and link-local.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Failed sign-ins allowed per account, and per address, in a window.
const ACCOUNT_TRIES: usize = 5;
const ADDRESS_TRIES: usize = 20;
const WINDOW: Duration = Duration::from_secs(15 * 60);
/// How many names and addresses to remember before forgetting old ones.
const MAX_TRACKED: usize = 10_000;

/// Slows password guessing: after too many failures for an account or
/// from an address, sign-ins wait until the window passes.
#[derive(Default)]
pub struct Limiter {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Limiter {
    /// How long until `account` from `ip` may try again; None if now.
    pub fn wait(&self, account: &str, ip: Option<IpAddr>) -> Option<Duration> {
        let now = Instant::now();
        let failures = self.failures.lock().ok()?;
        let wait_for = |key: &str, allowed: usize| {
            let recent: Vec<&Instant> = failures
                .get(key)?
                .iter()
                .filter(|t| now.duration_since(**t) < WINDOW)
                .collect();
            (recent.len() >= allowed).then(|| WINDOW.saturating_sub(now.duration_since(*recent[0])))
        };
        let by_account = wait_for(&account_key(account), ACCOUNT_TRIES);
        let by_address = ip.and_then(|ip| wait_for(&address_key(ip), ADDRESS_TRIES));
        by_account.max(by_address)
    }

    pub fn failed(&self, account: &str, ip: Option<IpAddr>) {
        let now = Instant::now();
        let Ok(mut failures) = self.failures.lock() else {
            return;
        };
        if failures.len() > MAX_TRACKED {
            failures.retain(|_, times| times.iter().any(|t| now.duration_since(*t) < WINDOW));
        }
        let mut add = |key: String| {
            let times = failures.entry(key).or_default();
            times.retain(|t| now.duration_since(*t) < WINDOW);
            times.push(now);
        };
        add(account_key(account));
        if let Some(ip) = ip {
            add(address_key(ip));
        }
    }

    /// A right password clears the account's failures (not the address's).
    pub fn succeeded(&self, account: &str) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.remove(&account_key(account));
        }
    }
}

fn account_key(account: &str) -> String {
    format!("account:{}", account.trim().to_lowercase())
}

fn address_key(ip: IpAddr) -> String {
    format!("address:{ip}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn home_addresses() {
        for home in [
            "127.0.0.1",
            "192.168.1.20",
            "10.1.2.3",
            "172.20.0.5",
            "100.101.102.103",
            "::1",
            "fd7a:115c:a1e0::1",
            "fe80::1",
            "::ffff:192.168.0.2",
        ] {
            assert!(is_home(ip(home)), "{home}");
        }
        for away in ["8.8.8.8", "100.128.0.1", "2001:4860::8888", "172.32.0.1"] {
            assert!(!is_home(ip(away)), "{away}");
        }
    }

    #[test]
    fn forwarding_headers_only_count_from_trusted_proxies() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("8.8.8.8"));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        let local = Some(ip("127.0.0.1"));

        // A tunnel on this computer, not trusted: not home, whatever the peer.
        let o = origin(local, &headers, &[]);
        assert_eq!((o.ip, o.home, o.https), (local, false, false));

        // Trusted: the forwarded client counts.
        let proxies = parse_proxies("127.0.0.1, 10.0.0.0/8 bad-entry");
        assert_eq!(proxies.len(), 2);
        let o = origin(local, &headers, &proxies);
        assert_eq!((o.ip, o.home, o.https), (Some(ip("8.8.8.8")), false, true));

        // No headers: the peer decides.
        let o = origin(Some(ip("192.168.1.9")), &HeaderMap::new(), &[]);
        assert!(o.home);
    }

    #[test]
    fn limiter_locks_accounts_then_addresses() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for _ in 0..ACCOUNT_TRIES {
            assert_eq!(limiter.wait("Sam", a), None);
            limiter.failed("sam", a);
        }
        assert!(limiter.wait("SAM", None).is_some(), "account locked");
        assert_eq!(limiter.wait("jo", a), None, "address not yet");
        for i in 0..ADDRESS_TRIES {
            limiter.failed(&format!("name{i}"), a);
        }
        assert!(limiter.wait("jo", a).is_some(), "address locked");
        assert_eq!(limiter.wait("jo", Some(ip("1.1.1.1"))), None);
        limiter.succeeded("sam");
        assert_eq!(limiter.wait("sam", None), None);
    }
}
