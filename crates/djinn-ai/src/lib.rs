//! Local AI for DJINN: a CLAP model (contrastive language-audio
//! pretraining, LAION's larger_clap_music_and_speech) that turns songs and
//! descriptions into points in one space, so "80s hair metal" or "music
//! for cooking dinner" can be compared with how songs actually sound.
//!
//! Runs on the CPU with tract (pure Rust). The model files are downloaded
//! on request; see [`models`].

mod clap;
mod mel;
pub mod models;

pub use clap::{Clap, EMBEDDING_SIZE, SAMPLE_RATE};

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("model files are missing or damaged: {0}")]
    Model(String),
    #[error("model failed: {0}")]
    Inference(String),
}

/// Cosine similarity of two normalized vectors.
pub fn similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
