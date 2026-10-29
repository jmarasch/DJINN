//! Saving a download to disk while checking it against the size and
//! SHA-256 it should have. Used for AI models, the GPU pack and updates.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub struct CheckedFile {
    path: PathBuf,
    out: File,
    hasher: Sha256,
    written: u64,
}

impl CheckedFile {
    /// Starts writing to `path`, replacing anything there.
    pub async fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            out: File::create(path).await?,
            hasher: Sha256::new(),
            written: 0,
        })
    }

    /// Adds a chunk; returns how many bytes are written so far.
    pub async fn write(&mut self, chunk: &[u8]) -> Result<u64, String> {
        self.hasher.update(chunk);
        self.out.write_all(chunk).await.map_err(|e| e.to_string())?;
        self.written += chunk.len() as u64;
        Ok(self.written)
    }

    /// Removes what was written, when the download stops part way.
    pub async fn discard(self) {
        drop(self.out);
        let _ = tokio::fs::remove_file(&self.path).await;
    }

    /// Finishes the file, or removes it and fails unless it's `size` bytes
    /// with the SHA-256 `sha256` (lowercase hex). `name` names it in the
    /// error.
    pub async fn finish(mut self, size: u64, sha256: &str, name: &str) -> Result<(), String> {
        self.out.flush().await.map_err(|e| e.to_string())?;
        drop(self.out);
        let sum: String = self
            .hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if self.written != size || sum != sha256 {
            let _ = tokio::fs::remove_file(&self.path).await;
            return Err(format!("{name} came down damaged; try again"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of "hello world".
    const HELLO: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("djinn-download-{name}-{}", std::process::id()))
    }

    async fn written(name: &str, chunks: &[&str]) -> (PathBuf, CheckedFile) {
        let path = path(name);
        let mut file = CheckedFile::create(&path).await.unwrap();
        for chunk in chunks {
            file.write(chunk.as_bytes()).await.unwrap();
        }
        (path, file)
    }

    #[tokio::test]
    async fn keeps_a_file_that_matches() {
        let (path, file) = written("good", &["hello ", "world"]).await;
        file.finish(11, HELLO, "test").await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn removes_a_damaged_file() {
        let (path, file) = written("damaged", &["hello there"]).await;
        let error = file.finish(11, HELLO, "test").await.unwrap_err();
        assert_eq!(error, "test came down damaged; try again");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn removes_a_short_file() {
        let (path, file) = written("short", &["hello world"]).await;
        assert!(file.finish(12, HELLO, "test").await.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn discard_removes_the_file() {
        let (path, file) = written("stopped", &["hello"]).await;
        file.discard().await;
        assert!(!path.exists());
    }
}
