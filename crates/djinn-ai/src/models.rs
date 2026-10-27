//! Which model files DJINN uses and where they come from. Pinned to exact
//! Hugging Face revisions; downloads are checked against these sizes and
//! SHA-256 sums.

/// Bumped when a different listening model is used, so songs get
/// listened to again.
pub const VERSION: i64 = 1;

/// File names the reader loads.
pub const READER_WEIGHTS: &str = "model.gguf";
pub const READER_TOKENIZER: &str = "tokenizer.json";

pub struct ModelFile {
    pub url: &'static str,
    /// File name on disk.
    pub local: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// A model: its files, in their own folder under DJINN's models folder.
pub struct Model {
    pub folder: &'static str,
    pub files: &'static [ModelFile],
}

macro_rules! hugging_face {
    ($repo:literal, $revision:literal, $path:literal) => {
        concat!(
            "https://huggingface.co/",
            $repo,
            "/resolve/",
            $revision,
            "/",
            $path
        )
    };
}

/// LAION's larger_clap_music_and_speech (Apache-2.0), as ONNX by Xenova:
/// listens to songs.
pub const CLAP: Model = Model {
    folder: "clap-e9fd5ac1",
    files: &[
        ModelFile {
            url: hugging_face!(
                "Xenova/larger_clap_music_and_speech",
                "e9fd5ac1dbf3280936a7fc3ec8a020453ff184db",
                "onnx/audio_model.onnx"
            ),
            local: "audio_model.onnx",
            size: 281_749_092,
            sha256: "3ecc72d27740e2a09ced20cf22fd6244122e5e506008763a0f368b3b4ff6eac8",
        },
        ModelFile {
            url: hugging_face!(
                "Xenova/larger_clap_music_and_speech",
                "e9fd5ac1dbf3280936a7fc3ec8a020453ff184db",
                "onnx/text_model_quantized.onnx"
            ),
            local: "text_model_quantized.onnx",
            size: 126_603_262,
            sha256: "8f9f29c5f6adee917553d4b3a70729c731c0d18b88efca0ae67c1a1fc278f3b6",
        },
        ModelFile {
            url: hugging_face!(
                "Xenova/larger_clap_music_and_speech",
                "e9fd5ac1dbf3280936a7fc3ec8a020453ff184db",
                "tokenizer.json"
            ),
            local: "tokenizer.json",
            size: 2_108_774,
            sha256: "dc239041d98de27ffc3975473a1a23e3db4c937b23c138c38bbc66588bd247e5",
        },
    ],
};

/// Qwen2.5 1.5B Instruct (Apache-2.0), 4-bit GGUF: reads requests.
pub const READER: Model = Model {
    folder: "qwen2.5-1.5b-91cad511",
    files: &[
        ModelFile {
            url: hugging_face!(
                "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
                "91cad51170dc346986eccefdc2dd33a9da36ead9",
                "qwen2.5-1.5b-instruct-q4_k_m.gguf"
            ),
            local: READER_WEIGHTS,
            size: 1_117_320_736,
            sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        },
        ModelFile {
            url: hugging_face!(
                "Qwen/Qwen2.5-1.5B-Instruct",
                "989aa7980e4cf806f80c7fef2b1adb7bc71aa306",
                "tokenizer.json"
            ),
            local: READER_TOKENIZER,
            size: 7_031_645,
            sha256: "c0382117ea329cdf097041132f6d735924b697924d6f6fc3945713e96ce87539",
        },
    ],
};

impl Model {
    /// Total download size in bytes.
    pub fn download_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// True if every file is in `dir` at the right size (sums are checked
    /// when downloading).
    pub fn installed(&self, dir: &std::path::Path) -> bool {
        self.files
            .iter()
            .all(|f| std::fs::metadata(dir.join(f.local)).is_ok_and(|m| m.len() == f.size))
    }
}
