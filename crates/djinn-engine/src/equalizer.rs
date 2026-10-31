//! Twelve-band equalizer on the engine thread, before speaker and stream fan-out.

use std::f64::consts::PI;

pub const BAND_HZ: [u32; 12] = [
    32, 64, 125, 250, 500, 1000, 2000, 4000, 6000, 8000, 12000, 16000,
];
pub const MAX_BAND_DB: f32 = 12.0;
pub const MIN_OUTPUT_GAIN_DB: f32 = -12.0;
pub const MAX_OUTPUT_GAIN_DB: f32 = 6.0;
const Q: f64 = 1.4;
const FADE_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualizerConfig {
    pub enabled: bool,
    pub bands_db: [f32; BAND_HZ.len()],
    pub output_gain_db: f32,
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bands_db: [0.0; BAND_HZ.len()],
            output_gain_db: 0.0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Channel {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    channels: [Channel; 2],
}

impl Biquad {
    // Peaking filter coefficients from the W3C Audio EQ Cookbook, with a0
    // normalized to 1. https://www.w3.org/TR/audio-eq-cookbook/
    fn peak(rate: u32, frequency: u32, gain_db: f32) -> Self {
        let rate = f64::from(rate.max(1));
        let frequency = f64::from(frequency).min(rate * 0.45);
        let omega = 2.0 * PI * frequency / rate;
        let alpha = omega.sin() / (2.0 * Q);
        let cosine = omega.cos();
        let a = 10.0_f64.powf(f64::from(gain_db) / 40.0);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cosine / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cosine / a0,
            a2: (1.0 - alpha / a) / a0,
            channels: [Channel::default(); 2],
        }
    }

    fn process(&mut self, input: f64, channel: usize) -> f64 {
        let state = &mut self.channels[channel];
        let output = self.b0 * input + self.b1 * state.x1 + self.b2 * state.x2
            - self.a1 * state.y1
            - self.a2 * state.y2;
        state.x2 = state.x1;
        state.x1 = input;
        state.y2 = state.y1;
        state.y1 = output;
        output
    }
}

struct Bank {
    filters: [Option<Biquad>; BAND_HZ.len()],
    active: bool,
    output_gain: f64,
}

impl Bank {
    fn new(rate: u32, config: EqualizerConfig) -> Self {
        let filters = std::array::from_fn(|i| {
            let gain = config.bands_db[i];
            (gain.is_finite() && gain != 0.0)
                .then(|| Biquad::peak(rate, BAND_HZ[i], gain.clamp(-MAX_BAND_DB, MAX_BAND_DB)))
        });
        let active =
            config.enabled && (filters.iter().any(Option::is_some) || config.output_gain_db != 0.0);
        let gain = if config.output_gain_db.is_finite() {
            config
                .output_gain_db
                .clamp(MIN_OUTPUT_GAIN_DB, MAX_OUTPUT_GAIN_DB)
        } else {
            0.0
        };
        Self {
            filters,
            active,
            output_gain: 10.0_f64.powf(f64::from(gain) / 20.0),
        }
    }

    fn process(&mut self, input: f32, channel: usize) -> f32 {
        if !self.active {
            return input;
        }
        let mut sample = f64::from(input);
        for filter in self.filters.iter_mut().flatten() {
            sample = filter.process(sample, channel);
        }
        soft_limit(sample * self.output_gain)
    }
}

// Preserve ordinary signal levels. Only peaks above 90% of full scale
// approach the digital ceiling smoothly, rather than turning every band
// boost into a quieter whole mix.
fn soft_limit(sample: f64) -> f32 {
    if !sample.is_finite() {
        return 0.0;
    }
    let magnitude = sample.abs();
    let limited = if magnitude <= 0.9 {
        magnitude
    } else {
        0.9 + 0.1 * ((magnitude - 0.9) / 0.1).tanh()
    };
    (sample.signum() * limited) as f32
}

pub(crate) struct Equalizer {
    rate: u32,
    config: EqualizerConfig,
    current: Bank,
    next: Option<Bank>,
    fade_left: usize,
}

impl Equalizer {
    pub fn new(rate: u32) -> Self {
        let config = EqualizerConfig::default();
        Self {
            rate,
            config,
            current: Bank::new(rate, config),
            next: None,
            fade_left: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.current.active || self.next.is_some()
    }

    pub fn set(&mut self, config: EqualizerConfig) {
        if self.config == config {
            return;
        }
        if let Some(next) = self.next.take() {
            self.current = next;
        }
        self.config = config;
        self.next = Some(Bank::new(self.rate, config));
        self.fade_left = FADE_FRAMES;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.as_chunks_mut::<2>().0 {
            let left = frame[0];
            let right = frame[1];
            if let Some(next) = self.next.as_mut() {
                let mix = 1.0 - self.fade_left as f32 / FADE_FRAMES as f32;
                frame[0] =
                    self.current.process(left, 0) * (1.0 - mix) + next.process(left, 0) * mix;
                frame[1] =
                    self.current.process(right, 1) * (1.0 - mix) + next.process(right, 1) * mix;
                self.fade_left -= 1;
                if self.fade_left == 0 {
                    if let Some(next) = self.next.take() {
                        self.current = next;
                    }
                }
            } else {
                frame[0] = self.current.process(left, 0);
                frame[1] = self.current.process(right, 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_bit_exact() {
        let mut eq = Equalizer::new(48_000);
        let mut samples = [0.25, -0.5, 0.75, -0.125];
        eq.process(&mut samples);
        assert_eq!(samples, [0.25, -0.5, 0.75, -0.125]);
    }

    #[test]
    fn enabled_flat_is_bit_exact() {
        let mut eq = Equalizer::new(48_000);
        eq.set(EqualizerConfig {
            enabled: true,
            ..EqualizerConfig::default()
        });
        let mut samples = [0.25, -0.5, 0.75, -0.125];
        eq.process(&mut samples);
        assert_eq!(samples, [0.25, -0.5, 0.75, -0.125]);
    }

    fn rms_after_filter(frequency: f64, config: EqualizerConfig) -> f64 {
        let mut eq = Equalizer::new(48_000);
        eq.set(config);
        let mut sum = 0.0;
        for frame in 0..48_000 {
            let input = (2.0 * PI * frequency * f64::from(frame) / 48_000.0).sin() as f32 * 0.3;
            let mut stereo = [input, 0.0];
            eq.process(&mut stereo);
            assert_eq!(stereo[1], 0.0, "channels stay separate");
            assert!(stereo[0].is_finite() && stereo[0].abs() <= 1.0);
            if frame > 9_600 {
                sum += f64::from(stereo[0]).powi(2);
            }
        }
        (sum / 38_399.0).sqrt()
    }

    #[test]
    fn bass_boost_changes_the_spectrum_without_clipping() {
        let config = EqualizerConfig {
            enabled: true,
            bands_db: [0.0, 9.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            output_gain_db: 0.0,
        };
        let bass = rms_after_filter(64.0, config);
        let mid = rms_after_filter(1_000.0, config);
        assert!(bass > mid * 2.0, "bass={bass}, mid={mid}");
        let flat_mid = rms_after_filter(1_000.0, EqualizerConfig::default());
        assert!(
            (mid - flat_mid).abs() < flat_mid * 0.1,
            "midrange should stay near its original level"
        );
    }

    #[test]
    fn output_gain_restores_level_and_soft_limit_bounds_peaks() {
        let config = EqualizerConfig {
            enabled: true,
            bands_db: [0.0; BAND_HZ.len()],
            output_gain_db: 6.0,
        };
        let boosted = rms_after_filter(1_000.0, config);
        let flat = rms_after_filter(1_000.0, EqualizerConfig::default());
        assert!(boosted > flat * 1.8);
        assert!(soft_limit(100.0) <= 1.0);
        assert!(soft_limit(-100.0) >= -1.0);
    }
}
