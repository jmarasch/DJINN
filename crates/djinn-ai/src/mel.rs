//! CLAP's input: log-mel spectrograms exactly as Hugging Face's
//! `ClapFeatureExtractor` makes them for this model (48 kHz, 1024-point
//! FFT, hop 480, 64 Slaney mel bands from 50 Hz to 14 kHz, power in dB).

use std::f32::consts::PI;
use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

pub const SAMPLE_RATE: usize = 48_000;
const N_FFT: usize = 1024;
const HOP: usize = 480;
pub const MELS: usize = 64;
/// Ten seconds per clip.
pub const CLIP: usize = SAMPLE_RATE * 10;
/// Frames in a clip's spectrogram.
pub const FRAMES: usize = 1 + CLIP / HOP;

pub struct MelSpectrogram {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Weights per FFT bin, per mel band: [bin][mel].
    filters: Vec<[f32; MELS]>,
}

impl MelSpectrogram {
    pub fn new() -> Self {
        Self {
            fft: RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT),
            // Periodic Hann, as numpy's window_function makes it.
            window: (0..N_FFT)
                .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / N_FFT as f32).cos())
                .collect(),
            filters: slaney_filters(),
        }
    }

    /// Features for one clip of exactly [`CLIP`] samples, as (FRAMES x
    /// MELS) row-major.
    pub fn features(&self, clip: &[f32]) -> Vec<f32> {
        debug_assert_eq!(clip.len(), CLIP);
        // Centered frames: reflect-pad half an FFT on both sides.
        let pad = N_FFT / 2;
        let mut x = Vec::with_capacity(clip.len() + 2 * pad);
        x.extend((1..=pad).rev().map(|i| clip[i]));
        x.extend_from_slice(clip);
        x.extend((0..pad).map(|i| clip[clip.len() - 2 - i]));

        let mut input = self.fft.make_input_vec();
        let mut output = self.fft.make_output_vec();
        let mut power = vec![0f32; N_FFT / 2 + 1];
        let mut out = Vec::with_capacity(FRAMES * MELS);
        for frame in 0..FRAMES {
            let start = frame * HOP;
            for ((dst, src), w) in input.iter_mut().zip(&x[start..]).zip(&self.window) {
                *dst = src * w;
            }
            if self.fft.process(&mut input, &mut output).is_err() {
                out.resize(out.len() + MELS, -100.0);
                continue;
            }
            for (p, c) in power.iter_mut().zip(&output) {
                *p = c.norm_sqr();
            }
            let mut mel = [0f32; MELS];
            for (p, weights) in power.iter().zip(&self.filters) {
                for (m, w) in mel.iter_mut().zip(weights) {
                    *m += p * w;
                }
            }
            out.extend(mel.iter().map(|e| 10.0 * e.max(1e-10).log10()));
        }
        out
    }
}

fn hz_to_mel(f: f64) -> f64 {
    // Slaney: linear below 1 kHz, logarithmic above.
    const F_SP: f64 = 200.0 / 3.0;
    if f >= 1000.0 {
        15.0 + (f / 1000.0).ln() / (6.4f64.ln() / 27.0)
    } else {
        f / F_SP
    }
}

fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    if m >= 15.0 {
        1000.0 * ((6.4f64.ln() / 27.0) * (m - 15.0)).exp()
    } else {
        F_SP * m
    }
}

/// Triangular filters in Hz, area-normalized ("slaney" norm).
fn slaney_filters() -> Vec<[f32; MELS]> {
    let bins = N_FFT / 2 + 1;
    let (lo, hi) = (hz_to_mel(50.0), hz_to_mel(14_000.0));
    let edges: Vec<f64> = (0..MELS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MELS + 1) as f64))
        .collect();
    (0..bins)
        .map(|k| {
            let f = k as f64 * (SAMPLE_RATE as f64 / 2.0) / (bins - 1) as f64;
            let mut row = [0f32; MELS];
            for (m, w) in row.iter_mut().enumerate() {
                let down = (f - edges[m]) / (edges[m + 1] - edges[m]);
                let up = (edges[m + 2] - f) / (edges[m + 2] - edges[m + 1]);
                let norm = 2.0 / (edges[m + 2] - edges[m]);
                *w = (down.min(up).max(0.0) * norm) as f32;
            }
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_and_loudness_order() {
        let mel = MelSpectrogram::new();
        let tone = |amp: f32| -> Vec<f32> {
            (0..CLIP)
                .map(|i| amp * (2.0 * PI * 1000.0 * i as f32 / SAMPLE_RATE as f32).sin())
                .collect()
        };
        let quiet = mel.features(&tone(0.01));
        let loud = mel.features(&tone(0.5));
        assert_eq!(quiet.len(), FRAMES * MELS);
        // The band holding 1 kHz is louder for the louder tone, by about
        // 20*log10(50) = 34 dB.
        let band = (0..MELS)
            .max_by(|&a, &b| loud[100 * MELS + a].total_cmp(&loud[100 * MELS + b]))
            .unwrap_or(0);
        let gain = loud[100 * MELS + band] - quiet[100 * MELS + band];
        assert!((gain - 34.0).abs() < 1.0, "gain {gain}");
        // Silence floors at -100 dB, not -inf.
        let silent = mel.features(&vec![0.0; CLIP]);
        assert!(silent.iter().all(|v| (*v + 100.0).abs() < 1e-3));
    }
}
