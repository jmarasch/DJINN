//! Reads playlists exported from other services and players: CSV and tab
//! separated tables (TuneMyMusic, Soundiiz, Exportify, Apple Music text,
//! Google Takeout), Spotify's account data JSON, JSPF, XSPF, Apple Music XML,
//! M3U, PLS, and plain "Artist - Title" lines. Only names come out of this;
//! `playlist_match` finds them in the library.

use std::collections::HashMap;

use djinn_store::WantedSong;

/// One song from a file. A YouTube video id comes with no title: the caller
/// has to look it up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportedSong {
    pub song: WantedSong,
    pub video_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportedList {
    pub name: Option<String>,
    pub songs: Vec<ImportedSong>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedImport {
    pub lists: Vec<ImportedList>,
    /// Lines that didn't name a song, such as links.
    pub skipped: usize,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("that's only links: a link doesn't say which songs it holds, so export the playlist to a file first")]
    OnlyLinks,
    #[error("no songs found in that")]
    NoSongs,
}

/// Reads a playlist. `filename` helps tell M3U and PLS from plain text.
pub fn parse(text: &str, filename: Option<&str>) -> Result<ParsedImport, ImportError> {
    let text = text.trim_start_matches('\u{feff}').trim();
    let extension = filename
        .and_then(|f| f.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    let extension = extension.as_deref();
    let lower_start: String = text.chars().take(64).collect::<String>().to_lowercase();

    let parsed = if let Some(lists) = from_json(text) {
        ParsedImport { lists, skipped: 0 }
    } else if text.starts_with('<') {
        from_xml(text).ok_or(ImportError::NoSongs)?
    } else if lower_start.starts_with("[playlist]") || extension == Some("pls") {
        from_pls(text)
    } else if lower_start.starts_with("#extm3u") || matches!(extension, Some("m3u" | "m3u8")) {
        from_m3u(text)
    } else if let Some(parsed) = from_table(text) {
        parsed
    } else {
        from_lines(text)?
    };
    let lists: Vec<ImportedList> = parsed
        .lists
        .into_iter()
        .filter(|l| !l.songs.is_empty())
        .collect();
    if lists.is_empty() {
        return Err(ImportError::NoSongs);
    }
    Ok(ParsedImport {
        lists,
        skipped: parsed.skipped,
    })
}

fn cleaned(text: &str) -> Option<String> {
    // Exports often space lists of names with no-break spaces.
    let text = text
        .replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

fn is_link(text: &str) -> bool {
    let text = text.trim().to_ascii_lowercase();
    text.starts_with("http://") || text.starts_with("https://") || text.starts_with("spotify:")
}

/// `Artist - Title` (either way round is possible), `Title by Artist`, or
/// just a title.
fn song_from_text(text: &str) -> Option<WantedSong> {
    let text = cleaned(text)?;
    for dash in [" - ", " \u{2013} ", " \u{2014} "] {
        if let Some((artist, title)) = text.split_once(dash) {
            return Some(WantedSong {
                title: cleaned(title)?,
                artist: cleaned(artist),
                either_way: true,
                ..Default::default()
            });
        }
    }
    if let Some((title, artist)) = text.rsplit_once(" by ") {
        return Some(WantedSong {
            title: cleaned(title)?,
            artist: cleaned(artist),
            ..Default::default()
        });
    }
    Some(WantedSong {
        title: text,
        ..Default::default()
    })
}

/// "1. ", "12) " and the like at the start of a line.
fn without_number(line: &str) -> &str {
    let rest = line.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() == line.len() {
        return line;
    }
    match rest.strip_prefix(['.', ')', ':']) {
        Some(rest) if rest.starts_with(' ') => rest.trim_start(),
        _ => line,
    }
}

/// "3:45" or "1:02:03" as milliseconds.
fn clock_ms(text: &str) -> Option<i64> {
    if !text.contains(':') {
        return None;
    }
    let mut seconds = 0i64;
    for part in text.split(':') {
        seconds = seconds * 60 + part.trim().parse::<i64>().ok()?;
    }
    Some(seconds * 1000)
}

/// A bare number is seconds unless it can only be milliseconds.
fn duration_ms(text: &str, always_ms: bool) -> Option<i64> {
    let text = text.trim();
    if let Some(ms) = clock_ms(text) {
        return Some(ms);
    }
    let number: f64 = text.parse().ok()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    let ms = if always_ms || number >= 10_000.0 {
        number
    } else {
        number * 1000.0
    };
    Some(ms as i64)
}

fn from_lines(text: &str) -> Result<ParsedImport, ImportError> {
    let (mut songs, mut skipped, mut links) = (Vec::new(), 0, 0);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if is_link(line) {
            links += 1;
            skipped += 1;
            continue;
        }
        let song = match line.split_once('\t') {
            // Copied out of a table: title, then artist.
            Some((title, artist)) => cleaned(title).map(|title| WantedSong {
                title,
                artist: cleaned(artist),
                either_way: true,
                ..Default::default()
            }),
            None => song_from_text(without_number(line)),
        };
        match song {
            Some(song) => songs.push(ImportedSong {
                song,
                video_id: None,
            }),
            None => skipped += 1,
        }
    }
    if songs.is_empty() && links > 0 {
        return Err(ImportError::OnlyLinks);
    }
    Ok(ParsedImport {
        lists: vec![ImportedList { name: None, songs }],
        skipped,
    })
}

/// Splits delimited text into rows, with double quotes as in RFC 4180.
fn rows(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let (mut rows, mut row, mut cell, mut quoted) = (Vec::new(), Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                cell.push('"');
            }
            '"' if quoted => quoted = false,
            '"' if cell.is_empty() => quoted = true,
            c if quoted => cell.push(c),
            c if c == delimiter => row.push(std::mem::take(&mut cell)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    row.push(cell);
    rows.push(row);
    rows
}

#[derive(Default)]
struct Columns {
    title: Option<usize>,
    artist: Option<usize>,
    album: Option<usize>,
    duration: Option<usize>,
    duration_is_ms: bool,
    video: Option<usize>,
    playlist: Option<usize>,
    path: Option<usize>,
    kind: Option<usize>,
}

/// Works out what a header row's columns hold. Spellings vary by service and
/// over time, so this goes by name rather than by knowing each format.
fn columns(header: &[String]) -> Option<Columns> {
    let mut found = Columns::default();
    for (i, name) in header.iter().enumerate() {
        let name = name.trim().trim_matches('\u{feff}').to_lowercase();
        let slot = match name.as_str() {
            "title" | "song" | "track" | "name" | "track name" | "track title" | "song name"
            | "song title" => &mut found.title,
            "artist" | "artists" | "artist name" | "artist name(s)" | "artist names"
            | "artist name 1" | "creator" | "band" | "performer" => &mut found.artist,
            "album" | "album name" | "album title" | "release" => &mut found.album,
            "duration"
            | "length"
            | "time"
            | "duration (ms)"
            | "track duration (ms)"
            | "duration_ms" => {
                found.duration_is_ms = name.contains("ms");
                &mut found.duration
            }
            "video id" | "videoid" | "video_id" => &mut found.video,
            "playlist" | "playlist name" | "playlist title" => &mut found.playlist,
            "path" | "file" | "location" | "filename" => &mut found.path,
            "type" | "kind" => &mut found.kind,
            _ => continue,
        };
        slot.get_or_insert(i);
    }
    (found.title.is_some() || found.video.is_some()).then_some(found)
}

/// A table with a header row we understand. Some exports put a few lines of
/// their own above it.
fn from_table(text: &str) -> Option<ParsedImport> {
    let first = text.lines().next()?;
    let delimiter = ['\t', ';', ','].into_iter().find(|&d| first.contains(d));
    let delimiter = delimiter.or_else(|| {
        text.lines()
            .take(10)
            .find_map(|l| ['\t', ';', ','].into_iter().find(|&d| l.contains(d)))
    })?;
    let rows = rows(text, delimiter);
    let (start, found) = rows
        .iter()
        .take(10)
        .enumerate()
        .find_map(|(i, row)| Some((i, columns(row)?)))?;

    let cell =
        |row: &[String], column: Option<usize>| -> Option<String> { cleaned(row.get(column?)?) };
    let mut lists: Vec<ImportedList> = Vec::new();
    let mut by_name: HashMap<Option<String>, usize> = HashMap::new();
    let mut skipped = 0;
    for row in &rows[start + 1..] {
        if row.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        // Some exports also list whole albums and artists you follow.
        let kind = cell(row, found.kind).map(|k| k.to_lowercase());
        if matches!(kind.as_deref(), Some("album" | "artist")) {
            skipped += 1;
            continue;
        }
        let video_id = cell(row, found.video);
        let title = cell(row, found.title);
        if title.is_none() && video_id.is_none() {
            skipped += 1;
            continue;
        }
        let song = ImportedSong {
            song: WantedSong {
                title: title.clone().unwrap_or_default(),
                artist: cell(row, found.artist),
                album: cell(row, found.album),
                duration_ms: cell(row, found.duration)
                    .and_then(|d| duration_ms(&d, found.duration_is_ms)),
                path: cell(row, found.path),
                either_way: false,
            },
            // With a title already there, nothing needs looking up.
            video_id: video_id.filter(|_| title.is_none()),
        };
        let name = cell(row, found.playlist);
        let index = *by_name.entry(name.clone()).or_insert_with(|| {
            lists.push(ImportedList {
                name,
                songs: Vec::new(),
            });
            lists.len() - 1
        });
        lists[index].songs.push(song);
    }
    Some(ParsedImport { lists, skipped })
}

fn from_m3u(text: &str) -> ParsedImport {
    let mut list = ImportedList::default();
    let mut skipped = 0;
    let mut pending: Option<WantedSong> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("#PLAYLIST:") {
            list.name = cleaned(name);
        } else if let Some(info) = line.strip_prefix("#EXTINF:") {
            let (seconds, label) = info.split_once(',').unwrap_or((info, ""));
            let mut song = song_from_text(label).unwrap_or_default();
            song.duration_ms = seconds
                .split_whitespace()
                .next()
                .and_then(|s| duration_ms(s, false));
            pending = Some(song);
        } else if let Some(album) = line.strip_prefix("#EXTALB:") {
            if let Some(song) = &mut pending {
                song.album = cleaned(album);
            }
        } else if !line.starts_with('#') {
            let song = pending.take().filter(|s| !s.title.is_empty());
            if is_link(line) && song.is_none() {
                skipped += 1;
                continue;
            }
            let mut song = song.unwrap_or_else(|| song_from_path(line));
            if !is_link(line) {
                song.path = Some(line.to_owned());
            }
            list.songs.push(ImportedSong {
                song,
                video_id: None,
            });
        }
    }
    ParsedImport {
        lists: vec![list],
        skipped,
    }
}

/// What a file's own name says: `01 - Artist - Title.mp3`, say.
fn song_from_path(path: &str) -> WantedSong {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let stem = stem.trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ');
    let stem = stem
        .strip_prefix("- ")
        .or_else(|| stem.strip_prefix(". "))
        .unwrap_or(stem);
    song_from_text(stem).unwrap_or(WantedSong {
        title: name.to_owned(),
        ..Default::default()
    })
}

fn from_pls(text: &str) -> ParsedImport {
    #[derive(Default)]
    struct Entry {
        file: Option<String>,
        title: Option<String>,
        length: Option<i64>,
    }
    let mut entries: std::collections::BTreeMap<u32, Entry> = Default::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let number = |prefix: &str| key.strip_prefix(prefix)?.parse::<u32>().ok();
        let value = value.trim();
        if let Some(n) = number("file") {
            entries.entry(n).or_default().file = Some(value.to_owned());
        } else if let Some(n) = number("title") {
            entries.entry(n).or_default().title = Some(value.to_owned());
        } else if let Some(n) = number("length") {
            entries.entry(n).or_default().length = duration_ms(value, false);
        }
    }
    let (mut songs, mut skipped) = (Vec::new(), 0);
    for entry in entries.into_values() {
        let link = entry.file.as_deref().is_some_and(is_link);
        let named = entry.title.as_deref().and_then(song_from_text);
        let mut song = match (named, &entry.file) {
            (Some(song), _) => song,
            (None, Some(file)) if !link => song_from_path(file),
            _ => {
                skipped += 1;
                continue;
            }
        };
        song.duration_ms = entry.length;
        song.path = entry.file.filter(|_| !link);
        songs.push(ImportedSong {
            song,
            video_id: None,
        });
    }
    ParsedImport {
        lists: vec![ImportedList { name: None, songs }],
        skipped,
    }
}

/// Spotify's account data (`Playlist1.json`), JSPF, or a plain array of
/// songs. `None` if it isn't JSON at all.
fn from_json(text: &str) -> Option<Vec<ImportedList>> {
    if !text.starts_with(['{', '[']) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let string = |v: &serde_json::Value, keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| v.get(*k)?.as_str().and_then(cleaned))
    };
    let song = |v: &serde_json::Value| -> Option<ImportedSong> {
        // Spotify nests the song under `track`.
        let v = v.get("track").filter(|t| t.is_object()).unwrap_or(v);
        let duration = v
            .get("duration")
            .or_else(|| v.get("duration_ms"))
            .and_then(serde_json::Value::as_i64);
        Some(ImportedSong {
            song: WantedSong {
                title: string(v, &["trackName", "title", "name", "track"])?,
                artist: string(v, &["artistName", "creator", "artist"]),
                album: string(v, &["albumName", "album"]),
                duration_ms: duration.filter(|&d| d > 0),
                ..Default::default()
            },
            video_id: None,
        })
    };
    let songs = |v: &serde_json::Value, key: &str| -> Vec<ImportedSong> {
        v.get(key)
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(song).collect())
            .unwrap_or_default()
    };

    if let Some(lists) = value.get("playlists").and_then(|p| p.as_array()) {
        return Some(
            lists
                .iter()
                .map(|l| ImportedList {
                    name: string(l, &["name", "title"]),
                    songs: songs(l, "items"),
                })
                .collect(),
        );
    }
    if let Some(list) = value.get("playlist") {
        return Some(vec![ImportedList {
            name: string(list, &["title", "name"]),
            songs: songs(list, "track"),
        }]);
    }
    let items = value.as_array()?;
    Some(vec![ImportedList {
        name: None,
        songs: items.iter().filter_map(song).collect(),
    }])
}

fn from_xml(text: &str) -> Option<ParsedImport> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "playlist" => Some(from_xspf(root)),
        "plist" => from_apple(root),
        _ => None,
    }
}

fn from_xspf(root: roxmltree::Node<'_, '_>) -> ParsedImport {
    let text_of = |node: roxmltree::Node<'_, '_>, tag: &str| -> Option<String> {
        node.children()
            .find(|c| c.tag_name().name() == tag)?
            .text()
            .and_then(cleaned)
    };
    let (mut songs, mut skipped) = (Vec::new(), 0);
    let tracks = root
        .descendants()
        .filter(|n| n.tag_name().name() == "track");
    for track in tracks {
        let location = text_of(track, "location");
        let path = location.as_deref().filter(|l| !is_link(l)).map(file_uri);
        let title = text_of(track, "title");
        let Some(title) = title.or_else(|| path.as_deref().map(|p| song_from_path(p).title)) else {
            skipped += 1;
            continue;
        };
        songs.push(ImportedSong {
            song: WantedSong {
                title,
                artist: text_of(track, "creator"),
                album: text_of(track, "album"),
                duration_ms: text_of(track, "duration").and_then(|d| duration_ms(&d, true)),
                path,
                either_way: false,
            },
            video_id: None,
        });
    }
    ParsedImport {
        lists: vec![ImportedList {
            name: text_of(root, "title"),
            songs,
        }],
        skipped,
    }
}

/// `file:///C:/Music/A%20Song.mp3` as a path.
fn file_uri(location: &str) -> String {
    let Some(rest) = location.strip_prefix("file://") else {
        return location.to_owned();
    };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| rest.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let Some(byte) = hex {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    // Windows drives come as /C:/...
    match path.as_bytes() {
        [b'/', _, b':', ..] => path[1..].to_owned(),
        _ => path,
    }
}

/// Apple Music's XML: a `Tracks` dict by id and a `Playlists` array whose
/// items point into it.
fn from_apple(root: roxmltree::Node<'_, '_>) -> Option<ParsedImport> {
    use roxmltree::Node;
    fn elements<'a, 'b>(node: Node<'a, 'b>) -> impl Iterator<Item = Node<'a, 'b>> {
        node.children().filter(|c| c.is_element())
    }
    /// The value after `<key>name</key>` in a plist dict.
    fn value<'a, 'b>(dict: Node<'a, 'b>, name: &str) -> Option<Node<'a, 'b>> {
        let mut children = elements(dict);
        while let Some(key) = children.next() {
            let item = children.next()?;
            if key.text() == Some(name) {
                return Some(item);
            }
        }
        None
    }
    let text = |dict: Node<'_, '_>, name: &str| value(dict, name)?.text().and_then(cleaned);

    let top = elements(root).next()?;
    let mut tracks = HashMap::new();
    let mut children = elements(value(top, "Tracks")?);
    while let (Some(key), Some(track)) = (children.next(), children.next()) {
        if let Some(id) = key.text() {
            tracks.insert(id.to_owned(), track);
        }
    }
    let mut lists = Vec::new();
    for playlist in elements(value(top, "Playlists")?) {
        // The whole library and the built-in lists aren't playlists of yours.
        if value(playlist, "Master").is_some() || value(playlist, "Distinguished Kind").is_some() {
            continue;
        }
        let Some(items) = value(playlist, "Playlist Items") else {
            continue;
        };
        let songs = elements(items)
            .filter_map(|item| {
                let track = *tracks.get(value(item, "Track ID")?.text()?)?;
                Some(ImportedSong {
                    song: WantedSong {
                        title: text(track, "Name")?,
                        artist: text(track, "Artist"),
                        album: text(track, "Album"),
                        duration_ms: text(track, "Total Time").and_then(|d| d.parse().ok()),
                        path: text(track, "Location").map(|l| file_uri(&l)),
                        either_way: false,
                    },
                    video_id: None,
                })
            })
            .collect();
        lists.push(ImportedList {
            name: text(playlist, "Name"),
            songs,
        });
    }
    Some(ParsedImport { lists, skipped: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(text: &str, filename: Option<&str>) -> Vec<WantedSong> {
        let parsed = parse(text, filename).unwrap();
        assert_eq!(parsed.lists.len(), 1);
        parsed.lists[0]
            .songs
            .iter()
            .map(|s| s.song.clone())
            .collect()
    }

    fn named(title: &str, artist: &str) -> WantedSong {
        WantedSong {
            title: title.into(),
            artist: Some(artist.into()),
            ..Default::default()
        }
    }

    #[test]
    fn tunemymusic_style_csv_groups_by_playlist_and_skips_albums() {
        let parsed = parse(
            "Track name,Artist name,Album,Playlist name,Type,ISRC\n\
             S&M,Rihanna,Loud,EDM,Playlist,USUM71100000\n\
             \"Numb, Again\",\"Marshmello, Khalid\",,EDM,Playlist,\n\
             Roygbiv,Boards of Canada,,Chill,Favorite,\n\
             Geogaddi,Boards of Canada,,,Album,\n",
            Some("export.csv"),
        )
        .unwrap();
        assert_eq!(parsed.lists.len(), 2);
        assert_eq!(parsed.lists[0].name.as_deref(), Some("EDM"));
        assert_eq!(parsed.lists[0].songs[1].song.title, "Numb, Again");
        assert_eq!(
            parsed.lists[0].songs[1].song.artist.as_deref(),
            Some("Marshmello, Khalid")
        );
        assert_eq!(parsed.lists[1].name.as_deref(), Some("Chill"));
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn exportify_durations_are_milliseconds() {
        let songs = one(
            "\"Track URI\",\"Track Name\",\"Album Name\",\"Artist Name(s)\",\"Track Duration (ms)\"\r\n\
             \"spotify:track:1\",\"I Cry\",\"Wild Ones\",\"Flo Rida\",\"223800\"\r\n",
            None,
        );
        assert_eq!(songs[0].title, "I Cry");
        assert_eq!(songs[0].duration_ms, Some(223_800));
        assert!(!songs[0].either_way);
    }

    #[test]
    fn apple_text_export_is_tab_separated() {
        let songs = one(
            "Name\tArtist\tComposer\tAlbum\tTime\nHalo\tBeyonce\t\tI Am\t261\n",
            Some("list.txt"),
        );
        assert_eq!(songs[0].title, "Halo");
        assert_eq!(songs[0].album.as_deref(), Some("I Am"));
        assert_eq!(songs[0].duration_ms, Some(261_000));
    }

    #[test]
    fn takeout_lists_only_video_ids() {
        let parsed = parse(
            "Video ID,Playlist Video Creation Timestamp\n\
             dQw4w9WgXcQ,2024-01-01T00:00:00+00:00\n",
            Some("My list videos.csv"),
        )
        .unwrap();
        let song = &parsed.lists[0].songs[0];
        assert_eq!(song.video_id.as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(song.song.title, "");
    }

    #[test]
    fn takeout_with_lines_above_the_header() {
        let parsed = parse(
            "Playlist Id,Channel Id\nPL1,UC1\n\nVideo Id,Time Added\nabc,2020\n",
            None,
        )
        .unwrap();
        assert_eq!(parsed.lists[0].songs[0].video_id.as_deref(), Some("abc"));
    }

    #[test]
    fn a_title_means_no_lookup() {
        let parsed = parse(
            "Video ID,Song Title,Artist Name 1\nabc,Halo,Beyonce\n",
            None,
        )
        .unwrap();
        let song = &parsed.lists[0].songs[0];
        assert_eq!(song.video_id, None);
        assert_eq!(song.song, named("Halo", "Beyonce"));
    }

    #[test]
    fn spotify_account_data_makes_one_list_each() {
        let parsed = parse(
            r#"{"playlists":[
                {"name":"EDM","items":[
                  {"track":{"trackName":"Numb","artistName":"Marshmello","albumName":"N","trackUri":"spotify:track:1"}},
                  {"track":null,"episode":{"name":"A podcast"}}]},
                {"name":"Empty","items":[]},
                {"name":"Chill","items":[{"track":{"trackName":"Roygbiv","artistName":"Boards of Canada"}}]}
            ]}"#,
            None,
        )
        .unwrap();
        let names: Vec<_> = parsed.lists.iter().map(|l| l.name.as_deref()).collect();
        assert_eq!(names, [Some("EDM"), Some("Chill")]);
        assert_eq!(parsed.lists[0].songs.len(), 1);
    }

    #[test]
    fn jspf() {
        let songs = one(
            r#"{"playlist":{"title":"Mix","track":[{"title":"Halo","creator":"Beyonce","duration":261000}]}}"#,
            None,
        );
        assert_eq!(songs[0].duration_ms, Some(261_000));
        assert_eq!(songs[0].artist.as_deref(), Some("Beyonce"));
    }

    #[test]
    fn m3u_with_and_without_extinf() {
        let parsed = parse(
            "#EXTM3U\n#PLAYLIST:Mine\n#EXTINF:242,Rihanna - S&M\n#EXTALB:Loud\nC:\\Music\\sm.mp3\n\
             /music/Boards of Canada/01 - Boards of Canada - Roygbiv.flac\n\
             https://example.com/stream.mp3\n#EXTINF:100,Named - Stream\nhttps://example.com/2.mp3\n",
            None,
        )
        .unwrap();
        assert_eq!(parsed.lists[0].name.as_deref(), Some("Mine"));
        let songs = &parsed.lists[0].songs;
        assert_eq!(songs[0].song.artist.as_deref(), Some("Rihanna"));
        assert_eq!(songs[0].song.title, "S&M");
        assert_eq!(songs[0].song.album.as_deref(), Some("Loud"));
        assert_eq!(songs[0].song.duration_ms, Some(242_000));
        assert_eq!(songs[0].song.path.as_deref(), Some("C:\\Music\\sm.mp3"));
        assert_eq!(songs[1].song.title, "Roygbiv");
        assert_eq!(songs[2].song.title, "Stream");
        assert_eq!(songs[2].song.path, None);
        assert_eq!(parsed.skipped, 1, "a bare link names nothing");
    }

    #[test]
    fn pls() {
        let songs = one(
            "[playlist]\nFile1=/m/a.mp3\nTitle1=Rihanna - S&M\nLength1=242\nFile2=/m/Roygbiv.mp3\nNumberOfEntries=2\n",
            None,
        );
        assert_eq!(songs[0].title, "S&M");
        assert_eq!(songs[0].duration_ms, Some(242_000));
        assert_eq!(songs[1].title, "Roygbiv");
        assert_eq!(songs[1].path.as_deref(), Some("/m/Roygbiv.mp3"));
    }

    #[test]
    fn xspf() {
        let songs = one(
            r#"<?xml version="1.0"?><playlist version="1" xmlns="http://xspf.org/ns/0/">
              <title>Mine</title><trackList>
              <track><location>file:///C:/Music/A%20Song.mp3</location><title>A Song</title>
                <creator>Someone</creator><duration>200000</duration></track>
              <track><location>file:///music/Other.flac</location></track>
              <track><location>https://example.com/x.mp3</location></track>
            </trackList></playlist>"#,
            None,
        );
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].path.as_deref(), Some("C:/Music/A Song.mp3"));
        assert_eq!(songs[0].duration_ms, Some(200_000));
        assert_eq!(songs[1].title, "Other");
    }

    #[test]
    fn apple_music_xml_leaves_out_the_library_itself() {
        let parsed = parse(
            r#"<?xml version="1.0"?><plist version="1.0"><dict>
              <key>Tracks</key><dict>
                <key>7</key><dict><key>Track ID</key><integer>7</integer><key>Name</key><string>Halo</string>
                  <key>Artist</key><string>Beyonce</string><key>Total Time</key><integer>261000</integer></dict>
              </dict>
              <key>Playlists</key><array>
                <dict><key>Name</key><string>Library</string><key>Master</key><true/>
                  <key>Playlist Items</key><array><dict><key>Track ID</key><integer>7</integer></dict></array></dict>
                <dict><key>Name</key><string>Mine</string>
                  <key>Playlist Items</key><array><dict><key>Track ID</key><integer>7</integer></dict></array></dict>
              </array></dict></plist>"#,
            Some("Mine.xml"),
        )
        .unwrap();
        assert_eq!(parsed.lists.len(), 1);
        assert_eq!(parsed.lists[0].name.as_deref(), Some("Mine"));
        assert_eq!(parsed.lists[0].songs[0].song.duration_ms, Some(261_000));
    }

    #[test]
    fn pasted_lines() {
        let songs = one(
            "1. Rihanna - S&M\n2) Flo Rida \u{2013} I Cry\n\nHalo by Beyonce\n\nJust A Title\n\
             99 Problems\nhttps://open.spotify.com/track/abc\n",
            None,
        );
        assert_eq!(
            songs[0],
            WantedSong {
                either_way: true,
                ..named("S&M", "Rihanna")
            }
        );
        assert_eq!(songs[1].artist.as_deref(), Some("Flo Rida"));
        assert_eq!(songs[2], named("Halo", "Beyonce"));
        assert_eq!(songs[3].title, "Just A Title");
        assert_eq!(songs[4].title, "99 Problems", "only list numbers come off");
        assert_eq!(songs.len(), 5);
    }

    #[test]
    fn a_share_link_gets_an_explanation() {
        let link = "https://open.spotify.com/playlist/3N39yn1USEz2d3WNZmrPFl?si=8675861f9755401a";
        assert_eq!(parse(link, None), Err(ImportError::OnlyLinks));
        assert_eq!(parse("", None), Err(ImportError::NoSongs));
    }
}
