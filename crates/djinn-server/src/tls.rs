//! Encryption with nothing to set up. DJINN makes its own certificate the first
//! time it's needed and keeps it next to the database. No authority vouches for
//! it, so browsers warn about it, but the app remembers its fingerprint and from
//! then on refuses anything else, which is as strong against someone listening
//! on the network and needs no domain name.
//!
//! Each port answers both ways: the first byte of a connection says whether it
//! opens with TLS or plain HTTP. So addresses, firewall rules and the devices
//! that can't do this (Chromecast won't accept a certificate like this) carry on.

use std::collections::HashMap;
use std::future::Future;
use std::io::{self, IoSlice};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::serve::{ListenerExt, TapIo};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Sleep;
use tokio_rustls::TlsAcceptor;

/// 1 to offer encryption, which is the default.
pub const ENCRYPTION: &str = "go_encryption";
const CERTIFICATE: &str = "djinn.crt";
const KEY: &str = "djinn.key";
/// How long a connection has to say anything.
const FIRST_BYTE: Duration = Duration::from_secs(30);
const HANDSHAKE: Duration = Duration::from_secs(10);
/// Connections still deciding what they are; more than this are dropped.
const MAX_UNDECIDED: usize = 256;
/// A client gets this long to finish sending its first request's headers.
const REQUEST_HEAD: Duration = Duration::from_secs(30);
/// Connections open at once on a port, and from one address (a proxy counts as
/// one). Kept under the 1024 files Linux allows by default, with room for the
/// database and music files; past these, new connections are closed.
const MAX_OPEN: usize = 768;
const MAX_FROM_ONE_ADDRESS: usize = 256;
/// What every TLS connection opens with.
const TLS_HANDSHAKE_RECORD: u8 = 0x16;

/// The certificate in use.
pub struct Identity {
    config: Arc<ServerConfig>,
    /// SHA-256 of the certificate, as lowercase hex: what the app remembers.
    pub fingerprint: String,
}

/// What connections check for an identity, so it can change without restarting
/// a listener. Without one, TLS connections are dropped.
#[derive(Default)]
pub struct Encryption {
    identity: RwLock<Option<Arc<Identity>>>,
}

impl Encryption {
    pub fn set(&self, identity: Option<Identity>) {
        if let Ok(mut slot) = self.identity.write() {
            *slot = identity.map(Arc::new);
        }
    }

    fn current(&self) -> Option<Arc<Identity>> {
        self.identity.read().ok().and_then(|slot| slot.clone())
    }

    pub fn is_on(&self) -> bool {
        self.current().is_some()
    }

    pub fn fingerprint(&self) -> Option<String> {
        self.current().map(|identity| identity.fingerprint.clone())
    }
}

/// A short form of the fingerprint for people to compare between screens.
pub fn security_code(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .take(16)
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|group| group.iter().collect::<String>().to_uppercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn fingerprint_of(certificate: &[u8]) -> String {
    Sha256::digest(certificate)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The certificate in `dir`, made the first time.
pub fn load_or_make(dir: &Path) -> anyhow::Result<Identity> {
    let certificate = dir.join(CERTIFICATE);
    let key = dir.join(KEY);
    if !certificate.exists() || !key.exists() {
        make(dir)?;
    }
    let chain = CertificateDer::pem_file_iter(&certificate)?.collect::<Result<Vec<_>, _>>()?;
    let first = chain
        .first()
        .ok_or_else(|| anyhow::anyhow!("{} has no certificate", certificate.display()))?;
    let fingerprint = fingerprint_of(first);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(chain, PrivateKeyDer::from_pem_file(&key)?)?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Identity {
        config: Arc::new(config),
        fingerprint,
    })
}

fn make(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let names = vec![
        "localhost".to_owned(),
        format!("{}.local", crate::go_announce::computer_name()),
    ];
    let made = rcgen::generate_simple_self_signed(names)?;
    write_private(&dir.join(KEY), made.signing_key.serialize_pem().as_bytes())?;
    std::fs::write(dir.join(CERTIFICATE), made.cert.pem())?;
    tracing::info!(dir = %dir.display(), "made a certificate for encrypted connections");
    Ok(())
}

/// Written so that only this account can read it, from the moment it exists.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(contents)
}

/// Who a connection is from, and whether it's encrypted by DJINN itself.
#[derive(Clone, Copy, Debug)]
pub struct Peer {
    pub addr: SocketAddr,
    pub tls: bool,
}

/// How much a port puts up with.
#[derive(Clone, Copy)]
pub struct Limits {
    pub open: usize,
    pub from_one_address: usize,
    pub request_head: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            open: MAX_OPEN,
            from_one_address: MAX_FROM_ONE_ADDRESS,
            request_head: REQUEST_HEAD,
        }
    }
}

/// Connections open on a port, by address.
struct Openings {
    any: Arc<Semaphore>,
    by_address: Arc<Mutex<HashMap<IpAddr, usize>>>,
    limits: Limits,
}

impl Openings {
    fn new(limits: Limits) -> Self {
        Self {
            any: Arc::new(Semaphore::new(limits.open)),
            by_address: Arc::default(),
            limits,
        }
    }

    /// Room for one more from `ip`, for as long as the result is kept.
    fn open(&self, ip: IpAddr) -> Option<Open> {
        let permit = Arc::clone(&self.any).try_acquire_owned().ok()?;
        let mut counts = self
            .by_address
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = counts.entry(ip).or_default();
        if *count >= self.limits.from_one_address {
            return None;
        }
        *count += 1;
        Some(Open {
            ip,
            counts: Arc::clone(&self.by_address),
            _permit: permit,
        })
    }
}

/// One connection's place in the count.
struct Open {
    ip: IpAddr,
    counts: Arc<Mutex<HashMap<IpAddr, usize>>>,
    _permit: OwnedSemaphorePermit,
}

impl Drop for Open {
    fn drop(&mut self) {
        let mut counts = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.ip);
            }
        }
    }
}

/// Waiting for the end of a connection's first request head, by a deadline.
/// Only the first, since after that a quiet connection is just kept alive.
struct RequestHead {
    deadline: Pin<Box<Sleep>>,
    line_started: bool,
    seen_any: bool,
}

impl RequestHead {
    fn new(wait: Duration) -> Self {
        Self {
            deadline: Box::pin(tokio::time::sleep(wait)),
            line_started: false,
            seen_any: false,
        }
    }

    /// Reads on; true at the empty line that ends the head.
    fn finished_by(&mut self, bytes: &[u8]) -> bool {
        for byte in bytes {
            match byte {
                b'\n' if self.seen_any && !self.line_started => return true,
                b'\n' => self.line_started = false,
                b'\r' => {}
                _ => {
                    self.line_started = true;
                    self.seen_any = true;
                }
            }
        }
        false
    }
}

/// A connection that's held to its limits, and leaves the count when it ends.
struct Held {
    inner: Box<dyn Stream>,
    head: Option<RequestHead>,
    _open: Open,
}

impl AsyncRead for Held {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let polled = Pin::new(&mut this.inner).poll_read(cx, buf);
        if let Some(head) = &mut this.head {
            match &polled {
                Poll::Ready(Ok(())) => {
                    if head.finished_by(&buf.filled()[before..]) {
                        this.head = None;
                    }
                }
                Poll::Pending => {
                    if head.deadline.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(Err(io::ErrorKind::TimedOut.into()));
                    }
                }
                Poll::Ready(Err(_)) => {}
            }
        }
        polled
    }
}

impl AsyncWrite for Held {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A port that answers both ways. Connections are sorted on their own tasks, so
/// one that's slow to say anything can't hold up the rest.
pub struct Listener {
    ready: mpsc::Receiver<(Box<dyn Stream>, Peer)>,
    local: SocketAddr,
    accepting: JoinHandle<()>,
}

impl Listener {
    pub fn new(tcp: TcpListener, encryption: Arc<Encryption>, limits: Limits) -> io::Result<Self> {
        let local = tcp.local_addr()?;
        let (send, ready) = mpsc::channel(64);
        let accepting = tokio::spawn(accept_loop(tcp, encryption, send, limits));
        Ok(Self {
            ready,
            local,
            accepting,
        })
    }
}

/// What `listen` gives `axum::serve`.
pub type Serving = TapIo<Listener, fn(&mut Box<dyn Stream>)>;

/// A port that answers both ways, ready for `axum::serve` with connection info.
pub fn listen(tcp: TcpListener, encryption: Arc<Encryption>) -> io::Result<Serving> {
    listen_within(tcp, encryption, Limits::default())
}

fn listen_within(
    tcp: TcpListener,
    encryption: Arc<Encryption>,
    limits: Limits,
) -> io::Result<Serving> {
    fn leave_it(_: &mut Box<dyn Stream>) {}
    let tap: fn(&mut Box<dyn Stream>) = leave_it;
    Ok(Listener::new(tcp, encryption, limits)?.tap_io(tap))
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

impl axum::serve::Listener for Listener {
    type Io = Box<dyn Stream>;
    type Addr = Peer;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(connection) => connection,
            // Only once the accepting task is gone, which is at shutdown.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(Peer {
            addr: self.local,
            tls: false,
        })
    }
}

async fn accept_loop(
    tcp: TcpListener,
    encryption: Arc<Encryption>,
    ready: mpsc::Sender<(Box<dyn Stream>, Peer)>,
    limits: Limits,
) {
    let undecided = Arc::new(Semaphore::new(MAX_UNDECIDED));
    let openings = Openings::new(limits);
    loop {
        let (stream, peer) = match tcp.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                // Usually out of file handles; give it a moment.
                tracing::debug!(error = %e, "couldn't accept a connection");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Some(open) = openings.open(peer.ip().to_canonical()) else {
            continue;
        };
        let Ok(permit) = Arc::clone(&undecided).try_acquire_owned() else {
            continue;
        };
        // Notices phones that went away without saying so, which would
        // otherwise hold their places for good.
        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(60))
            .with_interval(Duration::from_secs(15));
        let _ = socket2::SockRef::from(&stream).set_tcp_keepalive(&keepalive);
        let encryption = Arc::clone(&encryption);
        let ready = ready.clone();
        tokio::spawn(async move {
            let sorted = sort(stream, &encryption).await;
            drop(permit);
            if let Some((inner, tls)) = sorted {
                let held: Box<dyn Stream> = Box::new(Held {
                    inner,
                    head: Some(RequestHead::new(limits.request_head)),
                    _open: open,
                });
                let _ = ready.send((held, Peer { addr: peer, tls })).await;
            }
        });
    }
}

/// Plain HTTP as it is, or TLS unwrapped and true. None if it's neither wanted
/// nor quick.
async fn sort(stream: TcpStream, encryption: &Encryption) -> Option<(Box<dyn Stream>, bool)> {
    let _ = stream.set_nodelay(true);
    let mut first = [0u8; 1];
    let read = tokio::time::timeout(FIRST_BYTE, stream.peek(&mut first))
        .await
        .ok()?
        .ok()?;
    if read == 0 {
        return None;
    }
    if first[0] != TLS_HANDSHAKE_RECORD {
        return Some((Box::new(stream), false));
    }
    let identity = encryption.current()?;
    let acceptor = TlsAcceptor::from(Arc::clone(&identity.config));
    let unwrapped = tokio::time::timeout(HANDSHAKE, acceptor.accept(stream))
        .await
        .ok()?
        .ok()?;
    Some((Box::new(unwrapped), true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ConnectInfo;
    use axum::routing::get;
    use axum::Router;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("djinn-tls-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_certificate_is_made_once_and_kept() {
        let dir = folder("kept");
        let first = load_or_make(&dir).unwrap();
        assert_eq!(first.fingerprint.len(), 64);
        let again = load_or_make(&dir).unwrap();
        assert_eq!(first.fingerprint, again.fingerprint);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(KEY))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "the key is private");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn security_codes_are_short_and_readable() {
        assert_eq!(
            security_code(&"ab12cd34ef567890".repeat(4)),
            "AB12-CD34-EF56-7890"
        );
    }

    #[tokio::test]
    async fn one_port_answers_both_ways() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let dir = folder("both");
        let identity = load_or_make(&dir).unwrap();
        let expected = identity.fingerprint.clone();
        let encryption = Arc::new(Encryption::default());
        encryption.set(Some(identity));

        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        let listener = listen(tcp, Arc::clone(&encryption)).unwrap();
        let app = Router::new().route(
            "/who",
            get(|ConnectInfo(peer): ConnectInfo<Peer>| async move {
                format!("{} {}", peer.addr.ip(), peer.tls)
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<Peer>()).await
        });

        let client = reqwest::Client::builder()
            .tls_danger_accept_invalid_certs(true)
            .tls_info(true)
            .no_proxy()
            .build()
            .unwrap();
        let plain = client
            .get(format!("http://127.0.0.1:{port}/who"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            plain.text().await.unwrap(),
            "127.0.0.1 false",
            "plain, with the peer"
        );

        let secure = client
            .get(format!("https://127.0.0.1:{port}/who"))
            .send()
            .await
            .unwrap();
        let seen = secure
            .extensions()
            .get::<reqwest::tls::TlsInfo>()
            .and_then(|info| info.peer_certificate())
            .map(fingerprint_of);
        assert_eq!(
            seen.as_deref(),
            Some(expected.as_str()),
            "its own certificate"
        );
        assert_eq!(
            secure.text().await.unwrap(),
            "127.0.0.1 true",
            "encrypted, with the peer"
        );

        // Switched off: the same port stops answering TLS, and still does plain.
        encryption.set(None);
        let fresh = reqwest::Client::builder()
            .tls_danger_accept_invalid_certs(true)
            .no_proxy()
            .build()
            .unwrap();
        assert!(fresh
            .get(format!("https://127.0.0.1:{port}/who"))
            .send()
            .await
            .is_err());
        assert!(fresh
            .get(format!("http://127.0.0.1:{port}/who"))
            .send()
            .await
            .is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn limited_server(limits: Limits) -> SocketAddr {
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = listen_within(tcp, Arc::new(Encryption::default()), limits).unwrap();
        let app = Router::new().route("/", get(|| async { "hello" }));
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<Peer>()).await
        });
        addr
    }

    /// What's left to read: closed by the server, or still open.
    async fn closes_within(stream: &mut TcpStream, wait: Duration) -> bool {
        use tokio::io::AsyncReadExt;
        let mut buf = [0u8; 256];
        tokio::time::timeout(wait, async {
            loop {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        })
        .await
        .is_ok()
    }

    #[tokio::test]
    async fn a_request_that_never_finishes_is_dropped() {
        use tokio::io::AsyncWriteExt;
        let addr = limited_server(Limits {
            request_head: Duration::from_millis(200),
            ..Limits::default()
        })
        .await;
        let mut stalling = TcpStream::connect(addr).await.unwrap();
        stalling
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n")
            .await
            .unwrap();
        assert!(closes_within(&mut stalling, Duration::from_secs(3)).await);

        // One that finishes in time can stay open afterwards, idle.
        let mut kept = TcpStream::connect(addr).await.unwrap();
        kept.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .await
            .unwrap();
        assert!(!closes_within(&mut kept, Duration::from_millis(600)).await);
    }

    #[tokio::test]
    async fn there_are_only_so_many_connections() {
        use tokio::io::AsyncWriteExt;
        let addr = limited_server(Limits {
            from_one_address: 2,
            ..Limits::default()
        })
        .await;
        let mut first = TcpStream::connect(addr).await.unwrap();
        let _second = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut third = TcpStream::connect(addr).await.unwrap();
        assert!(closes_within(&mut third, Duration::from_secs(3)).await);

        // A place opens up when one closes.
        first.shutdown().await.unwrap();
        drop(first);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut fourth = TcpStream::connect(addr).await.unwrap();
        assert!(!closes_within(&mut fourth, Duration::from_millis(300)).await);
    }

    #[test]
    fn a_request_head_ends_at_its_empty_line() {
        let wait = Duration::from_secs(1);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let _in_runtime = rt.enter();
        let mut head = RequestHead::new(wait);
        assert!(!head.finished_by(b"\r\nGET / HTTP/1.1\r\nHost: x\r"));
        assert!(head.finished_by(b"\n\r\n"));
        let mut lf = RequestHead::new(wait);
        assert!(lf.finished_by(b"GET / HTTP/1.1\n\n"));
    }

    #[tokio::test]
    async fn a_silent_connection_holds_nothing_up() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = listen(tcp, Arc::new(Encryption::default())).unwrap();
        let app = Router::new().route(
            "/who",
            get(|ConnectInfo(peer): ConnectInfo<Peer>| async move { peer.addr.ip().to_string() }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<Peer>()).await
        });
        let _silent = TcpStream::connect(addr).await.unwrap();
        let answer = tokio::time::timeout(
            Duration::from_secs(5),
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .get(format!("http://{addr}/who"))
                .send(),
        )
        .await;
        assert!(matches!(answer, Ok(Ok(_))));
    }
}
