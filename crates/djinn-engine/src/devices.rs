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
    Ok(devices
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
        .collect())
}
