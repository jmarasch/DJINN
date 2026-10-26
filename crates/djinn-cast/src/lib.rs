//! Casting: play the party on network devices (Chromecast, DLNA renderers,
//! Sonos) by streaming the master mix as live MP3.

pub mod cast;
mod caster;
mod castv2;
pub mod discover;
pub mod stream;
mod upnp;

pub use caster::{CastStatus, Caster};
pub use discover::{Device, DeviceKind};

#[derive(Debug, thiserror::Error)]
pub enum CastError {
    #[error("could not start the stream encoder: {0}")]
    Encoder(String),
    #[error("that device wasn't found on the network")]
    UnknownDevice,
    #[error("that device can't show pages; cast audio to it instead")]
    NoScreen,
}

/// What to cast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CastMedia {
    /// The live audio stream, with something to show on screens.
    Stream { url: String, meta: Metadata },
    /// A web page shown full screen (the TV page, which plays the stream
    /// itself). Google Cast devices with a screen only.
    Page { url: String },
}

/// What a device shows while it plays the party.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    pub title: String,
    pub subtitle: String,
    /// A picture for screens (PNG or JPEG URL).
    pub image_url: Option<String>,
}

/// Commands to a running cast.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CastCommand {
    /// Device volume, 0.0 to 1.0.
    SetVolume(f32),
    Stop,
}

/// Live state of one cast.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CastState {
    pub playing: bool,
    pub buffering: bool,
    /// Device volume, when known.
    pub volume: Option<f32>,
    /// The device sets its own volume (a TV's remote); DJINN can't.
    pub volume_fixed: bool,
    pub error: Option<String>,
}
