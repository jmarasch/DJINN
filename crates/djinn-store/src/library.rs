//! Tracks, albums, artists, and search.

use serde::Serialize;
use sqlx::{FromRow, Sqlite, Transaction};

use crate::{now_ms, Result, Store};

/// A track as returned by the API.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist_id: Option<i64>,
    pub album_id: Option<i64>,
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub duration_ms: Option<i64>,
    pub art_hash: Option<String>,
}

// Column lists are macros so queries stay `&'static str` (built with
// `concat!`), which sqlx accepts as injection-safe.
macro_rules! track_columns {
    () => {
        "t.id, t.title, t.artist, t.album, t.album_artist, t.artist_id, t.album_id, \
         t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.art_hash"
    };
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist_id: i64,
    pub artist: String,
    pub year: Option<i64>,
    pub art_hash: Option<String>,
}

macro_rules! album_columns {
    () => {
        "al.id, al.title, al.artist_id, ar.name AS artist, al.year, al.art_hash"
    };
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub art_hash: Option<String>,
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

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackSort {
    #[default]
    Title,
    /// Artist, then album, then track order.
    Artist,
    /// Most recently added first.
    Added,
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
    /// Most requested (then most played) songs, best first.
    pub top_tracks: Vec<TopTrack>,
    pub albums: Vec<Album>,
    /// Tracks by this artist that are not on one of `albums`.
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
macro_rules! artist_counts {
    () => {
        "SELECT ar.id, ar.name, ar.art_hash, \
                (SELECT count(*) FROM tracks t LEFT JOIN albums al ON al.id = t.album_id \
                 WHERE t.removed_at IS NULL AND (t.artist_id = ar.id OR al.artist_id = ar.id)) \
                 AS track_count \
         FROM artists ar"
    };
}

impl Store {
    pub async fn known_files(&self) -> Result<Vec<KnownFile>> {
        let files = sqlx::query_as(
            "SELECT id, path, size, mtime, removed_at IS NOT NULL AS removed FROM tracks",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(files)
    }

    /// Inserts or updates scanned files in one transaction.
    pub async fn save_scanned(&self, tracks: &[ScannedTrack]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        for track in tracks {
            save_track(&mut tx, track, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Picks album and artist images from what the scanner found, and
    /// deletes artists and albums left with no tracks (unless a playlist
    /// uses them). Run after a scan.
    pub async fn refresh_artwork(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        // Albums: the first listed song's embedded cover, then the album
        // folder's image, then any song's artwork.
        sqlx::query(
            "UPDATE albums SET art_hash = coalesce(
               (SELECT t.embedded_art FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL
                ORDER BY t.disc_no IS NULL, t.disc_no, t.track_no IS NULL, t.track_no, t.path
                LIMIT 1),
               (SELECT t.folder_art FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL AND t.folder_art IS NOT NULL
                ORDER BY t.path LIMIT 1),
               (SELECT t.art_hash FROM tracks t
                WHERE t.album_id = albums.id AND t.removed_at IS NULL AND t.art_hash IS NOT NULL
                ORDER BY t.disc_no, t.track_no, t.path LIMIT 1))",
        )
        .execute(&mut *tx)
        .await?;
        // Artists: an image in the artist's folder, then their first album's
        // cover, then any of their songs' artwork.
        sqlx::query(
            "UPDATE artists SET art_hash = coalesce(
               (SELECT t.artist_art FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                WHERE t.removed_at IS NULL AND t.artist_art IS NOT NULL
                  AND (al.artist_id = artists.id OR t.artist_id = artists.id)
                ORDER BY t.path LIMIT 1),
               (SELECT al.art_hash FROM albums al
                WHERE al.artist_id = artists.id AND al.art_hash IS NOT NULL
                ORDER BY al.year IS NULL, al.year, al.title LIMIT 1),
               (SELECT t.art_hash FROM tracks t
                WHERE t.artist_id = artists.id AND t.removed_at IS NULL AND t.art_hash IS NOT NULL
                ORDER BY t.path LIMIT 1))",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM albums
             WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = albums.id)
               AND NOT EXISTS (SELECT 1 FROM playlist_entries p
                               WHERE p.kind = 'album' AND p.ref_id = albums.id)",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM artists
             WHERE NOT EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = artists.id)
               AND NOT EXISTS (SELECT 1 FROM albums al WHERE al.artist_id = artists.id)
               AND NOT EXISTS (SELECT 1 FROM playlist_entries p
                               WHERE p.kind = 'artist' AND p.ref_id = artists.id)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn mark_removed(&self, ids: &[i64]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        for id in ids {
            sqlx::query("UPDATE tracks SET removed_at = ? WHERE id = ? AND removed_at IS NULL")
                .bind(now)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

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
        let Some(album) = album else { return Ok(None) };
        let tracks = sqlx::query_as(concat!(
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
        Ok(Some(AlbumDetail { album, tracks }))
    }

    pub async fn artist(&self, id: i64) -> Result<Option<ArtistDetail>> {
        #[derive(FromRow)]
        struct Row {
            #[sqlx(flatten)]
            artist: Artist,
            bio: Option<String>,
            bio_url: Option<String>,
        }
        let row: Option<Row> =
            sqlx::query_as("SELECT id, name, art_hash, bio, bio_url FROM artists WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(Row {
            artist,
            bio,
            bio_url,
        }) = row
        else {
            return Ok(None);
        };
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
             WHERE t.artist_id = ? AND ",
            available!(),
            "
               AND (al.id IS NULL OR al.artist_id != t.artist_id)
             ORDER BY t.album, t.disc_no, t.track_no, t.title"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(ArtistDetail {
            artist,
            bio,
            bio_url,
            top_tracks,
            albums,
            other_tracks,
        }))
    }

    /// The artist's songs, most requested first, then most played.
    pub async fn artist_top_tracks(&self, artist_id: i64, limit: i64) -> Result<Vec<TopTrack>> {
        let tracks = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            ", (SELECT count(*) FROM request_events r WHERE r.track_id = t.id) AS requests,
               (SELECT count(*) FROM play_events p WHERE p.track_id = t.id) AS plays
             FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
             WHERE ",
            available!(),
            " AND (t.artist_id = ? OR al.artist_id = ?)
             ORDER BY requests DESC, plays DESC, t.title COLLATE NOCASE
             LIMIT ?"
        ))
        .bind(artist_id)
        .bind(artist_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(tracks)
    }

    /// Artists with playable tracks, A to Z.
    pub async fn artists_page(&self, offset: i64, limit: i64) -> Result<Page<ArtistSummary>> {
        let items = sqlx::query_as(concat!(
            "SELECT id, name, art_hash, track_count FROM (",
            artist_counts!(),
            ") WHERE track_count > 0 ORDER BY name COLLATE NOCASE LIMIT ? OFFSET ?"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM (",
            artist_counts!(),
            ") WHERE track_count > 0"
        ))
        .fetch_one(&self.pool)
        .await?;
        Ok(Page { items, total })
    }

    /// Albums with playable tracks, A to Z.
    pub async fn albums_page(&self, offset: i64, limit: i64) -> Result<Page<Album>> {
        let items = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ") ORDER BY al.title COLLATE NOCASE, ar.name COLLATE NOCASE LIMIT ? OFFSET ?"
        ))
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let total = sqlx::query_scalar(concat!(
            "SELECT count(*) FROM albums al
             WHERE EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ")"
        ))
        .fetch_one(&self.pool)
        .await?;
        Ok(Page { items, total })
    }

    /// All playable tracks in the given order.
    pub async fn tracks_page(
        &self,
        sort: TrackSort,
        offset: i64,
        limit: i64,
    ) -> Result<Page<Track>> {
        let sql = match sort {
            TrackSort::Title => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY t.title COLLATE NOCASE, t.artist COLLATE NOCASE LIMIT ? OFFSET ?"
            ),
            TrackSort::Artist => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY coalesce(t.album_artist, t.artist) IS NULL,
                           coalesce(t.album_artist, t.artist) COLLATE NOCASE,
                           t.album COLLATE NOCASE, t.disc_no, t.track_no, t.title COLLATE NOCASE
                  LIMIT ? OFFSET ?"
            ),
            TrackSort::Added => concat!(
                "SELECT ",
                track_columns!(),
                " FROM tracks t WHERE ",
                available!(),
                " ORDER BY t.added_at DESC, t.id DESC LIMIT ? OFFSET ?"
            ),
        };
        let items = sqlx::query_as(sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?;
        Ok(Page {
            items,
            total: self.track_count().await?,
        })
    }

    /// Search-as-you-type over tracks, albums, and artists. Every word must
    /// match the start of a word in the indexed fields.
    pub async fn search(&self, query: &str, limit: i64) -> Result<SearchResults> {
        let Some(fts) = fts_query(query) else {
            return Ok(SearchResults::default());
        };
        // Title matches rank above artist/album matches, genre lowest.
        let tracks = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            " FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
             WHERE tracks_fts MATCH ? AND ",
            available!(),
            "
             ORDER BY bm25(tracks_fts, 10.0, 5.0, 4.0, 3.0, 1.0)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let albums = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM albums_fts f
             JOIN albums al ON al.id = f.rowid
             JOIN artists ar ON ar.id = al.artist_id
             WHERE albums_fts MATCH ?
               AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id AND ",
            available!(),
            ")
             ORDER BY bm25(albums_fts, 5.0, 1.0)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let artists = sqlx::query_as(concat!(
            "SELECT ar.id, ar.name, ar.art_hash FROM artists_fts f JOIN artists ar ON ar.id = f.rowid
             WHERE artists_fts MATCH ?
               AND EXISTS (
                 SELECT 1 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                 WHERE ",
            available!(),
            " AND (t.artist_id = ar.id OR al.artist_id = ar.id))
             ORDER BY bm25(artists_fts)
             LIMIT ?"
        ))
        .bind(&fts)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(SearchResults {
            tracks,
            albums,
            artists,
        })
    }
}

async fn save_track(tx: &mut Transaction<'_, Sqlite>, t: &ScannedTrack, now: i64) -> Result<()> {
    let artist_id = match &t.artist {
        Some(name) => Some(upsert_artist(tx, name).await?),
        None => None,
    };
    let album_id = match &t.album {
        Some(title) => {
            let album_artist = t
                .album_artist
                .as_deref()
                .or(t.artist.as_deref())
                .unwrap_or(UNKNOWN_ARTIST);
            let album_artist_id = upsert_artist(tx, album_artist).await?;
            Some(upsert_album(tx, title, album_artist_id, t.year).await?)
        }
        None => None,
    };
    sqlx::query(
        "INSERT INTO tracks (path, size, mtime, duration_ms, title, artist, album, album_artist,
                             track_no, disc_no, year, genre, art_hash, sample_rate, channels,
                             codec, artist_id, album_id, added_at, embedded_art, folder_art,
                             artist_art)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (path) DO UPDATE SET
            size = excluded.size, mtime = excluded.mtime, duration_ms = excluded.duration_ms,
            title = excluded.title, artist = excluded.artist, album = excluded.album,
            album_artist = excluded.album_artist, track_no = excluded.track_no,
            disc_no = excluded.disc_no, year = excluded.year, genre = excluded.genre,
            art_hash = excluded.art_hash, sample_rate = excluded.sample_rate,
            channels = excluded.channels, codec = excluded.codec,
            artist_id = excluded.artist_id, album_id = excluded.album_id, removed_at = NULL,
            embedded_art = excluded.embedded_art, folder_art = excluded.folder_art,
            artist_art = excluded.artist_art",
    )
    .bind(&t.path)
    .bind(t.size)
    .bind(t.mtime)
    .bind(t.duration_ms)
    .bind(&t.title)
    .bind(&t.artist)
    .bind(&t.album)
    .bind(&t.album_artist)
    .bind(t.track_no)
    .bind(t.disc_no)
    .bind(t.year)
    .bind(&t.genre)
    .bind(t.art_hash())
    .bind(t.sample_rate)
    .bind(t.channels)
    .bind(&t.codec)
    .bind(artist_id)
    .bind(album_id)
    .bind(now)
    .bind(&t.embedded_art)
    .bind(&t.folder_art)
    .bind(&t.artist_art)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_artist(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<i64> {
    // The no-op update makes RETURNING yield the existing row's id.
    let id = sqlx::query_scalar(
        "INSERT INTO artists (name) VALUES (?)
         ON CONFLICT (name) DO UPDATE SET name = artists.name
         RETURNING id",
    )
    .bind(name)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

async fn upsert_album(
    tx: &mut Transaction<'_, Sqlite>,
    title: &str,
    artist_id: i64,
    year: Option<i64>,
) -> Result<i64> {
    let id = sqlx::query_scalar(
        "INSERT INTO albums (title, artist_id, year) VALUES (?, ?, ?)
         ON CONFLICT (title, artist_id) DO UPDATE SET year = coalesce(albums.year, excluded.year)
         RETURNING id",
    )
    .bind(title)
    .bind(artist_id)
    .bind(year)
    .fetch_one(&mut **tx)
    .await?;
    Ok(id)
}

/// Turns user input into an FTS5 prefix query: each word becomes a quoted
/// prefix term, all required. Splits on the same characters the unicode61
/// tokenizer does, so "AC/DC" searches for "ac" and "dc".
fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(path: &str, title: &str, artist: &str, album: &str) -> ScannedTrack {
        ScannedTrack {
            path: path.to_owned(),
            size: 1,
            mtime: 1,
            title: title.to_owned(),
            artist: Some(artist.to_owned()),
            album: Some(album.to_owned()),
            ..Default::default()
        }
    }

    #[test]
    fn fts_query_quotes_prefix_terms() {
        assert_eq!(fts_query("beat it").as_deref(), Some("\"beat\"* \"it\"*"));
        assert_eq!(fts_query("AC/DC").as_deref(), Some("\"AC\"* \"DC\"*"));
        assert_eq!(fts_query("\" OR *").as_deref(), Some("\"OR\"*"));
        assert_eq!(fts_query("  -- "), None);
    }

    #[tokio::test]
    async fn scan_save_builds_albums_and_artists() {
        let store = Store::open_in_memory().await.unwrap();
        let mut feat = scanned("/m/2.flac", "Guest Spot", "Guest", "Debut");
        feat.album_artist = Some("Band".to_owned());
        store
            .save_scanned(&[scanned("/m/1.flac", "Opener", "Band", "Debut"), feat])
            .await
            .unwrap();

        let results = store.search("debut", 10).await.unwrap();
        assert_eq!(results.albums.len(), 1);
        let album = store.album(results.albums[0].id).await.unwrap().unwrap();
        assert_eq!(album.album.artist, "Band");
        assert_eq!(album.tracks.len(), 2);

        // The featured artist has the track but not the album.
        let guest = store.search("guest", 10).await.unwrap().artists;
        let detail = store.artist(guest[0].id).await.unwrap().unwrap();
        assert!(detail.albums.is_empty());
        assert_eq!(detail.other_tracks.len(), 1);
    }

    #[tokio::test]
    async fn rescanning_updates_in_place_and_removal_hides() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[scanned("/m/1.flac", "Old Title", "A", "X")])
            .await
            .unwrap();
        store
            .save_scanned(&[scanned("/m/1.flac", "New Title", "A", "X")])
            .await
            .unwrap();
        let known = store.known_files().await.unwrap();
        assert_eq!(known.len(), 1);
        assert!(store.search("old", 10).await.unwrap().tracks.is_empty());
        assert_eq!(store.search("new", 10).await.unwrap().tracks.len(), 1);

        store.mark_removed(&[known[0].id]).await.unwrap();
        let results = store.search("new", 10).await.unwrap();
        assert!(results.tracks.is_empty());
        assert!(results.albums.is_empty());
        assert!(results.artists.is_empty());
        assert_eq!(store.track(known[0].id).await.unwrap(), None);

        // Coming back clears the removal and keeps the id.
        store
            .save_scanned(&[scanned("/m/1.flac", "New Title", "A", "X")])
            .await
            .unwrap();
        assert_eq!(store.known_files().await.unwrap()[0].id, known[0].id);
        assert_eq!(store.track_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn browse_pages() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[
                scanned("/m/1.flac", "beta", "Zed", "Second"),
                scanned("/m/2.flac", "Alpha", "zed", "Second"),
                scanned("/m/3.flac", "gamma", "Abba", "First"),
            ])
            .await
            .unwrap();

        let artists = store.artists_page(0, 10).await.unwrap();
        let names: Vec<_> = artists
            .items
            .iter()
            .map(|a| (a.name.as_str(), a.track_count))
            .collect();
        assert_eq!(names, [("Abba", 1), ("Zed", 2)]);
        assert_eq!(artists.total, 2);

        let albums = store.albums_page(0, 1).await.unwrap();
        assert_eq!((albums.items[0].title.as_str(), albums.total), ("First", 2));

        let titles =
            |page: Page<Track>| page.items.into_iter().map(|t| t.title).collect::<Vec<_>>();
        assert_eq!(
            titles(store.tracks_page(TrackSort::Title, 0, 10).await.unwrap()),
            ["Alpha", "beta", "gamma"]
        );
        assert_eq!(
            titles(store.tracks_page(TrackSort::Title, 1, 1).await.unwrap()),
            ["beta"]
        );
        assert_eq!(
            titles(store.tracks_page(TrackSort::Artist, 0, 10).await.unwrap()),
            ["gamma", "Alpha", "beta"]
        );

        // Removed files drop out, and so do artists left with nothing.
        let abba = store
            .known_files()
            .await
            .unwrap()
            .into_iter()
            .find(|k| k.path == "/m/3.flac")
            .unwrap();
        store.mark_removed(&[abba.id]).await.unwrap();
        assert_eq!(store.artists_page(0, 10).await.unwrap().total, 1);
        assert_eq!(store.albums_page(0, 10).await.unwrap().total, 1);
        assert_eq!(
            store
                .tracks_page(TrackSort::Added, 0, 10)
                .await
                .unwrap()
                .total,
            2
        );
    }

    #[tokio::test]
    async fn artwork_rules_and_tidy() {
        let store = Store::open_in_memory().await.unwrap();
        let track =
            |path: &str, no: i64, embedded: Option<&str>, folder: Option<&str>| ScannedTrack {
                path: path.into(),
                title: path.into(),
                artist: Some("Band".into()),
                album: Some("Record".into()),
                track_no: Some(no),
                embedded_art: embedded.map(Into::into),
                folder_art: folder.map(Into::into),
                artist_art: Some("artistpic".into()),
                ..Default::default()
            };
        // Track 2 is scanned first, but track 1's embedded cover wins.
        store
            .save_scanned(&[
                track("/m/b.flac", 2, Some("second"), Some("folder")),
                track("/m/a.flac", 1, Some("first"), Some("folder")),
            ])
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        let album = &store.albums_page(0, 5).await.unwrap().items[0];
        assert_eq!(album.art_hash.as_deref(), Some("first"));
        let artist = &store.artists_page(0, 5).await.unwrap().items[0];
        assert_eq!(artist.art_hash.as_deref(), Some("artistpic"));

        // Without an embedded cover on the first song, the folder image is used.
        store
            .save_scanned(&[track("/m/a.flac", 1, None, Some("folder"))])
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        let album = &store.albums_page(0, 5).await.unwrap().items[0];
        assert_eq!(album.art_hash.as_deref(), Some("folder"));

        // Renamed artist: the old one disappears once nothing uses it.
        let mut renamed = track("/m/a.flac", 1, None, None);
        renamed.artist = Some("New Band".into());
        let mut renamed_b = track("/m/b.flac", 2, None, None);
        renamed_b.artist = Some("New Band".into());
        store.save_scanned(&[renamed, renamed_b]).await.unwrap();
        store.refresh_artwork().await.unwrap();
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM artists")
            .fetch_all(store.pool())
            .await
            .unwrap();
        assert_eq!(names, ["New Band"]);
    }
}
