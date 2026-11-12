//! A timed copy of the master stream, for nodes: other boxes that play the
//! same audio in step with this one's speakers.
//!
//! Every chunk the engine sends to its outputs is copied here with the time
//! its first frame will be heard on this computer's clock output (its own
//! delay aside), on the shared [`clock`](crate::clock). Pausing, resuming,
//! flushing (a skip or seek) and volume changes go along as marks. A node
//! that matches clocks with this one plays each chunk when its time comes.
//!
//! Lock-free like the cast tap: the engine thread writes, a reader drains. A
//! chunk that doesn't fit (the reader stalled) is dropped whole; the next
//! one's time says where it belongs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

/// What happened, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    /// `frames` frames of interleaved stereo follow, the first heard at
    /// `play_at_us` on the shared clock.
    Audio {
        frames: u32,
        play_at_us: i64,
    },
    Pause,
    Resume,
    /// Everything not yet heard was thrown away (a skip or seek).
    Flush,
    /// Master volume, 0.0 to 1.0.
    Volume(f32),
}

/// What a reader gets: a mark, with its samples for audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub mark: Mark,
    pub samples: Vec<f32>,
}

/// The engine's end.
pub struct TimedTap {
    samples: Producer<f32>,
    marks: Producer<Mark>,
    active: Arc<AtomicBool>,
}

/// The reader's end.
pub struct TimedReader {
    samples: Consumer<f32>,
    marks: Consumer<Mark>,
    active: Arc<AtomicBool>,
}

/// A timed tap holding up to `seconds` of interleaved stereo at `rate`.
pub fn timed_tap(seconds: f32, rate: u32) -> (TimedTap, TimedReader) {
    let capacity = ((seconds.max(0.1) * rate as f32) as usize) * 2;
    let (samples_in, samples_out) = RingBuffer::new(capacity);
    let (marks_in, marks_out) = RingBuffer::new(4096);
    let active = Arc::new(AtomicBool::new(false));
    (
        TimedTap {
            samples: samples_in,
            marks: marks_in,
            active: Arc::clone(&active),
        },
        TimedReader {
            samples: samples_out,
            marks: marks_out,
            active,
        },
    )
}

impl TimedTap {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Copies a chunk in, whole or not at all.
    pub(crate) fn write(&mut self, samples: &[f32], play_at_us: i64) {
        if !self.is_active() || samples.len() < 2 {
            return;
        }
        let samples = &samples[..samples.len() & !1];
        if self.samples.slots() < samples.len() || self.marks.slots() == 0 {
            return;
        }
        let frames = (samples.len() / 2) as u32;
        if self.marks.push(Mark::Audio { frames, play_at_us }).is_ok() {
            let _ = self.samples.push_partial_slice(samples);
        }
    }

    /// Notes a pause, resume, flush or volume change.
    pub(crate) fn mark(&mut self, mark: Mark) {
        if self.is_active() {
            let _ = self.marks.push(mark);
        }
    }
}

impl TimedReader {
    /// Turns the tap on (a node is listening) or off.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Everything written so far, in order.
    pub fn read(&mut self) -> Vec<Piece> {
        let mut pieces = Vec::new();
        while let Ok(mark) = self.marks.pop() {
            let samples = match mark {
                Mark::Audio { frames, .. } => {
                    let wanted = frames as usize * 2;
                    let mut samples = Vec::with_capacity(wanted);
                    if let Ok(chunk) = self.samples.read_chunk(wanted.min(self.samples.slots())) {
                        let (a, b) = chunk.as_slices();
                        samples.extend_from_slice(a);
                        samples.extend_from_slice(b);
                        chunk.commit_all();
                    }
                    samples
                }
                _ => Vec::new(),
            };
            pieces.push(Piece { mark, samples });
        }
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_come_out_whole_with_their_times_and_marks() {
        let (mut tap, mut reader) = timed_tap(1.0, 1000);
        tap.write(&[0.1; 20], 5);
        assert!(reader.read().is_empty(), "nothing while nobody listens");
        reader.set_active(true);
        tap.write(&[0.1; 20], 5);
        tap.mark(Mark::Pause);
        tap.write(&[0.2; 10], 9);
        let pieces = reader.read();
        assert_eq!(pieces.len(), 3);
        assert_eq!(
            pieces[0].mark,
            Mark::Audio {
                frames: 10,
                play_at_us: 5
            }
        );
        assert_eq!(pieces[0].samples.len(), 20);
        assert_eq!(pieces[1].mark, Mark::Pause);
        assert_eq!(pieces[2].samples, vec![0.2; 10]);
    }

    #[test]
    fn a_chunk_that_does_not_fit_is_dropped_whole() {
        let (mut tap, mut reader) = timed_tap(0.1, 100);
        reader.set_active(true);
        tap.write(&[0.5; 16], 1);
        tap.write(&[0.5; 16], 2);
        let pieces = reader.read();
        assert_eq!(pieces.len(), 1, "the second didn't fit");
        assert_eq!(pieces[0].samples.len(), 16);
    }
}
