//! Reads tags, audio properties, and cover art from one file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use djinn_store::ScannedTrack;
use lofty::file::TaggedFile;
use lofty::picture::PictureType;
use lofty::prelude::*;

use crate::art;

/// Folder art already resolved during this scan, keyed by directory.
pub type FolderArtCache = Mutex<HashMap<PathBuf, Option<String>>>;

pub fn read(
    path: &Path,
    path_str: &str,
    size: i64,
    mtime: i64,
    art_dir: &Path,
    folder_art: &FolderArtCache,
) -> ScannedTrack {
    let mut track = ScannedTrack {
        path: path_str.to_owned(),
        size,
        mtime,
        ..Default::default()
    };

    let mut embedded_art = None;
    match lofty::read_from_path(path) {
        Ok(file) => {
            fill_from_tags(&mut track, &file);
            embedded_art = cover_picture(&file);
        }
        // Still list the file: symphonia may play what lofty can't parse.
        Err(e) => tracing::debug!(path = path_str, error = %e, "unreadable tags"),
    }
    if track.title.is_empty() {
        track.title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_str.to_owned());
    }

    track.art_hash = match embedded_art {
        Some(bytes) => art::store(art_dir, &bytes)
            .map_err(|e| tracing::warn!(path = path_str, error = %e, "cannot cache cover art"))
            .ok(),
        None => path
            .parent()
            .and_then(|dir| folder_art_hash(dir, art_dir, folder_art)),
    };
    track
}

fn fill_from_tags(track: &mut ScannedTrack, file: &TaggedFile) {
    let props = file.properties();
    track.duration_ms = i64::try_from(props.duration().as_millis()).ok();
    track.sample_rate = props.sample_rate().map(i64::from);
    track.channels = props.channels().map(i64::from);
    track.codec = Some(format!("{:?}", file.file_type()).to_lowercase());

    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return;
    };
    let text = |value: Option<std::borrow::Cow<'_, str>>| {
        value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
    };
    track.title = text(tag.title()).unwrap_or_default();
    track.artist = text(tag.artist());
    track.album = text(tag.album());
    track.album_artist = text(tag.get_string(ItemKey::AlbumArtist).map(Into::into));
    track.genre = text(tag.genre());
    track.track_no = tag.track().map(i64::from);
    track.disc_no = tag.disk().map(i64::from);
    track.year = tag.date().map(|d| i64::from(d.year));
}

/// Front cover if tagged as such, otherwise the first embedded picture.
fn cover_picture(file: &TaggedFile) -> Option<Vec<u8>> {
    let pictures: Vec<_> = file.tags().iter().flat_map(|t| t.pictures()).collect();
    pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .map(|p| p.data().to_vec())
}

fn folder_art_hash(dir: &Path, art_dir: &Path, cache: &FolderArtCache) -> Option<String> {
    if let Ok(cache) = cache.lock() {
        if let Some(hash) = cache.get(dir) {
            return hash.clone();
        }
    }
    let hash = art::find_folder_art(dir)
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| art::store(art_dir, &bytes).ok());
    if let Ok(mut cache) = cache.lock() {
        cache.insert(dir.to_path_buf(), hash.clone());
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_tags_and_embedded_art() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagged.flac");
        let art_dir = std::env::temp_dir().join(format!("djinn-tags-{}", std::process::id()));
        let cache = FolderArtCache::default();
        let t = read(&fixture, "tagged.flac", 1, 2, &art_dir, &cache);

        assert_eq!(t.title, "Jóga");
        assert_eq!(t.artist.as_deref(), Some("Björk"));
        assert_eq!(t.album_artist.as_deref(), Some("Björk"));
        assert_eq!(t.album.as_deref(), Some("Homogenic"));
        assert_eq!(
            (t.track_no, t.disc_no, t.year),
            (Some(3), Some(1), Some(1997))
        );
        assert_eq!(t.genre.as_deref(), Some("Electronic"));
        assert_eq!(t.codec.as_deref(), Some("flac"));
        assert_eq!((t.sample_rate, t.channels), (Some(44_100), Some(1)));
        assert_eq!(t.duration_ms, Some(272));

        let hash = t.art_hash.expect("embedded art");
        let bytes = std::fs::read(art::path(&art_dir, &hash).unwrap()).unwrap();
        assert_eq!(art::mime_type(&bytes), "image/jpeg");
        std::fs::remove_dir_all(art_dir).unwrap();
    }
}
