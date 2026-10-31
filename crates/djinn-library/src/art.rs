//! Cover art: content-addressed files in the art cache directory.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

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

/// File extensions the cache uses, from the image's type.
const EXTENSIONS: &[&str] = &["jpg", "png", "gif", "webp", "bmp", "img"];

static CACHE_LIMIT: AtomicU64 = AtomicU64::new(0);
static CACHE_USAGE: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();

/// A zero limit leaves the cache unlimited. Existing files are never removed
/// when a smaller limit is chosen; new writes wait until there is room.
pub fn set_cache_limit(bytes: u64) {
    CACHE_LIMIT.store(bytes, Ordering::Release);
    if let Some(usage) = CACHE_USAGE.get() {
        if let Ok(mut usage) = usage.lock() {
            usage.clear();
        }
    }
}

/// Size of the whole cache, including thumbnails, without following links.
pub fn cache_usage(art_dir: &Path) -> io::Result<u64> {
    fn count(dir: &Path) -> io::Result<u64> {
        let mut total = 0_u64;
        if !dir.exists() {
            return Ok(0);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                total = total.saturating_add(count(&entry.path())?);
            } else if kind.is_file() {
                total = total.saturating_add(entry.metadata()?.len());
            }
        }
        Ok(total)
    }
    count(art_dir)
}

/// Writes a new cache file only when the configured limit has room.
fn cache_write(
    art_dir: &Path,
    destination: &Path,
    bytes: &[u8],
    write: impl FnOnce() -> io::Result<()>,
) -> io::Result<bool> {
    if destination.is_file() {
        return Ok(true);
    }
    let limit = CACHE_LIMIT.load(Ordering::Acquire);
    if limit == 0 {
        write()?;
        return Ok(true);
    }
    let mut usage = CACHE_USAGE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| io::Error::other("art cache lock poisoned"))?;
    if destination.is_file() {
        return Ok(true);
    }
    let used = match usage.get(art_dir) {
        Some(used) => *used,
        None => {
            let used = cache_usage(art_dir)?;
            usage.insert(art_dir.to_path_buf(), used);
            used
        }
    };
    if used.saturating_add(bytes.len() as u64) > limit {
        return Ok(false);
    }
    write()?;
    usage.insert(art_dir.to_path_buf(), used + bytes.len() as u64);
    Ok(true)
}

/// Checks an artist photo's source resolution, then stores a bounded JPEG.
/// `None` means the source was too small for display.
pub fn prepare_artist_image(
    bytes: &[u8],
    min_width: u32,
    min_height: u32,
    max_edge: u32,
) -> io::Result<Option<Vec<u8>>> {
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(io::Error::other("image is larger than 20 MB"));
    }
    let reader = image::ImageReader::new(io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(io::Error::other)?;
    let (width, height) = reader.into_dimensions().map_err(io::Error::other)?;
    if width < min_width || height < min_height {
        return Ok(None);
    }
    if (width as u64).saturating_mul(height as u64) > 64_000_000 {
        return Err(io::Error::other("image resolution is too large"));
    }
    let image = image::load_from_memory(bytes).map_err(io::Error::other)?;
    let mut jpeg = Vec::new();
    image
        .thumbnail(max_edge, max_edge)
        .to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut jpeg, 85,
        ))
        .map_err(io::Error::other)?;
    Ok(Some(jpeg))
}

/// Recounts after files were removed outside `store` or `thumbnail`.
pub fn refresh_cache_usage(art_dir: &Path) {
    if let Some(usage) = CACHE_USAGE.get() {
        if let Ok(mut usage) = usage.lock() {
            usage.remove(art_dir);
        }
    }
}

/// Removes an unused cache image and its thumbnails after the caller has
/// checked that no database row still refers to its hash.
pub fn remove(art_dir: &Path, hash: &str) -> io::Result<()> {
    if let Some(file) = path(art_dir, hash) {
        fs::remove_file(file)?;
    }
    for size in THUMBNAIL_SIZES {
        let thumb = art_dir.join("thumbs").join(format!("{hash}-{size}.jpg"));
        if thumb.is_file() {
            fs::remove_file(thumb)?;
        }
    }
    refresh_cache_usage(art_dir);
    Ok(())
}

/// Stores `bytes` in `art_dir` as `<content hash>.<ext>` and returns the
/// hash. Identical images (one per album, usually) are stored once.
pub fn store(art_dir: &Path, bytes: &[u8]) -> io::Result<String> {
    let digest = Sha256::digest(bytes);
    let hash: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    if path(art_dir, &hash).is_some() {
        return Ok(hash);
    }
    let path = art_dir.join(format!("{hash}.{}", extension(bytes)));
    let stored = cache_write(art_dir, &path, bytes, || {
        fs::create_dir_all(art_dir)?;
        // Write then rename, so a reader never sees a partial file.
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
        Ok(())
    })?;
    if stored {
        Ok(hash)
    } else {
        Err(io::Error::other("art cache storage limit reached"))
    }
}

/// Path of cached art for `hash`, if the hash is well formed and present.
pub fn path(art_dir: &Path, hash: &str) -> Option<PathBuf> {
    let well_formed = hash.len() == 32 && hash.bytes().all(|b| b.is_ascii_hexdigit());
    if !well_formed {
        return None;
    }
    EXTENSIONS
        .iter()
        .map(|ext| art_dir.join(format!("{hash}.{ext}")))
        // Files cached before extensions were added.
        .chain(std::iter::once(art_dir.join(hash)))
        .find(|p| p.is_file())
}

/// Thumbnail sizes served, in pixels (the longest side).
pub const THUMBNAIL_SIZES: &[u32] = &[128, 512];

/// A thumbnail of the cached image `hash` that fits in `size` pixels,
/// made on first use and kept in `thumbs/`. Images already that small are
/// served as they are. Returns `None` if there's no such image.
pub fn thumbnail(art_dir: &Path, hash: &str, size: u32) -> io::Result<Option<PathBuf>> {
    let Some(original) = path(art_dir, hash) else {
        return Ok(None);
    };
    // Snap to a supported size so each image has at most two thumbnails.
    let size = THUMBNAIL_SIZES
        .iter()
        .copied()
        .find(|&s| s >= size)
        .unwrap_or(THUMBNAIL_SIZES[THUMBNAIL_SIZES.len() - 1]);
    let thumbs = art_dir.join("thumbs");
    let thumb = thumbs.join(format!("{hash}-{size}.jpg"));
    if thumb.is_file() {
        return Ok(Some(thumb));
    }
    let bytes = fs::read(&original)?;
    let Ok(image) = image::load_from_memory(&bytes) else {
        // Can't decode it; the original still works in the browser.
        return Ok(Some(original));
    };
    if image.width() <= size && image.height() <= size {
        return Ok(Some(original));
    }
    let mut jpeg = Vec::new();
    image
        .thumbnail(size, size)
        .to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut jpeg, 85,
        ))
        .map_err(io::Error::other)?;
    let stored = cache_write(art_dir, &thumb, &jpeg, || {
        fs::create_dir_all(&thumbs)?;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let tmp = thumbs.join(format!("{hash}-{size}.{}.{n}.tmp", std::process::id()));
        fs::write(&tmp, &jpeg)?;
        if let Err(e) = fs::rename(&tmp, &thumb) {
            let _ = fs::remove_file(&tmp);
            if !thumb.is_file() {
                return Err(e);
            }
        }
        Ok(())
    })?;
    Ok(Some(if stored { thumb } else { original }))
}

/// Gives cache files from before extensions were added one, so the folder
/// is easy to browse. Returns how many were renamed.
pub fn add_extensions(art_dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(art_dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.len() == 32 && n.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        .filter(|p| {
            let Ok(bytes) = fs::read(p) else { return false };
            fs::rename(p, p.with_extension(extension(&bytes))).is_ok()
        })
        .count()
}

fn extension(bytes: &[u8]) -> &'static str {
    match mime_type(bytes) {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "img",
    }
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
pub(crate) fn simplify(name: &str) -> String {
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
    fn files_get_extensions_and_old_ones_are_renamed() {
        let dir = std::env::temp_dir().join(format!("djinn-art-ext-{}", std::process::id()));
        let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
        let hash = store(&dir, &png).unwrap();
        assert!(dir.join(format!("{hash}.png")).is_file());

        // A file from before extensions existed is still found, then renamed.
        let old = "0123456789abcdef0123456789abcdef";
        fs::write(dir.join(old), [0xFF, 0xD8, 1]).unwrap();
        assert!(path(&dir, old).is_some());
        assert_eq!(add_extensions(&dir), 1);
        assert_eq!(path(&dir, old), Some(dir.join(format!("{old}.jpg"))));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn thumbnails_shrink_but_never_enlarge() {
        let dir = std::env::temp_dir().join(format!("djinn-thumbs-{}", std::process::id()));
        let encode = |w: u32, h: u32| {
            let mut png = Vec::new();
            image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 90]))
                .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            png
        };
        let big = store(&dir, &encode(1000, 800)).unwrap();
        let small = store(&dir, &encode(100, 100)).unwrap();

        let thumb = thumbnail(&dir, &big, 128).unwrap().unwrap();
        assert!(thumb.ends_with(format!("thumbs/{big}-128.jpg")));
        let (w, h) = image::image_dimensions(&thumb).unwrap();
        assert_eq!((w, h), (128, 102), "fits in 128, keeps the shape");
        // Odd sizes snap up to a supported one.
        assert!(thumbnail(&dir, &big, 300)
            .unwrap()
            .unwrap()
            .ends_with(format!("{big}-512.jpg")));
        // Small images are served as they are.
        assert_eq!(thumbnail(&dir, &small, 128).unwrap(), path(&dir, &small));
        assert_eq!(
            thumbnail(&dir, "0123456789abcdef0123456789abcdef", 128).unwrap(),
            None
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn artist_images_respect_source_minimum_and_output_edge() {
        let mut png = Vec::new();
        image::RgbImage::from_pixel(1200, 700, image::Rgb([40, 80, 120]))
            .write_to(&mut io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(prepare_artist_image(&png, 1300, 450, 800)
            .unwrap()
            .is_none());
        let jpeg = prepare_artist_image(&png, 800, 450, 800).unwrap().unwrap();
        let result = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((result.width(), result.height()), (800, 467));
        assert_eq!(mime_type(&jpeg), "image/jpeg");
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
