//! Playlists: named collections of songs, albums, and artists. Albums and
//! artists are expanded when the playlist is played, so music added to the
//! library later joins automatically. An imported playlist can also name
//! songs the library doesn't have ([`EntryKind::Wanted`]). They keep their
//! place in the list, flagged, and never play.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::library::track_columns;
use crate::{now_ms, Result, Store, StoreError, Track};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum EntryKind {
    Track,
    Album,
    Artist,
    /// A song the library doesn't have, from `playlist_wanted`.
    Wanted,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    /// Shuffle when played; otherwise play in order.
    pub shuffle: bool,
    /// Songs, albums, and artists in it.
    pub entry_count: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct PlaylistEntry {
    pub id: i64,
    pub kind: EntryKind,
    pub ref_id: i64,
    /// Song title, album title, or artist name; `None` if it was deleted.
    pub name: Option<String>,
    /// The artist, for songs and albums.
    pub subtitle: Option<String>,
    pub art_hash: Option<String>,
    pub position: i64,
    /// How many of its songs can't play: gone, broken, or never in the library.
    pub problem_songs: i64,
}

/// A song a playlist covers that can't play right now.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct UnplayableSong {
    pub track_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// The file is gone.
    pub removed: bool,
    /// Why the file wouldn't play, if it's there.
    pub error: Option<String>,
}

/// A song named by an import, as far as the source could say.
#[derive(Clone, Debug, Default, PartialEq, Eq, FromRow)]
pub struct WantedSong {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub path: Option<String>,
    /// "A - B" text: could be artist then title, or the other way round.
    pub either_way: bool,
}

/// One line of an import, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportRow {
    Track(i64),
    Wanted(WantedSong),
}

/// A playlist entry that a song in the library might fill.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct UnfilledEntry {
    pub entry_id: i64,
    #[sqlx(flatten)]
    pub song: WantedSong,
}

/// What matching an import needs to know about a song in the library.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct SongFacts {
    pub id: i64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub duration_ms: Option<i64>,
}

/// A file to test by playing it through.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct SongFile {
    pub id: i64,
    pub path: String,
    pub duration_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaylistDetail {
    #[serde(flatten)]
    pub playlist: PlaylistSummary,
    pub entries: Vec<PlaylistEntry>,
    /// Every song the playlist currently covers, in play order.
    pub tracks: Vec<Track>,
    /// Songs it covers that can't play, in list order. Songs the library
    /// never had are entries of kind `wanted` instead.
    pub unplayable: Vec<UnplayableSong>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddEntryError {
    #[error("not found")]
    NotFound,
    #[error("already in this playlist")]
    AlreadyIn,
    #[error(transparent)]
    Store(#[from] StoreError),
}

macro_rules! playlist_summary {
    () => {
        "SELECT p.id, p.name, p.shuffle, p.created_at, p.updated_at, \
                (SELECT count(*) FROM playlist_entries e WHERE e.playlist_id = p.id) AS entry_count \
         FROM playlists p"
    };
}

/// A song (`t`) that can't play: its file is gone, or it wouldn't play.
macro_rules! unplayable {
    () => {
        "(t.removed_at IS NOT NULL OR EXISTS (SELECT 1 FROM track_problems tp WHERE tp.track_id = t.id))"
    };
}

/// A song (`t`) that can play.
macro_rules! playable {
    () => {
        "(t.removed_at IS NULL AND NOT EXISTS (SELECT 1 FROM track_problems tp WHERE tp.track_id = t.id))"
    };
}

macro_rules! entry_select {
    () => {
        concat!(
            "SELECT e.id, e.kind, e.ref_id, e.position,
                CASE e.kind
                  WHEN 'track' THEN (SELECT title FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT title FROM albums WHERE id = e.ref_id)
                  WHEN 'artist' THEN (SELECT name FROM artists WHERE id = e.ref_id)
                  WHEN 'wanted' THEN (SELECT title FROM playlist_wanted WHERE id = e.ref_id)
                END AS name,
                CASE e.kind
                  WHEN 'track' THEN (SELECT artist FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT ar.name FROM albums al JOIN artists ar
                                     ON ar.id = al.artist_id WHERE al.id = e.ref_id)
                  WHEN 'wanted' THEN (SELECT artist FROM playlist_wanted WHERE id = e.ref_id)
                END AS subtitle,
                CASE e.kind
                  WHEN 'track' THEN (SELECT art_hash FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT art_hash FROM albums WHERE id = e.ref_id)
                  WHEN 'artist' THEN (SELECT art_hash FROM artists WHERE id = e.ref_id)
                END AS art_hash,
                CASE e.kind
                  WHEN 'wanted' THEN 1
                  WHEN 'track' THEN (SELECT count(*) FROM tracks t
                                     WHERE t.id = e.ref_id AND ",
            unplayable!(),
            ")
                  WHEN 'album' THEN (SELECT count(*) FROM tracks t
                                     WHERE t.album_id = e.ref_id AND ",
            unplayable!(),
            ")
                  WHEN 'artist' THEN (SELECT count(*) FROM tracks t
                                      WHERE ",
            unplayable!(),
            " AND t.id IN (
                                        SELECT track_id FROM track_artists WHERE artist_id = e.ref_id
                                        UNION SELECT id FROM tracks WHERE album_id IN
                                          (SELECT id FROM albums WHERE artist_id = e.ref_id)))
                END AS problem_songs
         FROM playlist_entries e"
        )
    };
}

/// Which tracks an entry covers (`e.kind`, `e.ref_id`): a track, an
/// album's tracks, or an artist's tracks and albums.
macro_rules! entry_covers {
    () => {
        concat!(
            playable!(),
            " AND (
              (e.kind = 'track' AND t.id = e.ref_id)
           OR (e.kind = 'album' AND t.album_id = e.ref_id)
           OR (e.kind = 'artist' AND (EXISTS (SELECT 1 FROM track_artists ta WHERE ta.track_id = t.id AND ta.artist_id = e.ref_id)
               OR t.album_id IN (SELECT id FROM albums WHERE artist_id = e.ref_id))))"
        )
    };
}

/// Play order within an entry: albums by year, then disc and track.
macro_rules! entry_order {
    () => {
        "al.year IS NULL, al.year, t.album COLLATE NOCASE, t.disc_no, t.track_no,
         t.title COLLATE NOCASE"
    };
}

/// Every song a playlist's entries (`e`, narrowed by `$entries`) name, as
/// `c.playlist_id`, `c.position` and `c.track_id`, playable or not. One
/// branch per kind so each can use its index: an imported playlist can have
/// thousands of entries, and this runs whenever the next song is picked.
macro_rules! covered {
    ($entries:expr) => {
        concat!(
            "(SELECT e.playlist_id, e.position, e.ref_id AS track_id
               FROM playlist_entries e WHERE e.kind = 'track' AND ",
            $entries,
            " UNION ALL
             SELECT e.playlist_id, e.position, t2.id
               FROM playlist_entries e JOIN tracks t2 ON t2.album_id = e.ref_id
              WHERE e.kind = 'album' AND ",
            $entries,
            " UNION ALL
             SELECT e.playlist_id, e.position, ta.track_id
               FROM playlist_entries e JOIN track_artists ta ON ta.artist_id = e.ref_id
              WHERE e.kind = 'artist' AND ",
            $entries,
            " UNION ALL
             SELECT e.playlist_id, e.position, t2.id
               FROM playlist_entries e JOIN albums a2 ON a2.artist_id = e.ref_id
               JOIN tracks t2 ON t2.album_id = a2.id
              WHERE e.kind = 'artist' AND ",
            $entries,
            ") c"
        )
    };
}

/// Songs covered by one playlist's entries (`?1`) that pass `$filter`, in
/// play order: entries in their order, albums by disc and track, artists by
/// album year.
macro_rules! expanded_tracks {
    ($columns:expr, $filter:expr) => {
        concat!(
            "SELECT ",
            $columns,
            " FROM ",
            covered!("e.playlist_id = ?1"),
            " JOIN tracks t ON t.id = c.track_id
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE ",
            $filter,
            " ORDER BY c.position, ",
            entry_order!()
        )
    };
}

impl Store {
    pub async fn playlists(&self) -> Result<Vec<PlaylistSummary>> {
        let playlists = sqlx::query_as(concat!(
            playlist_summary!(),
            " ORDER BY p.name COLLATE NOCASE"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(playlists)
    }

    pub async fn create_playlist(&self, owner_id: i64, name: &str) -> Result<PlaylistSummary> {
        let now = now_ms();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO playlists (owner_id, name, created_at, updated_at) VALUES (?, ?, ?, ?)
             RETURNING id",
        )
        .bind(owner_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        let playlist = sqlx::query_as(concat!(playlist_summary!(), " WHERE p.id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(playlist)
    }

    /// Renames and/or changes shuffle. Returns false if there's no such playlist.
    pub async fn update_playlist(
        &self,
        id: i64,
        name: Option<&str>,
        shuffle: Option<bool>,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE playlists SET name = coalesce(?, name), shuffle = coalesce(?, shuffle),
                                  updated_at = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(shuffle)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_playlist(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM playlists WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn playlist(&self, id: i64) -> Result<Option<PlaylistDetail>> {
        let playlist: Option<PlaylistSummary> =
            sqlx::query_as(concat!(playlist_summary!(), " WHERE p.id = ?"))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(playlist) = playlist else {
            return Ok(None);
        };
        let entries = sqlx::query_as(concat!(
            entry_select!(),
            " WHERE e.playlist_id = ? ORDER BY e.position"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let tracks: Vec<Track> = sqlx::query_as(expanded_tracks!(track_columns!(), playable!()))
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
        let unplayable: Vec<UnplayableSong> = sqlx::query_as(expanded_tracks!(
            "t.id AS track_id, t.title, t.artist, t.album, t.removed_at IS NOT NULL AS removed,
             (SELECT error FROM track_problems tp WHERE tp.track_id = t.id) AS error",
            unplayable!()
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(PlaylistDetail {
            playlist,
            entries,
            tracks: dedup_by(tracks, |t| t.id),
            unplayable: dedup_by(unplayable, |t| t.track_id),
        }))
    }

    /// Song ids the playlist covers, in play order, each once.
    pub async fn playlist_track_ids(&self, id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(expanded_tracks!("t.id", playable!()))
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
        Ok(dedup_by(ids, |&id| id))
    }

    /// Every song the playlist covers in play order, playable or not, so a
    /// song that stopped working still marks a place in the list.
    pub async fn playlist_order_ids(&self, id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(expanded_tracks!("t.id", "1 = 1"))
            .bind(id)
            .fetch_all(&self.pool)
            .await?;
        Ok(dedup_by(ids, |&id| id))
    }

    /// How many songs can't play, per playlist that has any.
    pub async fn playlist_problem_counts(&self) -> Result<Vec<(i64, i64)>> {
        let counts = sqlx::query_as(concat!(
            "SELECT playlist_id, count(*) FROM (
               SELECT DISTINCT c.playlist_id, t.id FROM ",
            covered!("1 = 1"),
            " JOIN tracks t ON t.id = c.track_id WHERE ",
            unplayable!(),
            " UNION ALL
               SELECT playlist_id, -id FROM playlist_entries WHERE kind = 'wanted'
             ) GROUP BY playlist_id"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(counts)
    }

    /// Makes a playlist from an import, in one step. A song listed twice
    /// keeps its first place.
    pub async fn import_playlist(
        &self,
        owner_id: i64,
        name: &str,
        rows: &[ImportRow],
    ) -> Result<PlaylistSummary> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO playlists (owner_id, name, created_at, updated_at) VALUES (?, ?, ?, ?)
             RETURNING id",
        )
        .bind(owner_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        for (position, row) in rows.iter().enumerate() {
            let (kind, ref_id) = match row {
                ImportRow::Track(track) => (EntryKind::Track, *track),
                ImportRow::Wanted(song) => {
                    let wanted: i64 = sqlx::query_scalar(
                        "INSERT INTO playlist_wanted
                           (playlist_id, title, artist, album, duration_ms, path, either_way)
                         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
                    )
                    .bind(id)
                    .bind(&song.title)
                    .bind(&song.artist)
                    .bind(&song.album)
                    .bind(song.duration_ms)
                    .bind(&song.path)
                    .bind(song.either_way)
                    .fetch_one(&mut *tx)
                    .await?;
                    (EntryKind::Wanted, wanted)
                }
            };
            sqlx::query(
                "INSERT OR IGNORE INTO playlist_entries
                   (playlist_id, kind, ref_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(kind)
            .bind(ref_id)
            .bind(i64::try_from(position).unwrap_or(i64::MAX) + 1)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        let playlist = sqlx::query_as(concat!(playlist_summary!(), " WHERE p.id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(playlist)
    }

    /// Entries a song in the library might fill: songs an import named, and
    /// songs whose file is gone (it may have moved, which makes a new song).
    pub async fn unfilled_entries(&self) -> Result<Vec<UnfilledEntry>> {
        let entries = sqlx::query_as(
            "SELECT e.id AS entry_id, w.title, w.artist, w.album, w.duration_ms, w.path, w.either_way
             FROM playlist_entries e JOIN playlist_wanted w ON w.id = e.ref_id
             WHERE e.kind = 'wanted'
             UNION ALL
             SELECT e.id, t.title, t.artist, t.album, t.duration_ms, NULL, 0
             FROM playlist_entries e JOIN tracks t ON t.id = e.ref_id
             WHERE e.kind = 'track' AND t.removed_at IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
    }

    /// Points an entry at a song in the library. If the playlist already
    /// has that song, the entry goes instead.
    pub async fn fill_playlist_entry(&self, entry_id: i64, track_id: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM playlist_wanted WHERE id =
               (SELECT ref_id FROM playlist_entries WHERE id = ? AND kind = 'wanted')",
        )
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
        let done = sqlx::query(
            "UPDATE OR IGNORE playlist_entries SET kind = 'track', ref_id = ? WHERE id = ?",
        )
        .bind(track_id)
        .bind(entry_id)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            sqlx::query("DELETE FROM playlist_entries WHERE id = ?")
                .bind(entry_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Every song in the library that's still on disk, for matching.
    pub async fn song_facts(&self) -> Result<Vec<SongFacts>> {
        let songs = sqlx::query_as(
            "SELECT id, path, title, artist, album_artist, duration_ms FROM tracks
             WHERE removed_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(songs)
    }

    /// A song's file, whether or not it's playable.
    pub async fn song_file(&self, id: i64) -> Result<Option<SongFile>> {
        let file = sqlx::query_as("SELECT id, path, duration_ms FROM tracks WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(file)
    }

    /// The files a playlist covers that should still be on disk.
    pub async fn playlist_files(&self, id: i64) -> Result<Vec<SongFile>> {
        let files: Vec<SongFile> = sqlx::query_as(expanded_tracks!(
            "t.id, t.path, t.duration_ms",
            "t.removed_at IS NULL"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(dedup_by(files, |f| f.id))
    }

    /// Files marked as not playing, to try again.
    pub async fn problem_files(&self) -> Result<Vec<SongFile>> {
        let files = sqlx::query_as(
            "SELECT t.id, t.path, t.duration_ms FROM track_problems tp
             JOIN tracks t ON t.id = tp.track_id WHERE t.removed_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(files)
    }

    /// Marks a file as not playing. `size` and `mtime` are the file's now:
    /// a scan that finds them changed takes the mark off.
    pub async fn mark_track_problem(
        &self,
        id: i64,
        error: &str,
        size: Option<i64>,
        mtime: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO track_problems (track_id, error, size, mtime, noticed_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(error)
        .bind(size)
        .bind(mtime)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// True if the file had a mark to take off.
    pub async fn clear_track_problem(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM track_problems WHERE track_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Song ids a song, album, or artist covers, in play order, each once.
    pub async fn tracks_for(&self, kind: EntryKind, ref_id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(concat!(
            "SELECT t.id FROM (SELECT ? AS kind, ? AS ref_id) e JOIN tracks t ON ",
            entry_covers!(),
            " LEFT JOIN albums al ON al.id = t.album_id ORDER BY ",
            entry_order!()
        ))
        .bind(kind)
        .bind(ref_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(dedup_by(ids, |&id| id))
    }

    pub async fn add_playlist_entry(
        &self,
        playlist_id: i64,
        kind: EntryKind,
        ref_id: i64,
    ) -> Result<PlaylistEntry, AddEntryError> {
        let exists: bool = sqlx::query_scalar(match kind {
            EntryKind::Track => "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?)",
            EntryKind::Album => "SELECT EXISTS (SELECT 1 FROM albums WHERE id = ?)",
            EntryKind::Artist => "SELECT EXISTS (SELECT 1 FROM artists WHERE id = ?)",
            // Only an import makes these.
            EntryKind::Wanted => return Err(AddEntryError::NotFound),
        })
        .bind(ref_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)?;
        let playlist_exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM playlists WHERE id = ?)")
                .bind(playlist_id)
                .fetch_one(&self.pool)
                .await
                .map_err(StoreError::from)?;
        if !exists || !playlist_exists {
            return Err(AddEntryError::NotFound);
        }
        let now = now_ms();
        let inserted = sqlx::query_scalar(
            "INSERT INTO playlist_entries (playlist_id, kind, ref_id, position, added_at)
             VALUES (?, ?, ?, (SELECT coalesce(max(position), 0) + 1 FROM playlist_entries
                               WHERE playlist_id = ?), ?)
             RETURNING id",
        )
        .bind(playlist_id)
        .bind(kind)
        .bind(ref_id)
        .bind(playlist_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await;
        let id: i64 = match inserted {
            Ok(id) => id,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(AddEntryError::AlreadyIn)
            }
            Err(e) => return Err(StoreError::from(e).into()),
        };
        self.touch_playlist(playlist_id).await?;
        let entry = sqlx::query_as(concat!(entry_select!(), " WHERE e.id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)?;
        Ok(entry)
    }

    pub async fn remove_playlist_entry(&self, playlist_id: i64, entry_id: i64) -> Result<bool> {
        sqlx::query(
            "DELETE FROM playlist_wanted WHERE id =
               (SELECT ref_id FROM playlist_entries
                WHERE id = ? AND playlist_id = ? AND kind = 'wanted')",
        )
        .bind(entry_id)
        .bind(playlist_id)
        .execute(&self.pool)
        .await?;
        let result = sqlx::query("DELETE FROM playlist_entries WHERE id = ? AND playlist_id = ?")
            .bind(entry_id)
            .bind(playlist_id)
            .execute(&self.pool)
            .await?;
        self.touch_playlist(playlist_id).await?;
        Ok(result.rows_affected() > 0)
    }

    /// Moves an entry to `index` (0-based, clamped) within its playlist.
    pub async fn move_playlist_entry(
        &self,
        playlist_id: i64,
        entry_id: i64,
        index: usize,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let mut ids: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM playlist_entries WHERE playlist_id = ? ORDER BY position",
        )
        .bind(playlist_id)
        .fetch_all(&mut *tx)
        .await?;
        let Some(from) = ids.iter().position(|&i| i == entry_id) else {
            return Ok(false);
        };
        ids.remove(from);
        ids.insert(index.min(ids.len()), entry_id);
        for (position, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE playlist_entries SET position = ? WHERE id = ?")
                .bind(i64::try_from(position).unwrap_or(i64::MAX) + 1)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        self.touch_playlist(playlist_id).await?;
        Ok(true)
    }

    async fn touch_playlist(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn dedup_by<T, K: Eq + std::hash::Hash>(items: Vec<T>, key: impl Fn(&T) -> K) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(key(item)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    async fn setup() -> (Store, i64) {
        let store = Store::open_in_memory().await.unwrap();
        let owner = store
            .create_user("admin", "h", Role::Admin)
            .await
            .unwrap()
            .id;
        let track = |path: &str, artist: &str, album: &str, no: i64| ScannedTrack {
            path: path.into(),
            title: path.trim_start_matches("/m/").into(),
            artist: Some(artist.into()),
            album: Some(album.into()),
            track_no: Some(no),
            ..Default::default()
        };
        store
            .save_scanned(&[
                track("/m/a2", "A", "Alpha", 2),
                track("/m/a1", "A", "Alpha", 1),
                track("/m/b1", "B", "Beta", 1),
                track("/m/c1", "C", "Gamma", 1),
            ])
            .await
            .unwrap();
        (store, owner)
    }

    async fn id_of(store: &Store, sql: &'static str, name: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(name)
            .fetch_one(store.pool())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn entries_expand_in_order_and_stay_live() {
        let (store, owner) = setup().await;
        let list = store.create_playlist(owner, "Chill").await.unwrap();
        assert!(list.shuffle, "shuffle by default");
        let c1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/c1").await;
        let alpha = id_of(&store, "SELECT id FROM albums WHERE title = ?", "Alpha").await;
        let b = id_of(&store, "SELECT id FROM artists WHERE name = ?", "B").await;

        store
            .add_playlist_entry(list.id, EntryKind::Track, c1)
            .await
            .unwrap();
        store
            .add_playlist_entry(list.id, EntryKind::Album, alpha)
            .await
            .unwrap();
        let artist = store
            .add_playlist_entry(list.id, EntryKind::Artist, b)
            .await
            .unwrap();
        assert_eq!(artist.name.as_deref(), Some("B"));
        assert!(matches!(
            store
                .add_playlist_entry(list.id, EntryKind::Album, alpha)
                .await,
            Err(AddEntryError::AlreadyIn)
        ));

        let titles = |d: PlaylistDetail| d.tracks.into_iter().map(|t| t.title).collect::<Vec<_>>();
        let detail = store.playlist(list.id).await.unwrap().unwrap();
        assert_eq!(detail.entries.len(), 3);
        assert_eq!(titles(detail), ["c1", "a1", "a2", "b1"]);

        // New music by a listed artist joins; duplicates appear once.
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/b2".into(),
                title: "b2".into(),
                artist: Some("B".into()),
                album: Some("Beta".into()),
                track_no: Some(2),
                ..Default::default()
            }])
            .await
            .unwrap();
        let a1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a1").await;
        store
            .add_playlist_entry(list.id, EntryKind::Track, a1)
            .await
            .unwrap();
        let ids = store.playlist_track_ids(list.id).await.unwrap();
        assert_eq!(ids.len(), 5);

        // Reorder: artist B first.
        store
            .move_playlist_entry(list.id, artist.id, 0)
            .await
            .unwrap();
        let detail = store.playlist(list.id).await.unwrap().unwrap();
        assert_eq!(titles(detail)[..2], ["b1", "b2"]);

        assert!(store
            .update_playlist(list.id, Some("Dance"), Some(false))
            .await
            .unwrap());
        let summary = &store.playlists().await.unwrap()[0];
        assert_eq!(
            (summary.name.as_str(), summary.shuffle, summary.entry_count),
            ("Dance", false, 4)
        );

        assert!(store
            .remove_playlist_entry(list.id, artist.id)
            .await
            .unwrap());
        assert!(store.delete_playlist(list.id).await.unwrap());
        assert!(store.playlists().await.unwrap().is_empty());
    }

    fn wanted(title: &str) -> ImportRow {
        ImportRow::Wanted(WantedSong {
            title: title.into(),
            artist: Some("Nobody".into()),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn imports_keep_missing_songs_in_place_and_never_play_them() {
        let (store, owner) = setup().await;
        let a1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a1").await;
        let b1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/b1").await;
        let list = store
            .import_playlist(
                owner,
                "Imported",
                &[
                    ImportRow::Track(a1),
                    wanted("Not here"),
                    ImportRow::Track(a1),
                    ImportRow::Track(b1),
                ],
            )
            .await
            .unwrap();
        assert_eq!(list.entry_count, 3, "the repeat keeps its first place");

        let detail = store.playlist(list.id).await.unwrap().unwrap();
        let kinds: Vec<_> = detail.entries.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [EntryKind::Track, EntryKind::Wanted, EntryKind::Track]
        );
        let missing = &detail.entries[1];
        assert_eq!(missing.name.as_deref(), Some("Not here"));
        assert_eq!(missing.subtitle.as_deref(), Some("Nobody"));
        assert_eq!(missing.problem_songs, 1);
        assert_eq!(store.playlist_track_ids(list.id).await.unwrap(), [a1, b1]);
        assert_eq!(
            store.playlist_problem_counts().await.unwrap(),
            [(list.id, 1)]
        );

        // Filling it puts the song where the gap was.
        let c1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/c1").await;
        let unfilled = store.unfilled_entries().await.unwrap();
        assert_eq!(unfilled.len(), 1);
        assert_eq!(unfilled[0].song.title, "Not here");
        store
            .fill_playlist_entry(unfilled[0].entry_id, c1)
            .await
            .unwrap();
        assert_eq!(
            store.playlist_track_ids(list.id).await.unwrap(),
            [a1, c1, b1]
        );
        assert!(store.playlist_problem_counts().await.unwrap().is_empty());
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM playlist_wanted")
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(left, 0);
    }

    #[tokio::test]
    async fn filling_with_a_song_already_listed_drops_the_gap() {
        let (store, owner) = setup().await;
        let a1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a1").await;
        let list = store
            .import_playlist(owner, "Dupes", &[ImportRow::Track(a1), wanted("Same song")])
            .await
            .unwrap();
        let entry = store.unfilled_entries().await.unwrap()[0].entry_id;
        store.fill_playlist_entry(entry, a1).await.unwrap();
        let detail = store.playlist(list.id).await.unwrap().unwrap();
        assert_eq!(detail.entries.len(), 1);
    }

    #[tokio::test]
    async fn gone_and_broken_files_are_flagged_and_skipped() {
        let (store, owner) = setup().await;
        let list = store.create_playlist(owner, "Mix").await.unwrap();
        let c1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/c1").await;
        let b1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/b1").await;
        let a1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a1").await;
        let a2 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a2").await;
        let alpha = id_of(&store, "SELECT id FROM albums WHERE title = ?", "Alpha").await;
        let b = id_of(&store, "SELECT id FROM artists WHERE name = ?", "B").await;
        for (kind, id) in [
            (EntryKind::Track, c1),
            (EntryKind::Album, alpha),
            (EntryKind::Artist, b),
        ] {
            store.add_playlist_entry(list.id, kind, id).await.unwrap();
        }

        store.mark_removed(&[c1]).await.unwrap();
        for id in [a1, b1] {
            store
                .mark_track_problem(id, "corrupt", Some(10), Some(20))
                .await
                .unwrap();
        }

        assert_eq!(store.playlist_track_ids(list.id).await.unwrap(), [a2]);
        assert_eq!(
            store.playlist_order_ids(list.id).await.unwrap(),
            [c1, a1, a2, b1]
        );
        let detail = store.playlist(list.id).await.unwrap().unwrap();
        let problems: Vec<_> = detail.entries.iter().map(|e| e.problem_songs).collect();
        assert_eq!(
            problems,
            [1, 1, 1],
            "the song, one of the album, the artist's"
        );
        assert_eq!(detail.unplayable.len(), 3);
        assert!(detail.unplayable[0].removed);
        assert_eq!(detail.unplayable[1].error.as_deref(), Some("corrupt"));
        assert_eq!(
            store.playlist_problem_counts().await.unwrap(),
            [(list.id, 3)]
        );

        // The same file scanned again stays marked; a changed one doesn't.
        let scanned = |size: i64| ScannedTrack {
            path: "/m/a1".into(),
            title: "a1".into(),
            artist: Some("A".into()),
            album: Some("Alpha".into()),
            track_no: Some(1),
            size,
            mtime: 20,
            ..Default::default()
        };
        store.save_scanned(&[scanned(10)]).await.unwrap();
        assert_eq!(store.playlist_track_ids(list.id).await.unwrap(), [a2]);
        store.save_scanned(&[scanned(11)]).await.unwrap();
        assert_eq!(store.playlist_track_ids(list.id).await.unwrap(), [a1, a2]);
        assert_eq!(store.problem_files().await.unwrap().len(), 1);
        assert!(store.clear_track_problem(b1).await.unwrap());
        assert!(!store.clear_track_problem(b1).await.unwrap());
    }

    #[tokio::test]
    async fn a_new_file_for_a_gone_song_can_fill_its_entry() {
        let (store, owner) = setup().await;
        let list = store.create_playlist(owner, "Mix").await.unwrap();
        let c1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/c1").await;
        store
            .add_playlist_entry(list.id, EntryKind::Track, c1)
            .await
            .unwrap();
        assert!(store.unfilled_entries().await.unwrap().is_empty());
        store.mark_removed(&[c1]).await.unwrap();
        let unfilled = store.unfilled_entries().await.unwrap();
        assert_eq!(unfilled[0].song.title, "c1");
        assert_eq!(unfilled[0].song.artist.as_deref(), Some("C"));
    }
}
