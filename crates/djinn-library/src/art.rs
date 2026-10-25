//! Cover art: content-addressed files in the art cache directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Folder image names, in order of preference (compared case-insensitively).
const FOLDER_ART: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "front.jpg",
    "front.jpeg",
    "front.png",
];

/// Stores `bytes` in `art_dir` under their content hash and returns the hash.
/// Identical images (one per album, usually) are stored once.
pub fn store(art_dir: &Path, bytes: &[u8]) -> io::Result<String> {
    let digest = Sha256::digest(bytes);
    let hash: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    let path = art_dir.join(&hash);
    if !path.exists() {
        fs::create_dir_all(art_dir)?;
        // Write then rename, so a reader never sees a partial file.
        let tmp = art_dir.join(format!("{hash}.tmp"));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
    }
    Ok(hash)
}

/// Path of cached art for `hash`, if the hash is well formed and present.
pub fn path(art_dir: &Path, hash: &str) -> Option<PathBuf> {
    let well_formed = hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    let path = art_dir.join(hash);
    (well_formed && path.is_file()).then_some(path)
}

/// MIME type from the image's magic bytes.
pub fn mime_type(bytes: &[u8]) -> &'static str {
    match bytes {
        [0xFF, 0xD8, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "image/webp",
        [b'B', b'M', ..] => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// The preferred cover image file in `dir`, if any.
pub fn find_folder_art(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_ascii_lowercase();
            let rank = FOLDER_ART.iter().position(|&n| n == name)?;
            Some((rank, e.path()))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_dedupes_and_paths_validate() {
        let dir = std::env::temp_dir().join(format!("djinn-art-{}", std::process::id()));
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
        let a = store(&dir, &jpeg).unwrap();
        let b = store(&dir, &jpeg).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        assert!(path(&dir, &a).is_some());
        assert!(path(&dir, "../../etc/passwd").is_none());
        assert_eq!(mime_type(&jpeg), "image/jpeg");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folder_art_prefers_cover() {
        let dir = std::env::temp_dir().join(format!("djinn-folder-art-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Folder.JPG"), b"x").unwrap();
        fs::write(dir.join("cover.png"), b"x").unwrap();
        let found = find_folder_art(&dir).unwrap();
        assert_eq!(found.file_name().unwrap(), "cover.png");
        fs::remove_dir_all(dir).unwrap();
    }
}
