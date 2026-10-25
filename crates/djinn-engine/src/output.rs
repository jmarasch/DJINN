//! A cpal output device fed through a lock-free ring buffer.
//!
//! The audio callback only pops samples, applies gain, and touches atomics:
//! no allocation, locks, or logging. Volume, pause, and flush are handled in
//! the callback so they take effect immediately regardless of how much audio
//! is buffered.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, ErrorKind, FromSample, SampleFormat, SizedSample, StreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::player::Sink;
use crate::resample::StreamResampler;
use crate::EngineError;

/// Device-side buffer. Large enough to ride out slow disks; skip, pause,
/// and volume don't wait for it to drain.
const BUFFER_SECS: f32 = 1.0;
/// Volume smoothing time constant.
const SMOOTHING_SECS: f32 = 0.005;
/// Fade applied to buffered audio when it is flushed, to avoid a click.
const FLUSH_FADE_SECS: f32 = 0.005;
const FLUSH_TIMEOUT: Duration = Duration::from_millis(250);

struct Shared {
    /// Device frames popped from the ring (played or flushed).
    consumed: AtomicU64,
    /// Target gain as f32 bits.
    volume: AtomicU32,
    paused: AtomicBool,
    flush_req: AtomicU64,
    flush_ack: AtomicU64,
}

pub(crate) struct Output {
    // Dropping the stream stops the callback. cpal streams are not always
    // Send, so the engine thread that opens an output also owns it.
    _stream: cpal::Stream,
    device_id: Option<DeviceId>,
    name: String,
    channels: u16,
    producer: Producer<f32>,
    shared: Arc<Shared>,
    errors: mpsc::Receiver<(ErrorKind, String)>,
    master_rate: u32,
    device_rate: u32,
    resampler: Option<StreamResampler>,
    /// Device-rate audio waiting for ring space.
    staging: Vec<f32>,
    /// Master frames written, including the starting base.
    frames_in: u64,
    /// Added to converted device frames to get master frames consumed.
    offset: i64,
}

impl Output {
    /// Opens the system default output. `base` is the player's frame count,
    /// so consumed frames line up with frames written.
    pub fn open_default(master_rate: u32, base: u64) -> Result<Self, EngineError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| EngineError::Output("no default output device".to_owned()))?;
        let name = device
            .description()
            .map(|d| d.name().to_owned())
            .unwrap_or_else(|_| "default".to_owned());
        let device_id = device.id().ok();

        let (format, config) = choose_config(&device, master_rate)?;
        let device_rate = config.sample_rate;
        let channels = config.channels;
        let capacity = (device_rate as f32 * BUFFER_SECS) as usize * 2;
        let (producer, consumer) = RingBuffer::new(capacity);
        let shared = Arc::new(Shared {
            consumed: AtomicU64::new(0),
            volume: AtomicU32::new(1.0f32.to_bits()),
            paused: AtomicBool::new(false),
            flush_req: AtomicU64::new(0),
            flush_ack: AtomicU64::new(0),
        });
        let (err_tx, errors) = mpsc::channel();
        let render = Render::new(consumer, Arc::clone(&shared), channels, device_rate);

        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, config, render, err_tx),
            SampleFormat::F64 => build::<f64>(&device, config, render, err_tx),
            SampleFormat::I16 => build::<i16>(&device, config, render, err_tx),
            SampleFormat::I32 => build::<i32>(&device, config, render, err_tx),
            SampleFormat::U16 => build::<u16>(&device, config, render, err_tx),
            SampleFormat::U8 => build::<u8>(&device, config, render, err_tx),
            SampleFormat::I8 => build::<i8>(&device, config, render, err_tx),
            other => Err(EngineError::Output(format!(
                "unsupported sample format {other}"
            ))),
        }?;
        stream
            .play()
            .map_err(|e| EngineError::Output(e.to_string()))?;

        let resampler = if device_rate == master_rate {
            None
        } else {
            Some(StreamResampler::adjustable(master_rate, device_rate, 1.01)?)
        };

        Ok(Self {
            _stream: stream,
            device_id,
            name,
            channels,
            producer,
            shared,
            errors,
            master_rate,
            device_rate,
            resampler,
            staging: Vec::new(),
            frames_in: base,
            offset: i64::try_from(base).unwrap_or(i64::MAX),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sample_rate(&self) -> u32 {
        self.device_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
    }

    /// Returns a stream error that ended this output, if any. Recoverable
    /// errors (xruns, automatic rerouting) are returned as `Ok` messages for
    /// logging.
    pub fn poll_errors(&self) -> Result<Vec<String>, String> {
        let mut minor = Vec::new();
        while let Ok((kind, message)) = self.errors.try_recv() {
            match kind {
                ErrorKind::Xrun | ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged => {
                    minor.push(message);
                }
                _ => return Err(message),
            }
        }
        Ok(minor)
    }

    /// True if the OS default device is no longer the one this output uses.
    pub fn default_device_changed(&self) -> bool {
        let current = cpal::default_host()
            .default_output_device()
            .and_then(|d| d.id().ok());
        current.is_some() && current != self.device_id
    }

    fn push_staging(&mut self) {
        if self.staging.is_empty() {
            return;
        }
        // Whole frames only, so the callback never sees half a frame.
        let n = self.staging.len().min(self.producer.slots()) & !1;
        let (pushed, _) = self.producer.push_partial_slice(&self.staging[..n]);
        let pushed = pushed.len();
        self.staging.drain(..pushed);
    }

    fn device_to_master(&self, frames: u64) -> u64 {
        let frames =
            u128::from(frames) * u128::from(self.master_rate) / u128::from(self.device_rate);
        u64::try_from(frames).unwrap_or(u64::MAX)
    }
}

impl Sink for Output {
    fn free_frames(&mut self) -> usize {
        self.push_staging();
        if !self.staging.is_empty() {
            return 0;
        }
        let free = (self.producer.slots() / 2) as u64;
        usize::try_from(self.device_to_master(free)).unwrap_or(0)
    }

    fn write(&mut self, samples: &[f32]) {
        self.frames_in += (samples.len() / 2) as u64;
        match self.resampler.as_mut() {
            Some(r) => {
                if r.process(samples, &mut self.staging).is_err() {
                    // Keep timing intact even if a chunk is lost.
                    self.staging.clear();
                }
            }
            None => self.staging.extend_from_slice(samples),
        }
        self.push_staging();
    }

    fn consumed_frames(&self) -> u64 {
        let ring_empty = self.producer.slots() == self.producer.buffer().capacity();
        if ring_empty && self.staging.is_empty() {
            // Everything written has been heard (bar a resampler's few
            // milliseconds of delay).
            return self.frames_in;
        }
        let device = self.shared.consumed.load(Ordering::Acquire);
        let master = i64::try_from(self.device_to_master(device)).unwrap_or(i64::MAX);
        u64::try_from(master.saturating_add(self.offset))
            .unwrap_or(0)
            .min(self.frames_in)
    }

    fn flush(&mut self) {
        self.staging.clear();
        if let Some(r) = self.resampler.as_mut() {
            r.reset();
        }
        let req = self.shared.flush_req.fetch_add(1, Ordering::AcqRel) + 1;
        let deadline = Instant::now() + FLUSH_TIMEOUT;
        while self.shared.flush_ack.load(Ordering::Acquire) < req && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let device = self.shared.consumed.load(Ordering::Acquire);
        let master = i64::try_from(self.device_to_master(device)).unwrap_or(i64::MAX);
        self.offset = i64::try_from(self.frames_in)
            .unwrap_or(i64::MAX)
            .saturating_sub(master);
    }
}

/// Prefers the master rate in stereo f32, then the master rate in any
/// format, then the device default.
fn choose_config(
    device: &cpal::Device,
    master_rate: u32,
) -> Result<(SampleFormat, StreamConfig), EngineError> {
    let supported = |f: SampleFormat| {
        matches!(
            f,
            SampleFormat::F32
                | SampleFormat::F64
                | SampleFormat::I16
                | SampleFormat::I32
                | SampleFormat::U16
                | SampleFormat::U8
                | SampleFormat::I8
        )
    };
    if let Ok(configs) = device.supported_output_configs() {
        let best = configs
            .filter(|c| {
                supported(c.sample_format())
                    && c.min_sample_rate() <= master_rate
                    && master_rate <= c.max_sample_rate()
            })
            .max_by_key(|c| {
                (
                    c.channels() == 2,
                    c.sample_format() == SampleFormat::F32,
                    c.channels() >= 2,
                )
            });
        if let Some(range) = best {
            if let Some(config) = range.try_with_sample_rate(master_rate) {
                return Ok((config.sample_format(), config.config()));
            }
        }
    }
    let config = device
        .default_output_config()
        .map_err(|e| EngineError::Output(e.to_string()))?;
    Ok((config.sample_format(), config.config()))
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut render: Render,
    errors: mpsc::Sender<(ErrorKind, String)>,
) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample + FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| render.render(data),
            move |err| {
                let _ = errors.send((err.kind(), err.to_string()));
            },
            None,
        )
        .map_err(|e| EngineError::Output(e.to_string()))
}

/// State owned by the audio callback.
struct Render {
    consumer: Consumer<f32>,
    shared: Arc<Shared>,
    channels: usize,
    gain: f32,
    smoothing: f32,
    fade_len: u32,
    /// Frames left in a flush fade; 0 when not flushing.
    fade_left: u32,
    flushing: Option<u64>,
}

impl Render {
    fn new(consumer: Consumer<f32>, shared: Arc<Shared>, channels: u16, rate: u32) -> Self {
        let rate = rate as f32;
        Self {
            consumer,
            shared,
            channels: usize::from(channels.max(1)),
            gain: 0.0,
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECS * rate)).exp(),
            fade_len: ((FLUSH_FADE_SECS * rate) as u32).max(1),
            fade_left: 0,
            flushing: None,
        }
    }

    fn render<T: SizedSample + FromSample<f32>>(&mut self, data: &mut [T]) {
        let req = self.shared.flush_req.load(Ordering::Acquire);
        if self.flushing.is_none() && req != self.shared.flush_ack.load(Ordering::Relaxed) {
            self.flushing = Some(req);
            self.fade_left = self.fade_len;
        }
        let paused = self.shared.paused.load(Ordering::Relaxed);
        let target = if paused {
            0.0
        } else {
            f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
        };

        let mut popped = 0u64;
        for frame in data.chunks_mut(self.channels) {
            let silent = paused && self.gain < 1e-4;
            let (l, r) = if !silent && self.consumer.slots() >= 2 {
                popped += 1;
                (
                    self.consumer.pop().unwrap_or(0.0),
                    self.consumer.pop().unwrap_or(0.0),
                )
            } else {
                (0.0, 0.0)
            };

            self.gain += (target - self.gain) * self.smoothing;
            let mut g = self.gain;
            if let Some(req) = self.flushing {
                g *= self.fade_left as f32 / self.fade_len as f32;
                self.fade_left = self.fade_left.saturating_sub(1);
                if self.fade_left == 0 {
                    // Fade done: drop the rest, then acknowledge.
                    let n = self.consumer.slots() & !1;
                    if let Ok(chunk) = self.consumer.read_chunk(n) {
                        chunk.commit_all();
                    }
                    popped += (n / 2) as u64;
                    self.shared.consumed.fetch_add(popped, Ordering::AcqRel);
                    popped = 0;
                    self.shared.flush_ack.store(req, Ordering::Release);
                    self.flushing = None;
                }
            }

            let (l, r) = ((l * g).clamp(-1.0, 1.0), (r * g).clamp(-1.0, 1.0));
            match frame {
                [mono] => *mono = T::from_sample((l + r) * 0.5),
                [left, right, rest @ ..] => {
                    *left = T::from_sample(l);
                    *right = T::from_sample(r);
                    for s in rest {
                        *s = T::from_sample(0.0f32);
                    }
                }
                [] => {}
            }
        }
        if popped > 0 {
            self.shared.consumed.fetch_add(popped, Ordering::AcqRel);
        }
    }
}
