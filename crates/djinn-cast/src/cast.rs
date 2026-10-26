//! Playing the stream on a Google Cast device (Chromecast, Google TV, Nest
//! speakers and groups) with Google's Default Media Receiver.
//!
//! Cast devices use self-signed certificates, so the TLS connection checks
//! signatures but not the certificate chain; the LAN address is what
//! identifies the device, as with every other Cast sender.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio_rustls::TlsConnector;

use crate::castv2::{CastMessage, MAX_FRAME, NS_CONNECTION, NS_HEARTBEAT, NS_MEDIA, NS_RECEIVER};
use crate::{CastCommand, CastMedia, CastState, Metadata};

/// Google's Default Media Receiver.
const MEDIA_RECEIVER: &str = "CC1AD845";
/// DashCast, a public receiver that shows any web page full screen (the
/// one Home Assistant uses for dashboards on Chromecasts and Nest Hubs).
/// Its message channel has been renamed over the years, so it's read from
/// the app's status; it only shows up once the app is ready.
const PAGE_RECEIVER: &str = "84912283";
const SENDER: &str = "sender-djinn";
const RECEIVER: &str = "receiver-0";
const HEARTBEAT: Duration = Duration::from_secs(5);
/// No reply to pings for this long means the device is gone.
const SILENCE_LIMIT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Our app missing this long (after it had loaded) means it was closed,
/// not restarted.
const APP_GONE: Duration = Duration::from_secs(6);

type Tls = tokio_rustls::client::TlsStream<TcpStream>;

/// Casts `media` to the device at `addr` until told to stop. Returns when
/// stopped (`Ok`) or when the connection or receiver fails (`Err`, so the
/// caller can retry).
pub(crate) async fn play(
    addr: SocketAddr,
    media: &CastMedia,
    commands: &mut mpsc::Receiver<CastCommand>,
    state: &watch::Sender<CastState>,
) -> Result<(), String> {
    let app_id = match media {
        CastMedia::Stream { .. } => MEDIA_RECEIVER,
        CastMedia::Page { .. } => PAGE_RECEIVER,
    };
    let mut conn = Connection::open(addr).await?;
    conn.send(RECEIVER, NS_CONNECTION, json!({"type": "CONNECT"}))
        .await?;
    conn.request(
        RECEIVER,
        NS_RECEIVER,
        json!({"type": "LAUNCH", "appId": app_id}),
    )
    .await?;

    let mut session: Option<String> = None;
    // The app instance (transport) the media or page went to. A device may
    // restart the app on launch, so a new instance gets it again.
    let mut loaded_into: Option<String> = None;
    let mut ever_loaded = false;
    // Since when our app has been missing, after it had loaded.
    let mut missing_since: Option<tokio::time::Instant> = None;
    let mut heartbeat = tokio::time::interval(HEARTBEAT);
    let mut last_heard = tokio::time::Instant::now();
    loop {
        tokio::select! {
            message = conn.receive() => {
                let message = message?;
                last_heard = tokio::time::Instant::now();
                let payload: Value = serde_json::from_str(&message.payload).unwrap_or(Value::Null);
                let kind = payload["type"].as_str().unwrap_or_default();
                if kind != "PING" && kind != "PONG" {
                    tracing::debug!(from = %message.source, namespace = %message.namespace, payload = %message.payload, "cast message");
                }
                match (message.namespace.as_str(), kind) {
                    (NS_HEARTBEAT, "PING") => {
                        conn.send(&message.source, NS_HEARTBEAT, json!({"type": "PONG"})).await?;
                    }
                    (NS_RECEIVER, "RECEIVER_STATUS") => {
                        let status = &payload["status"];
                        if let Some(level) = status["volume"]["level"].as_f64() {
                            let fixed = status["volume"]["controlType"] == "fixed";
                            state.send_modify(|s| {
                                s.volume = Some(level as f32);
                                s.volume_fixed = fixed;
                            });
                        }
                        let app = status["applications"].as_array().and_then(|apps| {
                            apps.iter().find(|a| a["appId"] == app_id)
                        });
                        match app {
                            Some(app) => {
                                session = app["sessionId"].as_str().map(str::to_owned);
                                let id = app["transportId"].as_str().map(str::to_owned);
                                // DashCast takes the page address on its own
                                // channel, listed once it's ready.
                                let page_channel = app["namespaces"].as_array().and_then(|list| {
                                    list.iter()
                                        .filter_map(|n| n["name"].as_str())
                                        .find(|n| n.contains("dashcast"))
                                        .map(str::to_owned)
                                });
                                let ready = match media {
                                    CastMedia::Stream { .. } => true,
                                    CastMedia::Page { .. } => page_channel.is_some(),
                                };
                                missing_since = None;
                                if loaded_into != id && ready {
                                    if let Some(id) = id.clone() {
                                        conn.send(&id, NS_CONNECTION, json!({"type": "CONNECT"})).await?;
                                        match media {
                                            CastMedia::Stream { url, meta } => {
                                                conn.request(&id, NS_MEDIA, load_request(url, meta)).await?;
                                            }
                                            CastMedia::Page { url } => {
                                                let channel = page_channel.as_deref().unwrap_or_default();
                                                // "force" opens the page itself rather than in a
                                                // frame, so a plain-HTTP LAN page loads.
                                                conn.send(&id, channel, json!({
                                                    "url": url, "force": true, "reload": false, "reload_time": 0,
                                                })).await?;
                                                state.send_modify(|s| {
                                                    s.playing = true;
                                                    s.buffering = false;
                                                });
                                            }
                                        }
                                        loaded_into = Some(id);
                                        ever_loaded = true;
                                    }
                                }
                            }
                            // Our app is gone: someone cast something else, or
                            // it was stopped on the device. Or it's restarting,
                            // which takes a moment, so only give up after a
                            // while (the heartbeat keeps asking meanwhile).
                            None if ever_loaded => {
                                loaded_into = None;
                                let since = *missing_since.get_or_insert_with(tokio::time::Instant::now);
                                if since.elapsed() >= APP_GONE {
                                    state.send_modify(|s| s.error = Some("Stopped on the device".into()));
                                    return Ok(());
                                }
                            }
                            None => {}
                        }
                    }
                    (NS_RECEIVER, "LAUNCH_STATUS") => {
                        // Some devices (Nest Hubs) ask on screen before a new
                        // app may cast.
                        let status = payload["status"].as_str().unwrap_or_default();
                        if status.contains("PENDING") {
                            state.send_modify(|s| s.error = Some("Waiting for someone to allow it on the device".into()));
                        } else if status.contains("DENIED") || status.contains("REJECT") {
                            state.send_modify(|s| s.error = Some("Not allowed on the device".into()));
                            return Ok(());
                        }
                    }
                    (NS_RECEIVER, "LAUNCH_ERROR") => {
                        return Err(format!("the device refused to start playback ({})", payload["reason"].as_str().unwrap_or("unknown")));
                    }
                    (NS_MEDIA, "MEDIA_STATUS") => {
                        if let Some(entry) = payload["status"].as_array().and_then(|s| s.first()) {
                            let player = entry["playerState"].as_str().unwrap_or_default();
                            if player == "IDLE" && entry["idleReason"] == "ERROR" {
                                return Err("the device could not play the stream".into());
                            }
                            state.send_modify(|s| {
                                s.playing = player == "PLAYING";
                                s.buffering = player == "BUFFERING" || player == "LOADING";
                                s.error = None;
                            });
                        }
                    }
                    (NS_MEDIA, "LOAD_FAILED" | "LOAD_CANCELLED" | "INVALID_REQUEST") => {
                        return Err(format!("the device rejected the stream ({kind})"));
                    }
                    // The app instance we loaded into closed. It may be
                    // restarting; the next status says whether it's gone.
                    (NS_CONNECTION, "CLOSE") if Some(&message.source) == loaded_into.as_ref() => {
                        loaded_into = None;
                        conn.request(RECEIVER, NS_RECEIVER, json!({"type": "GET_STATUS"})).await?;
                    }
                    _ => {}
                }
            }
            _ = heartbeat.tick() => {
                if last_heard.elapsed() > SILENCE_LIMIT {
                    return Err("the device stopped answering".into());
                }
                conn.send(RECEIVER, NS_HEARTBEAT, json!({"type": "PING"})).await?;
                // Still waiting for the app to be ready: ask again.
                if loaded_into.is_none() {
                    conn.request(RECEIVER, NS_RECEIVER, json!({"type": "GET_STATUS"})).await?;
                }
            }
            command = commands.recv() => {
                match command {
                    Some(CastCommand::SetVolume(level)) => {
                        conn.request(RECEIVER, NS_RECEIVER, json!({
                            "type": "SET_VOLUME",
                            "volume": {"level": level.clamp(0.0, 1.0)},
                        })).await?;
                    }
                    Some(CastCommand::Stop) | None => {
                        if let Some(id) = session.take() {
                            let _ = conn.request(RECEIVER, NS_RECEIVER, json!({
                                "type": "STOP", "sessionId": id,
                            })).await;
                        }
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Asks a device for its status without changing anything: for checking
/// that it answers.
pub async fn probe(ip: IpAddr, port: u16) -> Result<Value, String> {
    let mut conn = Connection::open(SocketAddr::new(ip, port)).await?;
    conn.send(RECEIVER, NS_CONNECTION, json!({"type": "CONNECT"}))
        .await?;
    conn.request(RECEIVER, NS_RECEIVER, json!({"type": "GET_STATUS"}))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let message = tokio::time::timeout_at(deadline, conn.receive())
            .await
            .map_err(|_| "no status from the device".to_owned())??;
        let payload: Value = serde_json::from_str(&message.payload).unwrap_or(Value::Null);
        if payload["type"] == "RECEIVER_STATUS" {
            let _ = conn
                .send(RECEIVER, NS_CONNECTION, json!({"type": "CLOSE"}))
                .await;
            return Ok(payload);
        }
    }
}

fn load_request(url: &str, meta: &Metadata) -> Value {
    let images: Vec<Value> = meta
        .image_url
        .iter()
        .map(|url| json!({"url": url}))
        .collect();
    json!({
        "type": "LOAD",
        "autoplay": true,
        "media": {
            "contentId": url,
            "contentUrl": url,
            "streamType": "LIVE",
            "contentType": crate::stream::CONTENT_TYPE,
            "metadata": {
                // MusicTrackMediaMetadata
                "metadataType": 3,
                "title": meta.title,
                "artist": meta.subtitle,
                "images": images,
            },
        },
    })
}

struct Connection {
    tls: Tls,
    request_id: u64,
}

impl Connection {
    async fn open(addr: SocketAddr) -> Result<Self, String> {
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| "the device didn't answer".to_owned())?
            .map_err(|e| format!("cannot reach the device: {e}"))?;
        let _ = tcp.set_nodelay(true);
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SelfSigned { provider }))
            .with_no_client_auth();
        let tls = tokio::time::timeout(
            CONNECT_TIMEOUT,
            TlsConnector::from(Arc::new(config))
                .connect(ServerName::IpAddress(addr.ip().into()), tcp),
        )
        .await
        .map_err(|_| "the device didn't finish connecting".to_owned())?
        .map_err(|e| format!("secure connection failed: {e}"))?;
        Ok(Self { tls, request_id: 0 })
    }

    /// Sends a request, numbering it.
    async fn request(
        &mut self,
        destination: &str,
        namespace: &str,
        mut payload: Value,
    ) -> Result<(), String> {
        self.request_id += 1;
        payload["requestId"] = json!(self.request_id);
        self.send(destination, namespace, payload).await
    }

    async fn send(
        &mut self,
        destination: &str,
        namespace: &str,
        payload: Value,
    ) -> Result<(), String> {
        let frame = CastMessage::new(SENDER, destination, namespace, payload.to_string()).encode();
        self.tls
            .write_all(&frame)
            .await
            .map_err(|e| format!("connection lost: {e}"))
    }

    async fn receive(&mut self) -> Result<CastMessage, String> {
        let mut len = [0u8; 4];
        self.tls
            .read_exact(&mut len)
            .await
            .map_err(|e| format!("connection lost: {e}"))?;
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_FRAME {
            return Err("the device sent an oversized message".into());
        }
        let mut body = vec![0u8; len];
        self.tls
            .read_exact(&mut body)
            .await
            .map_err(|e| format!("connection lost: {e}"))?;
        CastMessage::decode(&body).ok_or_else(|| "the device sent a garbled message".into())
    }
}

/// Accepts the device's self-signed certificate but still checks that the
/// handshake was signed with it.
#[derive(Debug)]
struct SelfSigned {
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for SelfSigned {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
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
