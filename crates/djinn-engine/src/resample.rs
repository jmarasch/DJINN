//! Streaming stereo resampling on top of rubato.
//!
//! The wrapper hides rubato's chunking: feed any number of frames, get
//! output aligned to the input (start-up delay trimmed) and, on
//! [`StreamResampler::finish`], exactly `round(input * ratio)` frames total.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Adjustable, Async, Fft, FixedAsync, FixedSync, Indexing, Resampler,
    SincInterpolationParameters, WindowFunction,
};

use crate::EngineError;

const CHANNELS: usize = 2;
const CHUNK: usize = 1024;

enum Inner {
    Fixed(Box<dyn Resampler<f32>>),
    Adjustable(Box<dyn Adjustable<f32>>),
}

impl Inner {
    fn get(&self) -> &dyn Resampler<f32> {
        match self {
            Self::Fixed(r) => r.as_ref(),
            Self::Adjustable(r) => r.as_ref(),
        }
    }

    fn get_mut(&mut self) -> &mut dyn Resampler<f32> {
        match self {
            Self::Fixed(r) => r.as_mut(),
            Self::Adjustable(r) => r.as_mut(),
        }
    }
}

pub struct StreamResampler {
    inner: Inner,
    from: u32,
    to: u32,
    /// Interleaved input not yet consumed by the resampler.
    pending: Vec<f32>,
    /// Scratch output, one chunk long.
    scratch: Vec<f32>,
    /// Output frames still to drop to cancel the resampler's delay.
    skip: usize,
    frames_in: u64,
    frames_out: u64,
}

impl StreamResampler {
    /// Fixed-ratio, high quality. For converting tracks to the master rate.
    pub fn fixed(from: u32, to: u32) -> Result<Self, EngineError> {
        let inner = Fft::<f32>::new(
            from as usize,
            to as usize,
            CHUNK,
            CHANNELS,
            FixedSync::Input,
        )
        .map_err(|e| EngineError::Resample(e.to_string()))?;
        Ok(Self::with(Inner::Fixed(Box::new(inner)), from, to))
    }

    /// Ratio adjustable at runtime by up to `max_relative`. For outputs,
    /// where drift correction nudges the ratio.
    pub fn adjustable(from: u32, to: u32, max_relative: f64) -> Result<Self, EngineError> {
        let params = SincInterpolationParameters::new(128, WindowFunction::BlackmanHarris2);
        let inner = Async::<f32>::new_sinc(
            f64::from(to) / f64::from(from),
            max_relative,
            &params,
            CHUNK,
            CHANNELS,
            FixedAsync::Input,
        )
        .map_err(|e| EngineError::Resample(e.to_string()))?;
        Ok(Self::with(Inner::Adjustable(Box::new(inner)), from, to))
    }

    fn with(inner: Inner, from: u32, to: u32) -> Self {
        let skip = inner.get().output_delay();
        let scratch = vec![0.0; inner.get().output_frames_max() * CHANNELS];
        Self {
            inner,
            from,
            to,
            pending: Vec::with_capacity(CHUNK * CHANNELS * 2),
            scratch,
            skip,
            frames_in: 0,
            frames_out: 0,
        }
    }

    /// Output frames that `input_frames` of input map to.
    pub fn output_frames_for(&self, input_frames: u64) -> u64 {
        let (from, to) = (u128::from(self.from), u128::from(self.to));
        let frames = (u128::from(input_frames) * to + from / 2) / from;
        u64::try_from(frames).unwrap_or(u64::MAX)
    }

    /// Resamples interleaved stereo `input`, appending output to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), EngineError> {
        self.frames_in += (input.len() / CHANNELS) as u64;
        self.pending.extend_from_slice(input);
        while self.pending.len() / CHANNELS >= self.inner.get().input_frames_next() {
            self.run_chunk(None, out)?;
        }
        Ok(())
    }

    /// Flushes buffered input and the resampler's delay line, so that the
    /// total output is exactly [`Self::output_frames_for`] the total input.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), EngineError> {
        let expected = self.output_frames_for(self.frames_in);
        while self.frames_out < expected {
            let valid = self.pending.len() / CHANNELS;
            self.run_chunk(Some(valid), out)?;
        }
        let excess = usize::try_from(self.frames_out - expected).unwrap_or(0);
        out.truncate(out.len().saturating_sub(excess * CHANNELS));
        self.frames_out = expected;
        Ok(())
    }

    /// Sets the ratio relative to the nominal one (above 1.0 makes more
    /// output), ramping smoothly over the next chunk.
    pub fn set_ratio_relative(&mut self, relative: f64) -> Result<(), EngineError> {
        match &mut self.inner {
            Inner::Adjustable(r) => r
                .set_resample_ratio_relative(relative, true)
                .map_err(|e| EngineError::Resample(e.to_string())),
            Inner::Fixed(_) => Err(EngineError::Resample("fixed-ratio resampler".to_owned())),
        }
    }

    /// Input frames waiting for a full chunk.
    pub fn pending_frames(&self) -> usize {
        self.pending.len() / CHANNELS
    }

    /// Drops all buffered audio and starts over.
    pub fn reset(&mut self) {
        self.inner.get_mut().reset();
        self.pending.clear();
        self.skip = self.inner.get().output_delay();
        self.frames_in = 0;
        self.frames_out = 0;
    }

    /// Runs one chunk. With `partial`, only that many pending frames are
    /// valid and the rest of the chunk is silence.
    fn run_chunk(&mut self, partial: Option<usize>, out: &mut Vec<f32>) -> Result<(), EngineError> {
        let need = self.inner.get().input_frames_next();
        if self.pending.len() < need * CHANNELS {
            self.pending.resize(need * CHANNELS, 0.0);
        }
        let input = InterleavedSlice::new(&self.pending, CHANNELS, need)
            .map_err(|e| EngineError::Resample(e.to_string()))?;
        let capacity = self.scratch.len() / CHANNELS;
        let mut output = InterleavedSlice::new_mut(&mut self.scratch, CHANNELS, capacity)
            .map_err(|e| EngineError::Resample(e.to_string()))?;
        let indexing = partial.map(|n| Indexing::new().partial_len(n));
        let (_, written) = self
            .inner
            .get_mut()
            .process_into_buffer(&input, &mut output, indexing.as_ref())
            .map_err(|e| EngineError::Resample(e.to_string()))?;

        let consumed = match partial {
            Some(n) => n,
            None => need,
        };
        self.pending.drain(..consumed * CHANNELS);
        if partial.is_some() {
            self.pending.clear();
        }

        let skip = self.skip.min(written);
        self.skip -= skip;
        out.extend_from_slice(&self.scratch[skip * CHANNELS..written * CHANNELS]);
        self.frames_out += (written - skip) as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine(rate: u32, freq: f32, frames: usize, offset: usize) -> Vec<f32> {
        (offset..offset + frames)
            .flat_map(|i| {
                let s = (TAU * freq * i as f32 / rate as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn exact_length_after_finish() {
        for (from, to, frames) in [
            (44_100, 48_000, 44_100),
            (96_000, 48_000, 12_345),
            (48_000, 44_100, 7),
        ] {
            let mut r = StreamResampler::fixed(from, to).unwrap();
            let mut out = Vec::new();
            // Feed in odd-sized pieces to exercise chunking.
            let input = sine(from, 440.0, frames, 0);
            for piece in input.chunks(2 * 333) {
                r.process(piece, &mut out).unwrap();
            }
            r.finish(&mut out).unwrap();
            assert_eq!(
                out.len() / 2,
                r.output_frames_for(frames as u64) as usize,
                "{from}->{to}"
            );
        }
    }

    #[test]
    fn output_is_aligned_with_input() {
        // A resampled sine must line up with the ideal sine at the new rate,
        // which only holds if the start-up delay was trimmed correctly.
        let mut r = StreamResampler::fixed(44_100, 48_000).unwrap();
        let mut out = Vec::new();
        r.process(&sine(44_100, 440.0, 44_100, 0), &mut out)
            .unwrap();
        r.finish(&mut out).unwrap();
        let ideal = sine(48_000, 440.0, out.len() / 2, 0);
        // Ignore the edges, where the filter sees silence beyond the signal.
        let max_err = out[4000..out.len() - 4000]
            .iter()
            .zip(&ideal[4000..ideal.len() - 4000])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(max_err < 1e-3, "max error {max_err}");
    }
}
