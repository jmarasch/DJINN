//! Track sequencing: decodes the current track, rolls over to the next one
//! without a gap, and turns boundaries into events when they become audible.
//!
//! Pipeline: decoder (source rate) -> stage (resample to master) -> sink.
//! Consecutive tracks at the same source rate share one resampling stage, so
//! a gapless album is resampled as one continuous signal with no edge
//! artifacts at track boundaries.

use std::collections::VecDeque;

use crate::decoder::TrackDecoder;
use crate::resample::StreamResampler;
use crate::{EndReason, Event, Track, TrackId};

/// Where the player writes master-rate audio.
pub(crate) trait Sink {
    /// Master-rate frames that can be written right now.
    fn free_frames(&mut self) -> usize;
    /// Writes interleaved stereo frames, never more than [`Sink::free_frames`].
    fn write(&mut self, samples: &[f32]);
    /// Master frames played (or flushed) so far, on the same scale as the
    /// total frames written.
    fn consumed_frames(&self) -> u64;
    /// Drops everything written but not yet played. Afterwards
    /// `consumed_frames` equals the total written.
    fn flush(&mut self);
}

struct Deck {
    track: Track,
    decoder: TrackDecoder,
}

/// Converts one or more consecutive same-rate tracks to the master rate.
struct Stage {
    rate: u32,
    /// Master frame at which this stage's output begins.
    base: u64,
    frames_in: u64,
    resampler: Option<StreamResampler>,
}

impl Stage {
    /// Master frame corresponding to the current input position.
    fn position(&self) -> u64 {
        self.base
            + match &self.resampler {
                Some(r) => r.output_frames_for(self.frames_in),
                None => self.frames_in,
            }
    }
}

#[derive(Debug)]
enum MarkKind {
    Start(TrackId),
    End(TrackId, EndReason),
}

#[derive(Debug)]
struct Mark {
    frame: u64,
    kind: MarkKind,
}

#[derive(Copy, Clone, Debug)]
struct Audible {
    id: TrackId,
    start: u64,
}

pub(crate) struct Player {
    master_rate: u32,
    current: Option<Deck>,
    next: Option<Deck>,
    stage: Option<Stage>,
    /// Scratch for decoded source-rate audio.
    decoded: Vec<f32>,
    /// Master-rate audio not yet written to the sink.
    buf: Vec<f32>,
    /// Master frames written to the sink so far.
    written: u64,
    marks: VecDeque<Mark>,
    audible: Option<Audible>,
    events: Vec<Event>,
}

impl Player {
    pub fn new(master_rate: u32) -> Self {
        Self {
            master_rate,
            current: None,
            next: None,
            stage: None,
            decoded: Vec::new(),
            buf: Vec::new(),
            written: 0,
            marks: VecDeque::new(),
            audible: None,
            events: Vec::new(),
        }
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn take_events(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    /// True while anything is playing or queued up to play.
    pub fn is_active(&self) -> bool {
        self.audible.is_some()
            || self.current.is_some()
            || self.next.is_some()
            || !self.marks.is_empty()
    }

    /// Position of the audible track in milliseconds.
    pub fn position(&self, consumed: u64) -> Option<(TrackId, u64)> {
        self.audible
            .map(|a| (a.id, self.frames_to_ms(consumed.saturating_sub(a.start))))
    }

    pub fn set_next(&mut self, track: Option<Track>) {
        self.next = None;
        let Some(track) = track else { return };
        match TrackDecoder::open(&track.path) {
            Ok(decoder) => self.next = Some(Deck { track, decoder }),
            Err(e) => {
                self.events.push(Event::TrackError {
                    id: track.id,
                    error: e.to_string(),
                });
                self.events.push(Event::NextNeeded);
            }
        }
    }

    /// Decodes and writes until the sink is full or there is nothing to play.
    pub fn fill(&mut self, sink: &mut dyn Sink) {
        loop {
            if !self.buf.is_empty() {
                let free = sink.free_frames();
                if free == 0 {
                    return;
                }
                let n = free.min(self.buf.len() / 2);
                sink.write(&self.buf[..n * 2]);
                self.buf.drain(..n * 2);
                self.written += n as u64;
                continue;
            }
            if !self.produce() {
                return;
            }
        }
    }

    /// Turns boundaries that have reached the output into events.
    pub fn update(&mut self, consumed: u64) {
        while self.marks.front().is_some_and(|m| m.frame <= consumed) {
            let Some(mark) = self.marks.pop_front() else {
                break;
            };
            match mark.kind {
                MarkKind::Start(id) => {
                    self.audible = Some(Audible {
                        id,
                        start: mark.frame,
                    });
                    self.events.push(Event::TrackStarted { id });
                }
                MarkKind::End(id, reason) => {
                    let played = match self.audible {
                        Some(a) if a.id == id => {
                            self.audible = None;
                            mark.frame.saturating_sub(a.start)
                        }
                        _ => 0,
                    };
                    self.events.push(Event::TrackEnded {
                        id,
                        played_ms: self.frames_to_ms(played),
                        reason,
                    });
                }
            }
        }
    }

    /// Ends the audible track (or, if none is audible yet, the first one
    /// waiting to start) and continues with what follows it.
    pub fn skip(&mut self, sink: &mut dyn Sink) {
        let consumed = sink.consumed_frames();
        self.update(consumed);

        let target = match self.audible.take() {
            Some(a) => {
                self.events.push(Event::TrackEnded {
                    id: a.id,
                    played_ms: self.frames_to_ms(consumed.saturating_sub(a.start)),
                    reason: EndReason::Skipped,
                });
                a.id
            }
            None => {
                let pending = self.marks.iter().find_map(|m| match m.kind {
                    MarkKind::Start(id) => Some(id),
                    MarkKind::End(..) => None,
                });
                let Some(id) = pending else { return };
                self.events.push(Event::TrackEnded {
                    id,
                    played_ms: 0,
                    reason: EndReason::Skipped,
                });
                id
            }
        };

        // Throw away everything decoded ahead of the output.
        self.marks.clear();
        self.buf.clear();
        self.stage = None;
        sink.flush();

        match self.current.take() {
            // The skipped track was still decoding: move on to the next one.
            Some(deck) if deck.track.id == target => {}
            // Decoding had already moved on to a later track whose start was
            // just flushed. Restart it from the top.
            Some(deck) => match TrackDecoder::open(&deck.track.path) {
                Ok(decoder) => self.start_deck(Deck {
                    track: deck.track,
                    decoder,
                }),
                Err(e) => self.events.push(Event::TrackError {
                    id: deck.track.id,
                    error: e.to_string(),
                }),
            },
            None => {}
        }
    }

    /// Produces more master-rate audio into `buf`. Returns false when there
    /// is nothing more to do until something changes.
    fn produce(&mut self) -> bool {
        let Some(deck) = self.current.as_mut() else {
            if let Some(next) = self.next.take() {
                self.start_deck(next);
                self.events.push(Event::NextNeeded);
                return true;
            }
            // Nothing left to play: drain the resampler's tail so the last
            // track's end becomes audible.
            if let Some(stage) = self.stage.take() {
                self.finish_stage(stage);
                return true;
            }
            return false;
        };

        self.decoded.clear();
        match deck.decoder.decode_next(&mut self.decoded) {
            Ok(true) => {
                if let Some(stage) = self.stage.as_mut() {
                    stage.frames_in += (self.decoded.len() / 2) as u64;
                    match stage.resampler.as_mut() {
                        Some(r) => {
                            if let Err(e) = r.process(&self.decoded, &mut self.buf) {
                                let id = deck.track.id;
                                self.fail_current(id, e.to_string());
                            }
                        }
                        None => self.buf.extend_from_slice(&self.decoded),
                    }
                }
            }
            Ok(false) => self.end_current(EndReason::Finished),
            Err(e) => {
                let id = deck.track.id;
                self.fail_current(id, e.to_string());
            }
        }
        true
    }

    fn fail_current(&mut self, id: TrackId, error: String) {
        self.events.push(Event::TrackError { id, error });
        self.end_current(EndReason::Error);
    }

    fn end_current(&mut self, reason: EndReason) {
        let Some(deck) = self.current.take() else {
            return;
        };
        let frame = self.stage.as_ref().map_or(self.produced(), Stage::position);
        self.marks.push_back(Mark {
            frame,
            kind: MarkKind::End(deck.track.id, reason),
        });
        // Keep the stage running only if the next track can continue it.
        let continues = self.next.as_ref().is_some_and(|next| {
            self.stage
                .as_ref()
                .is_some_and(|s| s.rate == next.decoder.sample_rate())
        });
        if !continues {
            if let Some(stage) = self.stage.take() {
                self.finish_stage(stage);
            }
        }
    }

    fn start_deck(&mut self, deck: Deck) {
        let rate = deck.decoder.sample_rate();
        if self.stage.as_ref().is_some_and(|s| s.rate != rate) {
            if let Some(stage) = self.stage.take() {
                self.finish_stage(stage);
            }
        }
        if self.stage.is_none() {
            let resampler = if rate == self.master_rate {
                None
            } else {
                match StreamResampler::fixed(rate, self.master_rate) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        self.events.push(Event::TrackError {
                            id: deck.track.id,
                            error: e.to_string(),
                        });
                        self.events.push(Event::NextNeeded);
                        return;
                    }
                }
            };
            self.stage = Some(Stage {
                rate,
                base: self.produced(),
                frames_in: 0,
                resampler,
            });
        }
        let frame = self.stage.as_ref().map_or(self.produced(), Stage::position);
        self.marks.push_back(Mark {
            frame,
            kind: MarkKind::Start(deck.track.id),
        });
        self.current = Some(deck);
    }

    fn finish_stage(&mut self, mut stage: Stage) {
        if let Some(r) = stage.resampler.as_mut() {
            // On failure the tail is lost; boundaries still resolve because
            // `consumed` catches up with `written` once the output drains.
            let _ = r.finish(&mut self.buf);
        }
    }

    /// Total master frames produced: written plus buffered.
    fn produced(&self) -> u64 {
        self.written + (self.buf.len() / 2) as u64
    }

    fn frames_to_ms(&self, frames: u64) -> u64 {
        frames * 1000 / u64::from(self.master_rate)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::f64::consts::TAU;
    use std::path::{Path, PathBuf};

    /// Collects everything and plays it instantly, accepting `budget`
    /// frames per round like a device buffer that drains between ticks.
    #[derive(Default)]
    pub struct VecSink {
        pub out: Vec<f32>,
        pub budget: usize,
    }

    impl Sink for VecSink {
        fn free_frames(&mut self) -> usize {
            self.budget
        }
        fn write(&mut self, samples: &[f32]) {
            self.budget -= samples.len() / 2;
            self.out.extend_from_slice(samples);
        }
        fn consumed_frames(&self) -> u64 {
            (self.out.len() / 2) as u64
        }
        fn flush(&mut self) {}
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("djinn-engine-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A continuous stereo sine, as 16-bit samples, from `start` to `end` frames.
    pub fn sine_i16(rate: u32, start: usize, end: usize) -> Vec<i16> {
        (start..end)
            .flat_map(|i| {
                let t = i as f64 / f64::from(rate);
                let l = (TAU * 440.0 * t).sin() * 0.5;
                let r = (TAU * 660.0 * t).sin() * 0.5;
                [(l * 32767.0) as i16, (r * 32767.0) as i16]
            })
            .collect()
    }

    pub fn write_wav(path: &Path, rate: u32, samples: &[i16]) {
        let data_len = (samples.len() * 2) as u32;
        let mut b = Vec::with_capacity(44 + samples.len() * 2);
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes()); // PCM
        b.extend_from_slice(&2u16.to_le_bytes()); // channels
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    pub fn track(id: i64, path: &Path) -> Track {
        Track {
            id: TrackId(id),
            path: path.to_path_buf(),
        }
    }

    /// Plays tracks back to back the way the engine does, answering every
    /// NextNeeded with the next file. Returns output and events.
    pub fn play_all(paths: &[PathBuf]) -> (Vec<f32>, Vec<Event>) {
        let mut player = Player::new(48_000);
        let mut sink = VecSink::default();
        let mut queue = paths.iter().enumerate();
        let mut events = Vec::new();
        if let Some((i, p)) = queue.next() {
            player.set_next(Some(track(i as i64, p)));
        }
        for _ in 0..100_000 {
            sink.budget = 4096;
            player.fill(&mut sink);
            player.update(sink.consumed_frames());
            let new: Vec<Event> = player.take_events().collect();
            let mut progressed = !new.is_empty();
            for e in new {
                if e == Event::NextNeeded {
                    if let Some((i, p)) = queue.next() {
                        player.set_next(Some(track(i as i64, p)));
                        progressed = true;
                    }
                }
                events.push(e);
            }
            if !progressed && !player.is_active() {
                break;
            }
        }
        (sink.out, events)
    }

    fn to_f32(samples: &[i16]) -> Vec<f32> {
        samples.iter().map(|&s| f32::from(s) / 32768.0).collect()
    }

    #[test]
    fn gapless_wav_is_bit_exact() {
        let dir = temp_dir("gapless-wav");
        // Split a continuous signal at an arbitrary, odd frame.
        let (n1, n2) = (12_345, 20_001);
        let all = sine_i16(48_000, 0, n1 + n2);
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        write_wav(&a, 48_000, &all[..n1 * 2]);
        write_wav(&b, 48_000, &all[n1 * 2..]);

        let (out, events) = play_all(&[a, b]);
        assert_eq!(out, to_f32(&all), "output differs from the uncut signal");

        let ended: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::TrackEnded {
                    id,
                    played_ms,
                    reason,
                } => Some((*id, *played_ms, *reason)),
                _ => None,
            })
            .collect();
        assert_eq!(
            ended,
            [
                (TrackId(0), (n1 * 1000 / 48_000) as u64, EndReason::Finished),
                (TrackId(1), (n2 * 1000 / 48_000) as u64, EndReason::Finished),
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gapless_resampled_has_no_seam() {
        let dir = temp_dir("gapless-44k");
        let (n1, n2) = (30_001, 30_000);
        let all = sine_i16(44_100, 0, n1 + n2);
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        write_wav(&a, 44_100, &all[..n1 * 2]);
        write_wav(&b, 44_100, &all[n1 * 2..]);

        let (out, _) = play_all(&[a, b]);
        let expected_frames = ((n1 + n2) as u64 * 48_000 + 22_050) / 44_100;
        assert_eq!(out.len() / 2, expected_frames as usize);

        // Compare with the ideal signal rendered directly at 48 kHz. The seam
        // sits near frame 32654; only the stream's outer edges may deviate.
        let ideal = to_f32(&sine_i16(48_000, 0, out.len() / 2));
        let margin = 2_000 * 2;
        let max_err = out[margin..out.len() - margin]
            .iter()
            .zip(&ideal[margin..])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(max_err < 2e-3, "seam or misalignment, max error {max_err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rate_change_keeps_timing() {
        let dir = temp_dir("rate-change");
        let a = dir.join("a.wav");
        let b = dir.join("b.wav");
        write_wav(&a, 44_100, &sine_i16(44_100, 0, 4_410));
        write_wav(&b, 48_000, &sine_i16(48_000, 0, 4_800));
        let (out, events) = play_all(&[a, b]);
        assert_eq!(out.len() / 2, 4_800 + 4_800);
        assert!(events.contains(&Event::TrackStarted { id: TrackId(1) }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unreadable_track_reports_and_asks_again() {
        let mut player = Player::new(48_000);
        player.set_next(Some(track(7, Path::new("/nonexistent/file.flac"))));
        let events: Vec<_> = player.take_events().collect();
        assert!(matches!(
            events[0],
            Event::TrackError { id: TrackId(7), .. }
        ));
        assert_eq!(events[1], Event::NextNeeded);
        assert!(!player.is_active());
    }

    #[test]
    fn skip_restarts_a_track_whose_start_was_flushed() {
        let dir = temp_dir("skip");
        let a = dir.join("a.wav");
        let b = dir.join("b.wav");
        write_wav(&a, 48_000, &sine_i16(48_000, 0, 1_000));
        write_wav(&b, 48_000, &sine_i16(48_000, 0, 48_000));

        // A sink that holds audio without playing it, like a full device buffer.
        struct Held {
            written: u64,
            consumed: u64,
        }
        impl Sink for Held {
            fn free_frames(&mut self) -> usize {
                (4_000u64.saturating_sub(self.written - self.consumed)) as usize
            }
            fn write(&mut self, samples: &[f32]) {
                self.written += (samples.len() / 2) as u64;
            }
            fn consumed_frames(&self) -> u64 {
                self.consumed
            }
            fn flush(&mut self) {
                self.consumed = self.written;
            }
        }

        let mut player = Player::new(48_000);
        let mut sink = Held {
            written: 0,
            consumed: 0,
        };
        player.set_next(Some(track(0, &a)));
        player.fill(&mut sink);
        player.set_next(Some(track(1, &b)));
        player.fill(&mut sink);
        sink.consumed = 500; // A is audible, B is decoding but not audible.
        player.update(sink.consumed);
        player.take_events().for_each(drop);

        player.skip(&mut sink);
        let after_skip = sink.written;
        player.fill(&mut sink);
        player.update(sink.consumed);
        let events: Vec<_> = player.take_events().collect();
        assert!(events.contains(&Event::TrackEnded {
            id: TrackId(0),
            played_ms: 500 * 1000 / 48_000,
            reason: EndReason::Skipped
        }));
        assert!(events.contains(&Event::TrackStarted { id: TrackId(1) }));
        // B restarted from its first frame: a full buffer's worth was written
        // again after the flush.
        assert_eq!(sink.written - after_skip, 4_000);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// The fixture signal (see tests/fixtures/README.md) rendered at 48 kHz.
    fn fixture_tone(frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f64 / 48_000.0;
                let s = ((TAU * 440.0 * t).sin() * 0.4 + (TAU * 1000.0 * t).sin() * 0.1) as f32;
                [s, s]
            })
            .collect()
    }

    const FIXTURE_FRAMES: (u64, u64) = (13_337, 12_000);

    /// Plays a fixture pair and returns (worst error in 10 ms around the
    /// seam, worst error elsewhere), ignoring the stream's outer edges.
    fn seam_errors(ext: &str) -> (f32, f32) {
        let paths = [
            fixture(&format!("gapless_a.{ext}")),
            fixture(&format!("gapless_b.{ext}")),
        ];
        let (out, _) = play_all(&paths);
        let (n1, n2) = FIXTURE_FRAMES;
        let expected = ((n1 + n2) * 48_000 + 22_050) / 44_100;
        assert_eq!(
            out.len() / 2,
            expected as usize,
            "{ext}: wrong length, delay/padding not trimmed"
        );

        let seam = ((n1 * 48_000 + 22_050) / 44_100) as usize;
        let ideal = fixture_tone(out.len() / 2);
        let (mut at_seam, mut elsewhere) = (0.0f32, 0.0f32);
        for frame in 2_000..out.len() / 2 - 2_000 {
            let err = (out[frame * 2] - ideal[frame * 2]).abs();
            if frame.abs_diff(seam) < 480 {
                at_seam = at_seam.max(err);
            } else {
                elsewhere = elsewhere.max(err);
            }
        }
        (at_seam, elsewhere)
    }

    #[test]
    fn gapless_flac_has_no_seam() {
        let (seam, elsewhere) = seam_errors("flac");
        assert!(seam < 1e-3, "seam error {seam}");
        assert!(elsewhere < 1e-3, "error {elsewhere}");
    }

    #[test]
    fn gapless_mp3_has_no_seam() {
        // Lossy, so compare the seam against the codec's own error level. A
        // gap or untrimmed encoder delay shows up as an error near 0.5.
        let (seam, elsewhere) = seam_errors("mp3");
        assert!(seam < 0.05, "seam error {seam}");
        assert!(
            seam < elsewhere * 3.0,
            "seam {seam} vs elsewhere {elsewhere}"
        );
    }
}
