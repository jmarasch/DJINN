use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("failed to open {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("unsupported or corrupt file {path}: {source}")]
    Probe {
        path: PathBuf,
        source: symphonia::core::errors::Error,
    },
    #[error("{path}: {reason}")]
    Track { path: PathBuf, reason: String },
    #[error("decode failed: {0}")]
    Decode(symphonia::core::errors::Error),
    #[error("resampler: {0}")]
    Resample(String),
    #[error("audio output: {0}")]
    Output(String),
    #[error("engine thread is not running")]
    Stopped,
}
