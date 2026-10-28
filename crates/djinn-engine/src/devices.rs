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
    number_twins(&mut found);
    Ok(found)
}

/// Gives devices that share a name (two identical monitors on one graphics
/// card) a number: "Sceptre F27 (1)", "Sceptre F27 (2)". Numbered in id
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
    fn identical_names_get_numbers_in_id_order() {
        let mut devices = vec![
            device("wasapi:{e0dd}", "Sceptre F27"),
            device("wasapi:{22d1}", "SONY TV"),
            device("wasapi:{987f}", "Sceptre F27"),
        ];
        number_twins(&mut devices);
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Sceptre F27 (2)", "SONY TV", "Sceptre F27 (1)"]);
    }
}
