//! Listing output devices.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::EngineError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Stable id to store and pass back in
    /// [`OutputDevice::Id`](crate::OutputDevice::Id).
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// The output devices connected right now. On Linux this lists PulseAudio
/// (or PipeWire) sinks when that server runs, which includes Bluetooth
/// speakers.
pub fn output_devices() -> Result<Vec<DeviceInfo>, EngineError> {
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.id().ok());
    let devices = host
        .output_devices()
        .map_err(|e| EngineError::Output(e.to_string()))?;
    let mut found: Vec<DeviceInfo> = devices
        .filter_map(|device| {
            let id = device.id().ok()?;
            let name = device
                .description()
                .map(|d| d.name().to_owned())
                .unwrap_or_else(|_| id.id().to_owned());
            Some(DeviceInfo {
                is_default: default.as_ref() == Some(&id),
                id: id.to_string(),
                name,
            })
        })
        .collect();
    found.retain(|d| !alsa_noise(&d.id));
    number_twins(&mut found);
    Ok(found)
}

/// ALSA lists every way to reach a card: the raw hardware, the same output
/// through plug converters, mixers, surround layouts and a "discard
/// everything" device, all under one name. Without a sound server a
/// computer shows dozens of them, so the plumbing is left out: what stays is
/// the default, each card's own entry, each HDMI port, and anything not
/// known as plumbing. (A device chosen earlier still opens by its id.)
fn alsa_noise(id: &str) -> bool {
    let Some(pcm) = id.strip_prefix("alsa:") else {
        return false;
    };
    let kind = pcm.split([':', ',']).next().unwrap_or(pcm);
    matches!(
        kind,
        "null"
            | "hw"
            | "plughw"
            | "front"
            | "dmix"
            | "dsnoop"
            | "iec958"
            | "usbstream"
            | "samplerate"
            | "speexrate"
            | "lavrate"
            | "upmix"
            | "vdownmix"
    ) || kind.starts_with("surround")
}

/// Gives devices that share a name (two identical monitors on one graphics
/// card) a number: "Monitor (1)", "Monitor (2)". Numbered in id
/// order, so each keeps its number from one listing to the next.
fn number_twins(devices: &mut [DeviceInfo]) {
    let ids: Vec<(String, String)> = devices
        .iter()
        .map(|d| (d.name.clone(), d.id.clone()))
        .collect();
    for device in devices.iter_mut() {
        let mut same: Vec<&str> = ids
            .iter()
            .filter(|(name, _)| *name == device.name)
            .map(|(_, id)| id.as_str())
            .collect();
        if same.len() < 2 {
            continue;
        }
        same.sort_unstable();
        if let Some(place) = same.iter().position(|id| *id == device.id) {
            device.name = format!("{} ({})", device.name, place + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.to_owned(),
            name: name.to_owned(),
            is_default: false,
        }
    }

    #[test]
    fn alsa_plumbing_is_left_out() {
        for plumbing in [
            "alsa:null",
            "alsa:hw:CARD=PCH,DEV=3",
            "alsa:plughw:CARD=PCH,DEV=0",
            "alsa:front:CARD=PCH,DEV=0",
            "alsa:surround51:CARD=PCH,DEV=0",
            "alsa:dmix:CARD=PCH,DEV=0",
            "alsa:iec958:CARD=PCH,DEV=0",
        ] {
            assert!(alsa_noise(plumbing), "{plumbing}");
        }
        for wanted in [
            "alsa:default",
            "alsa:sysdefault:CARD=PCH",
            "alsa:hdmi:CARD=PCH,DEV=3",
            "alsa:pulse",
            "wasapi:{e0dd}",
        ] {
            assert!(!alsa_noise(wanted), "{wanted}");
        }
    }

    #[test]
    fn identical_names_get_numbers_in_id_order() {
        let mut devices = vec![
            device("wasapi:{e0dd}", "Monitor"),
            device("wasapi:{22d1}", "TV"),
            device("wasapi:{987f}", "Monitor"),
        ];
        number_twins(&mut devices);
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Monitor (2)", "TV", "Monitor (1)"]);
    }
}
