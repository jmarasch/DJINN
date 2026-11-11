//! How loud each part of the sound is, timed to when it's heard: what a
//! node's screen draws its visualizer from. The node player measures audio
//! as it hands it to the speaker (on its own thread, never the audio
//! callback), and the screen asks for the moment it's showing.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use realfft::{RealFftPlanner, RealToComplex};

/// Bands from deep bass to the top, spaced evenly by pitch.
pub const BANDS: usize = 24;
/// Samples per measurement (about 21 ms at 48 kHz).
const BLOCK: usize = 1024;
const LOWEST_HZ: f32 = 40.0;
const HIGHEST_HZ: f32 = 16_000.0;
/// Loudness shown as nothing and as full, in dB.
const FLOOR_DB: f32 = -70.0;
const CEILING_DB: f32 = -12.0;
/// Measurements kept (a little over two seconds ahead).
const KEPT: usize = 128;
/// A measurement this old (µs) counts as silence: playback stopped or paused.
const STALE_US: i64 = 150_000;

/// Measurements shared between the player and whoever shows them.
#[derive(Clone, Default)]
pub struct Levels {
    inner: Arc<Mutex<Inner>>,
    live: Arc<AtomicBool>,
}

#[derive(Default)]
struct Inner {
    /// (when heard, bands), oldest first.
    frames: VecDeque<(i64, [f32; BANDS])>,
    paused: bool,
}

impl Levels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a player is measuring now (a speaker is open).
    pub fn live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }

    pub(crate) fn set_live(&self, live: bool) {
        self.live.store(live, Ordering::Release);
        if !live {
            self.clear();
        }
    }

    pub(crate) fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.frames.clear();
        }
    }

    /// Paused: nothing is heard until `resume`.
    pub(crate) fn pause(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.paused = true;
        }
    }

    /// Playing again after `held` µs: what was waiting is heard that much
    /// later.
    pub(crate) fn resume(&self, held: i64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.paused = false;
            for frame in &mut inner.frames {
                frame.0 += held;
            }
        }
    }

    fn push(&self, at: i64, bands: [f32; BANDS]) {
        if let Ok(mut inner) = self.inner.lock() {
            let frames = &mut inner.frames;
            // Measured again from earlier (a restart): drop what's after.
            while frames.back().is_some_and(|f| f.0 >= at) {
                frames.pop_back();
            }
            frames.push_back((at, bands));
            while frames.len() > KEPT {
                frames.pop_front();
            }
        }
    }

    /// The bands heard at `now` (µs on the engine's clock), each 0 to 1;
    /// all zero when nothing is playing.
    pub fn at(&self, now: i64) -> [f32; BANDS] {
        let Ok(mut inner) = self.inner.lock() else {
            return [0.0; BANDS];
        };
        if inner.paused {
            return [0.0; BANDS];
        }
        let frames = &mut inner.frames;
        // Forget what's been heard, keeping the one playing now.
        while frames.len() > 1 && frames[1].0 <= now {
            frames.pop_front();
        }
        match frames.front() {
            Some(&(at, bands)) if at <= now && now - at < STALE_US => bands,
            _ => [0.0; BANDS],
        }
    }
}

/// Measures stereo audio in blocks as it's played.
pub(crate) struct Meter {
    rate: u32,
    levels: Levels,
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    block: Vec<f32>,
    /// When the block's first sample is heard (µs).
    block_at: i64,
    /// The FFT bins each band covers.
    edges: [usize; BANDS + 1],
}

impl Meter {
    pub(crate) fn new(rate: u32, levels: Levels) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(BLOCK);
        let window = (0..BLOCK)
            .map(|i| {
                let x = std::f32::consts::TAU * i as f32 / BLOCK as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        let input = fft.make_input_vec();
        let spectrum = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        Meter {
            rate,
            levels,
            fft,
            window,
            input,
            spectrum,
            scratch,
            block: Vec::with_capacity(BLOCK),
            block_at: 0,
            edges: edges(rate),
        }
    }

    pub(crate) fn levels(&self) -> &Levels {
        &self.levels
    }

    /// Starts over (a flush or a jump): a part-measured block is dropped.
    pub(crate) fn reset(&mut self) {
        self.block.clear();
        self.levels.clear();
    }

    /// Takes interleaved stereo `samples` whose first frame is heard at
    /// `at` (µs).
    pub(crate) fn take(&mut self, samples: &[f32], at: i64) {
        for (i, frame) in samples.as_chunks::<2>().0.iter().enumerate() {
            if self.block.is_empty() {
                self.block_at = at + (i as i64 * 1_000_000) / i64::from(self.rate);
            }
            self.block.push((frame[0] + frame[1]) * 0.5);
            if self.block.len() == BLOCK {
                let bands = self.measure();
                self.levels.push(self.block_at, bands);
                self.block.clear();
            }
        }
    }

    fn measure(&mut self) -> [f32; BANDS] {
        for ((slot, sample), weight) in self.input.iter_mut().zip(&self.block).zip(&self.window) {
            *slot = sample * weight;
        }
        if self
            .fft
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_err()
        {
            return [0.0; BANDS];
        }
        // A full-scale sine comes out near 0 dB.
        let scale = 2.0 / (BLOCK as f32 * 0.5);
        let mut bands = [0.0; BANDS];
        for (band, out) in bands.iter_mut().enumerate() {
            let (from, to) = (self.edges[band], self.edges[band + 1]);
            let peak = self.spectrum[from..to]
                .iter()
                .map(|c| c.norm() * scale)
                .fold(0.0f32, f32::max);
            let db = 20.0 * peak.max(1e-9).log10();
            *out = ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0);
        }
        bands
    }
}

/// Bin edges for each band, at least one bin wide.
fn edges(rate: u32) -> [usize; BANDS + 1] {
    let bin_hz = rate as f32 / BLOCK as f32;
    let top = (HIGHEST_HZ.min(rate as f32 / 2.0) / bin_hz) as usize;
    let mut edges = [0; BANDS + 1];
    let mut last = 0;
    for (i, edge) in edges.iter_mut().enumerate() {
        let hz = LOWEST_HZ * (HIGHEST_HZ / LOWEST_HZ).powf(i as f32 / BANDS as f32);
        let bin = ((hz / bin_hz).round() as usize).clamp(1, top.max(1));
        *edge = if i == 0 { bin } else { bin.max(last + 1) };
        last = *edge;
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32, frames: usize, amp: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = amp * (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin();
                [s, s]
            })
            .collect()
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let levels = Levels::new();
        let mut meter = Meter::new(48_000, levels.clone());
        meter.take(&tone(1000.0, 48_000, BLOCK, 0.1), 1_000_000);
        let bands = levels.at(1_000_000);
        let loudest = bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        let edges = edges(48_000);
        let bin = (1000.0 / (48_000.0 / BLOCK as f32)).round() as usize;
        // The tone's bin, or the one beside it (the window spreads it a little).
        assert!(
            edges[loudest] <= bin + 1 && bin < edges[loudest + 1] + 1,
            "{loudest} {bin} {edges:?} {bands:?}"
        );
        assert!(bands[loudest] > 0.6, "{bands:?}");
        assert!(bands[0] < 0.3 && bands[BANDS - 1] < 0.3, "{bands:?}");
    }

    #[test]
    fn levels_follow_the_moment_heard() {
        let levels = Levels::new();
        let mut meter = Meter::new(48_000, levels.clone());
        let block_us = (BLOCK as i64 * 1_000_000) / 48_000;
        let mut audio = tone(200.0, 48_000, BLOCK, 0.0);
        audio.extend(tone(200.0, 48_000, BLOCK, 0.8));
        meter.take(&audio, 0);
        assert!(
            levels.at(block_us / 2).iter().all(|&b| b == 0.0),
            "quiet first"
        );
        assert!(
            levels.at(block_us + 1).iter().any(|&b| b > 0.5),
            "then loud"
        );
        // Long after the last block: playback stopped, so nothing.
        assert!(levels.at(block_us * 2 + STALE_US).iter().all(|&b| b == 0.0));
    }

    #[test]
    fn every_band_covers_at_least_one_bin() {
        for rate in [44_100, 48_000, 96_000] {
            let edges = edges(rate);
            assert!(edges.windows(2).all(|w| w[1] > w[0]), "{rate}: {edges:?}");
        }
    }
}
