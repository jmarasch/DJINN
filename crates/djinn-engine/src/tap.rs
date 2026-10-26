//! A copy of the master stream for network outputs (casting).
//!
//! The engine writes every master frame it sends to the outputs into a
//! lock-free ring while a reader is active; the reader (the server's
//! encoder thread) drains it. With no local output open, an active tap
//! makes the engine pace itself by the system clock, so casting works with
//! no speakers on the server at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

/// The engine's end. Give it to [`Engine::start_with_tap`](crate::Engine::start_with_tap).
pub struct StreamTap {
    producer: Producer<f32>,
    active: Arc<AtomicBool>,
}

/// The reader's end.
pub struct TapReader {
    consumer: Consumer<f32>,
    active: Arc<AtomicBool>,
}

/// A tap holding up to `seconds` of interleaved stereo at `rate`.
pub fn stream_tap(seconds: f32, rate: u32) -> (StreamTap, TapReader) {
    let capacity = ((seconds.max(0.1) * rate as f32) as usize) * 2;
    let (producer, consumer) = RingBuffer::new(capacity);
    let active = Arc::new(AtomicBool::new(false));
    (
        StreamTap {
            producer,
            active: Arc::clone(&active),
        },
        TapReader { consumer, active },
    )
}

impl StreamTap {
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Copies frames in while active. A full ring (reader stalled) drops
    /// the overflow rather than holding up playback.
    pub(crate) fn write(&mut self, samples: &[f32]) {
        if !self.is_active() {
            return;
        }
        let n = samples.len().min(self.producer.slots()) & !1;
        let _ = self.producer.push_partial_slice(&samples[..n]);
    }
}

impl TapReader {
    /// Turns the tap on (someone is listening) or off.
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Release);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Appends everything available to `out`, in whole frames. Returns the
    /// number of frames read.
    pub fn read(&mut self, out: &mut Vec<f32>) -> usize {
        let n = self.consumer.slots() & !1;
        if let Ok(chunk) = self.consumer.read_chunk(n) {
            let (a, b) = chunk.as_slices();
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            chunk.commit_all();
        }
        n / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_only_while_active() {
        let (mut tap, mut reader) = stream_tap(1.0, 100);
        tap.write(&[0.1, 0.2]);
        let mut out = Vec::new();
        assert_eq!(reader.read(&mut out), 0, "inactive taps drop audio");
        reader.set_active(true);
        tap.write(&[0.1, 0.2, 0.3, 0.4]);
        assert_eq!(reader.read(&mut out), 2);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
        // A stalled reader loses the overflow, never half a frame.
        tap.write(&vec![0.5; 1000]);
        out.clear();
        assert_eq!(reader.read(&mut out), 100);
    }
}
