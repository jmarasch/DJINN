//! Keeping a follower output in time with the clock output.
//!
//! Every device runs on its own crystal, so two outputs fed the same audio
//! slowly drift apart (tens of parts per million is typical: a few hundred
//! milliseconds over a three hour party). A follower measures how far
//! ahead or behind it is, and a PI controller nudges its resampling ratio
//! to hold that at zero. The integral term ends up matching the device's
//! drift, so the error settles at zero rather than at an offset.

/// Smoothing for the measured error. Callbacks take audio in chunks, so the
/// raw figure saws up and down by a chunk; this evens that out.
const FILTER_SECS: f64 = 2.0;
/// Measurements to collect before correcting, after a start or resync.
const WARMUP_SECS: f64 = 3.0;
/// Errors beyond this are fixed at once (skip or pad) rather than slewed.
const RESYNC_SECS: f64 = 0.04;
/// Proportional gain: ratio change per second of error.
const KP: f64 = 0.04;
/// Integral gain, for critical damping (KP squared over 4).
const KI: f64 = KP * KP / 4.0;
/// Largest speed correction: 0.2%, about 3 cents of pitch at most, and
/// only briefly while catching up.
const MAX_ADJUST: f64 = 0.002;

#[derive(Debug, Default)]
pub(crate) struct DriftControl {
    filtered: Option<f64>,
    age: f64,
    integral: f64,
    adjust: f64,
}

impl DriftControl {
    /// Feeds the timing error in seconds (positive when the output plays
    /// late) measured `dt` seconds after the last one. Returns an error to
    /// fix at once, if it is too large to slew.
    pub fn update(&mut self, error: f64, dt: f64) -> Option<f64> {
        let filtered = match self.filtered {
            Some(f) => f + (error - f) * (1.0 - (-dt / FILTER_SECS).exp()),
            None => error,
        };
        self.filtered = Some(filtered);
        self.age += dt;
        if self.age < WARMUP_SECS {
            return None;
        }
        if filtered.abs() > RESYNC_SECS {
            self.filtered = None;
            self.age = 0.0;
            return Some(filtered);
        }
        // Late means too much buffered: play faster (fewer output frames
        // per input frame), so the correction has the opposite sign.
        let integral = self.integral + filtered * dt;
        let adjust = -(KP * filtered + KI * integral);
        // Stop integrating while pinned at the limit (anti-windup).
        if adjust.abs() < MAX_ADJUST {
            self.integral = integral;
        }
        self.adjust = adjust.clamp(-MAX_ADJUST, MAX_ADJUST);
        None
    }

    /// Starts measuring afresh (after a jump), keeping the learned drift.
    pub fn restart(&mut self) {
        self.filtered = None;
        self.age = 0.0;
    }

    /// Relative speed correction: 0.0 for none.
    pub fn adjust(&self) -> f64 {
        self.adjust
    }

    /// The smoothed error in seconds.
    pub fn error(&self) -> f64 {
        self.filtered.unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates a follower whose device runs `drift` faster than the clock,
    /// measured with chunk-sized sawtooth noise. Returns the controller and
    /// the true error at the end.
    fn simulate(drift: f64, start_error: f64, secs: f64) -> (DriftControl, f64) {
        let dt = 0.01;
        let chunk = 1024.0 / 48_000.0;
        let mut control = DriftControl::default();
        let mut error = start_error;
        let mut t = 0.0;
        while t < secs {
            // A faster device drains its buffer faster: it runs early.
            error += (control.adjust() - drift) * dt;
            let noise = (t * 47.0).fract() * chunk - chunk / 2.0;
            if let Some(jump) = control.update(error + noise, dt) {
                error -= jump;
            }
            t += dt;
        }
        (control, error)
    }

    #[test]
    fn settles_on_the_devices_drift() {
        for drift in [-300e-6, -40e-6, 0.0, 40e-6, 300e-6] {
            let (control, error) = simulate(drift, 0.0, 1200.0);
            assert!(error.abs() < 0.002, "drift {drift}: error {error}");
            assert!(
                (control.adjust() - drift).abs() < 20e-6,
                "drift {drift}: adjust {}",
                control.adjust()
            );
        }
    }

    #[test]
    fn large_errors_are_fixed_at_once() {
        let mut control = DriftControl::default();
        let mut jump = None;
        for _ in 0..400 {
            jump = jump.or(control.update(0.25, 0.01));
        }
        let jump = jump.unwrap_or_default();
        assert!((jump - 0.25).abs() < 1e-6, "jump {jump}");
    }

    #[test]
    fn small_offsets_are_slewed_out() {
        let (_, error) = simulate(0.0, 0.03, 600.0);
        assert!(error.abs() < 0.002, "error {error}");
    }
}
