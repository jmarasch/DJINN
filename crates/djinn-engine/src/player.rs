//! Track sequencing: decodes the current track, rolls over to the next one
//! without a gap or with a crossfade, and turns boundaries into events when
//! they become audible.
//!
//! Pipeline: decoder (source rate) -> stage (resample to master) -> held
//! tail -> sink. Consecutive tracks at the same source rate share one
//! resampling stage, so a gapless album is resampled as one continuous
//! signal with no edge artifacts at track boundaries.
//!
//! With a crossfade of N seconds, the last N seconds of the current track
//! are held back. When the track ends, that tail is mixed with the start of
//! the next track using equal-power curves. The next track starts (and its
//! position counts from) the start of the blend; the previous one ends when
//! its tail runs out. With N = 0, or for tracks marked gapless, nothing is
//! held and tracks join sample-exactly.
//!
//! A skip with a crossfade set blends too, over a shorter stretch (at most
//! [`SKIP_FADE_SECS`]): what was decoded of the skipped track but not yet
//! written fades out under the start of the next one.

use std::collections::VecDeque;
use std::f32::consts::FRAC_PI_2;

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

/// Longest crossfade, in seconds.
pub const MAX_CROSSFADE_SECS: u32 = 12;
/// Longest blend on a skip, so skipping still feels quick.
pub const SKIP_FADE_SECS: u32 = 2;

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

/// The end of the previous track, being blended into the start of the next.
struct Mix {
    /// Interleaved stereo tail of the previous track.
    tail: Vec<f32>,
    /// Frames of `tail` blended so far.
    done: usize,
}

impl Mix {
    fn frames(&self) -> usize {
        self.tail.len() / 2
    }

    /// Equal-power gains (outgoing, incoming) at blend frame `i`.
    fn gains(&self, i: usize) -> (f32, f32) {
        let t = (i as f32 + 0.5) / self.frames() as f32 * FRAC_PI_2;
        (t.cos(), t.sin())
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

pub(crate) struct Player {
    master_rate: u32,
    /// Crossfade length; 0 joins tracks gaplessly.
    fade_frames: usize,
    current: Option<Deck>,
    next: Option<Deck>,
    stage: Option<Stage>,
    /// Scratch for decoded source-rate audio.
    decoded: Vec<f32>,
    /// Scratch for resampled audio on its way to `held`.
    staged: Vec<f32>,
    /// The newest master-rate audio, held back for a crossfade.
    held: VecDeque<f32>,
    mix: Option<Mix>,
    /// Master-rate audio ready to write to the sink.
    buf: Vec<f32>,
    /// Master frames written to the sink so far.
    written: u64,
    /// Boundaries not yet audible, in frame order.
    marks: VecDeque<Mark>,
    /// Audible tracks and where each started; during a crossfade there are
    /// two. The last is the one reported as playing.
    audible: Vec<(TrackId, u64)>,
    events: Vec<Event>,
}

impl Player {
    pub fn new(master_rate: u32) -> Self {
        Self {
            master_rate,
            fade_frames: 0,
            current: None,
            next: None,
            stage: None,
            decoded: Vec::new(),
            staged: Vec::new(),
            held: VecDeque::new(),
            mix: None,
            buf: Vec::new(),
            written: 0,
            marks: VecDeque::new(),
            audible: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn take_events(&mut self) -> std::vec::Drain<'_, Event> {
        self.events.drain(..)
    }

    /// Sets the crossfade for upcoming transitions (0 for gapless).
    pub fn set_crossfade(&mut self, secs: u32) {
        let secs = secs.min(MAX_CROSSFADE_SECS);
        self.fade_frames = (secs * self.master_rate) as usize;
    }

    /// True while anything is playing or queued up to play.
    pub fn is_active(&self) -> bool {
        !self.audible.is_empty()
            || self.current.is_some()
            || self.next.is_some()
            || !self.marks.is_empty()
            || !self.held.is_empty()
    }

    /// Position of the audible track in milliseconds.
    pub fn position(&self, consumed: u64) -> Option<(TrackId, u64)> {
        self.audible
            .last()
            .map(|&(id, start)| (id, self.frames_to_ms(consumed.saturating_sub(start))))
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
                self.events.push(Event::NextNeeded { after: None });
            }
        }
    }

    /// Decodes and writes until the sink is full or there is nothing to play.
    pub fn fill(&mut self, sink: &mut dyn Sink) {
        loop {
            self.release_held(false);
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
                    self.audible.push((id, mark.frame));
                    self.events.push(Event::TrackStarted { id });
                }
                MarkKind::End(id, reason) => {
                    let played = match self.audible.iter().position(|&(a, _)| a == id) {
                        Some(i) => mark.frame.saturating_sub(self.audible.remove(i).1),
                        None => 0,
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
    /// waiting to start) and continues with what follows it. A track fading
    /// out under it ends too.
    pub fn skip(&mut self, sink: &mut dyn Sink) {
        let consumed = sink.consumed_frames();
        self.update(consumed);
        if self.skip_with_fade(sink, consumed) {
            return;
        }

        let target = match self.audible.pop() {
            Some((id, start)) => {
                self.events.push(Event::TrackEnded {
                    id,
                    played_ms: self.frames_to_ms(consumed.saturating_sub(start)),
                    reason: EndReason::Skipped,
                });
                id
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
        // The track it was crossfading from goes with it.
        for (id, start) in self.audible.drain(..) {
            self.events.push(Event::TrackEnded {
                id,
                played_ms: (consumed.saturating_sub(start)) * 1000 / u64::from(self.master_rate),
                reason: EndReason::Finished,
            });
        }

        // Throw away everything decoded ahead of the output.
        self.marks.clear();
        self.buf.clear();
        self.held.clear();
        self.mix = None;
        self.stage = None;
        sink.flush();

        match self.current.take() {
            // The skipped track was still decoding: move on to the next one.
            Some(deck) if deck.track.id == target => {}
            // Decoding had already moved on to a later track whose start was
            // just flushed. Restart it from the top.
            Some(deck) => match TrackDecoder::open(&deck.track.path) {
                Ok(decoder) => {
                    self.start_deck(
                        Deck {
                            track: deck.track,
                            decoder,
                        },
                        0,
                    );
                }
                Err(e) => self.events.push(Event::TrackError {
                    id: deck.track.id,
                    error: e.to_string(),
                }),
            },
            None => {}
        }
    }

    /// With a crossfade set, blends the skipped track into the next one
    /// instead of cutting it off. Like a hard skip it drops what the output
    /// holds (a second's worth), so the blend starts right away; what was
    /// decoded after that fades out under the start of the next track. Only
    /// in the plain case of one track playing, still decoding, and the next
    /// one ready; returns false otherwise, for a hard cut.
    fn skip_with_fade(&mut self, sink: &mut dyn Sink, consumed: u64) -> bool {
        let playing = self.audible.last().map(|&(id, _)| id);
        let plain = self.fade_frames > 0
            && self.audible.len() == 1
            && self.mix.is_none()
            && self.marks.is_empty()
            && self.next.is_some()
            && self.current.as_ref().map(|deck| deck.track.id) == playing;
        if !plain {
            return false;
        }
        let (Some((id, start)), Some(next)) = (self.audible.pop(), self.next.take()) else {
            return false;
        };
        self.events.push(Event::TrackEnded {
            id,
            played_ms: self.frames_to_ms(consumed.saturating_sub(start)),
            reason: EndReason::Skipped,
        });
        let fade = self
            .fade_frames
            .min((SKIP_FADE_SECS * self.master_rate) as usize);
        let mut tail: Vec<f32> = self.buf.drain(..).chain(self.held.drain(..)).collect();
        tail.truncate(fade * 2);
        sink.flush();
        // The skipped track's decoder and resampler go; the next track
        // starts a stage of its own.
        self.current = None;
        self.stage = None;
        let next_id = next.track.id;
        let overlap = tail.len() / 2;
        if self.start_deck(next, overlap) {
            if overlap > 0 {
                self.mix = Some(Mix { tail, done: 0 });
            }
            self.events.push(Event::NextNeeded {
                after: Some(next_id),
            });
        }
        true
    }

    /// Produces more master-rate audio. Returns false when there is
    /// nothing more to do until something changes.
    fn produce(&mut self) -> bool {
        let Some(deck) = self.current.as_mut() else {
            if let Some(next) = self.next.take() {
                let id = next.track.id;
                if self.start_deck(next, 0) {
                    self.events.push(Event::NextNeeded { after: Some(id) });
                }
                return true;
            }
            // Nothing left to play: drain the resampler and the held tail so
            // the last track's end becomes audible.
            if let Some(stage) = self.stage.take() {
                self.finish_stage(stage);
                return true;
            }
            if !self.held.is_empty() {
                self.release_held(true);
                return true;
            }
            return false;
        };

        self.decoded.clear();
        match deck.decoder.decode_next(&mut self.decoded) {
            Ok(true) => {
                let id = deck.track.id;
                let gain = deck.track.gain;
                if gain != 1.0 {
                    for sample in &mut self.decoded {
                        *sample *= gain;
                    }
                }
                if let Some(stage) = self.stage.as_mut() {
                    stage.frames_in += (self.decoded.len() / 2) as u64;
                    self.staged.clear();
                    let result = match stage.resampler.as_mut() {
                        Some(r) => r.process(&self.decoded, &mut self.staged),
                        None => {
                            self.staged.extend_from_slice(&self.decoded);
                            Ok(())
                        }
                    };
                    match result {
                        Ok(()) => {
                            let staged = std::mem::take(&mut self.staged);
                            self.emit(&staged);
                            self.staged = staged;
                        }
                        Err(e) => self.fail_current(id, e.to_string()),
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

    /// Sends new audio of the current track on: blended into the outgoing
    /// track's tail during a crossfade, then held back.
    fn emit(&mut self, samples: &[f32]) {
        let mut rest = samples;
        if let Some(mix) = self.mix.as_mut() {
            let n = (mix.frames() - mix.done).min(rest.len() / 2);
            for i in 0..n {
                let (out, inc) = mix.gains(mix.done + i);
                for c in 0..2 {
                    let tail = mix.tail[(mix.done + i) * 2 + c];
                    self.held.push_back(tail * out + rest[i * 2 + c] * inc);
                }
            }
            mix.done += n;
            rest = &rest[n * 2..];
            if mix.done == mix.frames() {
                self.mix = None;
            }
        }
        self.held.extend(rest.iter().copied());
    }

    /// Finishes a blend early (the incoming track was shorter than it):
    /// the rest of the outgoing tail fades out alone.
    fn finish_mix(&mut self) {
        if let Some(mix) = self.mix.take() {
            for i in mix.done..mix.frames() {
                let (out, _) = mix.gains(i);
                self.held.push_back(mix.tail[i * 2] * out);
                self.held.push_back(mix.tail[i * 2 + 1] * out);
            }
        }
    }

    /// Moves held audio on to the sink buffer, keeping back a crossfade's
    /// worth unless `all`.
    fn release_held(&mut self, all: bool) {
        let keep = if all || self.current.is_none() && self.next.is_none() {
            0
        } else {
            self.fade_frames * 2
        };
        if self.held.len() > keep {
            let n = (self.held.len() - keep) & !1;
            self.buf.extend(self.held.drain(..n));
        }
    }

    fn fail_current(&mut self, id: TrackId, error: String) {
        self.events.push(Event::TrackError { id, error });
        self.end_current(EndReason::Error);
    }

    fn end_current(&mut self, reason: EndReason) {
        let Some(deck) = self.current.take() else {
            return;
        };
        // The next track decides how this one hands over.
        let crossfade = self.fade_frames > 0
            && self.next.as_ref().is_some_and(|next| !next.track.gapless)
            && reason == EndReason::Finished;
        let continues = !crossfade
            && self.next.as_ref().is_some_and(|next| {
                self.stage
                    .as_ref()
                    .is_some_and(|s| s.rate == next.decoder.sample_rate())
            });
        if !continues {
            if let Some(stage) = self.stage.take() {
                self.finish_stage(stage);
            }
        }
        // A blend still running means this track was shorter than it.
        self.finish_mix();
        let end = self.stage.as_ref().map_or(self.produced(), Stage::position);
        self.push_mark(end, MarkKind::End(deck.track.id, reason));

        if !crossfade {
            self.release_held(true);
            return;
        }
        // Keep the last stretch of this track to blend into the next one.
        let fade = self.fade_frames.min(self.held.len() / 2);
        let before = self.held.len() - fade * 2;
        self.buf.extend(self.held.drain(..before));
        let tail: Vec<f32> = self.held.drain(..).collect();
        if let Some(next) = self.next.take() {
            let id = next.track.id;
            if self.start_deck(next, fade) {
                if fade > 0 {
                    self.mix = Some(Mix { tail, done: 0 });
                }
                self.events.push(Event::NextNeeded { after: Some(id) });
            } else {
                // The next track couldn't start; let this one end normally.
                self.held.extend(tail);
                self.release_held(true);
            }
        }
    }

    /// Makes `deck` current, starting `overlap` frames before the end of
    /// what was produced so far (for a crossfade). Returns false (after
    /// reporting) if it could not be started.
    fn start_deck(&mut self, deck: Deck, overlap: usize) -> bool {
        let rate = deck.decoder.sample_rate();
        if self.stage.as_ref().is_some_and(|s| s.rate != rate) || overlap > 0 {
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
                        self.events.push(Event::NextNeeded { after: None });
                        return false;
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
        self.push_mark(frame, MarkKind::Start(deck.track.id));
        self.current = Some(deck);
        true
    }

    fn finish_stage(&mut self, mut stage: Stage) {
        if let Some(r) = stage.resampler.as_mut() {
            // On failure the tail is lost; boundaries still resolve because
            // `consumed` catches up with `written` once the output drains.
            let mut tail = std::mem::take(&mut self.staged);
            tail.clear();
            let _ = r.finish(&mut tail);
            self.emit(&tail);
            self.staged = tail;
        }
    }

    /// Adds a boundary, keeping marks in frame order (a crossfade's start
    /// comes before the previous track's end).
    fn push_mark(&mut self, frame: u64, kind: MarkKind) {
        let at = self.marks.partition_point(|m| m.frame <= frame);
        self.marks.insert(at, Mark { frame, kind });
    }

    /// Total master frames produced: written, buffered, and held. During a
    /// blend, the outgoing tail's frames are where the incoming track's go,
    /// so they don't count twice.
    fn produced(&self) -> u64 {
        self.written + (self.buf.len() / 2) as u64 + (self.held.len() / 2) as u64
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
            gapless: false,
            gain: 1.0,
        }
    }

    /// Plays tracks back to back the way the engine does, answering every
    /// NextNeeded with the next file. Returns output and events.
    pub fn play_all(paths: &[PathBuf]) -> (Vec<f32>, Vec<Event>) {
        play_all_with(paths, 0, &[])
    }

    /// Like [`play_all`] with a crossfade, marking the tracks at the given
    /// indexes as gapless.
    pub fn play_all_with(
        paths: &[PathBuf],
        crossfade: u32,
        gapless: &[usize],
    ) -> (Vec<f32>, Vec<Event>) {
        let track = |i: usize, p: &Path| Track {
            gapless: gapless.contains(&i),
            gain: 1.0,
            ..track(i as i64, p)
        };
        let mut player = Player::new(48_000);
        player.set_crossfade(crossfade);
        let mut sink = VecSink::default();
        let mut queue = paths.iter().enumerate();
        let mut events = Vec::new();
        if let Some((i, p)) = queue.next() {
            player.set_next(Some(track(i, p)));
        }
        for _ in 0..100_000 {
            sink.budget = 4096;
            player.fill(&mut sink);
            player.update(sink.consumed_frames());
            let new: Vec<Event> = player.take_events().collect();
            let mut progressed = !new.is_empty();
            for e in new {
                if matches!(e, Event::NextNeeded { .. }) {
                    if let Some((i, p)) = queue.next() {
                        player.set_next(Some(track(i, p)));
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
        let promoted: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::NextNeeded { after } => Some(*after),
                _ => None,
            })
            .collect();
        assert_eq!(promoted, [Some(TrackId(0)), Some(TrackId(1))]);

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
    fn track_gain_scales_the_signal() {
        let dir = temp_dir("gain");
        let samples = sine_i16(48_000, 0, 4_800);
        let path = dir.join("a.wav");
        write_wav(&path, 48_000, &samples);

        let mut player = Player::new(48_000);
        player.set_next(Some(Track {
            gain: 0.5,
            ..track(0, &path)
        }));
        let mut sink = VecSink::default();
        for _ in 0..100 {
            sink.budget = 4096;
            player.fill(&mut sink);
        }
        let expected: Vec<f32> = to_f32(&samples).iter().map(|s| s * 0.5).collect();
        assert_eq!(sink.out, expected);
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
        assert_eq!(events[1], Event::NextNeeded { after: None });
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

    /// A steady level, so a blend's curves are easy to check.
    fn level_i16(frames: usize, value: i16) -> Vec<i16> {
        vec![value; frames * 2]
    }

    #[test]
    fn crossfade_blends_with_equal_power_and_overlaps_events() {
        let dir = temp_dir("crossfade");
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        let n = 96_000; // 2 s each
        let tone = sine_i16(48_000, 0, n);
        let level = level_i16(n, 8_000);
        write_wav(&a, 48_000, &tone);
        write_wav(&b, 48_000, &level);

        let (out, events) = play_all_with(&[a, b], 1, &[]);
        let fade = 48_000;
        assert_eq!(out.len() / 2, 2 * n - fade, "the blend overlaps one second");
        let (tone, level) = (to_f32(&tone), to_f32(&level));
        // Before the blend: the first track as is.
        assert_eq!(out[..(n - fade) * 2], tone[..(n - fade) * 2]);
        // In the blend: cos/sin weighted sum.
        for i in [0, 1, 12_000, 24_000, 47_999] {
            let t = (i as f32 + 0.5) / fade as f32 * FRAC_PI_2;
            let want = tone[(n - fade + i) * 2] * t.cos() + level[i * 2] * t.sin();
            let got = out[(n - fade + i) * 2];
            assert!((got - want).abs() < 1e-5, "frame {i}: {got} vs {want}");
        }
        // After it: the second track, untouched.
        assert_eq!(out[n * 2..], level[fade * 2..]);

        let started = events
            .iter()
            .position(|e| *e == Event::TrackStarted { id: TrackId(1) });
        let ended = events
            .iter()
            .position(|e| matches!(e, Event::TrackEnded { id: TrackId(0), .. }));
        assert!(
            started < ended,
            "the next track starts before the last one ends"
        );
        let played: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::TrackEnded { id, played_ms, .. } => Some((*id, *played_ms)),
                _ => None,
            })
            .collect();
        assert_eq!(played, [(TrackId(0), 2_000), (TrackId(1), 2_000)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gapless_tracks_skip_the_crossfade() {
        let dir = temp_dir("crossfade-gapless");
        let (n1, n2) = (30_001, 50_000);
        let all = sine_i16(48_000, 0, n1 + n2);
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        write_wav(&a, 48_000, &all[..n1 * 2]);
        write_wav(&b, 48_000, &all[n1 * 2..]);
        let (out, _) = play_all_with(&[a, b], 1, &[1]);
        assert_eq!(out, to_f32(&all), "bit exact despite the crossfade setting");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn incoming_track_shorter_than_the_crossfade() {
        let dir = temp_dir("crossfade-short");
        let paths = [dir.join("a.wav"), dir.join("b.wav"), dir.join("c.wav")];
        write_wav(&paths[0], 48_000, &sine_i16(48_000, 0, 96_000));
        write_wav(&paths[1], 48_000, &level_i16(12_000, 4_000)); // 0.25 s
        write_wav(&paths[2], 48_000, &sine_i16(48_000, 0, 96_000));
        let (out, events) = play_all_with(&paths, 1, &[]);
        for id in 0..3 {
            assert!(
                events.contains(&Event::TrackStarted { id: TrackId(id) }),
                "{id} started"
            );
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, Event::TrackEnded { id: t, .. } if *t == TrackId(id))),
                "{id} ended"
            );
        }
        assert!(out.iter().all(|s| s.abs() <= 1.0));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skipping_with_a_crossfade_blends_into_the_next_track() {
        let dir = temp_dir("crossfade-skip-blend");
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        let tone = sine_i16(48_000, 0, 192_000); // 4 s
        let level = level_i16(96_000, 8_000); // 2 s
        write_wav(&a, 48_000, &tone);
        write_wav(&b, 48_000, &level);
        let mut player = Player::new(48_000);
        player.set_crossfade(1);
        let mut sink = VecSink::default();
        player.set_next(Some(track(0, &a)));
        sink.budget = 1_000;
        player.fill(&mut sink);
        player.set_next(Some(track(1, &b)));
        sink.budget = 59_000;
        player.fill(&mut sink);
        player.update(sink.consumed_frames());
        player.take_events().for_each(drop);

        let at = sink.out.len() / 2;
        player.skip(&mut sink);
        let mut events: Vec<Event> = player.take_events().collect();
        for _ in 0..1_000 {
            sink.budget = 4_096;
            player.fill(&mut sink);
            player.update(sink.consumed_frames());
            events.extend(player.take_events());
            if !player.is_active() {
                break;
            }
        }

        // The skipped track fades out over the crossfade (1 s, under the
        // 2 s cap) while the next one fades in, then the next one alone.
        let fade = 48_000;
        let (tone, level) = (to_f32(&tone), to_f32(&level));
        assert_eq!(sink.out.len() / 2, at + 96_000);
        for i in [0, 1, 24_000, 47_999] {
            let t = (i as f32 + 0.5) / fade as f32 * FRAC_PI_2;
            let want = tone[(at + i) * 2] * t.cos() + level[i * 2] * t.sin();
            let got = sink.out[(at + i) * 2];
            assert!((got - want).abs() < 1e-5, "frame {i}: {got} vs {want}");
        }
        assert_eq!(sink.out[(at + fade) * 2..], level[fade * 2..]);
        assert!(events.iter().any(|e| matches!(
            e,
            Event::TrackEnded {
                id: TrackId(0),
                reason: EndReason::Skipped,
                ..
            }
        )));
        assert!(events.contains(&Event::TrackStarted { id: TrackId(1) }));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skipping_during_a_crossfade_ends_both_tracks() {
        let dir = temp_dir("crossfade-skip");
        let (a, b, c) = (dir.join("a.wav"), dir.join("b.wav"), dir.join("c.wav"));
        for p in [&a, &b, &c] {
            write_wav(p, 48_000, &sine_i16(48_000, 0, 96_000));
        }
        let mut player = Player::new(48_000);
        player.set_crossfade(1);
        let mut sink = VecSink::default();
        player.set_next(Some(track(0, &a)));
        sink.budget = 1_000;
        player.fill(&mut sink);
        // The next track is lined up while the first one plays, as the
        // engine's owner does on NextNeeded.
        player.set_next(Some(track(1, &b)));
        sink.budget = 69_000; // into the blend, which starts at 48 000
        player.fill(&mut sink);
        player.update(sink.consumed_frames());
        player.set_next(Some(track(2, &c)));
        player.take_events().for_each(drop);

        player.skip(&mut sink);
        let events: Vec<_> = player.take_events().collect();
        let ended: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::TrackEnded { id, reason, .. } => Some((*id, *reason)),
                _ => None,
            })
            .collect();
        assert!(ended.contains(&(TrackId(1), EndReason::Skipped)));
        assert!(ended.contains(&(TrackId(0), EndReason::Finished)));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
