//! Reads tags, audio properties, and cover art from one file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use djinn_store::ScannedTrack;
use lofty::file::TaggedFile;
use lofty::picture::PictureType;
use lofty::prelude::*;

use crate::art;

/// Images already resolved during this scan, so each folder is read once.
#[derive(Default)]
pub struct ArtCache {
    /// Album folder images, by directory.
    folders: Mutex<HashMap<PathBuf, Option<String>>>,
    /// Artist folder images, by artist folder joined with artist name.
    artists: Mutex<HashMap<PathBuf, Option<String>>>,
}

pub fn read(
    path: &Path,
    path_str: &str,
    size: i64,
    mtime: i64,
    artist_dir: Option<&Path>,
    art_dir: &Path,
    cache: &ArtCache,
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

    track.embedded_art = embedded_art.and_then(|bytes| {
        art::store(art_dir, &bytes)
            .map_err(|e| tracing::warn!(path = path_str, error = %e, "cannot cache cover art"))
            .ok()
    });
    track.folder_art = path
        .parent()
        .and_then(|dir| cached(&cache.folders, dir, || art::find_folder_art(dir), art_dir));
    let artist = track.album_artist.clone().or_else(|| track.artist.clone());
    track.artist_art = match (artist_dir, artist) {
        (Some(dir), Some(artist)) => cached(
            &cache.artists,
            &dir.join(&artist),
            || art::find_artist_art(dir, &artist),
            art_dir,
        ),
        _ => None,
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
    track.artist = text(tag.artist()).map(clean_artist);
    track.album = text(tag.album());
    track.album_artist =
        text(tag.get_string(ItemKey::AlbumArtist).map(Into::into)).map(clean_artist);
    track.genre = text(tag.genre());
    track.track_no = tag.track().map(i64::from);
    track.disc_no = tag.disk().map(i64::from);
    track.year = tag.date().map(|d| i64::from(d.year));
}

/// Drops the " - Topic" that YouTube Music adds to auto-generated artist
/// channel names ("KREAM - Topic"), which downloads often carry as tags.
fn clean_artist(name: String) -> String {
    const SUFFIX: &str = " - topic";
    let lower = name.to_lowercase();
    match lower.strip_suffix(SUFFIX) {
        Some(rest) if !rest.trim().is_empty() && lower.len() == name.len() => {
            name[..name.len() - SUFFIX.len()].trim_end().to_owned()
        }
        _ => name,
    }
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

/// Looks `key` up in `cache`, finding and storing the image on a miss.
fn cached(
    cache: &Mutex<HashMap<PathBuf, Option<String>>>,
    key: &Path,
    find: impl FnOnce() -> Option<PathBuf>,
    art_dir: &Path,
) -> Option<String> {
    if let Ok(cache) = cache.lock() {
        if let Some(hash) = cache.get(key) {
            return hash.clone();
        }
    }
    let hash = find()
        .and_then(|file| std::fs::read(file).ok())
        .and_then(|bytes| art::store(art_dir, &bytes).ok());
    if let Ok(mut cache) = cache.lock() {
        cache.insert(key.to_path_buf(), hash.clone());
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
        let cache = ArtCache::default();
        let t = read(&fixture, "tagged.flac", 1, 2, None, &art_dir, &cache);

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

        let hash = t.embedded_art.expect("embedded art");
        let bytes = std::fs::read(art::path(&art_dir, &hash).unwrap()).unwrap();
        assert_eq!(art::mime_type(&bytes), "image/jpeg");
        std::fs::remove_dir_all(art_dir).unwrap();
    }

    #[test]
    fn topic_suffix_is_dropped() {
        assert_eq!(clean_artist("KREAM - Topic".into()), "KREAM");
        assert_eq!(clean_artist("Björk - TOPIC".into()), "Björk");
        assert_eq!(clean_artist("Topic".into()), "Topic");
        assert_eq!(clean_artist(" - Topic".into()), " - Topic");
        assert_eq!(clean_artist("Hot Topic".into()), "Hot Topic");
    }
}
