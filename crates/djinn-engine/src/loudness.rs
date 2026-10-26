//! Measuring a track's loudness (EBU R128), for ReplayGain on files that
//! have no ReplayGain tags.

use std::path::Path;

use ebur128::{EbuR128, Mode};

use crate::decoder::TrackDecoder;
use crate::EngineError;

/// ReplayGain 2 plays everything as if it measured this.
pub const REFERENCE_LUFS: f64 = -18.0;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Loudness {
    /// Integrated loudness in LUFS.
    pub lufs: f64,
    /// Highest sample, linear (1.0 is full scale).
    pub peak: f64,
}

impl Loudness {
    /// Gain in dB that brings this track to the reference loudness.
    pub fn gain_db(&self) -> f64 {
        REFERENCE_LUFS - self.lufs
    }
}

/// Decodes the whole file and measures it. Returns `None` for silence.
pub fn measure_loudness(path: &Path) -> Result<Option<Loudness>, EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let meter_error = |e: ebur128::Error| EngineError::Track {
        path: path.to_path_buf(),
        reason: format!("loudness meter: {e}"),
    };
    let mut meter =
        EbuR128::new(2, decoder.sample_rate(), Mode::I | Mode::SAMPLE_PEAK).map_err(meter_error)?;
    let mut samples = Vec::new();
    loop {
        samples.clear();
        if !decoder.decode_next(&mut samples)? {
            break;
        }
        meter.add_frames_f32(&samples).map_err(meter_error)?;
    }
    let lufs = meter.loudness_global().map_err(meter_error)?;
    let peak = (0..2)
        .filter_map(|c| meter.sample_peak(c).ok())
        .fold(0.0, f64::max);
    Ok(lufs.is_finite().then_some(Loudness { lufs, peak }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::tests::{sine_i16, temp_dir, write_wav};

    #[test]
    fn measures_a_sine() {
        let dir = temp_dir("loudness");
        let path = dir.join("a.wav");
        // 5 s of a half-scale sine.
        write_wav(&path, 48_000, &sine_i16(48_000, 0, 240_000));
        let loudness = measure_loudness(&path).unwrap().unwrap();
        assert!((loudness.peak - 0.5).abs() < 0.01, "peak {}", loudness.peak);
        // A stereo half-scale 440 Hz sine measures about -6.5 LUFS.
        assert!((loudness.lufs + 6.5).abs() < 1.0, "lufs {}", loudness.lufs);
        assert!((loudness.gain_db() + 11.5).abs() < 1.0);

        let silent = dir.join("silent.wav");
        write_wav(&silent, 48_000, &vec![0; 96_000]);
        assert_eq!(measure_loudness(&silent).unwrap(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
