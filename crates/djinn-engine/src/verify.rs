//! Does a file really play? Decoding it from end to end finds what opening
//! it won't: damage in the middle, or a file cut short.

use std::path::Path;

use crate::decoder::TrackDecoder;
use crate::EngineError;

/// Decoded audio this much shorter than the song should be means the file
/// was cut short. Lengths from tags are never quite exact.
const SHORT_RATIO: f64 = 0.9;
/// Songs under this long aren't checked for length.
const MIN_CHECKED_MS: u64 = 5000;

/// Decodes the whole file. `expected_ms` is how long the song should be, if
/// that's known.
pub fn verify(path: &Path, expected_ms: Option<u64>) -> Result<(), EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let rate = u64::from(decoder.sample_rate().max(1));
    let (mut samples, mut decoded) = (0u64, Vec::new());
    loop {
        decoded.clear();
        if !decoder.decode_next(&mut decoded)? {
            break;
        }
        samples += decoded.len() as u64;
    }
    let bad = |reason: String| EngineError::Track {
        path: path.to_path_buf(),
        reason,
    };
    if samples == 0 {
        return Err(bad("no audio in the file".to_owned()));
    }
    let got_ms = samples / 2 * 1000 / rate;
    if let Some(expected) = expected_ms.filter(|&ms| ms >= MIN_CHECKED_MS) {
        if (got_ms as f64) < expected as f64 * SHORT_RATIO {
            return Err(bad(format!(
                "cut short: {} s of {} s",
                got_ms / 1000,
                expected / 1000
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn temp_copy(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("djinn-verify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn good_files_pass() {
        for name in ["gapless_a.flac", "gapless_a.mp3"] {
            verify(&fixture(name), None).unwrap();
        }
    }

    #[test]
    fn garbage_and_empty_files_fail() {
        let garbage = temp_copy("garbage.flac", &[0x5a; 4096]);
        assert!(verify(&garbage, None).is_err());
        let empty = temp_copy("empty.mp3", &[]);
        assert!(verify(&empty, None).is_err());
        assert!(verify(Path::new("/no/such/file.mp3"), None).is_err());
    }

    #[test]
    fn a_file_shorter_than_its_song_fails() {
        // The fixture's real length is well under an hour.
        let path = fixture("gapless_a.flac");
        assert!(verify(&path, Some(3_600_000)).is_err());
        // Short songs' lengths aren't held against them.
        verify(&path, Some(MIN_CHECKED_MS - 1)).unwrap();
    }
}
