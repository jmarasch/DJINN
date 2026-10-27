//! Which model files DJINN uses and where they come from. Pinned to an
//! exact Hugging Face revision; downloads are checked against these sizes
//! and SHA-256 sums.

/// LAION's larger_clap_music_and_speech (Apache-2.0), as ONNX by Xenova.
pub const REPOSITORY: &str = "Xenova/larger_clap_music_and_speech";
pub const REVISION: &str = "e9fd5ac1dbf3280936a7fc3ec8a020453ff184db";
/// Bumped when a different model is used, so songs get listened to again.
pub const VERSION: i64 = 1;

pub struct ModelFile {
    /// Path in the repository.
    pub remote: &'static str,
    /// File name on disk.
    pub local: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub const FILES: &[ModelFile] = &[
    ModelFile {
        remote: "onnx/audio_model.onnx",
        local: "audio_model.onnx",
        size: 281_749_092,
        sha256: "3ecc72d27740e2a09ced20cf22fd6244122e5e506008763a0f368b3b4ff6eac8",
    },
    ModelFile {
        remote: "onnx/text_model_quantized.onnx",
        local: "text_model_quantized.onnx",
        size: 126_603_262,
        sha256: "8f9f29c5f6adee917553d4b3a70729c731c0d18b88efca0ae67c1a1fc278f3b6",
    },
    ModelFile {
        remote: "tokenizer.json",
        local: "tokenizer.json",
        size: 2_108_774,
        sha256: "dc239041d98de27ffc3975473a1a23e3db4c937b23c138c38bbc66588bd247e5",
    },
];

/// Total download size in bytes.
pub fn download_size() -> u64 {
    FILES.iter().map(|f| f.size).sum()
}

/// Where to download `file` from.
pub fn url(file: &ModelFile) -> String {
    format!(
        "https://huggingface.co/{REPOSITORY}/resolve/{REVISION}/{}",
        file.remote
    )
}

/// The folder for this model under DJINN's models folder.
pub fn folder_name() -> String {
    format!("clap-{}", &REVISION[..8])
}

/// True if every file is there at the right size (sums are checked when
/// downloading).
pub fn installed(dir: &std::path::Path) -> bool {
    FILES
        .iter()
        .all(|f| std::fs::metadata(dir.join(f.local)).is_ok_and(|m| m.len() == f.size))
}
