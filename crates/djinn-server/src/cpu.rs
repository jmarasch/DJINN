//! How much of this computer's processor background work may use, worked
//! out once at startup. Playback and the web server always keep a thread
//! to themselves.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
struct Counts {
    /// Logical processors (hardware threads).
    threads: usize,
    /// Physical cores.
    cores: usize,
}

static COUNTS: OnceLock<Counts> = OnceLock::new();

fn counts() -> Counts {
    *COUNTS.get_or_init(|| Counts {
        threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        cores: num_cpus::get_physical().max(1),
    })
}

/// Hardware threads on this computer.
pub fn threads() -> usize {
    counts().threads
}

/// Songs the sound analysis decodes at once: half the threads, at most 4.
pub fn analysis() -> usize {
    (threads() / 2).clamp(1, 4)
}

/// Threads the AI reader may use for an answer: one per physical core,
/// leaving at least one thread free.
pub fn reader() -> usize {
    let Counts { threads, cores } = counts();
    cores.min(threads.saturating_sub(1)).max(1)
}

/// Sizes the AI reader's thread pools and logs what this computer has.
/// Runs first thing in `main`, before any other thread starts, since it
/// sets environment variables.
pub fn init() {
    // candle reads these once, the first time the reader runs. A value
    // set by whoever started DJINN wins.
    for var in ["RAYON_NUM_THREADS", "CANDLE_NUM_THREADS"] {
        if std::env::var_os(var).is_none() {
            std::env::set_var(var, reader().to_string());
        }
    }
}

/// Logs the counts, once logging is up.
pub fn log() {
    let Counts { threads, cores } = counts();
    let memory_gb = djinn_platform::system::memory().map(|m| m.total as f64 / 1e9);
    tracing::info!(
        threads,
        cores,
        memory_gb = memory_gb.map(|gb| format!("{gb:.1}")),
        analysis = analysis(),
        reader = reader(),
        "this computer"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_a_thread_free() {
        let r = reader();
        assert!(r >= 1);
        assert!(r == 1 || r < threads());
        assert!((1..=4).contains(&analysis()));
    }
}
