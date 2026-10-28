//! Local AI for DJINN: a CLAP model (contrastive language-audio
//! pretraining, LAION's larger_clap_music_and_speech) that turns songs and
//! descriptions into points in one space, so "80s hair metal" or "music
//! for cooking dinner" can be compared with how songs actually sound.
//!
//! A small language model ([`Reader`]) reads free-form requests that the
//! phrase reader can't place.
//!
//! Runs on the CPU (tract and candle, pure Rust); the reader can use a GPU
//! when built with the `cuda` or `metal` feature. The model files are
//! downloaded on request; see [`models`].

mod clap;
mod mel;
pub mod models;
mod qwen2;
mod reader;

pub use clap::{Clap, EMBEDDING_SIZE, SAMPLE_RATE};
pub use reader::{Reader, Reading};

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("model files are missing or damaged: {0}")]
    Model(String),
    #[error("model failed: {0}")]
    Inference(String),
}

/// For a Windows app with no console: tract asks `wmic` for the CPU's cache
/// sizes the first time it runs a model, and a console program started from
/// a windowless one flashes a console window. tract asks once and remembers,
/// so ask now with `wmic` out of reach (it lives in `System32\wbem`, which
/// is only found through PATH). tract then uses its default sizes, as it
/// already does on Windows versions without `wmic`. Call before anything
/// loads a model. Does nothing elsewhere.
pub fn settle_cache_probe() {
    #[cfg(windows)]
    {
        use tract_onnx::tract_core::tract_linalg::cache::cache_info;
        let Some(path) = std::env::var_os("PATH") else {
            cache_info();
            return;
        };
        let kept = std::env::split_paths(&path).filter(|dir| {
            let dir = dir.to_string_lossy().to_ascii_lowercase();
            !dir.trim_end_matches(['\\', '/']).ends_with("wbem")
        });
        if let Ok(stripped) = std::env::join_paths(kept) {
            std::env::set_var("PATH", stripped);
            cache_info();
            std::env::set_var("PATH", path);
        }
    }
}

/// Cosine similarity of two normalized vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    #[test]
    fn settling_the_cache_probe_reaches_tract() {
        // The Windows branch can't be built here; the path into tract can.
        let _ = tract_onnx::tract_core::tract_linalg::cache::cache_info();
        super::settle_cache_probe();
    }
}
