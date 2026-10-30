//! Converting a song file to MP3, for DJINN GO's optional converting:
//! devices that can't play a format, or saving phone data. Uses the same
//! encoder as the live stream.

use std::io::Write;
use std::path::Path;

use rusty_mp3::{Mp3Encoder, Mp3EncoderConfig};

use crate::CastError;

/// Bit rates offered, in kbit/s.
pub const BITRATES: &[u32] = &[96, 128, 160, 192, 256, 320];

/// Writes `input` as an MP3 at `kbps` to `output`. Rates MP3 can't carry
/// (88.2 or 96 kHz, say) become 48 kHz; 32, 44.1 and 48 kHz stay.
pub fn to_mp3(input: &Path, output: &Path, kbps: u32) -> Result<(), CastError> {
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(output).map_err(|e| CastError::Encoder(e.to_string()))?,
    );
    let mut encoder = Mp3Encoder::new(Mp3EncoderConfig {
        bitrate_kbps: kbps,
        vbr_quality: None,
    });
    let mut failed: Option<String> = None;
    let drain = |encoder: &mut Mp3Encoder, file: &mut dyn Write| -> Result<(), String> {
        while let Ok(packet) = encoder.next_packet() {
            file.write_all(&packet).map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    djinn_engine::decode_stereo(
        input,
        |rate| match rate {
            32_000 | 44_100 | 48_000 => rate,
            _ => 48_000,
        },
        &mut |rate, pcm| {
            if let Err(e) = encoder.push_pcm_f32(pcm, 2, rate) {
                failed = Some(e.to_string());
                return false;
            }
            if let Err(e) = drain(&mut encoder, &mut file) {
                failed = Some(e);
                return false;
            }
            true
        },
    )
    .map_err(|e| CastError::Encoder(e.to_string()))?;
    if let Some(e) = failed {
        return Err(CastError::Encoder(e));
    }
    encoder.finish();
    drain(&mut encoder, &mut file).map_err(CastError::Encoder)?;
    file.flush()
        .map_err(|e| CastError::Encoder(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_a_flac_to_an_mp3() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures/gapless_a.flac");
        let output = std::env::temp_dir().join(format!("djinn-convert-{}.mp3", std::process::id()));
        to_mp3(&input, &output, 128).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        let _ = std::fs::remove_file(&output);
        assert!(bytes.len() > 1000, "{} bytes", bytes.len());
        // MPEG frame sync: 11 set bits.
        assert_eq!(bytes[0], 0xff);
        assert_eq!(bytes[1] & 0xe0, 0xe0);
    }
}
