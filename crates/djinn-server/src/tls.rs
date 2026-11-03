//! Encryption with nothing to set up. DJINN makes its own certificate the first
//! time it's needed and keeps it next to the database. No authority vouches for
//! it, so browsers warn about it, but the app remembers its fingerprint and from
//! then on refuses anything else, which is as strong against someone listening
//! on the network and needs no domain name.
//!
//! Each port answers both ways: the first byte of a connection says whether it
//! opens with TLS or plain HTTP. So addresses, firewall rules and the devices
//! that can't do this (Chromecast won't accept a certificate like this) carry on.

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use axum::serve::{ListenerExt, TapIo};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;
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

pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

/// A port that answers both ways. Connections are sorted on their own tasks, so
/// one that's slow to say anything can't hold up the rest.
pub struct Listener {
    ready: mpsc::Receiver<(Box<dyn Stream>, SocketAddr)>,
    local: SocketAddr,
    accepting: JoinHandle<()>,
}

impl Listener {
    pub fn new(tcp: TcpListener, encryption: Arc<Encryption>) -> io::Result<Self> {
        let local = tcp.local_addr()?;
        let (send, ready) = mpsc::channel(64);
        let accepting = tokio::spawn(accept_loop(tcp, encryption, send));
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
    fn leave_it(_: &mut Box<dyn Stream>) {}
    let tap: fn(&mut Box<dyn Stream>) = leave_it;
    Ok(Listener::new(tcp, encryption)?.tap_io(tap))
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.accepting.abort();
    }
}

impl axum::serve::Listener for Listener {
    type Io = Box<dyn Stream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        match self.ready.recv().await {
            Some(connection) => connection,
            // Only once the accepting task is gone, which is at shutdown.
            None => std::future::pending().await,
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.local)
    }
}

async fn accept_loop(
    tcp: TcpListener,
    encryption: Arc<Encryption>,
    ready: mpsc::Sender<(Box<dyn Stream>, SocketAddr)>,
) {
    let undecided = Arc::new(Semaphore::new(MAX_UNDECIDED));
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
        let Ok(permit) = Arc::clone(&undecided).try_acquire_owned() else {
            continue;
        };
        let encryption = Arc::clone(&encryption);
        let ready = ready.clone();
        tokio::spawn(async move {
            let sorted = sort(stream, &encryption).await;
            drop(permit);
            if let Some(io) = sorted {
                let _ = ready.send((io, peer)).await;
            }
        });
    }
}

/// Plain HTTP as it is, or TLS unwrapped. None if it's neither wanted nor quick.
async fn sort(stream: TcpStream, encryption: &Encryption) -> Option<Box<dyn Stream>> {
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
        return Some(Box::new(stream));
    }
    let identity = encryption.current()?;
    let acceptor = TlsAcceptor::from(Arc::clone(&identity.config));
    let unwrapped = tokio::time::timeout(HANDSHAKE, acceptor.accept(stream))
        .await
        .ok()?
        .ok()?;
    Some(Box::new(unwrapped))
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
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.ip().to_string() }),
        );
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
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
            "127.0.0.1",
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
            "127.0.0.1",
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

    #[tokio::test]
    async fn a_silent_connection_holds_nothing_up() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let listener = listen(tcp, Arc::new(Encryption::default())).unwrap();
        let app = Router::new().route(
            "/who",
            get(|ConnectInfo(peer): ConnectInfo<SocketAddr>| async move { peer.ip().to_string() }),
        );
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
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
