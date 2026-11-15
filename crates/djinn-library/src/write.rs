//! Writing tags into a music file, for the library editor. Only the fields
//! asked for change, in the file's main tag (made when it has none); the
//! other tags in the file and the audio are left as they are. The caller
//! keeps what the file said before ([`read`] it first) to undo.

use std::collections::BTreeMap;
use std::path::Path;

use lofty::config::WriteOptions;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::*;
use lofty::tag::items::Timestamp;
use lofty::tag::Tag;
use serde::{Deserialize, Serialize};

pub use crate::compare::{read, FileTags};

/// One field to change: its name (as in [`FileTags`]) and the new value,
/// None to remove it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldValue {
    pub field: String,
    pub value: Option<String>,
}

/// The fields the editor can write.
pub const FIELDS: [&str; 8] = [
    "title",
    "artist",
    "album",
    "album_artist",
    "track_no",
    "disc_no",
    "year",
    "genre",
];

/// Other tags are named in changes with this in front ("x:Composer").
pub const OTHER: &str = "x:";

/// Tags the grid has columns of its own for, or that DJINN keeps or works
/// out elsewhere (lyrics, play length, fingerprints): never other tags.
const NOT_OTHER: &[ItemKey] = &[
    ItemKey::TrackTitle,
    ItemKey::TrackArtist,
    ItemKey::TrackArtists,
    ItemKey::AlbumTitle,
    ItemKey::AlbumArtist,
    ItemKey::AlbumArtists,
    ItemKey::TrackNumber,
    ItemKey::DiscNumber,
    ItemKey::Year,
    ItemKey::RecordingDate,
    ItemKey::ReleaseDate,
    ItemKey::OriginalReleaseDate,
    ItemKey::Genre,
    ItemKey::Lyrics,
    ItemKey::UnsyncLyrics,
    ItemKey::Popularimeter,
    ItemKey::AcoustIdFingerprint,
    ItemKey::Length,
];

/// The tag an other tag's name stands for ("Composer"), if it is one.
pub fn other_tag_key(name: &str) -> Option<ItemKey> {
    ItemKey::VARIANTS
        .iter()
        .copied()
        .filter(|k| !NOT_OTHER.contains(k))
        .find(|k| format!("{k:?}") == name)
}

/// A tag's text values in `tag`, several of one tag joined by "; ".
pub fn other_tags(tag: &Tag) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for item in tag.items() {
        let key = item.key();
        if NOT_OTHER.contains(&key) {
            continue;
        }
        let Some(text) = item.value().text().map(str::trim).filter(|t| !t.is_empty()) else {
            continue;
        };
        out.entry(format!("{key:?}"))
            .and_modify(|v| {
                v.push_str("; ");
                v.push_str(text);
            })
            .or_insert_with(|| text.to_owned());
    }
    out
}

/// Checks a value for an other tag: plain text up to 1,000 characters, a
/// number for the number tags, 0 or 1 for the yes or no ones.
fn check_other(key: ItemKey, name: &str, value: &str) -> Result<(), String> {
    if key.is_flag() {
        return if value == "0" || value == "1" {
            Ok(())
        } else {
            Err(format!("{name} should be 1 (yes) or 0 (no)"))
        };
    }
    if key.is_numeric() && !value.parse::<f64>().is_ok_and(f64::is_finite) {
        return Err(format!("{name} should be a number"));
    }
    if value.trim().is_empty() {
        return Err(format!("{name} is empty"));
    }
    if value.chars().count() > 1000 || value.chars().any(char::is_control) {
        return Err(format!(
            "{name} should be plain text, at most 1,000 characters"
        ));
    }
    Ok(())
}

/// Checks a value for a field: plain text up to 300 characters, or a
/// number in range for the numbers.
pub fn check(change: &FieldValue) -> Result<(), String> {
    if let Some(name) = change.field.strip_prefix(OTHER) {
        let key = other_tag_key(name).ok_or_else(|| format!("{name} can't be changed"))?;
        return match &change.value {
            Some(value) => check_other(key, name, value),
            None => Ok(()),
        };
    }
    if !FIELDS.contains(&change.field.as_str()) {
        return Err(format!("{} can't be changed", change.field));
    }
    let Some(value) = &change.value else {
        return if change.field == "title" {
            Err("a song needs a title".into())
        } else {
            Ok(())
        };
    };
    match change.field.as_str() {
        "track_no" | "disc_no" => match value.parse::<u32>() {
            Ok(n) if (1..=999).contains(&n) => Ok(()),
            _ => Err(format!(
                "{} should be a number from 1 to 999",
                label(&change.field)
            )),
        },
        "year" => match value.parse::<i64>().ok().and_then(djinn_store::real_year) {
            Some(_) => Ok(()),
            None => Err(format!(
                "the year should be 1000 to {}",
                djinn_store::latest_year()
            )),
        },
        _ if value.trim().is_empty() => Err(format!("{} is empty", label(&change.field))),
        _ if value.chars().count() > 300 || value.chars().any(char::is_control) => Err(format!(
            "the {} should be plain text, at most 300 characters",
            label(&change.field)
        )),
        _ => Ok(()),
    }
}

fn label(field: &str) -> &str {
    match field {
        "track_no" => "the track number",
        "disc_no" => "the disc number",
        "album_artist" => "album artist",
        other => other,
    }
}

/// A field's value as [`FileTags`] has it, as text.
pub fn value_of(tags: &FileTags, field: &str) -> Option<String> {
    let number = |n: Option<i64>| n.map(|n| n.to_string());
    match field {
        "title" => Some(tags.title.clone()).filter(|t| !t.is_empty()),
        "artist" => tags.artist.clone(),
        "album" => tags.album.clone(),
        "album_artist" => tags.album_artist.clone(),
        "track_no" => number(tags.track_no),
        "disc_no" => number(tags.disc_no),
        "year" => number(tags.year),
        "genre" => tags.genre.clone(),
        other => other
            .strip_prefix(OTHER)
            .and_then(|name| tags.extra.get(name).cloned()),
    }
}

/// Writes `changes` into the file's main tag.
pub fn write_tags(path: &Path, changes: &[FieldValue]) -> Result<(), String> {
    for change in changes {
        check(change)?;
    }
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let mut tag = file
        .primary_tag()
        .cloned()
        .unwrap_or_else(|| Tag::new(file.primary_tag_type()));
    for change in changes {
        let value = change.value.clone();
        let number = || value.as_deref().and_then(|v| v.parse::<u32>().ok());
        match change.field.as_str() {
            "title" => match value {
                Some(v) => tag.set_title(v),
                None => tag.remove_title(),
            },
            "artist" => {
                // A list of artists would win over the plain tag when read.
                tag.remove_key(ItemKey::TrackArtists);
                match value {
                    Some(v) => tag.set_artist(v),
                    None => tag.remove_artist(),
                }
            }
            "album" => match value {
                Some(v) => tag.set_album(v),
                None => tag.remove_album(),
            },
            "album_artist" => {
                tag.remove_key(ItemKey::AlbumArtists);
                match value {
                    Some(v) => {
                        tag.insert_text(ItemKey::AlbumArtist, v);
                    }
                    None => tag.remove_key(ItemKey::AlbumArtist),
                }
            }
            "track_no" => match number() {
                Some(n) => tag.set_track(n),
                None => tag.remove_track(),
            },
            "disc_no" => match number() {
                Some(n) => tag.set_disk(n),
                None => tag.remove_disk(),
            },
            "year" => {
                // An original release date is read first; keep the two in step.
                let had_original = tag.get_string(ItemKey::OriginalReleaseDate).is_some();
                match number().and_then(|y| u16::try_from(y).ok()) {
                    Some(year) => {
                        tag.set_date(Timestamp {
                            year,
                            ..Timestamp::default()
                        });
                        if had_original {
                            tag.insert_text(ItemKey::OriginalReleaseDate, year.to_string());
                        }
                    }
                    None => {
                        tag.remove_date();
                        tag.remove_key(ItemKey::OriginalReleaseDate);
                    }
                }
            }
            "genre" => match value {
                Some(v) => tag.set_genre(v),
                None => tag.remove_genre(),
            },
            other => {
                let Some(name) = other.strip_prefix(OTHER) else {
                    continue;
                };
                let key = other_tag_key(name).ok_or_else(|| format!("{name} can't be changed"))?;
                tag.remove_key(key);
                if let Some(v) = value {
                    if !tag.insert_text(key, v) {
                        return Err(format!("this kind of file can't hold {name}"));
                    }
                }
            }
        }
    }
    tag.save_to_path(path, WriteOptions::default())
        .map_err(|e| e.to_string())
}

/// The front cover embedded in a file, as (bytes, MIME type), if it has one.
pub fn read_cover(path: &Path) -> Result<Option<(Vec<u8>, String)>, String> {
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return Ok(None);
    };
    let picture = tag
        .pictures()
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first());
    Ok(picture.map(|p| {
        (
            p.data().to_vec(),
            p.mime_type()
                .map_or("image/jpeg", MimeType::as_str)
                .to_owned(),
        )
    }))
}

/// Puts `image` (JPEG or PNG) in the file as its front cover, replacing any
/// cover it had; None takes the cover out.
pub fn write_cover(path: &Path, image: Option<&[u8]>) -> Result<(), String> {
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let mut tag = file
        .primary_tag()
        .cloned()
        .unwrap_or_else(|| Tag::new(file.primary_tag_type()));
    tag.remove_picture_type(PictureType::CoverFront);
    if let Some(image) = image {
        let mime = if image.starts_with(&[0x89, b'P', b'N', b'G']) {
            MimeType::Png
        } else if image.starts_with(&[0xFF, 0xD8]) {
            MimeType::Jpeg
        } else {
            return Err("the cover should be a JPEG or PNG picture".into());
        };
        tag.push_picture(
            Picture::unchecked(image.to_vec())
                .pic_type(PictureType::CoverFront)
                .mime_type(mime)
                .build(),
        );
    }
    tag.save_to_path(path, WriteOptions::default())
        .map_err(|e| e.to_string())
}

/// A picture as JPEG (for `folder.jpg`): a JPEG as it is, anything else
/// the image library reads converted at quality 90.
pub fn jpeg_of(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return Ok(bytes.to_vec());
    }
    let picture = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
    picture
        .to_rgb8()
        .write_with_encoder(encoder)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_checked() {
        let v = |field: &str, value: Option<&str>| FieldValue {
            field: field.into(),
            value: value.map(str::to_owned),
        };
        assert!(check(&v("title", Some("Song"))).is_ok());
        assert!(check(&v("title", None)).is_err());
        assert!(check(&v("genre", None)).is_ok());
        assert!(check(&v("track_no", Some("0"))).is_err());
        assert!(check(&v("track_no", Some("12"))).is_ok());
        assert!(check(&v("year", Some("85"))).is_err());
        assert!(check(&v("path", Some("x"))).is_err());
        assert!(check(&v("artist", Some("a\nb"))).is_err());
        assert!(check(&v("x:Composer", Some("Someone"))).is_ok());
        assert!(check(&v("x:Composer", None)).is_ok());
        assert!(check(&v("x:Bpm", Some("fast"))).is_err());
        assert!(check(&v("x:Bpm", Some("128"))).is_ok());
        assert!(check(&v("x:FlagCompilation", Some("2"))).is_err());
        // The grid's own fields and lyrics aren't other tags.
        assert!(check(&v("x:TrackTitle", Some("Song"))).is_err());
        assert!(check(&v("x:Lyrics", Some("la"))).is_err());
        assert!(check(&v("x:Nothing", Some("a"))).is_err());
    }

    #[test]
    fn the_editors_tag_list_names_real_tags() {
        let list = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/lib/editorTags.ts"),
        )
        .unwrap();
        let names: Vec<&str> = list
            .lines()
            .filter_map(|l| l.trim().strip_prefix("['"))
            .filter_map(|l| l.split('\'').next())
            .collect();
        assert!(names.len() > 50, "{}", names.len());
        for name in names {
            assert!(
                other_tag_key(name).is_some(),
                "{name} isn't a tag the editor can write"
            );
        }
    }

    #[test]
    fn pictures_become_jpeg() {
        let mut png = Vec::new();
        image::RgbImage::new(4, 4)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpeg = jpeg_of(&png).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        assert_eq!(jpeg_of(&jpeg).unwrap(), jpeg);
        assert!(jpeg_of(b"nothing").is_err());
    }

    fn copy_of(fixture: &str, name: &str) -> std::path::PathBuf {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../djinn-engine/tests/fixtures")
            .join(fixture);
        let to = std::env::temp_dir().join(format!(
            "djinn-write-{name}-{}-{fixture}",
            std::process::id()
        ));
        std::fs::copy(from, &to).unwrap();
        to
    }

    fn set(field: &str, value: Option<&str>) -> FieldValue {
        FieldValue {
            field: field.into(),
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn tags_and_covers_are_written_into_flac_and_mp3() {
        for fixture in ["gapless_a.flac", "gapless_a.mp3"] {
            let path = copy_of(fixture, "tags");
            let audio_before = crate::write::read(&path, true).unwrap();
            write_tags(
                &path,
                &[
                    set("title", Some("New Title")),
                    set("artist", Some("Band")),
                    set("album_artist", Some("Various")),
                    set("track_no", Some("7")),
                    set("disc_no", Some("2")),
                    set("year", Some("1999")),
                    set("genre", Some("Rock")),
                ],
            )
            .unwrap();
            let after = read(&path, true).unwrap();
            assert_eq!(after.title, "New Title", "{fixture}");
            assert_eq!(after.artist.as_deref(), Some("Band"));
            assert_eq!(after.album_artist.as_deref(), Some("Various"));
            assert_eq!(
                (after.track_no, after.disc_no, after.year),
                (Some(7), Some(2), Some(1999))
            );
            assert_eq!(after.genre.as_deref(), Some("Rock"));
            // Taking a field out, and putting the old value back (undo).
            write_tags(
                &path,
                &[
                    set("genre", None),
                    set(
                        "title",
                        value_of(&audio_before, "title").as_deref().or(Some("x")),
                    ),
                ],
            )
            .unwrap();
            let undone = read(&path, true).unwrap();
            assert_eq!(undone.genre, None);
            // Other tags go in, show up, and come out again.
            write_tags(
                &path,
                &[
                    set("x:Composer", Some("Someone")),
                    set("x:Comment", Some("A note")),
                ],
            )
            .unwrap();
            let other = read(&path, true).unwrap();
            assert_eq!(
                other.extra.get("Composer").map(String::as_str),
                Some("Someone"),
                "{fixture}"
            );
            assert_eq!(value_of(&other, "x:Comment").as_deref(), Some("A note"));
            write_tags(&path, &[set("x:Composer", None)]).unwrap();
            let gone = read(&path, true).unwrap();
            assert!(!gone.extra.contains_key("Composer"));
            assert!(gone.extra.contains_key("Comment"));
            // A cover goes in and comes out again.
            let png = [0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0];
            write_cover(&path, Some(&png)).unwrap();
            let (data, mime) = read_cover(&path).unwrap().unwrap();
            assert_eq!((data.as_slice(), mime.as_str()), (&png[..], "image/png"));
            write_cover(&path, None).unwrap();
            assert!(read_cover(&path).unwrap().is_none());
            assert!(write_cover(&path, Some(b"not a picture")).is_err());
            // The audio still plays the same length.
            assert!(!read(&path, true).unwrap().title.is_empty());
            std::fs::remove_file(&path).ok();
        }
    }
}
