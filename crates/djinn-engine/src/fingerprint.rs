//! Audio fingerprints (Chromaprint, as AcoustID uses them), for
//! identifying songs whose tags are missing or wrong.

use std::path::Path;

use rusty_chromaprint::{Configuration, FingerprintCompressor, Fingerprinter};

use crate::decoder::TrackDecoder;
use crate::EngineError;

/// AcoustID fingerprints the first two minutes.
const SECONDS: u64 = 120;

/// The compressed fingerprint of a file's first two minutes, encoded as
/// AcoustID expects it (URL-safe base64). `None` for silence or files too
/// short to identify.
pub fn fingerprint(path: &Path) -> Result<Option<String>, EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let rate = decoder.sample_rate();
    // AcoustID's own tools use the default algorithm.
    let config = Configuration::preset_test2();
    let mut printer = Fingerprinter::new(&config);
    printer.start(rate, 2).map_err(|e| EngineError::Track {
        path: path.to_path_buf(),
        reason: format!("fingerprint: {e:?}"),
    })?;
    let max = SECONDS * u64::from(rate) * 2;
    let (mut done, mut decoded, mut samples) = (0u64, Vec::new(), Vec::new());
    while done < max {
        decoded.clear();
        if !decoder.decode_next(&mut decoded)? {
            break;
        }
        samples.clear();
        samples.extend(
            decoded
                .iter()
                .take(usize::try_from(max - done).unwrap_or(usize::MAX))
                .map(|s| (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16),
        );
        done += samples.len() as u64;
        printer.consume(&samples);
    }
    printer.finish();
    let print = printer.fingerprint();
    if print.len() < 20 || print.iter().all(|&p| p == 0) {
        return Ok(None);
    }
    Ok(Some(base64_url(
        &FingerprintCompressor::from(&config).compress(print),
    )))
}

/// URL-safe base64 without padding.
fn base64_url(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 3);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        for i in 0..=chunk.len() {
            out.push(char::from(TABLE[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::tests::{sine_i16, temp_dir, write_wav};

    #[test]
    fn base64_matches_the_standard() {
        // RFC 4648 test vectors, URL-safe alphabet, no padding.
        assert_eq!(base64_url(b""), "");
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"fo"), "Zm8");
        assert_eq!(base64_url(b"foo"), "Zm9v");
        assert_eq!(base64_url(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn fingerprints_are_stable() {
        let dir = temp_dir("fingerprint");
        let path = dir.join("a.wav");
        // 30 s of a changing tone.
        let mut samples = Vec::new();
        for part in 0..6 {
            samples.extend(sine_i16(48_000, part * 240_000, (part + 1) * 240_000));
        }
        write_wav(&path, 48_000, &samples);
        let first = fingerprint(&path).unwrap();
        assert!(first.as_ref().is_some_and(|f| f.len() > 20));
        assert_eq!(fingerprint(&path).unwrap(), first, "same audio, same print");

        let silent = dir.join("silent.wav");
        write_wav(&silent, 48_000, &vec![0; 96_000]);
        assert_eq!(fingerprint(&silent).unwrap(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
