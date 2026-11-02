//! Optional UPnP-IGD mapping for an HTTPS reverse proxy on this computer.
//! The GO HTTP port is never mapped. The router receives a one-hour lease
//! for TCP 443 and DJINN renews it while the option stays on.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use reqwest::{Client, Url};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::watch;

const LEASE_SECONDS: u32 = 3600;
const RENEW_AFTER: Duration = Duration::from_secs(30 * 60);
const RETRY_AFTER: Duration = Duration::from_secs(60);

pub struct Mapping {
    pub hostname: String,
    stop: watch::Sender<bool>,
    pub error: Arc<Mutex<Option<String>>>,
    pub external_ip: Arc<Mutex<Option<Ipv4Addr>>>,
    pub verified: Arc<AtomicBool>,
}

impl Mapping {
    pub fn stop(self) {
        self.stop.send_replace(true);
    }
}

pub fn start(hostname: String, mut shutdown: watch::Receiver<bool>) -> Mapping {
    let (stop, mut stopping) = watch::channel(false);
    let error = Arc::new(Mutex::new(None));
    let external_ip = Arc::new(Mutex::new(None));
    let verified = Arc::new(AtomicBool::new(false));
    let task_error = Arc::clone(&error);
    let task_ip = Arc::clone(&external_ip);
    let task_verified = Arc::clone(&verified);
    let name = hostname.clone();
    tokio::spawn(async move {
        let mut gateway: Option<Gateway> = None;
        let mut mapped = false;
        loop {
            if gateway.is_none() {
                match discover().await {
                    Ok(found) => gateway = Some(found),
                    Err(e) => report(&task_error, format!("router mapping unavailable: {e:#}")),
                }
            }
            let mut delay = RETRY_AFTER;
            if let Some(router) = gateway.as_ref() {
                match router.map(&name).await {
                    Ok(ip) => {
                        mapped = true;
                        if let Ok(mut slot) = task_ip.lock() {
                            *slot = Some(ip);
                        }
                        // A fresh Caddy certificate can take a little while
                        // after the port first opens. Revoke the lease if a
                        // valid public certificate never appears.
                        let verified = tokio::select! {
                            result = verify_https(&name, router.local_ip) => Some(result),
                            _ = stopping.wait_for(|v| *v) => None,
                            _ = shutdown.wait_for(|v| *v) => None,
                        };
                        let Some(verified) = verified else { break };
                        match verified {
                            Ok(()) => {
                                task_verified.store(true, Ordering::Relaxed);
                                if let Ok(mut slot) = task_error.lock() {
                                    *slot = None;
                                }
                                tracing::info!(hostname = %name, external_ip = %ip, "GO HTTPS router mapping verified");
                                delay = RENEW_AFTER;
                            }
                            Err(e) => {
                                task_verified.store(false, Ordering::Relaxed);
                                report(&task_error, format!("HTTPS verification failed: {e:#}"));
                                let _ = router.unmap().await;
                                mapped = false;
                                if let Ok(mut slot) = task_ip.lock() {
                                    *slot = None;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        task_verified.store(false, Ordering::Relaxed);
                        report(&task_error, format!("router mapping failed: {e:#}"));
                        gateway = None;
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = stopping.wait_for(|v| *v) => break,
                _ = shutdown.wait_for(|v| *v) => break,
            }
        }
        if mapped {
            if let Some(router) = gateway {
                if let Err(e) = router.unmap().await {
                    tracing::warn!(error = %e, "couldn't remove GO HTTPS router mapping; its lease will expire");
                }
            }
        }
    });
    Mapping {
        hostname,
        stop,
        error,
        external_ip,
        verified,
    }
}

fn report(error: &Mutex<Option<String>>, message: String) {
    tracing::warn!(%message, "GO UPnP");
    if let Ok(mut slot) = error.lock() {
        *slot = Some(message);
    }
}

pub fn valid_hostname(host: &str) -> bool {
    let host = host.trim();
    host.len() <= 253
        && host.contains('.')
        && host.parse::<IpAddr>().is_err()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
}

struct Gateway {
    client: Client,
    control: Url,
    service: String,
    local_ip: Ipv4Addr,
}

async fn discover() -> anyhow::Result<Gateway> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let query = b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
    socket.send_to(query, "239.255.255.250:1900").await?;
    let mut buffer = [0u8; 4096];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        let received = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await;
        let Ok(Ok((size, from))) = received else {
            bail!("no UPnP-IGD router answered on the local network");
        };
        if !crate::security::is_home(from.ip()) {
            continue;
        }
        let response = String::from_utf8_lossy(&buffer[..size]);
        let location = response.lines().find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim().eq_ignore_ascii_case("location"))
                .map(|(_, value)| value.trim())
        });
        let Some(location) = location else { continue };
        if let Ok(gateway) = Gateway::from_location(location, from.ip()).await {
            return Ok(gateway);
        }
    }
}

impl Gateway {
    async fn from_location(location: &str, responder: IpAddr) -> anyhow::Result<Self> {
        let url = Url::parse(location)?;
        if url.scheme() != "http"
            || !url
                .host_str()
                .and_then(|s| s.parse::<IpAddr>().ok())
                .is_some_and(|ip| ip == responder && crate::security::is_home(ip))
        {
            bail!("router description is not on the local network");
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let description = client
            .get(url.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        if description.len() > 256_000 {
            bail!("router description is too large");
        }
        let service = service_from_description(&description)?;
        let control = url.join(&service.1)?;
        if control.host_str() != url.host_str()
            || control.port_or_known_default() != url.port_or_known_default()
            || control.scheme() != "http"
        {
            bail!("router control URL points elsewhere");
        }
        let router_ip = url
            .host_str()
            .ok_or_else(|| anyhow!("router address missing"))?;
        let router_ip: Ipv4Addr = router_ip.parse()?;
        let local = UdpSocket::bind("0.0.0.0:0").await?;
        local.connect(SocketAddr::from((router_ip, 1900))).await?;
        let IpAddr::V4(local_ip) = local.local_addr()?.ip() else {
            bail!("router is not reachable over IPv4");
        };
        Ok(Self {
            client,
            control,
            service: service.0,
            local_ip,
        })
    }

    async fn soap(&self, action: &str, arguments: &str) -> anyhow::Result<String> {
        let body = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service}\">{arguments}</u:{action}></s:Body></s:Envelope>",
            service = self.service
        );
        let result = self
            .client
            .post(self.control.clone())
            .header("Content-Type", "text/xml; charset=\"utf-8\"")
            .header("SOAPAction", format!("\"{}#{action}\"", self.service))
            .body(body)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(result)
    }

    async fn map(&self, hostname: &str) -> anyhow::Result<Ipv4Addr> {
        let answer = self.soap("GetExternalIPAddress", "").await?;
        let doc = roxmltree::Document::parse(&answer)?;
        let address = doc
            .descendants()
            .find(|n| n.has_tag_name("NewExternalIPAddress"))
            .and_then(|n| n.text())
            .context("router did not report its external address")?;
        let public_ip: Ipv4Addr = address
            .parse()
            .context("router reported an invalid external address")?;
        if !public_ipv4(public_ip) {
            bail!("router WAN address {public_ip} is not public (carrier-grade NAT or upstream router)");
        }
        let records = tokio::net::lookup_host((hostname, 443))
            .await
            .with_context(|| format!("couldn't resolve {hostname}"))?;
        if !records.into_iter().any(|address| {
            address.ip() == IpAddr::V4(public_ip) || address.ip() == IpAddr::V4(self.local_ip)
        }) {
            bail!("{hostname} does not point to router WAN address {public_ip}");
        }
        tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect(SocketAddr::from((self.local_ip, 443))),
        )
        .await
        .context("HTTPS proxy did not answer on TCP 443")?
        .context("start the HTTPS proxy on this computer before using UPnP")?;
        let args = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>443</NewExternalPort><NewProtocol>TCP</NewProtocol><NewInternalPort>443</NewInternalPort><NewInternalClient>{}</NewInternalClient><NewEnabled>1</NewEnabled><NewPortMappingDescription>DJINN GO HTTPS</NewPortMappingDescription><NewLeaseDuration>{LEASE_SECONDS}</NewLeaseDuration>",
            self.local_ip
        );
        self.soap("AddPortMapping", &args).await?;
        Ok(public_ip)
    }

    async fn unmap(&self) -> anyhow::Result<()> {
        let args = "<NewRemoteHost></NewRemoteHost><NewExternalPort>443</NewExternalPort><NewProtocol>TCP</NewProtocol>";
        let answer = self.soap("GetSpecificPortMappingEntry", args).await?;
        let doc = roxmltree::Document::parse(&answer)?;
        let owner = doc
            .descendants()
            .find(|n| n.has_tag_name("NewInternalClient"))
            .and_then(|n| n.text())
            .context("router didn't report the mapping owner")?;
        let port = doc
            .descendants()
            .find(|n| n.has_tag_name("NewInternalPort"))
            .and_then(|n| n.text())
            .context("router didn't report the mapping port")?;
        if owner != self.local_ip.to_string() || port != "443" {
            bail!("router mapping now belongs to another service");
        }
        self.soap("DeletePortMapping", args).await?;
        Ok(())
    }
}

fn service_from_description(description: &str) -> anyhow::Result<(String, String)> {
    let doc = roxmltree::Document::parse(description)?;
    doc.descendants()
        .filter(|n| n.has_tag_name("service"))
        .find_map(|node| {
            let kind = node
                .children()
                .find(|c| c.has_tag_name("serviceType"))?
                .text()?;
            if !kind.contains("WANIPConnection") && !kind.contains("WANPPPConnection") {
                return None;
            }
            let path = node
                .children()
                .find(|c| c.has_tag_name("controlURL"))?
                .text()?;
            Some((kind.to_owned(), path.to_owned()))
        })
        .ok_or_else(|| anyhow!("router has no WAN port-mapping service"))
}

async fn verify_https(hostname: &str, local_ip: Ipv4Addr) -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = Client::builder()
        .resolve(hostname, SocketAddr::from((local_ip, 443)))
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let url = format!("https://{hostname}/api/auth/mode");
    let mut last = None;
    for _ in 0..18 {
        match client.get(&url).send().await {
            Ok(reply) if reply.status().is_success() => return Ok(()),
            Ok(reply) => last = Some(format!("proxy returned {}", reply.status())),
            Err(e) => last = Some(e.to_string()),
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    bail!(
        "{}",
        last.unwrap_or_else(|| "HTTPS proxy did not answer".to_owned())
    )
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_unspecified()
        && a < 224
        && a != 0
        && !(a == 100 && (64..128).contains(&b))
        && !(a == 198 && (b == 18 || b == 19))
        && !(a == 192 && b == 0 && c == 2)
        && !(a == 198 && b == 51 && c == 100)
        && !(a == 203 && b == 0 && c == 113)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_wan_mapping_service_in_router_description() {
        let xml = r#"<root xmlns="urn:schemas-upnp-org:device-1-0"><device><serviceList>
            <service><serviceType>urn:schemas-upnp-org:service:WANCommonInterfaceConfig:1</serviceType><controlURL>/common</controlURL></service>
            <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType><controlURL>/upnp/control/WANIPConn1</controlURL></service>
            </serviceList></device></root>"#;
        assert_eq!(
            service_from_description(xml).unwrap(),
            (
                "urn:schemas-upnp-org:service:WANIPConnection:1".to_owned(),
                "/upnp/control/WANIPConn1".to_owned()
            )
        );
        assert!(service_from_description("<root/>").is_err());
    }

    #[test]
    fn hostname_and_wan_address_checks() {
        assert!(valid_hostname("music.example.com"));
        for bad in [
            "localhost",
            "127.0.0.1",
            "bad/name.com",
            "-bad.example",
            "a..b",
        ] {
            assert!(!valid_hostname(bad));
        }
        assert!(public_ipv4("8.8.8.8".parse().unwrap()));
        assert!(!public_ipv4("100.70.1.1".parse().unwrap()));
        assert!(!public_ipv4("192.168.1.1".parse().unwrap()));
    }
}
