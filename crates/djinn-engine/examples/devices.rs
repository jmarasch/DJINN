//! Lists output devices: `cargo run -p djinn-engine --example devices`.

use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    println!("host: {:?}", host.id());
    let default = host.default_output_device().and_then(|d| d.id().ok());
    println!("default: {default:?}");
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            let name = d
                .description()
                .map(|d| d.name().to_owned())
                .unwrap_or_default();
            let id = d.id().map(|i| i.to_string()).unwrap_or_default();
            let cfg = d.default_output_config().ok();
            println!("{id}\n    {name} {cfg:?}");
        }
    }
}
