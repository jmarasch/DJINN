//! One clock for timing audio across boxes: microseconds since this process
//! first asked. The timed tap stamps chunks with it, and a node learns how
//! its own copy of this clock lines up with Main's.

use std::sync::OnceLock;
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();

fn epoch() -> Instant {
    *EPOCH.get_or_init(Instant::now)
}

/// `at` on this clock, in microseconds.
pub fn micros(at: Instant) -> i64 {
    let epoch = epoch();
    match at.checked_duration_since(epoch) {
        Some(after) => after.as_micros() as i64,
        None => -(epoch.duration_since(at).as_micros() as i64),
    }
}

/// Now, in microseconds.
pub fn now_micros() -> i64 {
    micros(Instant::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn it_counts_forward_and_back() {
        let now = Instant::now();
        let a = micros(now);
        let later = micros(now + Duration::from_millis(5)) - a;
        assert!((4999..=5001).contains(&later), "{later}");
        assert!(now_micros() >= a);
    }
}
