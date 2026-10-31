//! What a song sounds like, as numbers: tempo, punch, brightness,
//! noisiness, dynamics, mode, and compact timbre and harmony profiles.
//! Songs close together on these sound alike, which is what "more like
//! this" and song radio are built on.
//!
//! Everything here is standard music information retrieval (short-time
//! spectra, onset autocorrelation, mel cepstra, chroma), computed on a
//! mono 22.05 kHz copy of the song.

use std::f32::consts::PI;
use std::path::Path;
use std::sync::Arc;

use ebur128::{EbuR128, Mode};
use realfft::{RealFftPlanner, RealToComplex};

use crate::decoder::TrackDecoder;
use crate::loudness::Loudness;
use crate::resample::StreamResampler;
use crate::EngineError;

/// Bumped when the features change meaning, so songs get analyzed again.
pub const FEATURES_VERSION: i64 = 1;
/// Length of [`Features::values`].
pub const FEATURE_COUNT: usize = 37;

const RATE: u32 = 22_050;
/// Spectral frames: about 93 ms, half overlapping.
const FRAME: usize = 2048;
const HOP: usize = 1024;
/// Onset frames, finer in time for tempo.
const ONSET_FRAME: usize = 1024;
const ONSET_HOP: usize = 256;
/// Long files (DJ mixes) are judged on their first stretch.
const MAX_SECS: usize = 8 * 60;
const MEL_BANDS: usize = 40;
const MFCCS: usize = 12;
/// Frames quieter than this don't describe the sound.
const SILENCE_DB: f32 = -60.0;

/// The numbers describing one song. See [`describe`] for what each is.
#[derive(Clone, Debug, PartialEq)]
pub struct Features {
    pub values: Vec<f32>,
    /// Estimated tempo, also at `values[0]` as log2.
    pub bpm: f32,
}

/// What each value is, for people reading the database or tuning weights.
pub fn describe() -> [&'static str; FEATURE_COUNT] {
    let mut names = [""; FEATURE_COUNT];
    let head = [
        "tempo (log2 bpm)",
        "punch (onset strength)",
        "brightness (log centroid)",
        "brightness spread",
        "rolloff (log Hz)",
        "rolloff spread",
        "noisiness (flatness)",
        "noisiness spread",
        "zero crossings",
        "dynamics (dB spread)",
        "loudness (dB)",
        "major vs minor",
        "key clarity",
    ];
    names[..head.len()].copy_from_slice(&head);
    const MFCC: [&str; MFCCS] = [
        "timbre 1",
        "timbre 2",
        "timbre 3",
        "timbre 4",
        "timbre 5",
        "timbre 6",
        "timbre 7",
        "timbre 8",
        "timbre 9",
        "timbre 10",
        "timbre 11",
        "timbre 12",
    ];
    const CHROMA: [&str; 12] = [
        "harmony 0",
        "harmony 1",
        "harmony 2",
        "harmony 3",
        "harmony 4",
        "harmony 5",
        "harmony 6",
        "harmony 7",
        "harmony 8",
        "harmony 9",
        "harmony 10",
        "harmony 11",
    ];
    names[13..25].copy_from_slice(&MFCC);
    names[25..37].copy_from_slice(&CHROMA);
    names
}

/// Both measurements from one decode.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    /// `None` for silence.
    pub loudness: Option<Loudness>,
    /// `None` for silence or very short files.
    pub features: Option<Features>,
}

/// Decodes the file once and measures its loudness and sound.
pub fn analyze(path: &Path) -> Result<Analysis, EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let rate = decoder.sample_rate();
    let meter_error = |e: ebur128::Error| EngineError::Track {
        path: path.to_path_buf(),
        reason: format!("loudness meter: {e}"),
    };
    let mut meter = EbuR128::new(2, rate, Mode::I | Mode::SAMPLE_PEAK).map_err(meter_error)?;
    let mut resampler = if rate == RATE {
        None
    } else {
        Some(StreamResampler::fixed(rate, RATE)?)
    };
    let mut mono = Vec::new();
    let mut stereo = Vec::new();
    let mut staged = Vec::new();
    let max = MAX_SECS * RATE as usize;
    loop {
        stereo.clear();
        if !decoder.decode_next(&mut stereo)? {
            break;
        }
        meter.add_frames_f32(&stereo).map_err(meter_error)?;
        if mono.len() >= max {
            continue;
        }
        staged.clear();
        match resampler.as_mut() {
            Some(r) => r.process(&stereo, &mut staged)?,
            None => staged.extend_from_slice(&stereo),
        }
        mono.extend(staged.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5));
    }
    if let Some(r) = resampler.as_mut() {
        if mono.len() < max {
            staged.clear();
            r.finish(&mut staged)?;
            mono.extend(staged.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5));
        }
    }
    mono.truncate(max);

    let lufs = meter.loudness_global().map_err(meter_error)?;
    let peak = (0..2)
        .filter_map(|c| meter.sample_peak(c).ok())
        .fold(0.0, f64::max);
    let loudness = lufs.is_finite().then_some(Loudness { lufs, peak });
    Ok(Analysis {
        features: features(&mono, loudness.map(|l| l.lufs as f32)),
        loudness,
    })
}

/// Decodes a whole file to interleaved stereo, a chunk at a time (for
/// converting it to another format). `rate_for` picks the output rate from
/// the file's own; `on_chunk` gets the rate and each chunk, and returns
/// false to stop early.
pub fn decode_stereo(
    path: &Path,
    rate_for: impl Fn(u32) -> u32,
    on_chunk: &mut dyn FnMut(u32, &[f32]) -> bool,
) -> Result<(), EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let source = decoder.sample_rate();
    let rate = rate_for(source);
    let mut resampler = if source == rate {
        None
    } else {
        Some(StreamResampler::fixed(source, rate)?)
    };
    let (mut stereo, mut staged) = (Vec::new(), Vec::new());
    loop {
        stereo.clear();
        if !decoder.decode_next(&mut stereo)? {
            if let Some(r) = resampler.as_mut() {
                staged.clear();
                r.finish(&mut staged)?;
                if !staged.is_empty() {
                    on_chunk(rate, &staged);
                }
            }
            return Ok(());
        }
        let chunk = match resampler.as_mut() {
            Some(r) => {
                staged.clear();
                r.process(&stereo, &mut staged)?;
                &staged
            }
            None => &stereo,
        };
        if !chunk.is_empty() && !on_chunk(rate, chunk) {
            return Ok(());
        }
    }
}

/// Decodes up to `max_secs` of a file to mono at `rate` (for models that
/// listen to songs).
pub fn decode_mono(path: &Path, rate: u32, max_secs: u32) -> Result<Vec<f32>, EngineError> {
    let mut decoder = TrackDecoder::open(path)?;
    let source = decoder.sample_rate();
    let mut resampler = if source == rate {
        None
    } else {
        Some(StreamResampler::fixed(source, rate)?)
    };
    let max = max_secs as usize * rate as usize;
    let (mut mono, mut stereo, mut staged) = (Vec::new(), Vec::new(), Vec::new());
    while mono.len() < max {
        stereo.clear();
        if !decoder.decode_next(&mut stereo)? {
            if let Some(r) = resampler.as_mut() {
                staged.clear();
                r.finish(&mut staged)?;
                mono.extend(staged.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5));
            }
            break;
        }
        staged.clear();
        match resampler.as_mut() {
            Some(r) => r.process(&stereo, &mut staged)?,
            None => staged.extend_from_slice(&stereo),
        }
        mono.extend(staged.as_chunks::<2>().0.iter().map(|[l, r]| (l + r) * 0.5));
    }
    mono.truncate(max);
    Ok(mono)
}

/// Features of mono audio at 22.05 kHz.
fn features(mono: &[f32], lufs: Option<f32>) -> Option<Features> {
    if mono.len() < FRAME * 8 {
        return None;
    }
    let spectral = Spectral::new().run(mono)?;
    let bpm = tempo(mono);
    let (mode, clarity, chroma) = harmony(&spectral.chroma);
    let mut values = Vec::with_capacity(FEATURE_COUNT);
    values.push(bpm.log2());
    values.push((1.0 + spectral.flux).ln());
    values.push(spectral.centroid.mean().max(1.0).ln());
    values.push(spectral.centroid.relative_spread());
    values.push(spectral.rolloff.mean().max(1.0).ln());
    values.push(spectral.rolloff.relative_spread());
    values.push(spectral.flatness.mean());
    values.push(spectral.flatness.spread());
    values.push(spectral.zcr.mean());
    values.push(spectral.rms_db.spread());
    values.push(lufs.unwrap_or_else(|| spectral.rms_db.mean()));
    values.push(mode);
    values.push(clarity);
    values.extend(spectral.mfcc.iter().map(Stat::mean));
    values.extend(chroma);
    debug_assert_eq!(values.len(), FEATURE_COUNT);
    values
        .iter()
        .all(|v| v.is_finite())
        .then_some(Features { values, bpm })
}

/// Running mean and variance (Welford).
#[derive(Clone, Copy, Default)]
struct Stat {
    n: f64,
    mean: f64,
    m2: f64,
}

impl Stat {
    fn add(&mut self, x: f32) {
        let x = f64::from(x);
        self.n += 1.0;
        let delta = x - self.mean;
        self.mean += delta / self.n;
        self.m2 += delta * (x - self.mean);
    }

    fn mean(&self) -> f32 {
        self.mean as f32
    }

    fn spread(&self) -> f32 {
        if self.n < 2.0 {
            0.0
        } else {
            (self.m2 / (self.n - 1.0)).sqrt() as f32
        }
    }

    /// Spread relative to the mean, so it doesn't just echo the mean.
    fn relative_spread(&self) -> f32 {
        self.spread() / self.mean().abs().max(1e-6)
    }
}

struct SpectralResult {
    centroid: Stat,
    rolloff: Stat,
    flatness: Stat,
    zcr: Stat,
    rms_db: Stat,
    flux: f32,
    mfcc: [Stat; MFCCS],
    /// Energy per pitch class, C first.
    chroma: [f32; 12],
}

struct Spectral {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// Triangular mel filters over the spectrum bins: (first bin, weights).
    mel: Vec<(usize, Vec<f32>)>,
    /// DCT-II rows for MFCC 1..=12.
    dct: Vec<[f32; MEL_BANDS]>,
    /// Pitch class of each bin, for bins between 60 Hz and 5 kHz.
    pitch_class: Vec<Option<usize>>,
}

impl Spectral {
    fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FRAME);
        let bins = FRAME / 2 + 1;
        let hz = |bin: usize| bin as f32 * RATE as f32 / FRAME as f32;
        let mel_of = |f: f32| 2595.0 * (1.0 + f / 700.0).log10();
        let hz_of = |m: f32| 700.0 * (10f32.powf(m / 2595.0) - 1.0);
        let (low, high) = (mel_of(64.0), mel_of(8_000.0));
        let edges: Vec<f32> = (0..MEL_BANDS + 2)
            .map(|i| hz_of(low + (high - low) * i as f32 / (MEL_BANDS + 1) as f32))
            .collect();
        let mel = (0..MEL_BANDS)
            .map(|b| {
                let (lo, mid, hi) = (edges[b], edges[b + 1], edges[b + 2]);
                let first = (0..bins).find(|&k| hz(k) >= lo).unwrap_or(bins - 1);
                let weights: Vec<f32> = (first..bins)
                    .take_while(|&k| hz(k) <= hi)
                    .map(|k| {
                        let f = hz(k);
                        if f <= mid {
                            (f - lo) / (mid - lo)
                        } else {
                            (hi - f) / (hi - mid)
                        }
                    })
                    .collect();
                (first, weights)
            })
            .collect();
        let dct = (1..=MFCCS)
            .map(|c| {
                let mut row = [0.0; MEL_BANDS];
                for (b, value) in row.iter_mut().enumerate() {
                    *value = (PI * c as f32 * (b as f32 + 0.5) / MEL_BANDS as f32).cos();
                }
                row
            })
            .collect();
        let pitch_class = (0..bins)
            .map(|k| {
                let f = hz(k);
                (60.0..5_000.0).contains(&f).then(|| {
                    // MIDI note 69 is A4 = 440 Hz; pitch class 0 is C.
                    let note = 69.0 + 12.0 * (f / 440.0).log2();
                    (note.round() as i64).rem_euclid(12) as usize
                })
            })
            .collect();
        Self {
            fft,
            window: hann(FRAME),
            mel,
            dct,
            pitch_class,
        }
    }

    fn run(&self, mono: &[f32]) -> Option<SpectralResult> {
        let mut input = self.fft.make_input_vec();
        let mut output = self.fft.make_output_vec();
        let mut power = vec![0.0f32; FRAME / 2 + 1];
        let mut previous = vec![0.0f32; FRAME / 2 + 1];
        let mut result = SpectralResult {
            centroid: Stat::default(),
            rolloff: Stat::default(),
            flatness: Stat::default(),
            zcr: Stat::default(),
            rms_db: Stat::default(),
            flux: 0.0,
            mfcc: [Stat::default(); MFCCS],
            chroma: [0.0; 12],
        };
        let mut flux = Stat::default();
        let bin_hz = RATE as f32 / FRAME as f32;
        let mut start = 0;
        while start + FRAME <= mono.len() {
            let frame = &mono[start..start + FRAME];
            start += HOP;
            let mean_square = frame.iter().map(|s| s * s).sum::<f32>() / FRAME as f32;
            let rms_db = 10.0 * mean_square.max(1e-12).log10();
            if rms_db < SILENCE_DB {
                continue;
            }
            result.rms_db.add(rms_db);
            let crossings = frame
                .windows(2)
                .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
                .count();
            result.zcr.add(crossings as f32 / FRAME as f32);

            for ((dst, src), w) in input.iter_mut().zip(frame).zip(&self.window) {
                *dst = src * w;
            }
            if self.fft.process(&mut input, &mut output).is_err() {
                return None;
            }
            for (p, c) in power.iter_mut().zip(&output) {
                *p = c.norm_sqr();
            }
            let total: f32 = power[1..].iter().sum();
            if total <= 0.0 {
                continue;
            }
            let centroid = power[1..]
                .iter()
                .enumerate()
                .map(|(k, p)| (k + 1) as f32 * bin_hz * p)
                .sum::<f32>()
                / total;
            result.centroid.add(centroid);
            let mut cumulative = 0.0;
            let rolloff_bin = power[1..]
                .iter()
                .position(|p| {
                    cumulative += p;
                    cumulative >= 0.85 * total
                })
                .unwrap_or(0)
                + 1;
            result.rolloff.add(rolloff_bin as f32 * bin_hz);
            let n = (power.len() - 1) as f32;
            let log_mean = power[1..].iter().map(|p| (p + 1e-12).ln()).sum::<f32>() / n;
            result.flatness.add(log_mean.exp() / (total / n));

            // Onset strength: how much louder the spectrum got.
            let mut rise = 0.0;
            for (now, before) in power.iter().zip(previous.iter_mut()) {
                let level = (1.0 + 100.0 * now.sqrt()).ln();
                rise += (level - *before).max(0.0);
                *before = level;
            }
            flux.add(rise / n);

            let mut mel = [0.0f32; MEL_BANDS];
            for (band, (first, weights)) in mel.iter_mut().zip(&self.mel) {
                let energy: f32 = weights
                    .iter()
                    .zip(&power[*first..])
                    .map(|(w, p)| w * p)
                    .sum();
                *band = (energy + 1e-10).ln();
            }
            for (stat, row) in result.mfcc.iter_mut().zip(&self.dct) {
                stat.add(row.iter().zip(&mel).map(|(d, m)| d * m).sum::<f32>() / MEL_BANDS as f32);
            }
            for (class, p) in self.pitch_class.iter().zip(&power) {
                if let Some(class) = class {
                    result.chroma[*class] += p.sqrt();
                }
            }
        }
        if result.rms_db.n < 8.0 {
            return None;
        }
        result.flux = flux.mean();
        Some(result)
    }
}

fn hann(len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / len as f32).cos())
        .collect()
}

/// Tempo in BPM from the autocorrelation of the onset envelope, favoring
/// tempos near 120 the way people tap along.
fn tempo(mono: &[f32]) -> f32 {
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(ONSET_FRAME);
    let window = hann(ONSET_FRAME);
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let mut previous = vec![0.0f32; ONSET_FRAME / 2 + 1];
    let mut envelope = Vec::with_capacity(mono.len() / ONSET_HOP);
    let mut start = 0;
    while start + ONSET_FRAME <= mono.len() {
        for ((dst, src), w) in input.iter_mut().zip(&mono[start..]).zip(&window) {
            *dst = src * w;
        }
        start += ONSET_HOP;
        if fft.process(&mut input, &mut output).is_err() {
            break;
        }
        let mut rise = 0.0;
        for (c, before) in output.iter().zip(previous.iter_mut()) {
            let level = (1.0 + 100.0 * c.norm()).ln();
            rise += (level - *before).max(0.0);
            *before = level;
        }
        envelope.push(rise);
    }
    if envelope.len() < 64 {
        return 120.0;
    }
    // Remove the slow trend so the autocorrelation sees beats, not level.
    let span = 16;
    let detrended: Vec<f32> = (0..envelope.len())
        .map(|i| {
            let lo = i.saturating_sub(span);
            let hi = (i + span + 1).min(envelope.len());
            let local = envelope[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
            (envelope[i] - local).max(0.0)
        })
        .collect();
    let rate = RATE as f32 / ONSET_HOP as f32;
    let lag_of = |bpm: f32| 60.0 * rate / bpm;
    let (min_lag, max_lag) = (lag_of(200.0).floor() as usize, lag_of(55.0).ceil() as usize);
    let autocorr = |lag: usize| -> f32 {
        detrended
            .iter()
            .zip(&detrended[lag.min(detrended.len())..])
            .map(|(a, b)| a * b)
            .sum::<f32>()
            / (detrended.len() - lag.min(detrended.len())).max(1) as f32
    };
    let scores: Vec<f32> = (min_lag..=max_lag + 1).map(autocorr).collect();
    let weighted = |i: usize| {
        let bpm = 60.0 * rate / (min_lag + i) as f32;
        let prior = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp();
        scores[i] * prior
    };
    let best = (1..scores.len() - 1)
        .max_by(|&a, &b| weighted(a).total_cmp(&weighted(b)))
        .unwrap_or(1);
    // Parabolic interpolation around the peak for a finer lag.
    let (a, b, c) = (scores[best - 1], scores[best], scores[best + 1]);
    let denom = a - 2.0 * b + c;
    let offset = if denom.abs() > 1e-9 {
        (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    (60.0 * rate / ((min_lag + best) as f32 + offset)).clamp(55.0, 200.0)
}

/// Mode (major minus minor fit), key clarity, and the chroma profile
/// turned so its key comes first, so songs in different keys compare by
/// harmony rather than by pitch.
fn harmony(chroma: &[f32; 12]) -> (f32, f32, [f32; 12]) {
    // Krumhansl-Kessler key profiles.
    const MAJOR: [f32; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const MINOR: [f32; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    let total: f32 = chroma.iter().sum();
    if total <= 0.0 {
        return (0.0, 0.0, [0.0; 12]);
    }
    let normalized: Vec<f32> = chroma.iter().map(|c| c / total).collect();
    let correlate = |profile: &[f32; 12], key: usize| -> f32 {
        let rotated: Vec<f32> = (0..12).map(|i| profile[(i + 12 - key) % 12]).collect();
        pearson(&normalized, &rotated)
    };
    let (mut major, mut minor, mut tonic) = (f32::MIN, f32::MIN, 0);
    for key in 0..12 {
        let (maj, min) = (correlate(&MAJOR, key), correlate(&MINOR, key));
        if maj.max(min) > major.max(minor) {
            tonic = key;
        }
        major = major.max(maj);
        minor = minor.max(min);
    }
    let mut turned = [0.0; 12];
    for (i, value) in turned.iter_mut().enumerate() {
        *value = normalized[(i + tonic) % 12];
    }
    (major - minor, major.max(minor), turned)
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len() as f32;
    let (ma, mb) = (a.iter().sum::<f32>() / n, b.iter().sum::<f32>() / n);
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        num += (x - ma) * (y - mb);
        da += (x - ma).powi(2);
        db += (y - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize)
            .map(|i| 0.3 * (2.0 * PI * freq * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn noise(secs: f32) -> Vec<f32> {
        // A small LCG: deterministic and good enough for white-ish noise.
        let mut state = 12345u32;
        (0..(secs * RATE as f32) as usize)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((state >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 0.6
            })
            .collect()
    }

    fn clicks(bpm: f32, secs: f32) -> Vec<f32> {
        let period = (60.0 / bpm * RATE as f32) as usize;
        let mut out = noise(secs).iter().map(|n| n * 0.02).collect::<Vec<_>>();
        for start in (0..out.len()).step_by(period) {
            for (i, sample) in out.iter_mut().skip(start).take(400).enumerate() {
                *sample += 0.8
                    * (2.0 * PI * 1000.0 * i as f32 / RATE as f32).sin()
                    * (1.0 - i as f32 / 400.0);
            }
        }
        out
    }

    #[test]
    fn finds_the_tempo_of_a_click_track() {
        for bpm in [90.0, 120.0, 128.0, 150.0] {
            let found = tempo(&clicks(bpm, 30.0));
            assert!((found - bpm).abs() < 2.0, "{bpm}: found {found}");
        }
    }

    #[test]
    fn tells_tones_from_noise_and_bright_from_dark() {
        let low = features(&tone(220.0, 10.0), None).unwrap();
        let high = features(&tone(3_000.0, 10.0), None).unwrap();
        let hiss = features(&noise(10.0), None).unwrap();
        assert_eq!(low.values.len(), FEATURE_COUNT);
        // Brightness (log centroid) rises with pitch.
        assert!(high.values[2] > low.values[2] + 1.0);
        // Noise is far flatter than a tone.
        assert!(hiss.values[6] > low.values[6] * 10.0 + 0.1);
        // And crosses zero far more often.
        assert!(hiss.values[8] > low.values[8] * 5.0);
    }

    #[test]
    fn hears_major_and_minor() {
        let chord = |notes: &[f32]| -> Vec<f32> {
            let mut out = vec![0.0; 10 * RATE as usize];
            for &f in notes {
                for (o, t) in out.iter_mut().zip(tone(f, 10.0)) {
                    *o += t / notes.len() as f32;
                }
            }
            out
        };
        // C major (C E G) against C minor (C Eb G).
        let major = features(&chord(&[261.63, 329.63, 392.0]), None).unwrap();
        let minor = features(&chord(&[261.63, 311.13, 392.0]), None).unwrap();
        assert!(major.values[11] > minor.values[11]);
    }

    #[test]
    fn silence_and_tiny_files_have_no_features() {
        assert!(features(&vec![0.0; 10 * RATE as usize], None).is_none());
        assert!(features(&tone(440.0, 0.1), None).is_none());
    }
}
