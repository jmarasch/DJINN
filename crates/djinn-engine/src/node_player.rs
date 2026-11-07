//! A node's player: plays Main's timed stream (see [`crate::timed`]) on this
//! box's speakers so each moment is heard when Main's are heard.
//!
//! Chunks arrive stamped with Main's clock; the server side keeps the
//! difference between Main's clock and this one (`offset`, Main minus here)
//! up to date, and [`Schedule`] turns each stamp into a time on this clock.
//! It starts the speaker so the first frame lands on time (padding with
//! silence or skipping in), then measures how early or late the speaker runs
//! and nudges its speed with the same [`DriftControl`] that keeps Main's own
//! extra speakers in step; big jumps (a hiccup on the network) are fixed at
//! once. A pause holds the speaker and moves the waiting chunks later by
//! however long it lasted, as Main's own buffered audio is.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::drift::DriftControl;
use crate::error::EngineError;
use crate::output::Output;
use crate::timed::{Mark, Piece};
use crate::OutputDevice;

/// How far ahead of its time audio is handed to the speaker.
const AHEAD_SECS: f64 = 0.5;
/// A gap or overlap between chunks bigger than this is filled or skipped.
const SEAM_SECS: f64 = 0.005;
/// How often the player thread looks at the speaker.
const TICK: Duration = Duration::from_millis(5);

/// What the schedule needs from a speaker (the real device, or a simulated
/// one in tests).
pub(crate) trait Speaker {
    /// Seconds until audio written now is heard.
    fn latency(&self) -> f64;
    fn free_frames(&mut self) -> usize;
    fn write(&mut self, samples: &[f32]);
    fn skip_ahead(&mut self, secs: f64);
    fn insert_silence(&mut self, secs: f64);
    fn set_ratio(&mut self, relative: f64);
    fn flush(&mut self);
    fn set_paused(&mut self, paused: bool);
    fn set_gain(&mut self, gain: f32);
}

impl Speaker for Output {
    fn latency(&self) -> f64 {
        Output::latency(self)
    }
    fn free_frames(&mut self) -> usize {
        Output::free_frames(self)
    }
    fn write(&mut self, samples: &[f32]) {
        Output::write(self, samples);
    }
    fn skip_ahead(&mut self, secs: f64) {
        Output::skip_ahead(self, secs);
    }
    fn insert_silence(&mut self, secs: f64) {
        Output::insert_silence(self, secs);
    }
    fn set_ratio(&mut self, relative: f64) {
        let _ = Output::set_ratio(self, relative);
    }
    fn flush(&mut self) {
        Output::flush(self);
    }
    fn set_paused(&mut self, paused: bool) {
        Output::set_paused(self, paused);
    }
    fn set_gain(&mut self, gain: f32) {
        Output::set_gain(self, gain);
    }
}

struct Chunk {
    /// When frame 0 is due, on this clock (µs).
    at: i64,
    samples: Vec<f32>,
    /// Frames already handed to the speaker.
    pos: usize,
}

impl Chunk {
    fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}

/// Lines Main's chunks up with this clock and keeps the speaker on time.
pub(crate) struct Schedule {
    rate: u32,
    /// Extra delay for this box's speakers (Bluetooth lag and such), µs.
    delay_us: i64,
    trim: f32,
    queue: VecDeque<Chunk>,
    /// When the next frame handed over is due; None until started.
    next_due: Option<i64>,
    paused_at: Option<i64>,
    drift: DriftControl,
    last_measure: Option<i64>,
    volume: f32,
}

impl Schedule {
    pub fn new(rate: u32, delay_ms: u32, trim: f32) -> Self {
        Schedule {
            rate,
            delay_us: i64::from(delay_ms) * 1000,
            trim,
            queue: VecDeque::new(),
            next_due: None,
            paused_at: None,
            drift: DriftControl::default(),
            last_measure: None,
            volume: 1.0,
        }
    }

    fn frames_to_us(&self, frames: usize) -> i64 {
        (frames as i64 * 1_000_000) / i64::from(self.rate)
    }

    fn us_to_frames(&self, us: i64) -> usize {
        (us.max(0) as i128 * i128::from(self.rate) / 1_000_000) as usize
    }

    /// Takes a piece of Main's stream. `offset` is Main's clock minus this
    /// one, `now` this clock (µs).
    pub fn push(&mut self, piece: Piece, offset: i64, now: i64, speaker: &mut impl Speaker) {
        match piece.mark {
            Mark::Audio { play_at_us, .. } => {
                if piece.samples.len() >= 2 {
                    self.queue.push_back(Chunk {
                        at: play_at_us - offset + self.delay_us,
                        samples: piece.samples,
                        pos: 0,
                    });
                }
            }
            Mark::Flush => {
                speaker.flush();
                self.queue.clear();
                self.next_due = None;
                self.drift.restart();
            }
            Mark::Pause => {
                if self.paused_at.is_none() {
                    self.paused_at = Some(now);
                    speaker.set_paused(true);
                }
            }
            Mark::Resume => {
                if let Some(at) = self.paused_at.take() {
                    let held = now - at;
                    for chunk in &mut self.queue {
                        chunk.at += held;
                    }
                    if let Some(due) = self.next_due.as_mut() {
                        *due += held;
                    }
                    speaker.set_paused(false);
                    self.drift.restart();
                    self.last_measure = None;
                }
            }
            Mark::Volume(volume) => {
                self.volume = volume;
                speaker.set_gain(volume * self.trim);
            }
        }
    }

    /// The speaker's timing error now, in seconds (late is positive), if
    /// it's playing.
    fn error(&self, now: i64, speaker: &impl Speaker) -> Option<f64> {
        let due = self.next_due?;
        let heard = now + (speaker.latency() * 1e6) as i64;
        Some((heard - due) as f64 / 1e6)
    }

    /// Keeps the speaker fed and on time. Call often.
    pub fn tick(&mut self, now: i64, speaker: &mut impl Speaker) {
        if self.paused_at.is_some() {
            return;
        }
        if self.next_due.is_none() && !self.start(now, speaker) {
            return;
        }
        // Speed: how far off the speaker runs.
        if let Some(error) = self.error(now, speaker) {
            let dt = self.last_measure.map_or(0.0, |t| (now - t) as f64 / 1e6);
            self.last_measure = Some(now);
            if dt > 0.0 && speaker.latency() > 0.05 {
                if let Some(jump) = self.drift.update(error, dt) {
                    if jump > 0.0 {
                        speaker.skip_ahead(jump);
                    } else {
                        speaker.insert_silence(-jump);
                    }
                }
                speaker.set_ratio(1.0 + self.drift.adjust());
            }
        }
        self.feed(now, speaker);
    }

    /// Starts playing so the first frame due is heard on time. False if
    /// there's nothing to play yet.
    fn start(&mut self, now: i64, speaker: &mut impl Speaker) -> bool {
        // Drop what's already too late to play.
        while let Some(chunk) = self.queue.front_mut() {
            let end = chunk.at + (chunk.frames() as i64 * 1_000_000) / i64::from(self.rate);
            if end <= now {
                self.queue.pop_front();
                continue;
            }
            if chunk.at < now {
                let late = ((now - chunk.at) as i128 * i128::from(self.rate) / 1_000_000) as usize;
                chunk.pos = late.min(chunk.frames());
            }
            break;
        }
        let Some(chunk) = self.queue.front() else {
            return false;
        };
        let due = chunk.at + self.frames_to_us(chunk.pos);
        if due - now > (AHEAD_SECS * 1e6) as i64 {
            return false;
        }
        let heard = now + (speaker.latency() * 1e6) as i64;
        if due > heard {
            speaker.insert_silence((due - heard) as f64 / 1e6);
        }
        self.next_due = Some(due);
        self.drift.restart();
        self.last_measure = None;
        true
    }

    /// Hands audio over up to [`AHEAD_SECS`] before it's due.
    fn feed(&mut self, now: i64, speaker: &mut impl Speaker) {
        let horizon = now + (AHEAD_SECS * 1e6) as i64;
        let seam = (SEAM_SECS * 1e6) as i64;
        loop {
            let Some(due) = self.next_due else { return };
            if due > horizon {
                return;
            }
            let rate = self.rate;
            let room = self.us_to_frames(horizon - due).max(1);
            let Some(chunk) = self.queue.front_mut() else {
                return;
            };
            if chunk.pos == 0 {
                // A new chunk: line its start up with where playback is.
                let gap = chunk.at - due;
                if gap > seam {
                    speaker.insert_silence(gap as f64 / 1e6);
                    self.next_due = Some(chunk.at);
                    continue;
                } else if gap < -seam {
                    let skip = ((-gap) as i128 * i128::from(rate) / 1_000_000) as usize;
                    chunk.pos = skip.min(chunk.frames());
                    if chunk.pos >= chunk.frames() {
                        self.queue.pop_front();
                        continue;
                    }
                }
            }
            let free = speaker.free_frames();
            if free == 0 {
                return;
            }
            let n = (chunk.frames() - chunk.pos).min(free).min(room);
            let start = chunk.pos * 2;
            speaker.write(&chunk.samples[start..start + n * 2]);
            chunk.pos += n;
            let done = chunk.pos >= chunk.frames();
            self.next_due = Some(due + (n as i64 * 1_000_000) / i64::from(rate));
            if done {
                self.queue.pop_front();
            }
        }
    }
}

enum Msg {
    Piece(Piece),
    Stop,
}

/// Plays Main's timed stream on one of this box's outputs, on its own
/// thread.
pub struct NodePlayer {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

impl NodePlayer {
    /// Opens `device` (None for the system default) at Main's `rate`.
    /// `offset` is Main's clock minus this one in µs, kept current by the
    /// caller.
    pub fn start(
        device: Option<String>,
        rate: u32,
        delay_ms: u32,
        trim: f32,
        offset: Arc<AtomicI64>,
    ) -> Result<NodePlayer, EngineError> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("djinn-node-player".to_owned())
            .spawn(move || {
                let host = cpal::default_host();
                let device = device.map_or(OutputDevice::Default, OutputDevice::Id);
                let mut output = match Output::open(&host, &device, rate, 0, true) {
                    Ok(output) => {
                        let _ = ready_tx.send(Ok(()));
                        output
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let mut schedule = Schedule::new(rate, delay_ms, trim);
                loop {
                    loop {
                        match rx.try_recv() {
                            Ok(Msg::Piece(piece)) => {
                                let now = crate::clock::now_micros();
                                schedule.push(
                                    piece,
                                    offset.load(Ordering::Acquire),
                                    now,
                                    &mut output,
                                );
                            }
                            Ok(Msg::Stop) | Err(mpsc::TryRecvError::Disconnected) => return,
                            Err(mpsc::TryRecvError::Empty) => break,
                        }
                    }
                    if let Err(e) = output.poll_errors() {
                        tracing::warn!(error = %e, "node speaker failed");
                        return;
                    }
                    schedule.tick(crate::clock::now_micros(), &mut output);
                    std::thread::sleep(TICK);
                }
            })
            .map_err(|e| EngineError::Output(format!("failed to start node player: {e}")))?;
        ready_rx
            .recv()
            .map_err(|_| EngineError::Output("node player stopped".into()))??;
        Ok(NodePlayer {
            tx,
            thread: Some(thread),
        })
    }

    /// Hands over a piece of Main's stream.
    pub fn piece(&self, piece: Piece) -> Result<(), EngineError> {
        self.tx
            .send(Msg::Piece(piece))
            .map_err(|_| EngineError::Stopped)
    }
}

impl Drop for NodePlayer {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speaker whose crystal runs `drift` fast, which records when each
    /// written frame is actually heard.
    struct Simulated {
        rate: f64,
        drift: f64,
        ratio: f64,
        paused: bool,
        /// Frames waiting to be heard (fractional, as the device drains).
        buffered: f64,
        capacity: f64,
        /// (stream frame, heard in µs from the write) for each chunk's first
        /// frame. Samples carry their own stream position (see `encode`).
        heard: Vec<(u64, i64)>,
        skip: f64,
    }

    impl Simulated {
        fn new(rate: u32, drift: f64) -> Self {
            Simulated {
                rate: f64::from(rate),
                drift,
                ratio: 1.0,
                paused: false,
                buffered: 0.0,
                capacity: f64::from(rate),
                heard: Vec::new(),
                skip: 0.0,
            }
        }

        /// Master frames consumed per second.
        fn speed(&self) -> f64 {
            self.rate * (1.0 + self.drift) / self.ratio
        }

        fn advance(&mut self, secs: f64) {
            if !self.paused {
                self.buffered = (self.buffered - secs * self.speed()).max(0.0);
            }
        }
    }

    impl Speaker for Simulated {
        fn latency(&self) -> f64 {
            self.buffered / self.speed()
        }
        fn free_frames(&mut self) -> usize {
            (self.capacity - self.buffered).max(0.0) as usize
        }
        fn write(&mut self, samples: &[f32]) {
            let frames = samples.len() / 2;
            let skipped = self.skip.min(frames as f64);
            self.skip -= skipped;
            let kept = frames as f64 - skipped;
            let first = skipped as usize;
            if first < frames {
                // Note when this chunk's first kept frame is heard.
                let frame = decode(samples[first * 2], samples[first * 2 + 1]);
                self.heard.push((frame, (self.latency() * 1e6) as i64));
            }
            self.buffered += kept;
        }
        fn skip_ahead(&mut self, secs: f64) {
            let frames = secs * self.rate;
            let from_buffer = frames.min(self.buffered);
            self.buffered -= from_buffer;
            self.skip += frames - from_buffer;
        }
        fn insert_silence(&mut self, secs: f64) {
            self.buffered += secs * self.rate;
        }
        fn set_ratio(&mut self, relative: f64) {
            self.ratio = relative;
        }
        fn flush(&mut self) {
            self.buffered = 0.0;
        }
        fn set_paused(&mut self, paused: bool) {
            self.paused = paused;
        }
        fn set_gain(&mut self, _gain: f32) {}
    }

    /// A stream frame's position, in its two samples (each part exact in an
    /// f32).
    fn encode(frame: u64) -> [f32; 2] {
        [(frame & 0xF_FFFF) as f32, (frame >> 20) as f32]
    }

    fn decode(left: f32, right: f32) -> u64 {
        left as u64 | ((right as u64) << 20)
    }

    fn numbered(first: u64, frames: usize) -> Vec<f32> {
        (0..frames as u64).flat_map(|i| encode(first + i)).collect()
    }

    /// Main sends 10 ms chunks stamped `start + frame/rate`; the node's
    /// clock differs by `offset`; its speaker drifts. Returns the worst
    /// error (ms) between when stream frames are heard and when they're due,
    /// after the first `settle` seconds.
    fn run(rate: u32, drift: f64, offset: i64, secs: f64, settle: f64) -> f64 {
        let mut speaker = Simulated::new(rate, drift);
        let mut schedule = Schedule::new(rate, 0, 1.0);
        let chunk = (rate / 100) as usize;
        let start_main = 2_000_000i64;
        let mut now = 0i64;
        let mut sent = 0u64;
        let mut worst: f64 = 0.0;
        let step = 1000i64;
        while (now as f64) < secs * 1e6 {
            // Main sends a little ahead of when audio is due (its own buffer).
            let main_now = now + offset;
            while start_main + (sent as i64 * 1_000_000) / i64::from(rate) < main_now + 300_000 {
                let play_at_us = start_main + (sent as i64 * 1_000_000) / i64::from(rate);
                schedule.push(
                    Piece {
                        mark: Mark::Audio {
                            frames: chunk as u32,
                            play_at_us,
                        },
                        samples: numbered(sent, chunk),
                    },
                    offset,
                    now,
                    &mut speaker,
                );
                sent += chunk as u64;
            }
            let heard_before = speaker.heard.len();
            schedule.tick(now, &mut speaker);
            for &(frame, heard_in) in &speaker.heard[heard_before..] {
                if (now as f64) < settle * 1e6 {
                    continue;
                }
                let due_main = start_main + (frame as i64 * 1_000_000) / i64::from(rate);
                let due_local = due_main - offset;
                let error = (now + heard_in - due_local) as f64 / 1000.0;
                worst = worst.max(error.abs());
            }
            speaker.advance(step as f64 / 1e6);
            now += step;
        }
        worst
    }

    #[test]
    fn plays_on_time_with_a_clock_offset() {
        let worst = run(48_000, 0.0, 7_654_321, 30.0, 5.0);
        assert!(worst < 1.0, "worst {worst} ms");
    }

    /// Crystals differ by tens of parts per million (200 is a bad one):
    /// within a few ms while it learns the speed (an echo needs tens), then
    /// well under one.
    #[test]
    fn holds_time_against_a_drifting_crystal() {
        for drift in [-200e-6, -50e-6, 50e-6, 200e-6] {
            let learning = run(48_000, drift, -3_000_000, 300.0, 60.0);
            assert!(
                learning < 5.0,
                "drift {drift}: worst {learning} ms while learning"
            );
            let settled = run(48_000, drift, -3_000_000, 900.0, 600.0);
            assert!(
                settled < 0.1,
                "drift {drift}: worst {settled} ms once settled"
            );
        }
    }

    #[test]
    fn a_pause_moves_waiting_chunks_later() {
        let mut speaker = Simulated::new(1000, 0.0);
        let mut schedule = Schedule::new(1000, 0, 1.0);
        let audio = |at: i64| Piece {
            mark: Mark::Audio {
                frames: 100,
                play_at_us: at,
            },
            samples: vec![0.0; 200],
        };
        schedule.push(audio(1_000_000), 0, 0, &mut speaker);
        schedule.push(
            Piece {
                mark: Mark::Pause,
                samples: vec![],
            },
            0,
            0,
            &mut speaker,
        );
        schedule.push(
            Piece {
                mark: Mark::Resume,
                samples: vec![],
            },
            0,
            5_000_000,
            &mut speaker,
        );
        assert_eq!(schedule.queue[0].at, 6_000_000, "five seconds later");
        schedule.push(
            Piece {
                mark: Mark::Flush,
                samples: vec![],
            },
            0,
            5_000_000,
            &mut speaker,
        );
        assert!(schedule.queue.is_empty());
    }

    #[test]
    fn late_audio_is_skipped_in_not_played_late() {
        let mut speaker = Simulated::new(1000, 0.0);
        let mut schedule = Schedule::new(1000, 0, 1.0);
        // Due 300 ms ago: start 300 frames in.
        schedule.push(
            Piece {
                mark: Mark::Audio {
                    frames: 1000,
                    play_at_us: 700_000,
                },
                samples: numbered(0, 1000),
            },
            0,
            1_000_000,
            &mut speaker,
        );
        schedule.tick(1_000_000, &mut speaker);
        assert_eq!(speaker.heard[0].0, 300, "{:?}", speaker.heard);
        assert_eq!(speaker.heard[0].1, 0, "heard right away");
    }
}
