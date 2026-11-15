//! The library editor's compare screen: what a file's own tags say next to
//! what DJINN's database holds for the song, field by field, and why they
//! differ. Reads only; neither the files nor the database change.

use std::collections::BTreeMap;
use std::path::Path;

use djinn_store::{CompareRow, ScannedTrack, TagEdit};
use serde::{Deserialize, Serialize};

use crate::tags::fill_from_tags;

/// The tags of one file, read the way a library check reads them but without
/// DJINN's corrections or identifications.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct FileTags {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    /// The file's other text tags (composer, comment, label...) by their
    /// name in [`crate::write::other_tag_key`], from its main tag.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

/// Reads a file's tags; the error says why it couldn't be read.
pub fn read(path: &Path, original_years: bool) -> Result<FileTags, String> {
    let file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    let mut track = ScannedTrack::default();
    fill_from_tags(&mut track, &file, original_years);
    let title = if track.title.is_empty() {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        track.title
    };
    Ok(FileTags {
        title,
        artist: track.artist,
        album: track.album,
        album_artist: track.album_artist,
        track_no: track.track_no,
        disc_no: track.disc_no,
        year: track.year,
        genre: track.genre,
        extra: lofty::file::TaggedFileExt::primary_tag(&file)
            .map(crate::write::other_tags)
            .unwrap_or_default(),
    })
}

/// Why the database and the file differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Corrected in DJINN (Edit tags or Edit album).
    Correction,
    /// The file has nothing and an applied identification filled it in.
    Identified,
    /// Neither: the file changed since it was read, or the two readings
    /// differ for another reason.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldDiff {
    pub field: &'static str,
    /// What DJINN holds.
    pub database: Option<String>,
    /// What the file says.
    pub file: Option<String>,
    pub why: Why,
}

fn text(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn number(value: Option<i64>) -> Option<String> {
    value.map(|v| v.to_string())
}

/// The fields where the database and the file differ.
pub fn differences(
    row: &CompareRow,
    file: &FileTags,
    edit: Option<&TagEdit>,
    identified: bool,
) -> Vec<FieldDiff> {
    let mut out = Vec::new();
    let mut check =
        |field: &'static str, db: Option<String>, file: Option<String>, edited: bool| {
            if db == file {
                return;
            }
            let why = if edited {
                Why::Correction
            } else if identified && file.is_none() && db.is_some() {
                Why::Identified
            } else {
                Why::Other
            };
            out.push(FieldDiff {
                field,
                database: db,
                file,
                why,
            });
        };
    let e = |f: fn(&TagEdit) -> bool| edit.is_some_and(f);
    check(
        "title",
        text(&Some(row.title.clone())),
        text(&Some(file.title.clone())),
        e(|x| x.title.is_some()),
    );
    check(
        "artist",
        text(&row.artist),
        text(&file.artist),
        e(|x| x.artist.is_some()),
    );
    check(
        "album",
        text(&row.album),
        text(&file.album),
        e(|x| x.album.is_some()),
    );
    check(
        "album_artist",
        text(&row.album_artist),
        text(&file.album_artist),
        e(|x| x.album_artist.is_some()),
    );
    check(
        "track_no",
        number(row.track_no),
        number(file.track_no),
        e(|x| x.track_no.is_some()),
    );
    check(
        "disc_no",
        number(row.disc_no),
        number(file.disc_no),
        e(|x| x.disc_no.is_some()),
    );
    check(
        "year",
        number(row.year),
        number(file.year),
        e(|x| x.year.is_some()),
    );
    check(
        "genre",
        text(&row.genre),
        text(&file.genre),
        e(|x| x.genre.is_some()),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> CompareRow {
        CompareRow {
            id: 1,
            path: "/m/a.mp3".into(),
            title: "Song".into(),
            artist: Some("Band".into()),
            album: Some("Album".into()),
            album_artist: None,
            track_no: Some(3),
            disc_no: None,
            year: Some(1999),
            genre: Some("Rock".into()),
        }
    }

    fn same_file() -> FileTags {
        FileTags {
            title: "Song".into(),
            artist: Some("Band".into()),
            album: Some("Album".into()),
            album_artist: None,
            track_no: Some(3),
            disc_no: None,
            year: Some(1999),
            genre: Some("Rock".into()),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn matching_tags_have_no_differences_and_blank_equals_missing() {
        assert!(differences(&row(), &same_file(), None, false).is_empty());
        let mut file = same_file();
        file.genre = Some("  ".into());
        file.album_artist = Some(String::new());
        let mut db = row();
        db.genre = None;
        assert!(differences(&db, &file, None, false).is_empty());
    }

    #[test]
    fn differences_say_why() {
        let mut file = same_file();
        file.title = "Sng".into();
        file.year = None;
        file.genre = None;
        let edit = TagEdit {
            title: Some("Song".into()),
            ..TagEdit::default()
        };
        let diffs = differences(&row(), &file, Some(&edit), true);
        let why = |f: &str| diffs.iter().find(|d| d.field == f).map(|d| d.why);
        // Corrected in DJINN.
        assert_eq!(why("title"), Some(Why::Correction));
        // The file has none and an identification filled both.
        assert_eq!(why("year"), Some(Why::Identified));
        assert_eq!(why("genre"), Some(Why::Identified));
        // Nothing explains a changed track number.
        file = same_file();
        file.track_no = Some(4);
        let diffs = differences(&row(), &file, None, true);
        assert_eq!(diffs.len(), 1);
        assert_eq!((diffs[0].field, diffs[0].why), ("track_no", Why::Other));
    }
}
