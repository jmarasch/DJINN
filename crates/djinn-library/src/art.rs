//! Cover art: content-addressed files in the art cache directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Image names for an artist's folder, in order of preference.
const ARTIST_ART: &[&str] = &["artist.jpg", "artist.jpeg", "artist.png", "artist.webp"];

/// Stores `bytes` in `art_dir` under their content hash and returns the hash.
/// Identical images (one per album, usually) are stored once.
pub fn store(art_dir: &Path, bytes: &[u8]) -> io::Result<String> {
    let digest = Sha256::digest(bytes);
    let hash: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    let path = art_dir.join(&hash);
    if !path.exists() {
        fs::create_dir_all(art_dir)?;
        // Write then rename, so a reader never sees a partial file. Scanner
        // threads can store the same image at once, so each write gets its
        // own temporary name.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let tmp = art_dir.join(format!("{hash}.{}.{n}.tmp", std::process::id()));
        fs::write(&tmp, bytes)?;
        if let Err(e) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            if !path.exists() {
                return Err(e);
            }
        }
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
    find_named(dir, FOLDER_ART)
}

/// An artist's picture in their folder: `artist.jpg` and friends, or, when
/// the folder is named after the artist, the usual `folder.jpg`/`cover.jpg`.
pub fn find_artist_art(dir: &Path, artist: &str) -> Option<PathBuf> {
    find_named(dir, ARTIST_ART).or_else(|| {
        let folder = dir.file_name()?.to_str()?;
        (simplify(folder) == simplify(artist))
            .then(|| find_folder_art(dir))
            .flatten()
    })
}

/// Lowercase letters and digits only, for loose name comparison.
fn simplify(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_named(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_ascii_lowercase();
            let rank = names.iter().position(|&n| n == name)?;
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
    fn concurrent_stores_of_one_image_all_succeed() {
        let dir = std::env::temp_dir().join(format!("djinn-art-race-{}", std::process::id()));
        let image = vec![0xFF, 0xD8, 7, 7, 7];
        let results: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| store(&dir, &image))).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(results.iter().all(|r| r.is_ok()), "{results:?}");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no leftover temp files"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn artist_art_rules() {
        let root = std::env::temp_dir().join(format!("djinn-artist-art-{}", std::process::id()));
        let named = root.join("The Beatles");
        let other = root.join("Misc Downloads");
        fs::create_dir_all(&named).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(named.join("folder.jpg"), b"x").unwrap();
        fs::write(other.join("folder.jpg"), b"x").unwrap();
        // A folder named after the artist may use folder.jpg...
        assert!(find_artist_art(&named, "the beatles").is_some());
        // ...an unrelated folder may not, unless it has artist.jpg.
        assert!(find_artist_art(&other, "The Beatles").is_none());
        fs::write(other.join("Artist.PNG"), b"x").unwrap();
        assert!(find_artist_art(&other, "The Beatles").is_some());
        fs::remove_dir_all(root).unwrap();
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
