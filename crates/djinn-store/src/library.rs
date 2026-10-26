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
    /// Comma separated, from MusicBrainz.
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
    /// Comma separated, from MusicBrainz.
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
macro_rules! artist_counts {
    () => {
        "SELECT ar.id, ar.name, ar.art_hash, \
                (SELECT count(*) FROM tracks t LEFT JOIN albums al ON al.id = t.album_id \
                 WHERE t.removed_at IS NULL AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) OR al.artist_id = ar.id)) \
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
        // folder's image, then any song's artwork, then a cover found online.
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
                ORDER BY t.disc_no, t.track_no, t.path LIMIT 1),
               albums.online_art_hash)",
        )
        .execute(&mut *tx)
        .await?;
        // Artists: an image in the artist's folder, then a photo looked up
        // online, then their first album's cover, then any song's artwork.
        sqlx::query(
            "UPDATE artists SET art_hash = coalesce(
               (SELECT t.artist_art FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                WHERE t.removed_at IS NULL AND t.artist_art IS NOT NULL
                  AND (al.artist_id = artists.id OR EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = artists.id))
                ORDER BY t.path LIMIT 1),
               artists.online_art_hash,
               (SELECT al.art_hash FROM albums al
                WHERE al.artist_id = artists.id AND al.art_hash IS NOT NULL
                ORDER BY al.year IS NULL, al.year, al.title LIMIT 1),
               (SELECT t.art_hash FROM tracks t
                WHERE EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = artists.id) AND t.removed_at IS NULL AND t.art_hash IS NOT NULL
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
               AND NOT EXISTS (SELECT 1 FROM track_artists ta WHERE ta.artist_id = artists.id)
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
            genres,
            photo_credit,
            photo_credit_url,
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

    /// Artists with playable tracks whose online info was never looked up
    /// or was last looked up before `older_than` (Unix ms).
    pub async fn artists_for_info_lookup(&self, older_than: i64) -> Result<Vec<InfoLookupArtist>> {
        let artists = sqlx::query_as(
            "SELECT ar.id, ar.name,
                    EXISTS (SELECT 1 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                            WHERE t.artist_art IS NOT NULL AND t.removed_at IS NULL
                              AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) OR al.artist_id = ar.id))
                        AS has_folder_art,
                    coalesce((SELECT group_concat(title, char(10)) FROM (
                        SELECT t.title FROM tracks t
                        WHERE EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) AND t.removed_at IS NULL
                        ORDER BY t.id LIMIT 5)), '') AS sample_titles
             FROM artists ar
             WHERE (ar.info_checked_at IS NULL OR ar.info_checked_at < ?)
               AND EXISTS (SELECT 1 FROM tracks t LEFT JOIN albums al ON al.id = t.album_id
                           WHERE t.removed_at IS NULL
                             AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) OR al.artist_id = ar.id))
             ORDER BY ar.name",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await?;
        Ok(artists)
    }

    /// Saves what an online lookup found for an artist, keeping earlier
    /// finds for anything it didn't find this time.
    pub async fn save_artist_info(&self, id: i64, info: &ArtistInfo) -> Result<()> {
        sqlx::query(
            "UPDATE artists SET online_art_hash = coalesce(?, online_art_hash),
                                photo_credit = CASE WHEN ? IS NULL THEN photo_credit ELSE ? END,
                                photo_credit_url = CASE WHEN ? IS NULL THEN photo_credit_url
                                                        ELSE ? END,
                                bio = coalesce(?, bio), bio_url = coalesce(?, bio_url),
                                mbid = coalesce(?, mbid), genres = coalesce(?, genres),
                                info_checked_at = CASE WHEN ? THEN ? ELSE info_checked_at END
             WHERE id = ?",
        )
        .bind(&info.photo)
        // A new photo replaces the credit (even with none, e.g. from Deezer).
        .bind(&info.photo)
        .bind(&info.photo_credit)
        .bind(&info.photo)
        .bind(&info.photo_credit_url)
        .bind(&info.bio)
        .bind(&info.bio_url)
        .bind(&info.mbid)
        .bind(&info.genres)
        .bind(info.checked)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Albums with playable tracks not looked up online since `older_than`.
    pub async fn albums_for_info_lookup(&self, older_than: i64) -> Result<Vec<InfoLookupAlbum>> {
        let albums = sqlx::query_as(
            "SELECT al.id, al.title, ar.name AS artist,
                    NOT EXISTS (SELECT 1 FROM tracks t
                                WHERE t.album_id = al.id AND t.art_hash IS NOT NULL)
                        AS needs_cover
             FROM albums al JOIN artists ar ON ar.id = al.artist_id
             WHERE (al.info_checked_at IS NULL OR al.info_checked_at < ?)
               AND EXISTS (SELECT 1 FROM tracks t
                           WHERE t.album_id = al.id AND t.removed_at IS NULL)
             ORDER BY ar.name, al.title",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await?;
        Ok(albums)
    }

    pub async fn save_album_info(
        &self,
        id: i64,
        cover: Option<&str>,
        mbid: Option<&str>,
        genres: Option<&str>,
        checked: bool,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE albums SET online_art_hash = coalesce(?, online_art_hash),
                               mbid = coalesce(?, mbid), genres = coalesce(?, genres),
                               info_checked_at = CASE WHEN ? THEN ? ELSE info_checked_at END
             WHERE id = ?",
        )
        .bind(cover)
        .bind(mbid)
        .bind(genres)
        .bind(checked)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
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
            " AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ?) OR al.artist_id = ?)
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
            " AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = ar.id) OR al.artist_id = ar.id))
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
    let credits: Vec<(&str, bool)> = if t.credits.is_empty() {
        t.artist
            .as_deref()
            .map(|a| (a, false))
            .into_iter()
            .collect()
    } else {
        t.credits.iter().map(|(n, f)| (n.as_str(), *f)).collect()
    };
    let mut credit_ids = Vec::with_capacity(credits.len());
    for (name, featured) in &credits {
        credit_ids.push((upsert_artist(tx, name).await?, *featured));
    }
    let artist_id = credit_ids.first().map(|&(id, _)| id);
    let album_id = match &t.album {
        Some(title) => {
            let album_artist = t
                .album_owner
                .as_deref()
                .or(t.album_artist.as_deref())
                .or(credits.first().map(|&(name, _)| name))
                .unwrap_or(UNKNOWN_ARTIST);
            let album_artist_id = upsert_artist(tx, album_artist).await?;
            Some(upsert_album(tx, title, album_artist_id, t.year).await?)
        }
        None => None,
    };
    let track_id: i64 = sqlx::query_scalar(
        "INSERT INTO tracks (path, size, mtime, duration_ms, title, artist, album, album_artist,
                             track_no, disc_no, year, genre, art_hash, sample_rate, channels,
                             codec, artist_id, album_id, added_at, embedded_art, folder_art,
                             artist_art, rg_track_gain, rg_track_peak, rg_album_gain,
                             rg_album_peak)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (path) DO UPDATE SET
            size = excluded.size, mtime = excluded.mtime, duration_ms = excluded.duration_ms,
            title = excluded.title, artist = excluded.artist, album = excluded.album,
            album_artist = excluded.album_artist, track_no = excluded.track_no,
            disc_no = excluded.disc_no, year = excluded.year, genre = excluded.genre,
            art_hash = excluded.art_hash, sample_rate = excluded.sample_rate,
            channels = excluded.channels, codec = excluded.codec,
            artist_id = excluded.artist_id, album_id = excluded.album_id, removed_at = NULL,
            embedded_art = excluded.embedded_art, folder_art = excluded.folder_art,
            artist_art = excluded.artist_art, rg_track_gain = excluded.rg_track_gain,
            rg_track_peak = excluded.rg_track_peak, rg_album_gain = excluded.rg_album_gain,
            rg_album_peak = excluded.rg_album_peak,
            -- A changed file gets measured again.
            loudness_lufs = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                 THEN loudness_lufs END,
            loudness_peak = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                 THEN loudness_peak END,
            loudness_checked_at = CASE WHEN size = excluded.size AND mtime = excluded.mtime
                                       THEN loudness_checked_at END
         RETURNING id",
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
    .bind(t.replay_gain.track_gain)
    .bind(t.replay_gain.track_peak)
    .bind(t.replay_gain.album_gain)
    .bind(t.replay_gain.album_peak)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM track_artists WHERE track_id = ?")
        .bind(track_id)
        .execute(&mut **tx)
        .await?;
    for (position, (artist, featured)) in credit_ids.iter().enumerate() {
        sqlx::query(
            "INSERT OR IGNORE INTO track_artists (track_id, artist_id, position, featured)
             VALUES (?, ?, ?, ?)",
        )
        .bind(track_id)
        .bind(artist)
        .bind(i64::try_from(position).unwrap_or(i64::MAX))
        .bind(featured)
        .execute(&mut **tx)
        .await?;
    }
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

    #[tokio::test]
    async fn songs_without_art_show_the_album_cover() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[scanned("/m/1.flac", "One", "A", "Record")])
            .await
            .unwrap();
        let album = store.albums_page(0, 1).await.unwrap().items[0].id;
        store
            .save_album_info(album, Some("onlinecover"), None, None, true)
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        let track = &store.search("one", 5).await.unwrap().tracks[0];
        assert_eq!(track.art_hash.as_deref(), Some("onlinecover"));
    }

    #[tokio::test]
    async fn collaborations_count_for_every_artist() {
        let store = Store::open_in_memory().await.unwrap();
        let mut collab = scanned(
            "/m/v.flac",
            "Volcano",
            "Zara Larsson & David Guetta",
            "Volcano",
        );
        collab.credits = vec![
            ("Zara Larsson".into(), false),
            ("David Guetta".into(), false),
            ("KREAM".into(), true),
        ];
        collab.album_owner = Some("Zara Larsson".into());
        let solo = scanned(
            "/m/t.flac",
            "Titanium",
            "David Guetta",
            "Nothing but the Beat",
        );
        store.save_scanned(&[collab, solo]).await.unwrap();

        let artists = store.artists_page(0, 10).await.unwrap();
        let names: Vec<_> = artists
            .items
            .iter()
            .map(|a| (a.name.as_str(), a.track_count))
            .collect();
        assert_eq!(
            names,
            [("David Guetta", 2), ("KREAM", 1), ("Zara Larsson", 1)],
            "no combined artist"
        );

        let track = &store.search("volcano", 5).await.unwrap().tracks[0];
        let credits: Vec<_> = track
            .credits
            .iter()
            .map(|c| (c.name.as_str(), c.featured))
            .collect();
        assert_eq!(
            credits,
            [
                ("Zara Larsson", false),
                ("David Guetta", false),
                ("KREAM", true)
            ]
        );
        assert_eq!(
            track.artist.as_deref(),
            Some("Zara Larsson & David Guetta"),
            "display text kept"
        );

        // The single belongs to Zara and appears on David's page.
        let guetta = artists
            .items
            .iter()
            .find(|a| a.name == "David Guetta")
            .unwrap()
            .id;
        let detail = store.artist(guetta).await.unwrap().unwrap();
        assert_eq!(detail.albums.len(), 1, "his own album");
        assert_eq!(detail.appears_on.len(), 1, "Zara's single");
        assert_eq!(detail.top_tracks.len(), 2);
    }
}
