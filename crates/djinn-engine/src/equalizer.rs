//! Five-band equalizer on the engine thread, before speaker and stream fan-out.

use std::f64::consts::PI;

pub const BAND_HZ: [u32; 5] = [60, 250, 1000, 4000, 12000];
pub const MAX_BAND_DB: f32 = 12.0;
const Q: f64 = 1.0;
const FADE_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqualizerConfig {
    pub enabled: bool,
    pub bands_db: [f32; BAND_HZ.len()],
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bands_db: [0.0; BAND_HZ.len()],
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

    fn magnitude(&self, omega: f64) -> f64 {
        let cosine = omega.cos();
        let cosine2 = (2.0 * omega).cos();
        let sine = omega.sin();
        let sine2 = (2.0 * omega).sin();
        let numerator = (self.b0 + self.b1 * cosine + self.b2 * cosine2).powi(2)
            + (self.b1 * sine + self.b2 * sine2).powi(2);
        let denominator = (1.0 + self.a1 * cosine + self.a2 * cosine2).powi(2)
            + (self.a1 * sine + self.a2 * sine2).powi(2);
        (numerator / denominator).sqrt()
    }
}

struct Bank {
    filters: [Biquad; BAND_HZ.len()],
    active: bool,
    headroom: f64,
}

impl Bank {
    fn new(rate: u32, config: EqualizerConfig) -> Self {
        let filters = std::array::from_fn(|i| {
            let gain = config.bands_db[i];
            Biquad::peak(
                rate,
                BAND_HZ[i],
                if gain.is_finite() {
                    gain.clamp(-MAX_BAND_DB, MAX_BAND_DB)
                } else {
                    0.0
                },
            )
        });
        let active = config.enabled && config.bands_db.iter().any(|gain| *gain != 0.0);
        let headroom = if active {
            // Compute the filter cascade's largest frequency gain once when
            // settings change, then leave that much digital headroom.
            let peak = (0..1024)
                .map(|step| {
                    let omega = PI * (step as f64 + 0.5) / 1024.0;
                    filters
                        .iter()
                        .map(|filter| filter.magnitude(omega))
                        .product::<f64>()
                })
                .fold(1.0_f64, f64::max);
            1.0 / peak
        } else {
            1.0
        };
        Self {
            filters,
            active,
            headroom,
        }
    }

    fn process(&mut self, input: f32, channel: usize) -> f32 {
        if !self.active {
            return input;
        }
        let mut sample = f64::from(input);
        for filter in &mut self.filters {
            sample = filter.process(sample, channel);
        }
        (sample * self.headroom).clamp(-1.0, 1.0) as f32
    }
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
            bands_db: [9.0, 0.0, 0.0, 0.0, 0.0],
        };
        let bass = rms_after_filter(60.0, config);
        let mid = rms_after_filter(1_000.0, config);
        assert!(bass > mid * 2.0, "bass={bass}, mid={mid}");
    }
}
