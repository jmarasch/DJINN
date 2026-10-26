//! All outputs together: fans the master stream out to every open device
//! and keeps them in time.
//!
//! The first output to open is the clock. The player paces itself to it,
//! keeping [`BUFFER_SECS`] plus its delay buffered. Every other output is a
//! follower: it gets the same audio at the same moment and, through
//! [`DriftControl`], adjusts its speed so its buffered time (less its own
//! delay) matches the clock's. A device that fails is closed and retried on
//! its own; the rest keep playing. If the clock fails, the next open output
//! takes over.

use std::cell::Cell;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};

use crate::drift::DriftControl;
use crate::output::{Output, BUFFER_SECS};
use crate::player::Sink;
use crate::{Event, OutputConfig, OutputDevice, OutputState, OutputStatus, MAX_DELAY_MS};

const RETRY: Duration = Duration::from_secs(2);
/// Named devices (often Bluetooth speakers that come and go) retry less
/// eagerly than the default one.
const RETRY_NAMED: Duration = Duration::from_secs(5);
const DEVICE_CHECK_INTERVAL: Duration = Duration::from_secs(3);
/// How often sync figures are reported while several outputs play.
const SYNC_REPORT_INTERVAL: Duration = Duration::from_secs(5);

struct Slot {
    config: OutputConfig,
    output: Option<Output>,
    retry_at: Instant,
    error: Option<String>,
    drift: DriftControl,
}

impl Slot {
    fn delay(&self) -> f64 {
        f64::from(self.config.delay_ms.min(MAX_DELAY_MS)) / 1000.0
    }

    fn retry_interval(&self) -> Duration {
        match self.config.device {
            OutputDevice::Default => RETRY,
            OutputDevice::Id(_) => RETRY_NAMED,
        }
    }
}

pub(crate) struct Outputs {
    master_rate: u32,
    host: Option<cpal::Host>,
    slots: Vec<Slot>,
    /// Index of the clock output, while one is open.
    clock: Option<usize>,
    volume: f32,
    paused: bool,
    /// Master frames written.
    frames_in: u64,
    /// Keeps consumed frames from going backwards when outputs change.
    consumed_floor: Cell<u64>,
    last_sync: Option<Instant>,
    next_device_check: Instant,
    next_report: Instant,
    reported: Vec<OutputStatus>,
    events: Vec<Event>,
}

impl Outputs {
    pub fn new(master_rate: u32, configs: Vec<OutputConfig>) -> Self {
        let now = Instant::now();
        let mut outputs = Self {
            master_rate,
            host: None,
            slots: Vec::new(),
            clock: None,
            volume: 1.0,
            paused: false,
            frames_in: 0,
            consumed_floor: Cell::new(0),
            last_sync: None,
            next_device_check: now + DEVICE_CHECK_INTERVAL,
            next_report: now,
            reported: Vec::new(),
            events: Vec::new(),
        };
        outputs.configure(configs);
        outputs
    }

    pub fn take_events(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    /// Applies a new output list, keeping outputs whose id and device are
    /// unchanged open.
    pub fn configure(&mut self, configs: Vec<OutputConfig>) {
        let now = Instant::now();
        let mut old: Vec<Slot> = std::mem::take(&mut self.slots);
        let clock_id = self
            .clock
            .and_then(|i| old.get(i))
            .map(|slot| slot.config.id);
        self.clock = None;
        for config in configs {
            let reuse = old
                .iter()
                .position(|s| s.config.id == config.id && s.config.device == config.device);
            let mut slot = match reuse {
                Some(i) => old.remove(i),
                None => Slot {
                    config: config.clone(),
                    output: None,
                    retry_at: now,
                    error: None,
                    drift: DriftControl::default(),
                },
            };
            if !config.enabled {
                if slot.output.take().is_some() {
                    tracing::info!(output = config.id, "audio output disabled");
                }
                slot.error = None;
            }
            let change = f64::from(config.delay_ms.min(MAX_DELAY_MS)) / 1000.0 - slot.delay();
            if let Some(output) = slot.output.as_mut() {
                if change > 0.0 {
                    output.insert_silence(change);
                } else if change < 0.0 {
                    output.skip_ahead(-change);
                }
            }
            slot.config = config;
            if Some(slot.config.id) == clock_id && slot.output.is_some() {
                self.clock = Some(self.slots.len());
            }
            self.slots.push(slot);
        }
        self.apply_gains();
        self.pick_clock();
        self.report();
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        self.apply_gains();
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        for output in self.slots.iter().filter_map(|s| s.output.as_ref()) {
            output.set_paused(paused);
        }
    }

    /// Opens, retries, and watches outputs, and keeps followers in time.
    pub fn tick(&mut self, now: Instant) {
        self.watch(now);
        self.open_waiting(now);
        self.sync(now);
        if now >= self.next_report {
            self.next_report = now + SYNC_REPORT_INTERVAL;
            if self.slots.iter().filter(|s| s.output.is_some()).count() > 1 {
                self.events.push(Event::Outputs(self.status()));
            }
        }
    }

    pub fn status(&self) -> Vec<OutputStatus> {
        self.slots
            .iter()
            .enumerate()
            .map(|(i, slot)| {
                let output = slot.output.as_ref();
                let state = if !slot.config.enabled {
                    OutputState::Disabled
                } else if output.is_some() {
                    OutputState::Playing
                } else {
                    OutputState::Waiting
                };
                let follower = output.is_some() && self.clock != Some(i);
                OutputStatus {
                    id: slot.config.id,
                    state,
                    device: output.map(|o| o.name().to_owned()),
                    sample_rate: output.map(Output::sample_rate),
                    channels: output.map(Output::channels),
                    error: slot.error.clone(),
                    clock: self.clock == Some(i),
                    drift_ppm: if follower {
                        slot.drift.adjust() * 1e6
                    } else {
                        0.0
                    },
                    offset_ms: if follower {
                        slot.drift.error() * 1000.0
                    } else {
                        0.0
                    },
                }
            })
            .collect()
    }

    /// Emits status if anything but the sync figures changed.
    fn report(&mut self) {
        let status = self.status();
        let coarse = |list: &[OutputStatus]| {
            list.iter()
                .map(|s| {
                    (
                        s.id,
                        s.state,
                        s.device.clone(),
                        s.sample_rate,
                        s.error.clone(),
                        s.clock,
                    )
                })
                .collect::<Vec<_>>()
        };
        if coarse(&status) != coarse(&self.reported) {
            self.reported = status.clone();
            self.events.push(Event::Outputs(status));
        }
    }

    fn apply_gains(&self) {
        for slot in &self.slots {
            if let Some(output) = &slot.output {
                output.set_gain(self.volume * slot.config.trim.clamp(0.0, 1.0));
            }
        }
    }

    /// Keeps the clock where it is while it plays, else takes the first
    /// open output.
    fn pick_clock(&mut self) {
        let alive = |i: usize| self.slots.get(i).is_some_and(|s| s.output.is_some());
        if self.clock.is_some_and(alive) {
            return;
        }
        let clock = self.slots.iter().position(|s| s.output.is_some());
        if clock != self.clock {
            if let Some(i) = clock {
                tracing::info!(output = self.slots[i].config.id, "audio clock output");
            }
            self.clock = clock;
            // Followers measure against the new clock from scratch.
            for slot in &mut self.slots {
                slot.drift.restart();
            }
            self.last_sync = None;
        }
    }

    /// Handles stream errors and default device changes.
    fn watch(&mut self, now: Instant) {
        let check_default = now >= self.next_device_check;
        if check_default {
            self.next_device_check = now + DEVICE_CHECK_INTERVAL;
        }
        let default_id = if check_default {
            self.host
                .as_ref()
                .and_then(|h| h.default_output_device())
                .and_then(|d| d.id().ok())
        } else {
            None
        };
        let mut changed = false;
        for slot in &mut self.slots {
            let Some(output) = slot.output.as_ref() else {
                continue;
            };
            let mut lost = None;
            match output.poll_errors() {
                Ok(minor) => {
                    for message in minor {
                        tracing::debug!(output = slot.config.id, %message, "audio output");
                    }
                }
                Err(message) => lost = Some(message),
            }
            if let Some(message) = lost {
                tracing::warn!(output = slot.config.id, %message, "audio output lost");
                slot.output = None;
                slot.error = Some(message);
                slot.retry_at = now + slot.retry_interval();
                changed = true;
                continue;
            }
            let moved = slot.config.device == OutputDevice::Default
                && default_id.is_some()
                && default_id.as_ref() != output.device_id();
            if moved {
                tracing::info!("default output device changed, reopening");
                slot.output = None;
                slot.retry_at = now;
                changed = true;
            }
        }
        if changed {
            // A lost host connection (audio server restart) shows up as
            // lost streams; reconnect afresh.
            self.host = None;
            self.pick_clock();
            self.report();
        }
    }

    fn open_waiting(&mut self, now: Instant) {
        let due = self
            .slots
            .iter()
            .any(|s| s.config.enabled && s.output.is_none() && now >= s.retry_at);
        if !due {
            return;
        }
        let host = self.host.get_or_insert_with(cpal::default_host);
        let mut opened = false;
        for i in 0..self.slots.len() {
            let clock = self
                .clock
                .and_then(|c| self.slots.get(c))
                .and_then(|c| c.output.as_ref().map(|o| (o.latency(), c.delay())));
            let slot = &mut self.slots[i];
            if !slot.config.enabled || slot.output.is_some() || now < slot.retry_at {
                continue;
            }
            match Output::open(
                host,
                &slot.config.device,
                self.master_rate,
                self.frames_in,
                clock.is_some(),
            ) {
                Ok(mut output) => {
                    output.set_paused(self.paused);
                    output.set_gain(self.volume * slot.config.trim.clamp(0.0, 1.0));
                    // Join in time: buffer as much as the clock, adjusted
                    // for the difference in delay.
                    if let Some((latency, delay)) = clock {
                        output.insert_silence(latency - delay + slot.delay());
                    }
                    tracing::info!(
                        output = slot.config.id,
                        device = output.name(),
                        rate = output.sample_rate(),
                        channels = output.channels(),
                        "audio output opened"
                    );
                    slot.output = Some(output);
                    slot.error = None;
                    slot.drift = DriftControl::default();
                    opened = true;
                }
                Err(e) => {
                    let message = e.to_string();
                    if slot.error.as_deref() != Some(message.as_str()) {
                        tracing::warn!(output = slot.config.id, %message, "cannot open audio output, retrying");
                    }
                    slot.error = Some(message);
                    slot.retry_at = now + slot.retry_interval();
                }
            }
        }
        if opened {
            self.pick_clock();
        }
        self.report();
    }

    /// Runs drift control on each follower.
    fn sync(&mut self, now: Instant) {
        let dt = self
            .last_sync
            .map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        self.last_sync = Some(now);
        let Some(clock) = self.clock else { return };
        let Some(reference) = self
            .slots
            .get(clock)
            .and_then(|c| c.output.as_ref().map(|o| o.latency() - c.delay()))
        else {
            return;
        };
        // Only while audio streams steadily: an idle or draining clock
        // says nothing about drift.
        let streaming = reference + self.slots[clock].delay() > BUFFER_SECS / 2.0;
        if self.paused || dt <= 0.0 || !streaming {
            for slot in &mut self.slots {
                slot.drift.restart();
            }
            return;
        }
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if i == clock {
                continue;
            }
            let delay = slot.delay();
            let Some(output) = slot.output.as_mut() else {
                continue;
            };
            if output.take_overflow() {
                slot.drift.restart();
            }
            let error = output.latency() - delay - reference;
            if let Some(jump) = slot.drift.update(error, dt) {
                tracing::debug!(
                    output = slot.config.id,
                    ms = jump * 1000.0,
                    "resyncing output"
                );
                if jump > 0.0 {
                    output.skip_ahead(jump);
                } else {
                    output.insert_silence(-jump);
                }
            }
            if let Err(e) = output.set_ratio(1.0 + slot.drift.adjust()) {
                tracing::debug!(output = slot.config.id, error = %e, "cannot adjust output speed");
            }
        }
    }

    fn clock_output(&mut self) -> Option<(&mut Output, f64)> {
        let clock = self.clock?;
        let slot = self.slots.get_mut(clock)?;
        let delay = slot.delay();
        slot.output.as_mut().map(|o| (o, delay))
    }
}

impl Sink for Outputs {
    fn free_frames(&mut self) -> usize {
        let master_rate = f64::from(self.master_rate);
        let Some((clock, delay)) = self.clock_output() else {
            // Nothing to play to: hold still until an output opens.
            return 0;
        };
        let room = clock.free_frames();
        let wanted = ((BUFFER_SECS + delay - clock.latency()) * master_rate).max(0.0) as usize;
        room.min(wanted)
    }

    fn write(&mut self, samples: &[f32]) {
        self.frames_in += (samples.len() / 2) as u64;
        for output in self.slots.iter_mut().filter_map(|s| s.output.as_mut()) {
            output.write(samples);
        }
    }

    fn consumed_frames(&self) -> u64 {
        let clock = self
            .clock
            .and_then(|i| self.slots.get(i))
            .and_then(|s| s.output.as_ref());
        let consumed = match clock {
            Some(output) => output.consumed_frames(),
            None => self.frames_in,
        };
        let consumed = consumed.max(self.consumed_floor.get()).min(self.frames_in);
        self.consumed_floor.set(consumed);
        consumed
    }

    fn flush(&mut self) {
        for slot in &mut self.slots {
            let delay = slot.delay();
            if let Some(output) = slot.output.as_mut() {
                output.flush();
                // Keep each output's delay after the flush.
                output.insert_silence(delay);
            }
        }
        // Silence padding aside, everything written counts as consumed.
        self.consumed_floor.set(self.frames_in);
    }
}
