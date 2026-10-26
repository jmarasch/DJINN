//! Casting: play the party on network devices (Chromecast, DLNA renderers,
//! Sonos) by streaming the master mix as live MP3.

pub mod stream;

#[derive(Debug, thiserror::Error)]
pub enum CastError {
    #[error("could not start the stream encoder: {0}")]
    Encoder(String),
}
