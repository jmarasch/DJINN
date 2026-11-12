//! Who's really asking and from where (behind trusted proxies), whether
//! that's the home network, and slowing down password guessing.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request};
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;

use crate::error::ApiError;

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

/// What pages on the party port may load: its own files, and the photos an
/// admin chose to show straight from Unsplash.
pub const PARTY_CSP: HeaderValue = HeaderValue::from_static(
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob: https://images.unsplash.com; media-src 'self' blob:; \
     connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; \
     frame-ancestors 'none'",
);
/// The TV and lyrics pages: the same, but a node's own page (on the box
/// itself) may show them in a frame, over its visualizer.
pub const SCREEN_CSP: HeaderValue = HeaderValue::from_static(
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob: https://images.unsplash.com; media-src 'self' blob:; \
     connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; \
     frame-ancestors 'self' http://localhost:8066 http://127.0.0.1:8066",
);
/// Pages a node's screen shows in a frame.
pub const SCREEN_PAGES: [&str; 2] = ["/tv", "/lyrics"];
/// The same for DJINN GO's page.
pub const GO_CSP: HeaderValue = HeaderValue::from_static(
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
     object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
);

/// Pages on a node's own port: its status page, which shows Main's TV or
/// lyrics page in a frame (so frames may come from any DJINN address).
pub const NODE_CSP: HeaderValue = HeaderValue::from_static(
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
     frame-src http: https:; object-src 'none'; base-uri 'none'; form-action 'self'; \
     frame-ancestors 'none'",
);

/// Bytes as lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// `n` random bytes from the system, hex encoded: tokens, ids and secrets.
pub fn random_hex(n: usize) -> anyhow::Result<String> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    Ok(hex(&bytes))
}

/// `len` random characters from `alphabet`, each equally likely: a byte
/// that would favor the alphabet's first letters is thrown away rather than
/// wrapped around.
pub fn random_text(len: usize, alphabet: &[u8]) -> anyhow::Result<String> {
    debug_assert!(!alphabet.is_empty() && alphabet.len() <= 256);
    let fair_below = 256 - (256 % alphabet.len());
    let mut out = String::with_capacity(len);
    let mut bytes = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
        for b in bytes {
            if out.len() == len {
                break;
            }
            if usize::from(b) < fair_below {
                out.push(char::from(alphabet[usize::from(b) % alphabet.len()]));
            }
        }
    }
    Ok(out)
}

/// Whether two secrets match, taking the same time whatever differs, so
/// nobody learns how much of a guess was right from how fast it was refused.
pub fn same_secret(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Security headers for a response. HSTS only through a proxy's HTTPS: on
/// DJINN's own certificate, which browsers warn about, it would leave no way
/// past the warning.
pub fn set_headers(headers: &mut HeaderMap, csp: HeaderValue, proxied_https: bool) {
    headers.insert(header::CONTENT_SECURITY_POLICY, csp);
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    if proxied_https {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000"),
        );
    }
}

/// Whether a browser says this request was made by a page from somewhere
/// else. Cookies keep to their site but not their port, so another program on
/// this computer could otherwise press DJINN's buttons for someone signed in.
/// Forwarding headers mean a proxy may have changed the Host; pages can't add
/// those to a request without asking first, which DJINN never allows.
fn from_another_origin(headers: &HeaderMap, authority: Option<&str>) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    if [
        "x-forwarded-for",
        "x-forwarded-host",
        "cf-connecting-ip",
        "forwarded",
    ]
    .iter()
    .any(|name| headers.contains_key(*name))
    {
        return false;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .or(authority);
    let Some(host) = host else { return false };
    let from = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    !from.eq_ignore_ascii_case(host)
}

/// Refuses changes asked for by pages from somewhere else. Apps send no
/// Origin, and Subsonic calls carry their own sign-in rather than a cookie.
pub async fn same_origin(request: Request, next: Next) -> Response {
    let reads = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    if !reads
        && !request.uri().path().starts_with("/rest/")
        && from_another_origin(
            request.headers(),
            request.uri().authority().map(|a| a.as_str()),
        )
    {
        return ApiError::forbidden().into_response();
    }
    next.run(request).await
}

/// Tries allowed in a window: one account from one address, one address on
/// any accounts, and one account from everywhere. A stranger can use up the
/// first two but not the owner's own, so they can't lock anyone out without
/// many addresses; the last one still stops guessing spread across many.
const PAIR_TRIES: usize = 5;
const ADDRESS_TRIES: usize = 20;
const ACCOUNT_TRIES: usize = 50;
/// Two-step codes for one account, from anywhere. Only someone who has the
/// password gets this far, so a right password doesn't start these over.
const CODE_TRIES: usize = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);
/// How many names and addresses to remember before forgetting old ones.
const MAX_TRACKED: usize = 10_000;

/// One thing being counted, and how many of it a window allows.
struct Rule {
    key: String,
    allowed: usize,
}

fn tries_rules(account: Option<&str>, ip: Option<IpAddr>) -> Vec<Rule> {
    let mut rules = Vec::with_capacity(3);
    if let Some(account) = account.map(|a| a.trim().to_lowercase()) {
        let from = ip.map(|ip| ip.to_string()).unwrap_or_default();
        rules.push(Rule {
            key: format!("pair:{account}|{from}"),
            allowed: PAIR_TRIES,
        });
        rules.push(Rule {
            key: format!("account:{account}"),
            allowed: ACCOUNT_TRIES,
        });
    }
    if let Some(ip) = ip {
        rules.push(Rule {
            key: format!("address:{ip}"),
            allowed: ADDRESS_TRIES,
        });
    }
    rules
}

fn code_rules(account: &str, ip: Option<IpAddr>) -> Vec<Rule> {
    let mut rules = vec![Rule {
        key: format!("code:{}", account.trim().to_lowercase()),
        allowed: CODE_TRIES,
    }];
    rules.extend(tries_rules(None, ip));
    rules
}

/// Slows password guessing. Password sign-ins and two-step codes are counted
/// before they're checked, so tries sent all at once count too; app passwords
/// and passwords asked of someone already signed in are counted only when
/// wrong, since those checks cost next to nothing.
#[derive(Default)]
pub struct Limiter {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Limiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Instant>>> {
        // A panic elsewhere must not switch the limits off.
        self.failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wait_for(
        failures: &HashMap<String, Vec<Instant>>,
        rules: &[Rule],
        now: Instant,
    ) -> Option<Duration> {
        rules
            .iter()
            .filter_map(|rule| {
                let recent: Vec<&Instant> = failures
                    .get(&rule.key)?
                    .iter()
                    .filter(|t| now.duration_since(**t) < WINDOW)
                    .collect();
                (recent.len() >= rule.allowed)
                    .then(|| WINDOW.saturating_sub(now.duration_since(*recent[0])))
            })
            .max()
    }

    fn count(failures: &mut HashMap<String, Vec<Instant>>, rules: &[Rule], now: Instant) {
        if failures.len() > MAX_TRACKED {
            failures.retain(|_, times| times.iter().any(|t| now.duration_since(*t) < WINDOW));
        }
        // Still full of recent names: forget the one quiet longest.
        while failures.len() > MAX_TRACKED {
            let quietest = failures
                .iter()
                .min_by_key(|(_, times)| times.last().copied())
                .map(|(key, _)| key.clone());
            match quietest {
                Some(key) => failures.remove(&key),
                None => break,
            };
        }
        for rule in rules {
            let times = failures.entry(rule.key.clone()).or_default();
            times.retain(|t| now.duration_since(*t) < WINDOW);
            times.push(now);
        }
    }

    fn begin(&self, rules: &[Rule]) -> Result<(), Duration> {
        let now = Instant::now();
        let mut failures = self.lock();
        if let Some(wait) = Self::wait_for(&failures, rules, now) {
            return Err(wait);
        }
        Self::count(&mut failures, rules, now);
        Ok(())
    }

    /// Takes back the newest try under each rule.
    fn take_back(&self, rules: &[Rule]) {
        let mut failures = self.lock();
        for rule in rules {
            if let Some(times) = failures.get_mut(&rule.key) {
                times.pop();
                if times.is_empty() {
                    failures.remove(&rule.key);
                }
            }
        }
    }

    /// Counts a password sign-in, or says how long until one is allowed.
    pub fn begin_sign_in(&self, account: &str, ip: Option<IpAddr>) -> Result<(), Duration> {
        self.begin(&tries_rules(Some(account), ip))
    }

    /// The server couldn't check that try, so it doesn't count.
    pub fn undo_sign_in(&self, account: &str, ip: Option<IpAddr>) {
        self.take_back(&tries_rules(Some(account), ip));
    }

    /// The password was right: this try doesn't count, nor do this address's
    /// earlier ones for the account. Two-step codes aren't touched.
    pub fn sign_in_ok(&self, account: &str, ip: Option<IpAddr>) {
        let rules = tries_rules(Some(account), ip);
        self.take_back(&rules);
        if let Some(pair) = rules.first() {
            self.lock().remove(&pair.key);
        }
    }

    /// Counts a two-step code, or says how long until one is allowed.
    pub fn begin_code(&self, account: &str, ip: Option<IpAddr>) -> Result<(), Duration> {
        self.begin(&code_rules(account, ip))
    }

    /// The code was right: the sign-in is finished, and the count starts over.
    pub fn code_ok(&self, account: &str, ip: Option<IpAddr>) {
        let rules = code_rules(account, ip);
        self.take_back(&rules);
        if let Some(code) = rules.first() {
            self.lock().remove(&code.key);
        }
    }

    /// How long until `account` from `ip` may try again; None if now. With no
    /// account, as for app keys that don't name one, only the address counts.
    pub fn wait(&self, account: Option<&str>, ip: Option<IpAddr>) -> Option<Duration> {
        Self::wait_for(&self.lock(), &tries_rules(account, ip), Instant::now())
    }

    pub fn failed(&self, account: Option<&str>, ip: Option<IpAddr>) {
        Self::count(&mut self.lock(), &tries_rules(account, ip), Instant::now());
    }

    /// Counts something an address may only do so often, like making
    /// accounts. Requests with no known address aren't limited.
    pub fn allow(&self, what: &str, ip: Option<IpAddr>, allowed: usize) -> Result<(), Duration> {
        let Some(ip) = ip else { return Ok(()) };
        self.begin(&[Rule {
            key: format!("{what}:{ip}"),
            allowed,
        }])
    }
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
    fn pages_from_elsewhere_are_noticed() {
        let with = |pairs: &[(&'static str, &'static str)]| {
            let mut headers = HeaderMap::new();
            for (name, value) in pairs {
                headers.insert(*name, HeaderValue::from_static(value));
            }
            from_another_origin(&headers, None)
        };
        let host = ("host", "192.168.1.5:8066");
        assert!(!with(&[host]), "an app sends no Origin");
        assert!(!with(&[host, ("origin", "http://192.168.1.5:8066")]));
        assert!(!with(&[host, ("origin", "https://192.168.1.5:8066")]));
        assert!(!with(&[
            ("host", "Music.example.com"),
            ("origin", "https://music.example.com")
        ]));
        assert!(
            with(&[host, ("origin", "http://192.168.1.5:9000")]),
            "another port"
        );
        assert!(with(&[host, ("origin", "https://evil.example")]));
        assert!(with(&[host, ("origin", "null")]));
        assert!(
            !with(&[
                host,
                ("origin", "https://music.example.com"),
                ("x-forwarded-for", "1.2.3.4")
            ]),
            "a proxy may have changed the Host"
        );
    }

    #[test]
    fn a_stranger_cant_lock_the_owner_out() {
        let limiter = Limiter::default();
        let stranger = Some(ip("8.8.8.8"));
        let owner = Some(ip("1.1.1.1"));
        for _ in 0..PAIR_TRIES {
            assert_eq!(limiter.begin_sign_in("Sam", stranger), Ok(()));
        }
        assert!(limiter.begin_sign_in("SAM", stranger).is_err());
        assert_eq!(limiter.begin_sign_in("sam", owner), Ok(()), "the owner");
        limiter.sign_in_ok("sam", owner);
    }

    #[test]
    fn addresses_and_accounts_have_their_own_limits() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for i in 0..ADDRESS_TRIES {
            assert_eq!(limiter.begin_sign_in(&format!("name{i}"), a), Ok(()));
        }
        assert!(limiter.begin_sign_in("jo", a).is_err(), "address locked");
        assert_eq!(limiter.begin_sign_in("jo", Some(ip("1.1.1.1"))), Ok(()));

        // Guessing at one account from many addresses.
        let limiter = Limiter::default();
        for i in 0..ACCOUNT_TRIES {
            let from = IpAddr::from([10, 0, 0, u8::try_from(i).unwrap()]);
            assert_eq!(limiter.begin_sign_in("sam", Some(from)), Ok(()));
        }
        assert!(limiter.begin_sign_in("sam", Some(ip("1.1.1.1"))).is_err());
    }

    #[test]
    fn tries_sent_at_once_count_before_any_is_checked() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        let started = (0..100)
            .filter(|_| limiter.begin_sign_in("sam", a).is_ok())
            .count();
        assert_eq!(started, PAIR_TRIES);
    }

    #[test]
    fn a_right_password_clears_only_its_own_tries() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for _ in 0..PAIR_TRIES - 1 {
            limiter.begin_sign_in("sam", a).unwrap();
        }
        limiter.begin_sign_in("sam", a).unwrap();
        limiter.sign_in_ok("sam", a);
        for _ in 0..PAIR_TRIES {
            assert_eq!(limiter.begin_sign_in("sam", a), Ok(()));
        }
        assert!(limiter.begin_sign_in("sam", a).is_err());
    }

    #[test]
    fn a_failure_the_server_caused_doesnt_count() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for _ in 0..PAIR_TRIES * 2 {
            limiter.begin_sign_in("sam", a).unwrap();
            limiter.undo_sign_in("sam", a);
        }
        assert_eq!(limiter.begin_sign_in("sam", a), Ok(()));
    }

    #[test]
    fn a_right_password_doesnt_start_codes_over() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for _ in 0..CODE_TRIES {
            limiter.begin_sign_in("sam", a).unwrap();
            limiter.sign_in_ok("sam", a);
            assert_eq!(limiter.begin_code("sam", a), Ok(()));
        }
        limiter.begin_sign_in("sam", a).unwrap();
        limiter.sign_in_ok("sam", a);
        assert!(limiter.begin_code("sam", a).is_err(), "still locked");
        // From anywhere else too.
        assert!(limiter.begin_code("sam", Some(ip("1.1.1.1"))).is_err());

        let limiter = Limiter::default();
        limiter.begin_code("sam", a).unwrap();
        limiter.code_ok("sam", a);
        for _ in 0..CODE_TRIES {
            assert_eq!(limiter.begin_code("sam", a), Ok(()), "a right code clears");
        }
    }

    #[test]
    fn app_keys_only_count_the_address() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        for _ in 0..PAIR_TRIES {
            limiter.failed(None, a);
        }
        assert_eq!(limiter.wait(None, a), None, "under the address's limit");
        assert_eq!(limiter.wait(None, Some(ip("1.1.1.1"))), None);
        for _ in PAIR_TRIES..ADDRESS_TRIES {
            limiter.failed(None, a);
        }
        assert!(limiter.wait(None, a).is_some());
        assert_eq!(
            limiter.wait(None, Some(ip("1.1.1.1"))),
            None,
            "others go on"
        );
    }

    #[test]
    fn counts_things_an_address_does_often() {
        let limiter = Limiter::default();
        let a = Some(ip("8.8.8.8"));
        assert_eq!(limiter.allow("signup", a, 2), Ok(()));
        assert_eq!(limiter.allow("signup", a, 2), Ok(()));
        assert!(limiter.allow("signup", a, 2).is_err());
        assert_eq!(limiter.allow("guest", a, 2), Ok(()), "counted apart");
        assert_eq!(limiter.allow("signup", None, 2), Ok(()));
    }

    #[test]
    fn hex_and_random_text() {
        assert_eq!(hex(&[0, 15, 255]), "000fff");
        assert_eq!(random_hex(16).unwrap().len(), 32);
        let text = random_text(200, b"abc").unwrap();
        assert_eq!(text.len(), 200);
        assert!(text.bytes().all(|b| b"abc".contains(&b)));
        assert!(same_secret(b"abc", b"abc"));
        assert!(!same_secret(b"abc", b"abd") && !same_secret(b"abc", b"ab"));
    }

    #[test]
    fn the_list_of_names_stays_bounded() {
        let limiter = Limiter::default();
        for i in 0..MAX_TRACKED + 50 {
            limiter.failed(Some(&format!("name{i}")), None);
        }
        assert!(limiter.lock().len() <= MAX_TRACKED + 2);
    }
}
