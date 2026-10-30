//! Tracks, albums, artists, and search.

use serde::Serialize;
use sqlx::{FromRow, Sqlite, Transaction};

use crate::{now_ms, Result, Store};

/// An artist credited on a track.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Credit {
    pub id: i64,
    pub name: String,
    /// "feat." rather than a main artist.
    pub featured: bool,
}

/// A track as returned by the API.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Track {
    pub id: i64,
    pub title: String,
    /// The artist credit as tagged, like "Zara Larsson & David Guetta".
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist_id: Option<i64>,
    pub album_id: Option<i64>,
    /// Each credited artist, in order.
    #[sqlx(json)]
    pub credits: Vec<Credit>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_ms: Option<i64>,
    pub art_hash: Option<String>,
}

// Column lists are macros so queries stay `&'static str` (built with
// `concat!`), which sqlx accepts as injection-safe. A song without artwork
// of its own shows its album's cover.
macro_rules! track_columns {
    () => {
        "t.id, t.title, t.artist, t.album, t.album_artist, t.artist_id, t.album_id, \
         (SELECT json_group_array(json_object('id', c.id, 'name', c.name, \
                 'featured', json(CASE WHEN c.featured THEN 'true' ELSE 'false' END))) \
          FROM (SELECT a.id, a.name, ta.featured FROM track_artists ta \
                JOIN artists a ON a.id = ta.artist_id \
                WHERE ta.track_id = t.id ORDER BY ta.position) c) AS credits, \
         t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, \
         coalesce(t.art_hash, (SELECT fa.art_hash FROM albums fa WHERE fa.id = t.album_id)) AS art_hash"
    };
}
pub(crate) use track_columns;

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist_id: i64,
    pub artist: String,
    pub year: Option<i64>,
    pub art_hash: Option<String>,
    /// Comma separated, from MusicBrainz, or from its songs once their
    /// genres are corrected in the app (album pages only).
    pub genres: Option<String>,
}

macro_rules! album_columns {
    () => {
        "al.id, al.title, al.artist_id, ar.name AS artist, al.year, al.art_hash, al.genres"
    };
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub art_hash: Option<String>,
}

/// An artist due for an online info lookup.
#[derive(Clone, Debug, FromRow)]
pub struct InfoLookupArtist {
    pub id: i64,
    pub name: String,
    /// Has a picture in their own folder, so no photo is needed.
    pub has_folder_art: bool,
    /// A few of their song titles, for checking an online match is really
    /// them. Separated by newlines.
    pub sample_titles: String,
}

/// An album due for an online info lookup.
#[derive(Clone, Debug, FromRow)]
pub struct InfoLookupAlbum {
    pub id: i64,
    pub title: String,
    pub artist: String,
    /// No cover from the files or folder, so one is worth finding online.
    pub needs_cover: bool,
}

/// What an online lookup found for an artist. `None` fields keep whatever
/// was found before.
#[derive(Clone, Debug, Default)]
pub struct ArtistInfo {
    pub photo: Option<String>,
    pub photo_credit: Option<String>,
    pub photo_credit_url: Option<String>,
    pub bio: Option<String>,
    pub bio_url: Option<String>,
    pub mbid: Option<String>,
    pub genres: Option<String>,
    /// Both services answered; don't look again until the periodic recheck.
    pub checked: bool,
}

/// A song with how often it's been requested and played, for "top songs".
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct TopTrack {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub track: Track,
    pub requests: i64,
    pub plays: i64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

/// One page of a browse listing.
#[derive(Clone, Debug, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Total across all pages.
    pub total: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct ArtistSummary {
    pub id: i64,
    pub name: String,
    pub art_hash: Option<String>,
    /// Playable tracks by this artist or on their albums.
    pub track_count: i64,
}

/// A name as sorted: a leading "The", "A", or "An" doesn't count, so The
/// Weeknd files under W.
macro_rules! sort_key {
    ($col:literal) => {
        concat!(
            "(CASE WHEN ",
            $col,
            " LIKE 'the %' THEN substr(",
            $col,
            ", 5) ",
            "WHEN ",
            $col,
            " LIKE 'an %' THEN substr(",
            $col,
            ", 4) ",
            "WHEN ",
            $col,
            " LIKE 'a %' THEN substr(",
            $col,
            ", 3) ",
            "ELSE ",
            $col,
            " END) COLLATE NOCASE"
        )
    };
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlbumSort {
    #[default]
    Name,
    /// Newest first.
    Year,
    /// Oldest first.
    Oldest,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackSort {
    #[default]
    Title,
    /// Artist, then album, then track order.
    Artist,
    /// Most recently added first.
    Added,
    /// Most requested, then most played (as artist pages rank top songs).
    Popular,
    /// Newest first, then by artist.
    Year,
    /// Oldest first, then by artist.
    Oldest,
}

/// Where a letter or a year starts in a sorted list, for jumping there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Jump {
    pub label: String,
    pub offset: i64,
}

/// The first place each label appears in a list's sort keys (in list
/// order). Names go under their first letter, with digits and other
/// characters under "#"; years are their own labels; unknown keys have none.
fn jumps(keys: Vec<Option<String>>, years: bool) -> Vec<Jump> {
    let mut found: Vec<Jump> = Vec::new();
    for (offset, key) in keys.into_iter().enumerate() {
        let Some(key) = key else { continue };
        let label = if years { key } else { letter(&key) };
        if !found.iter().any(|j| j.label == label) {
            found.push(Jump {
                label,
                offset: offset as i64,
            });
        }
    }
    found
}

/// "A" to "Z" for names starting with a letter (accents dropped), "#" for
/// everything else.
fn letter(name: &str) -> String {
    let first = name.trim_start().chars().next().unwrap_or('#');
    let plain = match first.to_uppercase().next().unwrap_or(first) {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'Ç' => 'C',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ñ' => 'N',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'Ý' => 'Y',
        c => c,
    };
    if plain.is_ascii_uppercase() {
        plain.to_string()
    } else {
        "#".to_owned()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AlbumDetail {
    #[serde(flatten)]
    pub album: Album,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtistDetail {
    #[serde(flatten)]
    pub artist: Artist,
    /// Short biography, when looked up online.
    pub bio: Option<String>,
    /// Where the bio came from.
    pub bio_url: Option<String>,
    /// Comma separated, from MusicBrainz, plus or instead of genres
    /// corrected in the app on songs they lead.
    pub genres: Option<String>,
    /// Credit for a free-licensed photo ("Name (CC BY-SA 4.0)").
    pub photo_credit: Option<String>,
    pub photo_credit_url: Option<String>,
    /// Most requested (then most played) songs, best first.
    pub top_tracks: Vec<TopTrack>,
    pub albums: Vec<Album>,
    /// Other artists' albums this artist is credited on.
    pub appears_on: Vec<Album>,
    /// Tracks crediting this artist that are not on one of `albums`.
    pub other_tracks: Vec<Track>,
}

/// What the scanner knows about a file already in the database.
#[derive(Clone, Debug, FromRow)]
pub struct KnownFile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub removed: bool,
}

/// Everything the scanner extracts from one file.
#[derive(Clone, Debug, Default)]
pub struct ScannedTrack {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub codec: Option<String>,
    /// Cover embedded in the file's tags.
    pub embedded_art: Option<String>,
    /// Image in the file's folder (the album folder, usually).
    pub folder_art: Option<String>,
    /// Image in the artist's folder.
    pub artist_art: Option<String>,
    /// Each credited artist (name, featured), in order. Empty means the
    /// whole `artist` text is one artist.
    pub credits: Vec<(String, bool)>,
    /// The album's owner: its first credited album artist. Falls back to
    /// `album_artist`, then the first credit.
    pub album_owner: Option<String>,
    pub replay_gain: ReplayGainTags,
}

/// ReplayGain values from a file's tags.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReplayGainTags {
    /// dB.
    pub track_gain: Option<f64>,
    /// Linear, 1.0 is full scale.
    pub track_peak: Option<f64>,
    pub album_gain: Option<f64>,
    pub album_peak: Option<f64>,
}

impl ScannedTrack {
    /// The track's own artwork: embedded, else its folder's image.
    pub fn art_hash(&self) -> Option<&str> {
        self.embedded_art.as_deref().or(self.folder_art.as_deref())
    }
}

/// Album artist used when a file has an album but no artist tags at all.
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";

/// A track is playable if its file is present.
macro_rules! available {
    () => {
        "t.removed_at IS NULL"
    };
}

/// Every artist with a count of playable tracks they appear on, as track
/// artist or album artist.
/// Albums with playable tracks.
macro_rules! listed_albums {
    () => {
        concat!(
            " FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ")"
        )
    };
}

/// The order of each album sort.
macro_rules! album_order {
    (name) => {
        concat!(sort_key!("al.title"), ", ", sort_key!("ar.name"))
    };
    (newest) => {
        concat!("al.year IS NULL, al.year DESC, ", sort_key!("al.title"))
    };
    (oldest) => {
        concat!("al.year IS NULL, al.year, ", sort_key!("al.title"))
    };
}

/// The order of each song sort.
macro_rules! track_order {
    (title) => {
        "t.title COLLATE NOCASE, t.artist COLLATE NOCASE"
    };
    (artist) => {
        concat!(
            "coalesce(t.album_artist, t.artist) IS NULL, ",
            sort_key!("coalesce(t.album_artist, t.artist)"),
            ", ",
            sort_key!("t.album"),
            ", t.disc_no, t.track_no, t.title COLLATE NOCASE"
        )
    };
    (newest) => {
        concat!("t.year IS NULL, t.year DESC, ", track_order!(artist))
    };
    (oldest) => {
        concat!("t.year IS NULL, t.year, ", track_order!(artist))
    };
}

// Ids of an artist's songs, credited or on their albums, found through the
// indexes. `$artist` is the SQL for the artist's id, like "ar.id". (Testing
// every song for each artist instead took seconds on 36,000 songs.)
macro_rules! artist_songs {
    ($artist:literal) => {
        concat!(
            "(SELECT ta.track_id FROM track_artists ta WHERE ta.artist_id = ",
            $artist,
            " UNION ALL SELECT t2.id FROM albums al JOIN tracks t2 ON t2.album_id = al.id \
             WHERE al.artist_id = ",
            $artist,
            ")"
        )
    };
}

// Counted in one pass: (artist, song) pairs from credits and from album
// artists, each pair once. (Counting per artist with an OR over every song
// took seconds per page on a library of 36,000 songs.)
macro_rules! artist_counts {
    () => {
        "SELECT ar.id, ar.name, ar.art_hash, coalesce(c.track_count, 0) AS track_count \
         FROM artists ar LEFT JOIN ( \
           SELECT artist_id, count(*) AS track_count FROM ( \
             SELECT ta.artist_id, ta.track_id FROM track_artists ta \
               JOIN tracks t ON t.id = ta.track_id WHERE t.removed_at IS NULL \
             UNION \
             SELECT al.artist_id, t.id FROM tracks t \
               JOIN albums al ON al.id = t.album_id WHERE t.removed_at IS NULL \
           ) GROUP BY artist_id \
         ) c ON c.artist_id = ar.id"
    };
}

impl Store {
    pub async fn track_count(&self) -> Result<i64> {
        let count = sqlx::query_scalar("SELECT count(*) FROM tracks t WHERE t.removed_at IS NULL")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    pub async fn track(&self, id: i64) -> Result<Option<Track>> {
        let track = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks t WHERE t.id = ? AND ",
            available!()
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(track)
    }

    /// Playable tracks by id, in the order given (missing ones skipped).
    pub async fn tracks_by_ids(&self, ids: &[i64]) -> Result<Vec<Track>> {
        let list = json_ids(ids);
        let found: Vec<Track> = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks t WHERE t.id IN (SELECT value FROM json_each(?)) AND ",
            available!()
        ))
        .bind(list)
        .fetch_all(&self.pool)
        .await?;
        Ok(in_order(ids, found, |t| t.id))
    }

    /// Artists by id, in the order given, with their song counts.
    pub async fn artists_by_ids(&self, ids: &[i64]) -> Result<Vec<ArtistSummary>> {
        let list = json_ids(ids);
        let found: Vec<ArtistSummary> = sqlx::query_as(concat!(
            "SELECT id, name, art_hash, track_count FROM (",
            artist_counts!(),
            ") WHERE track_count > 0 AND id IN (SELECT value FROM json_each(?))"
        ))
        .bind(list)
        .fetch_all(&self.pool)
        .await?;
        Ok(in_order(ids, found, |a| a.id))
    }

    /// Every playable track (for wild play).
    pub async fn library_track_ids(&self) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(concat!(
            "SELECT t.id FROM tracks t WHERE ",
            available!(),
            " ORDER BY t.id"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    /// Filesystem path of a playable track, for the engine.
    pub async fn track_path(&self, id: i64) -> Result<Option<String>> {
        let path = sqlx::query_scalar(concat!(
            "SELECT t.path FROM tracks t WHERE t.id = ? AND ",
            available!()
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(path)
    }

    pub async fn album(&self, id: i64) -> Result<Option<AlbumDetail>> {
        let album: Option<Album> = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE al.id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(mut album) = album else {
            return Ok(None);
        };
        let tracks: Vec<Track> = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks t WHERE t.album_id = ? AND ",
            available!(),
            "
             ORDER BY t.disc_no, t.track_no, t.title"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        // Genres corrected in the app replace the ones looked up online.
        let corrected: bool = sqlx::query_scalar(concat!(
            "SELECT EXISTS (SELECT 1 FROM tag_edits e JOIN tracks t ON t.id = e.track_id
                            WHERE t.album_id = ? AND e.genre IS NOT NULL AND ",
            available!(),
            ")"
        ))
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        if corrected {
            album.genres = genre_list(tracks.iter().filter_map(|t| t.genre.as_deref()));
        }
        Ok(Some(AlbumDetail { album, tracks }))
    }

    pub async fn artist(&self, id: i64) -> Result<Option<ArtistDetail>> {
        #[derive(FromRow)]
        struct Row {
            #[sqlx(flatten)]
            artist: Artist,
            bio: Option<String>,
            bio_url: Option<String>,
            genres: Option<String>,
            photo_credit: Option<String>,
            photo_credit_url: Option<String>,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT id, name, art_hash, bio, bio_url, genres, photo_credit, photo_credit_url
             FROM artists WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(Row {
            artist,
            bio,
            bio_url,
            mut genres,
            photo_credit,
            photo_credit_url,
        }) = row
        else {
            return Ok(None);
        };
        // Genres corrected on songs they lead go in place of the ones looked
        // up online, which still stand for the songs not corrected.
        let led: Vec<(Option<String>, bool)> = sqlx::query_as(concat!(
            "SELECT t.genre, EXISTS (SELECT 1 FROM tag_edits e
                                     WHERE e.track_id = t.id AND e.genre IS NOT NULL)
             FROM tracks t JOIN track_artists ta ON ta.track_id = t.id
             WHERE ta.artist_id = ? AND NOT ta.featured AND ",
            available!()
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        if led.iter().any(|(_, corrected)| *corrected) {
            let looked_up = led.iter().any(|(_, corrected)| !corrected);
            let corrected = led
                .iter()
                .filter(|(_, c)| *c)
                .filter_map(|(g, _)| g.as_deref());
            genres = genre_list(corrected.chain(genres.as_deref().filter(|_| looked_up)));
        }
        let top_tracks = self.artist_top_tracks(id, 10).await?;
        let albums = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE al.artist_id = ?
               AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ")
             ORDER BY al.year, al.title"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let other_tracks = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks t
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ?) AND ",
            available!(),
            "
               AND (al.id IS NULL OR al.artist_id != ?)
             ORDER BY t.album, t.disc_no, t.track_no, t.title"
        ))
        .bind(id)
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let appears_on = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE al.artist_id != ?
               AND EXISTS (SELECT 1 FROM tracks t JOIN track_artists ta ON ta.track_id = t.id
                           WHERE t.album_id = al.id AND ta.artist_id = ? AND ",
            available!(),
            ")
             ORDER BY al.year, al.title"
        ))
        .bind(id)
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(ArtistDetail {
            artist,
            bio,
            bio_url,
            genres,
            photo_credit,
            photo_credit_url,
            top_tracks,
            albums,
            appears_on,
            other_tracks,
        }))
    }
}

/// Genres from several songs, each once, as "a, b, c"; `None` if there are
/// none.
fn genre_list<'a>(genres: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for genre in genres.flat_map(|g| g.split(',')).map(str::trim) {
        if !genre.is_empty() && !seen.iter().any(|s| s.eq_ignore_ascii_case(genre)) {
            seen.push(genre);
        }
    }
    (!seen.is_empty()).then(|| seen.join(", "))
}

/// Ids as a JSON array, for `json_each(?)`: one static query for any
/// number of ids.
fn json_ids(ids: &[i64]) -> String {
    let parts: Vec<String> = ids.iter().map(i64::to_string).collect();
    format!("[{}]", parts.join(","))
}

/// Puts `found` in the order of `ids`.
fn in_order<T>(ids: &[i64], found: Vec<T>, id: impl Fn(&T) -> i64) -> Vec<T> {
    let mut by_id: std::collections::HashMap<i64, T> =
        found.into_iter().map(|item| (id(&item), item)).collect();
    ids.iter().filter_map(|i| by_id.remove(i)).collect()
}

mod browse;
mod info;
mod scan;
#[cfg(test)]
mod tests;
