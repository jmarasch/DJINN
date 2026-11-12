//! The speaker timing sound: 30 seconds of soft "tocks", one a second (a
//! higher one every fourth), played through a quiet room's speakers and
//! nodes so each one can be moved earlier or later by ear until they land
//! together. Short and quickly fading, so it's easy to hear where each one
//! starts without being harsh. Made once as a WAV in DJINN's data folder.

use std::path::{Path, PathBuf};

const RATE: u32 = 48_000;
const SECONDS: u32 = 30;
/// How long each tock rings, in seconds; silence after that.
const RING_SECS: f32 = 0.3;

/// The sound's file, made if it isn't there yet. (A new name whenever the
/// sound changes, so an older file isn't played.)
pub fn file(data_dir: &Path) -> std::io::Result<PathBuf> {
    let path = data_dir.join("timing-tock.wav");
    if !path.is_file() {
        std::fs::write(&path, wav())?;
    }
    Ok(path)
}

/// Stereo 16-bit WAV bytes of the tocks.
fn wav() -> Vec<u8> {
    let frames = RATE * SECONDS;
    let mut samples = Vec::with_capacity(frames as usize);
    for i in 0..frames {
        let second = i / RATE;
        let within = (i % RATE) as f32 / RATE as f32;
        let value = if within < RING_SECS {
            let pitch = if second.is_multiple_of(4) {
                990.0
            } else {
                660.0
            };
            // A soft start (no click), then a quick fade like a wood block.
            let attack = (within / 0.002).min(1.0);
            let fade = (-within / 0.05).exp() * (1.0 - within / RING_SECS);
            let phase = 2.0 * std::f32::consts::PI * pitch * within;
            let tone = phase.sin() + 0.3 * (2.0 * phase).sin();
            tone / 1.3 * 0.45 * attack * fade
        } else {
            0.0
        };
        samples.push((value * f32::from(i16::MAX)) as i16);
    }
    let data_len = frames * 4;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&2u16.to_le_bytes()); // stereo
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sound_is_a_wav_with_a_tock_each_second() {
        let bytes = wav();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + (RATE * SECONDS * 4) as usize);
        let sample =
            |frame: usize| i16::from_le_bytes([bytes[44 + frame * 4], bytes[45 + frame * 4]]);
        let loud = |start: usize| {
            (start..start + 1000)
                .map(|f| sample(f).unsigned_abs())
                .max()
                .unwrap_or(0)
        };
        assert!(loud(48_000) > 8_000, "a tock at one second");
        assert_eq!(loud(48_000 + 24_000), 0, "quiet between tocks");
        assert!(
            sample(0).unsigned_abs() < 200,
            "starts softly, without a click"
        );
        let dir = std::env::temp_dir().join(format!("djinn-tone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = file(&dir).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
