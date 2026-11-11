//! What the sound looks like, timed to when it's heard: what a node's
//! screen draws its visualizer from. The node player measures audio as it
//! hands it to the speaker (on its own thread, never the audio callback),
//! and the screen asks for the moment it's showing. Nothing is measured
//! while no screen asks.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use realfft::{RealFftPlanner, RealToComplex};

/// Bands from deep bass to the top, spaced evenly by pitch.
pub const BANDS: usize = 24;
/// Finer rows of the same, for a spectrogram.
pub const FINE: usize = 96;
/// Points of the wave each channel sends, across one measurement.
pub const WAVE: usize = 128;
/// Samples per measurement (about 21 ms at 48 kHz).
const BLOCK: usize = 1024;
/// Samples the finer rows look over, for detail low down.
const LONG: usize = 4096;
const LOWEST_HZ: f32 = 40.0;
const FINE_LOWEST_HZ: f32 = 30.0;
const HIGHEST_HZ: f32 = 16_000.0;
/// Loudness shown as nothing and as full, in dB.
const FLOOR_DB: f32 = -70.0;
const CEILING_DB: f32 = -12.0;
/// Measurements kept (a little over two seconds ahead).
const KEPT: usize = 128;
/// A measurement this old (µs) counts as silence: playback stopped or paused.
const STALE_US: i64 = 150_000;
/// How long after the last ask the meter keeps measuring (µs).
const WATCHED_FOR_US: i64 = 3_000_000;

/// One measurement. Levels are 0 to 1; the wave is -1 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub bands: [f32; BANDS],
    pub fine: [f32; FINE],
    pub left: [f32; WAVE],
    pub right: [f32; WAVE],
    /// How loud each channel is overall.
    pub loudness: [f32; 2],
}

impl Default for Frame {
    fn default() -> Self {
        Frame {
            bands: [0.0; BANDS],
            fine: [0.0; FINE],
            left: [0.0; WAVE],
            right: [0.0; WAVE],
            loudness: [0.0; 2],
        }
    }
}

/// Measurements shared between the player and whoever shows them.
#[derive(Clone, Default)]
pub struct Levels {
    inner: Arc<Mutex<Inner>>,
    live: Arc<AtomicBool>,
    /// When a screen last asked (µs on the engine's clock), 0 for never.
    asked_at: Arc<AtomicI64>,
}

#[derive(Default)]
struct Inner {
    /// (when heard, measurement), oldest first.
    frames: VecDeque<(i64, Box<Frame>)>,
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

    /// Whether a screen has asked lately, so measuring is worth it.
    pub fn watched(&self, now: i64) -> bool {
        let asked = self.asked_at.load(Ordering::Acquire);
        asked != 0 && now - asked < WATCHED_FOR_US
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

    fn push(&self, at: i64, frame: Box<Frame>) {
        if let Ok(mut inner) = self.inner.lock() {
            let frames = &mut inner.frames;
            // Measured again from earlier (a restart): drop what's after.
            while frames.back().is_some_and(|f| f.0 >= at) {
                frames.pop_back();
            }
            frames.push_back((at, frame));
            while frames.len() > KEPT {
                frames.pop_front();
            }
        }
    }

    /// What's heard at `now` (µs on the engine's clock); all quiet when
    /// nothing is playing. Asking keeps the meter measuring for a while.
    pub fn at(&self, now: i64) -> Frame {
        self.asked_at.store(now.max(1), Ordering::Release);
        let Ok(mut inner) = self.inner.lock() else {
            return Frame::default();
        };
        if inner.paused {
            return Frame::default();
        }
        let frames = &mut inner.frames;
        // Forget what's been heard, keeping the one playing now.
        while frames.len() > 1 && frames[1].0 <= now {
            frames.pop_front();
        }
        match frames.front() {
            Some((at, frame)) if *at <= now && now - at < STALE_US => **frame,
            _ => Frame::default(),
        }
    }
}

type Complex = realfft::num_complex::Complex<f32>;

/// One FFT size, with its window and buffers.
struct Fft {
    plan: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    spectrum: Vec<Complex>,
    scratch: Vec<Complex>,
}

impl Fft {
    fn new(size: usize) -> Self {
        let plan = RealFftPlanner::<f32>::new().plan_fft_forward(size);
        let window = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / size as f32).cos())
            .collect();
        Fft {
            input: plan.make_input_vec(),
            spectrum: plan.make_output_vec(),
            scratch: plan.make_scratch_vec(),
            plan,
            window,
        }
    }

    /// Runs over `samples` (as long as the FFT); false if it couldn't.
    fn run(&mut self, samples: impl Iterator<Item = f32>) -> bool {
        for ((slot, sample), weight) in self.input.iter_mut().zip(samples).zip(&self.window) {
            *slot = sample * weight;
        }
        self.plan
            .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
            .is_ok()
    }

    /// The loudest bin in `from..to`, 0 to 1. A full-scale sine comes out
    /// near 0 dB.
    fn level(&self, from: usize, to: usize) -> f32 {
        let scale = 4.0 / self.window.len() as f32;
        let peak = self.spectrum[from..to.max(from + 1)]
            .iter()
            .map(|c| c.norm() * scale)
            .fold(0.0f32, f32::max);
        to_level(peak)
    }
}

fn to_level(amplitude: f32) -> f32 {
    let db = 20.0 * amplitude.max(1e-9).log10();
    ((db - FLOOR_DB) / (CEILING_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// Measures stereo audio in blocks as it's played.
pub(crate) struct Meter {
    rate: u32,
    levels: Levels,
    short: Fft,
    long: Fft,
    /// This block, interleaved stereo.
    block: Vec<f32>,
    /// The last `LONG` samples, mono, oldest first.
    history: VecDeque<f32>,
    /// When the block's first sample is heard (µs).
    block_at: i64,
    /// The bins each band and each fine row covers.
    edges: Vec<usize>,
    fine_edges: Vec<usize>,
}

impl Meter {
    pub(crate) fn new(rate: u32, levels: Levels) -> Self {
        Meter {
            rate,
            levels,
            short: Fft::new(BLOCK),
            long: Fft::new(LONG),
            block: Vec::with_capacity(BLOCK * 2),
            history: VecDeque::with_capacity(LONG),
            block_at: 0,
            edges: edges(rate, BLOCK, LOWEST_HZ, BANDS, true),
            fine_edges: edges(rate, LONG, FINE_LOWEST_HZ, FINE, false),
        }
    }

    pub(crate) fn levels(&self) -> &Levels {
        &self.levels
    }

    /// Starts over (a flush or a jump): a part-measured block is dropped.
    pub(crate) fn reset(&mut self) {
        self.block.clear();
        self.history.clear();
        self.levels.clear();
    }

    /// Takes interleaved stereo `samples` whose first frame is heard at
    /// `at` (µs).
    pub(crate) fn take(&mut self, samples: &[f32], at: i64) {
        if !self.levels.watched(crate::clock::now_micros()) {
            if !self.block.is_empty() || !self.history.is_empty() {
                self.reset();
            }
            return;
        }
        for (i, frame) in samples.as_chunks::<2>().0.iter().enumerate() {
            if self.block.is_empty() {
                self.block_at = at + (i as i64 * 1_000_000) / i64::from(self.rate);
            }
            self.block.extend_from_slice(frame);
            if self.history.len() == LONG {
                self.history.pop_front();
            }
            self.history.push_back((frame[0] + frame[1]) * 0.5);
            if self.block.len() == BLOCK * 2 {
                let frame = self.measure();
                self.levels.push(self.block_at, frame);
                self.block.clear();
            }
        }
    }

    fn measure(&mut self) -> Box<Frame> {
        let mut frame = Box::<Frame>::default();
        let mono = self
            .block
            .as_chunks::<2>()
            .0
            .iter()
            .map(|f| (f[0] + f[1]) * 0.5);
        if self.short.run(mono) {
            for (band, out) in frame.bands.iter_mut().enumerate() {
                *out = self.short.level(self.edges[band], self.edges[band + 1]);
            }
        }
        // Until the history fills, the long look waits.
        if self.history.len() == LONG && self.long.run(self.history.iter().copied()) {
            for (row, out) in frame.fine.iter_mut().enumerate() {
                *out = self
                    .long
                    .level(self.fine_edges[row], self.fine_edges[row + 1]);
            }
        }
        let step = BLOCK / WAVE;
        let pairs = self.block.as_chunks::<2>().0;
        for i in 0..WAVE {
            let [l, r] = pairs[i * step];
            frame.left[i] = l.clamp(-1.0, 1.0);
            frame.right[i] = r.clamp(-1.0, 1.0);
        }
        let mut power = [0.0f32; 2];
        for [l, r] in pairs {
            power[0] += l * l;
            power[1] += r * r;
        }
        // An RMS level, lifted so a full-scale sine reads like its band.
        frame.loudness =
            power.map(|p| to_level((p / BLOCK as f32).sqrt() * std::f32::consts::SQRT_2));
        frame
    }
}

/// Measures Main's own timed stream (`timed_tap`'s pieces), for a screen
/// showing what Main's speakers play.
pub struct PieceMeter {
    meter: Meter,
    paused_at: Option<i64>,
}

impl PieceMeter {
    pub fn new(rate: u32, levels: Levels) -> Self {
        levels.set_live(true);
        PieceMeter {
            meter: Meter::new(rate, levels),
            paused_at: None,
        }
    }

    /// Takes one piece; `now` is the shared clock (µs).
    pub fn take(&mut self, piece: &crate::Piece, now: i64) {
        match piece.mark {
            crate::Mark::Audio { play_at_us, .. } => self.meter.take(&piece.samples, play_at_us),
            crate::Mark::Flush => self.meter.reset(),
            crate::Mark::Pause => {
                if self.paused_at.is_none() {
                    self.paused_at = Some(now);
                    self.meter.levels().pause();
                }
            }
            crate::Mark::Resume => {
                if let Some(at) = self.paused_at.take() {
                    // Main's stamps after a pause already allow for it; only
                    // what was measured before moves.
                    self.meter.levels().resume(now - at);
                }
            }
            crate::Mark::Volume(_) => {}
        }
    }
}

/// Bin edges for `rows` rows of an FFT of `size`, spaced by pitch from
/// `lowest`. With `distinct`, each row gets a bin of its own (pushing the
/// low ones up); otherwise rows may share one low down, where bins are wide.
fn edges(rate: u32, size: usize, lowest: f32, rows: usize, distinct: bool) -> Vec<usize> {
    let bin_hz = rate as f32 / size as f32;
    let top = (HIGHEST_HZ.min(rate as f32 / 2.0) / bin_hz) as usize;
    let mut edges = Vec::with_capacity(rows + 1);
    let mut last = 0;
    for i in 0..=rows {
        let hz = lowest * (HIGHEST_HZ / lowest).powf(i as f32 / rows as f32);
        let bin = ((hz / bin_hz).round() as usize).clamp(1, top.max(1));
        let edge = if i > 0 && distinct {
            bin.max(last + 1)
        } else {
            bin.max(last)
        };
        edges.push(edge);
        last = edge;
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

    /// A meter whose screen is watching.
    fn watched() -> (Levels, Meter) {
        let levels = Levels::new();
        levels.at(crate::clock::now_micros());
        let meter = Meter::new(48_000, levels.clone());
        (levels, meter)
    }

    fn loudest(levels: &[f32]) -> usize {
        levels
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap()
    }

    #[test]
    fn a_tone_lights_its_own_band_and_row() {
        let (levels, mut meter) = watched();
        meter.take(&tone(1000.0, 48_000, LONG, 0.1), 1_000_000);
        let frame = levels.at(1_000_000 + 80_000);
        let band = loudest(&frame.bands);
        let edges = edges(48_000, BLOCK, LOWEST_HZ, BANDS, true);
        let bin = (1000.0 / (48_000.0 / BLOCK as f32)).round() as usize;
        // The tone's bin, or the one beside it (the window spreads it a little).
        assert!(
            edges[band] <= bin + 1 && bin < edges[band + 1] + 1,
            "{band} {bin} {edges:?} {:?}",
            frame.bands
        );
        assert!(frame.bands[band] > 0.6, "{:?}", frame.bands);
        assert!(frame.bands[0] < 0.3 && frame.bands[BANDS - 1] < 0.3);
        // 1 kHz sits a little over halfway up the fine rows (30 Hz to 16 kHz).
        let row = loudest(&frame.fine);
        let expected = (FINE as f32 * (1000f32 / 30.0).ln() / (16_000f32 / 30.0).ln()) as usize;
        assert!(
            row.abs_diff(expected) <= 2,
            "row {row}, expected {expected}"
        );
        // The wave and loudness come through on both sides.
        assert!(frame.left.iter().any(|&s| s > 0.05) && frame.right.iter().any(|&s| s < -0.05));
        assert!(frame.loudness[0] > 0.4 && (frame.loudness[0] - frame.loudness[1]).abs() < 0.01);
    }

    #[test]
    fn levels_follow_the_moment_heard() {
        let (levels, mut meter) = watched();
        let block_us = (BLOCK as i64 * 1_000_000) / 48_000;
        let mut audio = tone(200.0, 48_000, BLOCK, 0.0);
        audio.extend(tone(200.0, 48_000, BLOCK, 0.8));
        meter.take(&audio, 0);
        assert!(
            levels.at(block_us / 2).bands.iter().all(|&b| b == 0.0),
            "quiet first"
        );
        assert!(
            levels.at(block_us + 1).bands.iter().any(|&b| b > 0.5),
            "then loud"
        );
        // Long after the last block: playback stopped, so nothing.
        assert_eq!(levels.at(block_us * 2 + STALE_US), Frame::default());
    }

    #[test]
    fn nothing_is_measured_while_no_screen_asks() {
        let levels = Levels::new();
        let mut meter = Meter::new(48_000, levels.clone());
        meter.take(&tone(200.0, 48_000, BLOCK * 2, 0.8), 0);
        assert!(levels.inner.lock().unwrap().frames.is_empty());
    }

    #[test]
    fn mains_stream_is_measured_and_waits_out_a_pause() {
        let levels = Levels::new();
        levels.at(crate::clock::now_micros());
        let mut meter = PieceMeter::new(48_000, levels.clone());
        assert!(levels.live());
        let piece = |mark| crate::Piece {
            mark,
            samples: Vec::new(),
        };
        meter.take(
            &crate::Piece {
                mark: crate::Mark::Audio {
                    frames: BLOCK as u32,
                    play_at_us: 1_000_000,
                },
                samples: tone(200.0, 48_000, BLOCK, 0.8),
            },
            900_000,
        );
        meter.take(&piece(crate::Mark::Pause), 950_000);
        assert_eq!(
            levels.at(1_000_000),
            Frame::default(),
            "nothing while paused"
        );
        // Half a second later it plays on: what was waiting is heard later.
        meter.take(&piece(crate::Mark::Resume), 1_450_000);
        assert!(levels.at(1_500_000).bands.iter().any(|&b| b > 0.5));
    }

    #[test]
    fn rows_cover_the_range_in_order() {
        for rate in [44_100, 48_000, 96_000] {
            let bands = edges(rate, BLOCK, LOWEST_HZ, BANDS, true);
            assert!(bands.windows(2).all(|w| w[1] > w[0]), "{rate}: {bands:?}");
            let fine = edges(rate, LONG, FINE_LOWEST_HZ, FINE, false);
            assert_eq!(fine.len(), FINE + 1);
            assert!(fine.windows(2).all(|w| w[1] >= w[0]), "{rate}: {fine:?}");
            assert!(*fine.last().unwrap() < LONG / 2 + 1);
        }
    }
}
