//! Reads tags, audio properties, and cover art from one file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use djinn_store::{ReplayGainTags, ScannedTrack};
use lofty::file::TaggedFile;
use lofty::picture::PictureType;
use lofty::prelude::*;

use crate::{art, credits};

/// What every file in a scan shares.
#[derive(Clone, Copy)]
pub struct ScanContext<'a> {
    pub art_dir: &'a Path,
    pub cache: &'a ArtCache,
    /// Artist names never split.
    pub keep_whole: &'a [String],
}

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
    scan: &ScanContext<'_>,
) -> ScannedTrack {
    let ScanContext {
        art_dir,
        cache,
        keep_whole,
    } = *scan;
    let mut track = ScannedTrack {
        path: path_str.to_owned(),
        size,
        mtime,
        ..Default::default()
    };

    let mut embedded_art = None;
    let mut artists = Vec::new();
    let mut album_artists = Vec::new();
    match lofty::read_from_path(path) {
        Ok(file) => {
            (artists, album_artists) = fill_from_tags(&mut track, &file);
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

    // Split "A & B feat. C" into separate artists.
    if !artists.is_empty() {
        track.credits = credits::track_credits(&artists, &track.title, keep_whole)
            .into_iter()
            .map(|c| (c.name, c.featured))
            .collect();
    }
    track.album_owner = album_artists
        .first()
        .and_then(|a| credits::split_credit(a, keep_whole).into_iter().next())
        .map(|c| c.name);

    track.embedded_art = embedded_art.and_then(|bytes| {
        art::store(art_dir, &bytes)
            .map_err(|e| tracing::warn!(path = path_str, error = %e, "cannot cache cover art"))
            .ok()
    });
    track.folder_art = path
        .parent()
        .and_then(|dir| cached(&cache.folders, dir, || art::find_folder_art(dir), art_dir));
    let artist = track
        .album_owner
        .clone()
        .or_else(|| track.credits.first().map(|(name, _)| name.clone()))
        .or_else(|| track.artist.clone());
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

/// Fills in tags and audio properties. Returns the artist and album artist
/// values (several when the file has proper multi-artist tags).
fn fill_from_tags(track: &mut ScannedTrack, file: &TaggedFile) -> (Vec<String>, Vec<String>) {
    let props = file.properties();
    track.duration_ms = i64::try_from(props.duration().as_millis()).ok();
    track.sample_rate = props.sample_rate().map(i64::from);
    track.channels = props.channels().map(i64::from);
    track.codec = Some(format!("{:?}", file.file_type()).to_lowercase());

    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return (Vec::new(), Vec::new());
    };
    let text = |value: Option<std::borrow::Cow<'_, str>>| {
        value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
    };
    // Proper multi-artist tags (Picard's ARTISTS) first, then every value of
    // the plain artist tag.
    let values = |many: ItemKey, one: ItemKey| -> Vec<String> {
        let clean = |v: &str| Some(clean_artist(v.trim().to_owned())).filter(|v| !v.is_empty());
        let multi: Vec<String> = tag.get_strings(many).filter_map(clean).collect();
        if multi.is_empty() {
            tag.get_strings(one).filter_map(clean).collect()
        } else {
            multi
        }
    };
    let artists = values(ItemKey::TrackArtists, ItemKey::TrackArtist);
    let album_artists = values(ItemKey::AlbumArtists, ItemKey::AlbumArtist);
    track.title = text(tag.title()).unwrap_or_default();
    track.artist = (!artists.is_empty()).then(|| artists.join(" & "));
    track.album = text(tag.album());
    track.album_artist = (!album_artists.is_empty()).then(|| album_artists.join(" & "));
    track.genre = text(tag.genre());
    track.track_no = tag.track().map(i64::from);
    track.disc_no = tag.disk().map(i64::from);
    track.year = tag.date().map(|d| i64::from(d.year));
    let number = |key: ItemKey| tag.get_string(key).and_then(parse_replay_gain);
    track.replay_gain = ReplayGainTags {
        track_gain: number(ItemKey::ReplayGainTrackGain),
        track_peak: number(ItemKey::ReplayGainTrackPeak).filter(|p| *p > 0.0),
        album_gain: number(ItemKey::ReplayGainAlbumGain),
        album_peak: number(ItemKey::ReplayGainAlbumPeak).filter(|p| *p > 0.0),
    };
    (artists, album_artists)
}

/// Reads a ReplayGain value like "-6.54 dB", "+1.2dB", or "0.98765".
fn parse_replay_gain(value: &str) -> Option<f64> {
    let value = value.trim();
    let number = value
        .strip_suffix("dB")
        .or_else(|| value.strip_suffix("db"))
        .or_else(|| value.strip_suffix("DB"))
        .unwrap_or(value)
        .trim();
    number
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && n.abs() < 100.0)
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
    fn parses_replay_gain_values() {
        assert_eq!(parse_replay_gain("-6.54 dB"), Some(-6.54));
        assert_eq!(parse_replay_gain("+1.2dB"), Some(1.2));
        assert_eq!(parse_replay_gain(" 0.987654 "), Some(0.987654));
        assert_eq!(parse_replay_gain("loud"), None);
        assert_eq!(parse_replay_gain("NaN dB"), None);
    }

    #[test]
    fn reads_replay_gain_tags() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagged.flac");
        let dir = std::env::temp_dir().join(format!("djinn-rg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Tag a copy; fixtures stay untouched.
        let copy = dir.join("rg.flac");
        std::fs::copy(&fixture, &copy).unwrap();
        let mut file = lofty::read_from_path(&copy).unwrap();
        let tag = file.primary_tag_mut().unwrap();
        tag.insert_text(ItemKey::ReplayGainTrackGain, "-7.25 dB".into());
        tag.insert_text(ItemKey::ReplayGainTrackPeak, "0.95".into());
        tag.insert_text(ItemKey::ReplayGainAlbumGain, "-6.5 dB".into());
        file.save_to_path(&copy, lofty::config::WriteOptions::default())
            .unwrap();

        let cache = ArtCache::default();
        let scan = ScanContext {
            art_dir: &dir,
            cache: &cache,
            keep_whole: &[],
        };
        let t = read(&copy, "rg.flac", 1, 2, None, &scan);
        assert_eq!(
            t.replay_gain,
            ReplayGainTags {
                track_gain: Some(-7.25),
                track_peak: Some(0.95),
                album_gain: Some(-6.5),
                album_peak: None,
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_tags_and_embedded_art() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagged.flac");
        let art_dir = std::env::temp_dir().join(format!("djinn-tags-{}", std::process::id()));
        let cache = ArtCache::default();
        let scan = ScanContext {
            art_dir: &art_dir,
            cache: &cache,
            keep_whole: &[],
        };
        let t = read(&fixture, "tagged.flac", 1, 2, None, &scan);

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
