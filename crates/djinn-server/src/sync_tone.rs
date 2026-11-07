//! The speaker sync tone: 30 seconds of short clicks, one a second (a
//! higher one every fourth), played through a quiet room's speakers and
//! nodes so each one's delay can be set by ear until the clicks land
//! together. Made once as a WAV in DJINN's data folder.

use std::path::{Path, PathBuf};

const RATE: u32 = 48_000;
const SECONDS: u32 = 30;

/// The tone's file, made if it isn't there yet.
pub fn file(data_dir: &Path) -> std::io::Result<PathBuf> {
    let path = data_dir.join("sync-tone.wav");
    if !path.is_file() {
        std::fs::write(&path, wav())?;
    }
    Ok(path)
}

/// Stereo 16-bit WAV bytes of the clicks.
fn wav() -> Vec<u8> {
    let frames = RATE * SECONDS;
    let mut samples = Vec::with_capacity(frames as usize);
    for i in 0..frames {
        let second = i / RATE;
        let within = (i % RATE) as f32 / RATE as f32;
        let click = 0.04;
        let value = if within < click {
            let pitch = if second.is_multiple_of(4) {
                1760.0
            } else {
                880.0
            };
            // A quick fade in and out keeps it a click, not a pop.
            let edge = (within / 0.004).min((click - within) / 0.004).min(1.0);
            (2.0 * std::f32::consts::PI * pitch * within).sin() * 0.5 * edge
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
    fn the_tone_is_a_wav_with_clicks_each_second() {
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
        assert!(loud(48_000) > 10_000, "a click at one second");
        assert_eq!(loud(48_000 + 24_000), 0, "quiet between clicks");
        let dir = std::env::temp_dir().join(format!("djinn-tone-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = file(&dir).unwrap();
        assert!(path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
