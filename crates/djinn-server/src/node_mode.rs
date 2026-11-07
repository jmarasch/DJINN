//! Node mode: this box plays a room for a main DJINN instead of being one.
//!
//! A node keeps no library and serves no party app; phones go to Main. It
//! has its own small page (`/node`) to enter Main's address and to see how
//! pairing stands. It connects to Main over Main's encrypted connection,
//! pins Main's certificate the first time (both screens show Main's security
//! code, so a wrong server shows a wrong code), and asks to join with a
//! secret only it knows. It then checks in now and then: Main answers
//! waiting, accepted (with the room to play), or "unknown node", which after
//! being accepted means kicked: the node wipes its cache at once. Once
//! accepted, the node's page only shows status, so nobody on the network can
//! repoint it; leaving goes through Main's Kick.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Redirect;
use axum::routing::{get, post};
use axum::{Json, Router};
use djinn_store::Store;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::error::{ApiError, ApiResult};

/// Setting key: "node" makes this box a node at the next start.
pub const ROLE: &str = "djinn_role";
const NODE_ID: &str = "node_id";
const NODE_SECRET: &str = "node_secret";
const NODE_MAIN: &str = "node_main";
const NODE_MAIN_FP: &str = "node_main_fingerprint";
const NODE_NAME: &str = "node_name";
const NODE_STATE: &str = "node_state";
/// Main's port when an address doesn't say.
const MAIN_PORT: u16 = 8066;
const CHECK_WAITING: Duration = Duration::from_secs(5);
const CHECK_JOINED: Duration = Duration::from_secs(30);

/// True if this box is set up as a node.
pub async fn is_node(store: &Store) -> anyhow::Result<bool> {
    Ok(store.get_setting(ROLE).await?.as_deref() == Some("node"))
}

/// Where a pairing stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pairing {
    /// No Main yet.
    Setup,
    /// Asking Main, or waiting for its admin.
    Waiting,
    Joined,
    /// Main's admin said no.
    Declined,
    /// Main's admin kicked this node; its cache is wiped.
    Kicked,
    /// Main answered with a different certificate than before.
    Changed,
}

impl Pairing {
    fn as_str(self) -> &'static str {
        match self {
            Pairing::Setup => "setup",
            Pairing::Waiting => "waiting",
            Pairing::Joined => "joined",
            Pairing::Declined => "declined",
            Pairing::Kicked => "kicked",
            Pairing::Changed => "changed",
        }
    }

    fn parse(text: &str) -> Pairing {
        match text {
            "waiting" => Pairing::Waiting,
            "joined" => Pairing::Joined,
            "declined" => Pairing::Declined,
            "kicked" => Pairing::Kicked,
            "changed" => Pairing::Changed,
            _ => Pairing::Setup,
        }
    }
}

/// What the node's page shows.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub pairing: Pairing,
    /// This node's name, as Main lists it.
    pub name: String,
    /// Main's address, like "https://192.168.1.10:8066".
    pub main: Option<String>,
    pub main_name: Option<String>,
    /// Main's security code (from its certificate), to compare with Main's
    /// Admin > Nodes.
    pub main_code: Option<String>,
    /// This request's code, which Main's admin sees beside it.
    pub code: Option<String>,
    pub room_id: Option<i64>,
    /// Why the last try didn't work, in words.
    pub problem: Option<String>,
}

/// The node's running parts.
#[derive(Clone)]
pub struct Node {
    store: Store,
    cache_dir: Arc<PathBuf>,
    status: Arc<RwLock<Status>>,
    wake: Arc<Notify>,
}

impl Node {
    pub async fn new(store: Store, cache_dir: PathBuf) -> anyhow::Result<Node> {
        let name = match store.get_setting(NODE_NAME).await? {
            Some(name) if !name.is_empty() => name,
            _ => crate::go_announce::computer_name(),
        };
        let pairing = Pairing::parse(&store.get_setting(NODE_STATE).await?.unwrap_or_default());
        let main = store
            .get_setting(NODE_MAIN)
            .await?
            .filter(|m| !m.is_empty());
        let main_code = store
            .get_setting(NODE_MAIN_FP)
            .await?
            .filter(|f| !f.is_empty())
            .map(|f| crate::tls::security_code(&f));
        Ok(Node {
            store,
            cache_dir: Arc::new(cache_dir),
            status: Arc::new(RwLock::new(Status {
                pairing,
                name,
                main,
                main_name: None,
                main_code,
                code: None,
                room_id: None,
                problem: None,
            })),
            wake: Arc::new(Notify::new()),
        })
    }

    pub fn status(&self) -> Status {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    fn update(&self, change: impl FnOnce(&mut Status)) {
        match self.status.write() {
            Ok(mut s) => change(&mut s),
            Err(e) => change(&mut e.into_inner()),
        }
    }

    async fn set_pairing(&self, pairing: Pairing) -> anyhow::Result<()> {
        self.store.set_setting(NODE_STATE, pairing.as_str()).await?;
        self.update(|s| s.pairing = pairing);
        Ok(())
    }

    /// This node's id and secret, made the first time.
    async fn identity(&self) -> anyhow::Result<(String, String)> {
        let id = self
            .store
            .get_setting(NODE_ID)
            .await?
            .filter(|v| !v.is_empty());
        let secret = self
            .store
            .get_setting(NODE_SECRET)
            .await?
            .filter(|v| !v.is_empty());
        if let (Some(id), Some(secret)) = (id, secret) {
            return Ok((id, secret));
        }
        let id = format!("node-{}", random_hex(8)?);
        let secret = random_hex(32)?;
        self.store.set_setting(NODE_ID, &id).await?;
        self.store.set_setting(NODE_SECRET, &secret).await?;
        Ok((id, secret))
    }

    /// Points this node at a Main and asks to join. Not once joined: that
    /// goes through Main's Kick.
    pub async fn connect(&self, address: &str, name: &str) -> Result<(), String> {
        if self.status().pairing == Pairing::Joined {
            return Err("this node has joined; kick it on Main to change that".into());
        }
        let main = main_url(address).ok_or("that doesn't look like an address")?;
        let name = name.trim();
        let name = if name.is_empty() {
            crate::go_announce::computer_name()
        } else {
            name.chars().take(60).collect()
        };
        let save = async {
            self.store.set_setting(NODE_MAIN, &main).await?;
            self.store.set_setting(NODE_NAME, &name).await?;
            self.store.set_setting(NODE_MAIN_FP, "").await?;
            self.set_pairing(Pairing::Waiting).await
        };
        save.await.map_err(|e| e.to_string())?;
        self.update(|s| {
            s.main = Some(main);
            s.name = name;
            s.main_code = None;
            s.main_name = None;
            s.code = None;
            s.problem = None;
        });
        self.wake.notify_one();
        Ok(())
    }

    /// After being declined or kicked, or if Main's certificate changed:
    /// a fresh id and secret, Main's certificate learned again, and a new
    /// request.
    pub async fn ask_again(&self) -> Result<(), String> {
        let status = self.status();
        if matches!(status.pairing, Pairing::Joined | Pairing::Setup) {
            return Err("there's nothing to ask again".into());
        }
        let reset = async {
            self.store.set_setting(NODE_ID, "").await?;
            self.store.set_setting(NODE_SECRET, "").await?;
            self.store.set_setting(NODE_MAIN_FP, "").await?;
            self.set_pairing(Pairing::Waiting).await
        };
        reset.await.map_err(|e| e.to_string())?;
        self.update(|s| {
            s.main_code = None;
            s.code = None;
            s.problem = None;
        });
        self.wake.notify_one();
        Ok(())
    }

    /// Deletes every cached song.
    pub fn wipe_cache(&self) {
        wipe(&self.cache_dir);
    }

    /// Keeps the pairing going: asks, waits, checks in, and wipes the cache
    /// if kicked. Runs until the process ends.
    pub async fn run(self) {
        loop {
            let wait = match self.check().await {
                Ok(wait) => wait,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "node check failed");
                    Some(CHECK_WAITING)
                }
            };
            match wait {
                Some(wait) => {
                    let _ = tokio::time::timeout(wait, self.wake.notified()).await;
                }
                None => self.wake.notified().await,
            }
        }
    }

    /// One look at Main. Returns how long until the next, or None to wait
    /// until someone acts on the node's page.
    async fn check(&self) -> anyhow::Result<Option<Duration>> {
        let status = self.status();
        let Some(main) = status.main.clone() else {
            return Ok(None);
        };
        if !matches!(status.pairing, Pairing::Waiting | Pairing::Joined) {
            return Ok(None);
        }
        let (id, secret) = self.identity().await?;
        let pinned = self
            .store
            .get_setting(NODE_MAIN_FP)
            .await?
            .filter(|f| !f.is_empty());
        let seen = Arc::new(Mutex::new(None));
        let client = pinned_client(pinned.clone(), Arc::clone(&seen))?;
        let auth = format!("Node {id}:{secret}");

        // Already asked (and Main's certificate known): check in.
        let checked = if pinned.is_some() {
            Some(
                client
                    .get(format!("{main}/api/nodes/me"))
                    .header("authorization", &auth)
                    .send()
                    .await,
            )
        } else {
            None
        };
        let response = match checked {
            Some(sent) => sent,
            None => {
                let body = serde_json::json!({
                    "node_id": id,
                    "name": status.name,
                    "secret": secret,
                    "version": env!("DJINN_RELEASE_VERSION"),
                    "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                });
                client
                    .post(format!("{main}/api/nodes/join"))
                    .json(&body)
                    .send()
                    .await
            }
        };
        let response = match response {
            Ok(response) => response,
            Err(e) => {
                let changed = matches!((&pinned, seen.lock().ok().and_then(|s| s.clone())), (Some(p), Some(s)) if *p != s);
                if changed {
                    self.set_pairing(Pairing::Changed).await?;
                    self.update(|s| {
                        s.problem = Some(
                            "Main answered with a different identity than before. If you reinstalled Main, \
                             ask to join again; otherwise something else may be answering at that address."
                                .into(),
                        )
                    });
                    return Ok(None);
                }
                self.update(|s| s.problem = Some(format!("Can't reach Main: {}", plain_error(&e))));
                return Ok(Some(CHECK_WAITING));
            }
        };
        // The certificate this connection saw; pinned from the first answer.
        if pinned.is_none() {
            if let Some(fingerprint) = seen.lock().ok().and_then(|s| s.clone()) {
                self.store.set_setting(NODE_MAIN_FP, &fingerprint).await?;
                self.update(|s| s.main_code = Some(crate::tls::security_code(&fingerprint)));
            }
        }
        let code = crate::nodes::pairing_code(&id, &crate::nodes::hash_secret(&secret));
        let http = response.status();
        if http == reqwest::StatusCode::UNAUTHORIZED {
            // Main doesn't know this node: declined while waiting, or kicked.
            if status.pairing == Pairing::Joined {
                self.wipe_cache();
                self.set_pairing(Pairing::Kicked).await?;
                self.update(|s| s.problem = None);
                tracing::warn!("kicked by Main; cache wiped");
            } else {
                self.set_pairing(Pairing::Declined).await?;
                self.update(|s| s.problem = None);
            }
            return Ok(None);
        }
        let answer: serde_json::Value = response.json().await.unwrap_or_default();
        if !http.is_success() {
            let message = answer["error"]
                .as_str()
                .unwrap_or("Main said no")
                .to_owned();
            self.update(|s| {
                s.code = Some(code.clone());
                s.problem = Some(message);
            });
            // Asking again later (joining may be turned on meanwhile) is
            // only a fresh join if nothing was pinned yet.
            if http != reqwest::StatusCode::UPGRADE_REQUIRED {
                self.store.set_setting(NODE_MAIN_FP, "").await?;
            }
            return Ok(Some(CHECK_WAITING));
        }
        let joined = answer["status"].as_str() == Some("approved");
        if joined && status.pairing != Pairing::Joined {
            self.set_pairing(Pairing::Joined).await?;
            tracing::info!(main = %main, "joined Main");
        }
        self.update(|s| {
            s.code = Some(code);
            s.problem = None;
            if let Some(name) = answer["server_name"].as_str() {
                s.main_name = Some(name.to_owned());
            }
            if let Some(room) = answer["room_id"].as_i64() {
                s.room_id = Some(room);
            }
        });
        Ok(Some(if joined { CHECK_JOINED } else { CHECK_WAITING }))
    }
}

fn random_hex(bytes: usize) -> anyhow::Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no system randomness: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// The error's own words, without reqwest's long chain.
fn plain_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "it didn't answer in time".into()
    } else if e.is_connect() {
        "nothing answered at that address".into()
    } else {
        e.to_string()
    }
}

/// Main's base URL from what was typed: "192.168.1.10", "djinn.local:8066",
/// "http://..." all become "https://host:port" (nodes always encrypt).
pub fn main_url(typed: &str) -> Option<String> {
    let rest = typed.trim().trim_end_matches('/');
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    let host_port = rest.split('/').next()?;
    if host_port.is_empty() || host_port.contains(char::is_whitespace) {
        return None;
    }
    let has_port = match host_port.rsplit_once(':') {
        // An IPv6 address in brackets has colons of its own.
        Some((host, port)) => {
            !host.ends_with(':')
                && port.parse::<u16>().is_ok()
                && (!host.contains(':') || host.ends_with(']'))
        }
        None => false,
    };
    Some(if has_port {
        format!("https://{host_port}")
    } else {
        format!("https://{host_port}:{MAIN_PORT}")
    })
}

fn wipe(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let removed = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            if let Err(e) = removed {
                tracing::error!(path = %path.display(), error = %e, "couldn't wipe a cached file");
            }
        }
    }
}

/// Accepts Main's certificate if it's the pinned one, or (the first time,
/// nothing pinned) whatever it is, noting what it saw. The handshake's
/// signatures are still checked, so whoever answers must hold the
/// certificate's key.
#[derive(Debug)]
struct Pin {
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fingerprint: String = Sha256::digest(end_entity.as_ref())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some(fingerprint.clone());
        }
        match &self.expected {
            Some(expected) if *expected != fingerprint => {
                Err(rustls::Error::General("Main's certificate changed".into()))
            }
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn pinned_client(
    expected: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
) -> anyhow::Result<reqwest::Client> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pin {
            expected,
            seen,
            provider,
        }))
        .with_no_client_auth();
    Ok(reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("DJINN-node/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

// --- The node's page ---

pub fn router(node: Node) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/node") }))
        .route("/api/node/status", get(status))
        .route("/api/node/connect", post(connect))
        .route("/api/node/ask-again", post(ask_again))
        .fallback(crate::web::static_handler)
        .with_state(node)
}

async fn status(State(node): State<Node>) -> Json<Status> {
    Json(node.status())
}

#[derive(Deserialize)]
struct Connect {
    address: String,
    #[serde(default)]
    name: String,
}

async fn connect(State(node): State<Node>, Json(body): Json<Connect>) -> ApiResult<Json<Status>> {
    node.connect(&body.address, &body.name)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(node.status()))
}

async fn ask_again(State(node): State<Node>) -> ApiResult<Json<Status>> {
    node.ask_again()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(node.status()))
}

/// Runs this box as a node until asked to stop.
pub async fn serve(
    store: Store,
    db_path: &Path,
    listen: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let cache_dir = db_path
        .parent()
        .map_or_else(|| PathBuf::from("node-cache"), |dir| dir.join("node-cache"));
    std::fs::create_dir_all(&cache_dir)?;
    let node = Node::new(store.clone(), cache_dir).await?;
    tokio::spawn(node.clone().run());
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(%listen, "running as a node; its page is at /node");
    axum::serve(listener, router(node))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    store.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn addresses_become_encrypted_urls() {
        assert_eq!(
            main_url("192.168.1.10").as_deref(),
            Some("https://192.168.1.10:8066")
        );
        assert_eq!(
            main_url("http://djinn.local:9000/").as_deref(),
            Some("https://djinn.local:9000")
        );
        assert_eq!(
            main_url(" djinn.local ").as_deref(),
            Some("https://djinn.local:8066")
        );
        assert_eq!(
            main_url("https://host:8066/admin").as_deref(),
            Some("https://host:8066")
        );
        assert_eq!(
            main_url("[::1]:8066").as_deref(),
            Some("https://[::1]:8066")
        );
        assert_eq!(main_url("").as_deref(), None);
        assert_eq!(main_url("two words").as_deref(), None);
    }

    /// A real Main over its encrypted port and a real node pairing with it.
    #[tokio::test]
    async fn a_node_pairs_with_main_and_wipes_its_cache_when_kicked() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let main = AppState::for_tests().await;
        main.store
            .set_setting(crate::nodes::NODES_JOIN, "1")
            .await
            .unwrap();
        crate::go::apply_encryption(&main).await.unwrap();
        let main_fp = main.go.encryption.fingerprint().unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = tcp.local_addr().unwrap().port();
        let listener = crate::tls::listen(tcp, Arc::clone(&main.go.encryption)).unwrap();
        let app = crate::app(main.clone());
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<crate::tls::Peer>(),
            )
            .await
        });

        let dir = std::env::temp_dir().join(format!("djinn-node-test-{}", std::process::id()));
        let cache = dir.join("node-cache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("song.flac"), b"cached").unwrap();
        let node = Node::new(Store::open_in_memory().await.unwrap(), cache.clone())
            .await
            .unwrap();
        assert_eq!(node.status().pairing, Pairing::Setup);
        node.connect(&format!("127.0.0.1:{port}"), "Kitchen Pi")
            .await
            .unwrap();

        // Asks: Main pinned, request waiting with matching codes.
        node.check().await.unwrap();
        let status = node.status();
        assert_eq!(status.pairing, Pairing::Waiting, "{:?}", status.problem);
        assert_eq!(status.main_code, Some(crate::tls::security_code(&main_fp)));
        let waiting = main.store.nodes().await.unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            status.code,
            Some(crate::nodes::pairing_code(
                &waiting[0].node_id,
                &waiting[0].secret_hash
            ))
        );

        // Accepted on Main: the node sees it.
        let admin = main
            .store
            .create_user("boss", "h", djinn_store::Role::Admin)
            .await
            .unwrap();
        main.store
            .approve_node(waiting[0].id, admin.id)
            .await
            .unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Joined);
        assert!(
            node.connect("elsewhere", "x").await.is_err(),
            "can't be repointed once joined"
        );
        assert!(cache.join("song.flac").exists());

        // Kicked: cache gone.
        main.store.remove_node(waiting[0].id).await.unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Kicked);
        assert!(!cache.join("song.flac").exists(), "cache wiped");

        // Asking again is a new request.
        node.ask_again().await.unwrap();
        node.check().await.unwrap();
        assert_eq!(node.status().pairing, Pairing::Waiting);
        assert_eq!(main.store.nodes().await.unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_different_main_certificate_stops_the_node() {
        let node = Node::new(Store::open_in_memory().await.unwrap(), std::env::temp_dir())
            .await
            .unwrap();
        let seen = Arc::new(Mutex::new(None));
        let client = pinned_client(Some("00".repeat(32)), Arc::clone(&seen)).unwrap();
        // Nothing to connect to here; the pin itself is what's tested.
        drop(client);
        let pin = Pin {
            expected: Some("00".repeat(32)),
            seen,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        };
        let cert = CertificateDer::from(vec![1u8, 2, 3]);
        let name = ServerName::try_from("main").unwrap();
        assert!(pin
            .verify_server_cert(&cert, &[], &name, &[], UnixTime::now())
            .is_err());
        let open = Pin {
            expected: None,
            ..pin
        };
        assert!(open
            .verify_server_cert(&cert, &[], &name, &[], UnixTime::now())
            .is_ok());
        assert!(
            open.seen.lock().unwrap().is_some(),
            "the first look is noted"
        );
        assert_eq!(node.status().pairing, Pairing::Setup);
    }
}
