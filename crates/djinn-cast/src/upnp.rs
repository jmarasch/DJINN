//! Playing the stream on DLNA / UPnP media renderers and Sonos: the
//! device description says where its AVTransport (play, stop) and
//! RenderingControl (volume) services live, and those take SOAP calls.

use std::net::IpAddr;
use std::time::Duration;

use reqwest::Url;
use tokio::sync::{mpsc, watch};

use crate::discover::{Device, DeviceKind};
use crate::{CastCommand, CastState, Metadata};

const AV_TRANSPORT: &str = "urn:schemas-upnp-org:service:AVTransport:";
const RENDERING_CONTROL: &str = "urn:schemas-upnp-org:service:RenderingControl:";
const POLL: Duration = Duration::from_secs(5);
/// Stopped this many polls in a row (after playing) means someone stopped
/// it on the device or switched to something else.
const STOPPED_POLLS: u32 = 2;
/// Far more than a real device description takes.
const MAX_DESCRIPTION: usize = 1024 * 1024;

/// Where to send control calls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpnpControl {
    pub av_transport: Service,
    pub rendering: Option<Service>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    /// Full service type, with its version, as the device lists it.
    pub kind: String,
    pub control_url: String,
}

pub(crate) fn client() -> reqwest::Client {
    // reqwest is built without a TLS provider of its own; LAN devices
    // speak plain HTTP, but the client still needs one installed.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default()
}

/// A device on the local network, on the address it answered from, so that
/// anyone on the network answering a search can't send DJINN somewhere else.
pub(crate) fn describes_itself(location: &str, responder: IpAddr) -> bool {
    let local = match responder.to_canonical() {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.is_private() || ip.is_link_local() || (a == 100 && (64..128).contains(&b))
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    };
    local
        && Url::parse(location).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url
                    .host_str()
                    .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
                    .and_then(|host| host.parse::<IpAddr>().ok())
                    .is_some_and(|host| host.to_canonical() == responder.to_canonical())
        })
}

/// Fetches and reads a device description. `None` if it can't play media.
pub(crate) async fn describe(
    client: &reqwest::Client,
    location: &str,
) -> Result<Option<Device>, String> {
    let mut response = client
        .get(location)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_DESCRIPTION {
            return Err("the description is too big".to_owned());
        }
    }
    let xml = String::from_utf8_lossy(&bytes);
    let addr = Url::parse(location)
        .ok()
        .and_then(|u| u.socket_addrs(|| Some(80)).ok())
        .and_then(|a| a.into_iter().next())
        .ok_or_else(|| "bad description URL".to_owned())?;
    Ok(
        parse_description(&xml, location).map(|(id, name, kind, model, control)| Device {
            id,
            name,
            kind,
            model,
            group: false,
            screen: false,
            addr,
            upnp: Some(control),
        }),
    )
}

/// Whether `url` is on the same host as the address the description came from.
fn same_device(url: &Url, location: &str) -> bool {
    Url::parse(location).is_ok_and(|from| url.host_str() == from.host_str())
}

type Description = (String, String, DeviceKind, Option<String>, UpnpControl);

fn parse_description(xml: &str, location: &str) -> Option<Description> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let child_text = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|c| c.tag_name().name() == name)
            .and_then(|c| c.text())
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    let root = doc.root_element();
    let base = child_text(root, "URLBase")
        .and_then(|b| Url::parse(&b).ok())
        .or_else(|| Url::parse(location).ok())?;
    let top = root.children().find(|c| c.tag_name().name() == "device")?;
    // Sonos nests the renderer inside its ZonePlayer device; search them all.
    let devices: Vec<_> = top
        .descendants()
        .filter(|d| d.tag_name().name() == "device")
        .collect();
    let services = |device: roxmltree::Node<'_, '_>| -> Vec<Service> {
        device
            .children()
            .filter(|c| c.tag_name().name() == "serviceList")
            .flat_map(|list| list.children().filter(|s| s.tag_name().name() == "service"))
            .filter_map(|s| {
                let kind = child_text(s, "serviceType")?;
                let control = base.join(&child_text(s, "controlURL")?).ok()?;
                // Control stays on the device that described itself.
                if !same_device(&control, location) {
                    return None;
                }
                Some(Service {
                    kind,
                    control_url: control.to_string(),
                })
            })
            .collect()
    };
    let (renderer, list) = devices.iter().find_map(|&d| {
        let list = services(d);
        list.iter()
            .any(|s| s.kind.starts_with(AV_TRANSPORT))
            .then_some((d, list))
    })?;
    let av_transport = list
        .iter()
        .find(|s| s.kind.starts_with(AV_TRANSPORT))?
        .clone();
    let rendering = list
        .iter()
        .find(|s| s.kind.starts_with(RENDERING_CONTROL))
        .cloned();
    let manufacturer = child_text(top, "manufacturer").unwrap_or_default();
    let sonos = manufacturer.to_lowercase().starts_with("sonos");
    let udn = child_text(top, "UDN").or_else(|| child_text(renderer, "UDN"))?;
    let name = child_text(top, "roomName")
        .or_else(|| child_text(top, "friendlyName"))
        .or_else(|| child_text(renderer, "friendlyName"))?;
    let model = child_text(top, "modelName");
    let kind = if sonos {
        DeviceKind::Sonos
    } else {
        DeviceKind::Dlna
    };
    Some((
        format!("upnp:{}", udn.trim_start_matches("uuid:")),
        name,
        kind,
        model,
        UpnpControl {
            av_transport,
            rendering,
        },
    ))
}

/// Plays `url` until stopped (`Ok`) or the device fails (`Err`).
pub(crate) async fn play(
    device: &Device,
    url: &str,
    meta: &Metadata,
    commands: &mut mpsc::Receiver<CastCommand>,
    state: &watch::Sender<CastState>,
) -> Result<(), String> {
    let control = device
        .upnp
        .as_ref()
        .ok_or_else(|| "not a UPnP device".to_owned())?;
    let client = client();
    let sonos = device.kind == DeviceKind::Sonos;
    let uri = if sonos {
        // Sonos plays endless MP3 as internet radio with this scheme.
        url.replacen("http://", "x-rincon-mp3radio://", 1)
    } else {
        url.to_owned()
    };
    let av = &control.av_transport;
    soap(
        &client,
        av,
        "SetAVTransportURI",
        &format!(
            "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData>",
            escape(&uri),
            escape(&didl(url, meta, sonos))
        ),
    )
    .await?;
    soap(
        &client,
        av,
        "Play",
        "<InstanceID>0</InstanceID><Speed>1</Speed>",
    )
    .await?;
    if let Some(rendering) = &control.rendering {
        if let Ok(reply) = soap(
            &client,
            rendering,
            "GetVolume",
            "<InstanceID>0</InstanceID><Channel>Master</Channel>",
        )
        .await
        {
            if let Some(volume) = tag(&reply, "CurrentVolume").and_then(|v| v.parse::<f32>().ok()) {
                state.send_modify(|s| s.volume = Some(volume / 100.0));
            }
        }
    }

    let mut poll = tokio::time::interval(POLL);
    let mut played = false;
    let mut stopped_polls = 0;
    loop {
        tokio::select! {
            _ = poll.tick() => {
                let reply = soap(&client, av, "GetTransportInfo", "<InstanceID>0</InstanceID>").await?;
                let transport = tag(&reply, "CurrentTransportState").unwrap_or_default();
                let playing = transport == "PLAYING";
                played |= playing;
                stopped_polls = if played && matches!(transport.as_str(), "STOPPED" | "NO_MEDIA_PRESENT") {
                    stopped_polls + 1
                } else {
                    0
                };
                if stopped_polls >= STOPPED_POLLS {
                    state.send_modify(|s| s.error = Some("Stopped on the device".into()));
                    return Ok(());
                }
                state.send_modify(|s| {
                    s.playing = playing;
                    s.buffering = transport == "TRANSITIONING";
                    s.error = None;
                });
            }
            command = commands.recv() => match command {
                Some(CastCommand::SetVolume(level)) => {
                    if let Some(rendering) = &control.rendering {
                        let volume = (level.clamp(0.0, 1.0) * 100.0).round() as u32;
                        soap(&client, rendering, "SetVolume", &format!(
                            "<InstanceID>0</InstanceID><Channel>Master</Channel><DesiredVolume>{volume}</DesiredVolume>"
                        )).await?;
                        state.send_modify(|s| s.volume = Some(level));
                    }
                }
                Some(CastCommand::Stop) | None => {
                    let _ = soap(&client, av, "Stop", "<InstanceID>0</InstanceID>").await;
                    return Ok(());
                }
            }
        }
    }
}

/// Calls `action` on `service` and returns the response body.
async fn soap(
    client: &reqwest::Client,
    service: &Service,
    action: &str,
    args: &str,
) -> Result<String, String> {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{kind}\">{args}</u:{action}></s:Body></s:Envelope>",
        kind = service.kind
    );
    let response = client
        .post(&service.control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPACTION", format!("\"{}#{action}\"", service.kind))
        .body(body)
        .send()
        .await
        .map_err(|e| format!("cannot reach the device: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let reason = tag(&text, "errorDescription").unwrap_or_else(|| status.to_string());
        return Err(format!("the device refused {action}: {reason}"));
    }
    Ok(text)
}

/// DIDL-Lite metadata describing the stream as a radio broadcast.
fn didl(url: &str, meta: &Metadata, sonos: bool) -> String {
    let art = meta
        .image_url
        .as_deref()
        .map(|u| format!("<upnp:albumArtURI>{}</upnp:albumArtURI>", escape(u)))
        .unwrap_or_default();
    // Sonos wants to be told this is a radio stream from "anywhere".
    let desc = if sonos {
        "<desc id=\"cdudn\" nameSpace=\"urn:schemas-rinconnetworks-com:metadata-1-0/\">SA_RINCON65031_</desc>"
    } else {
        ""
    };
    format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">\
         <item id=\"djinn-live\" parentID=\"0\" restricted=\"1\">\
         <dc:title>{title}</dc:title><upnp:artist>{artist}</upnp:artist>{art}\
         <upnp:class>object.item.audioItem.audioBroadcast</upnp:class>\
         <res protocolInfo=\"http-get:*:audio/mpeg:DLNA.ORG_PN=MP3;DLNA.ORG_OP=00;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000\">{url}</res>\
         {desc}</item></DIDL-Lite>",
        title = escape(&meta.title),
        artist = escape(&meta.subtitle),
        url = escape(url),
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The text of the first `<name>` element, any namespace.
fn tag(xml: &str, name: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    doc.descendants()
        .find(|n| n.tag_name().name() == name)
        .and_then(|n| n.text())
        .map(|t| t.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_devices_describing_themselves_nearby_count() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let sonos = "http://192.168.0.40:1400/xml/device_description.xml";
        assert!(describes_itself(sonos, ip("192.168.0.40")));
        assert!(
            !describes_itself(sonos, ip("192.168.0.41")),
            "someone else's"
        );
        assert!(!describes_itself(
            "http://203.0.113.5/x.xml",
            ip("203.0.113.5")
        ));
        assert!(!describes_itself(
            "http://127.0.0.1:8066/api/x",
            ip("192.168.0.40")
        ));
        assert!(!describes_itself(
            "http://169.254.169.254/x",
            ip("192.168.0.40")
        ));
        assert!(!describes_itself(
            "http://printer.lan/x.xml",
            ip("192.168.0.40")
        ));
        assert!(!describes_itself("file:///etc/passwd", ip("192.168.0.40")));
        assert!(describes_itself("http://[fe80::1]:80/x.xml", ip("fe80::1")));
    }

    #[test]
    fn control_stays_on_the_device() {
        let location = "http://192.168.0.40:1400/desc.xml";
        let on = |u: &str| same_device(&Url::parse(u).unwrap(), location);
        assert!(on("http://192.168.0.40:1400/control"));
        assert!(!on("http://192.168.0.1/control"));
        assert!(!on("http://127.0.0.1:8066/api/system/quit"));
    }

    const TV: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Living Room TV</friendlyName>
    <manufacturer>Example Electronics</manufacturer>
    <modelName>X900</modelName>
    <UDN>uuid:1234-abcd</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
        <controlURL>/upnp/control/RenderingControl1</controlURL>
      </service>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
        <controlURL>upnp/control/AVTransport1</controlURL>
      </service>
    </serviceList>
  </device>
</root>"#;

    const SONOS: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>192.168.0.40 - Sonos One - RINCON_000000</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>Sonos One</modelName>
    <roomName>Living Room</roomName>
    <UDN>uuid:RINCON_000000</UDN>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AlarmClock:1</serviceType>
        <controlURL>/AlarmClock/Control</controlURL>
      </service>
    </serviceList>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <friendlyName>Living Room - Sonos One Media Renderer</friendlyName>
        <UDN>uuid:RINCON_000000_MR</UDN>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
            <controlURL>/MediaRenderer/RenderingControl/Control</controlURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <controlURL>/MediaRenderer/AVTransport/Control</controlURL>
          </service>
        </serviceList>
      </device>
    </deviceList>
  </device>
</root>"#;

    #[test]
    fn reads_a_dlna_tv() {
        let (id, name, kind, model, control) =
            parse_description(TV, "http://192.168.0.30:9197/dmr").unwrap();
        assert_eq!(
            (id.as_str(), name.as_str()),
            ("upnp:1234-abcd", "Living Room TV")
        );
        assert_eq!((kind, model.as_deref()), (DeviceKind::Dlna, Some("X900")));
        assert_eq!(
            control.av_transport.control_url,
            "http://192.168.0.30:9197/upnp/control/AVTransport1"
        );
        assert_eq!(
            control.rendering.unwrap().control_url,
            "http://192.168.0.30:9197/upnp/control/RenderingControl1"
        );
    }

    #[test]
    fn reads_a_sonos_speaker() {
        let (id, name, kind, _, control) =
            parse_description(SONOS, "http://192.168.0.40:1400/xml/device_description.xml")
                .unwrap();
        assert_eq!(
            (id.as_str(), name.as_str(), kind),
            ("upnp:RINCON_000000", "Living Room", DeviceKind::Sonos)
        );
        assert_eq!(
            control.av_transport.control_url,
            "http://192.168.0.40:1400/MediaRenderer/AVTransport/Control"
        );
    }

    #[test]
    fn devices_without_av_transport_are_skipped() {
        let server = TV.replace("AVTransport", "ContentDirectory");
        assert!(parse_description(&server, "http://192.168.0.58/").is_none());
    }

    #[test]
    fn metadata_is_escaped() {
        let meta = Metadata {
            title: "Rock & Roll <live>".into(),
            subtitle: "DJINN".into(),
            image_url: None,
        };
        let didl = didl("http://h/cast/t/live.mp3", &meta, true);
        assert!(didl.contains("Rock &amp; Roll &lt;live&gt;"));
        assert!(didl.contains("SA_RINCON65031_"));
        assert_eq!(
            tag(
                "<a><b:CurrentVolume xmlns:b=\"x\"> 35 </b:CurrentVolume></a>",
                "CurrentVolume"
            )
            .as_deref(),
            Some("35")
        );
    }
}
