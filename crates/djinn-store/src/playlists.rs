//! Playlists: a person's named collections of songs, albums, and artists,
//! used the same way from the party, GO, and phone apps. Each is private to
//! its owner unless shared with everyone or with particular people. Albums and
//! artists are expanded when the playlist is played, so music added to the
//! library later joins automatically. An imported playlist can also name
//! songs the library doesn't have ([`EntryKind::Wanted`]). They keep their
//! place in the list, flagged, and never play.

use std::collections::{HashMap, HashSet};

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
    pub owner_id: i64,
    /// Who made it, for shared ones.
    pub owner: String,
    pub name: String,
    /// Shuffle when played; otherwise play in order.
    pub shuffle: bool,
    /// Everyone can see and play it.
    pub shared: bool,
    /// Songs, albums, and artists in it.
    pub entry_count: i64,
    /// Songs it covers that can play.
    pub song_count: i64,
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

/// Someone a playlist can be shared with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct ShareTarget {
    pub id: i64,
    pub name: String,
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

/// Why a line of a playlist can't play.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowProblem {
    /// Named by an import, and the library doesn't have it.
    NotInLibrary,
    /// The file is gone.
    FileMissing,
    /// The file is there but wouldn't play.
    FileBroken,
}

/// One line of a playlist as people see it, in order: songs that play and
/// songs that don't, each where it belongs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlaylistRow {
    pub entry_id: i64,
    /// Its entry is this one song, so taking it out is taking the entry out.
    /// A song of an album or artist goes with the whole entry.
    pub removable: bool,
    /// Where it is in `tracks`, if it can play.
    pub song_index: Option<usize>,
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: Option<String>,
    pub problem: Option<RowProblem>,
    pub error: Option<String>,
}

/// A song a playlist covers, with which entry brought it in.
#[derive(FromRow)]
struct CoveredSong {
    position: i64,
    id: i64,
    title: String,
    artist: Option<String>,
    removed: bool,
    error: Option<String>,
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
    /// Everything in it, in order.
    pub rows: Vec<PlaylistRow>,
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

macro_rules! playlist_summary {
    () => {
        concat!(
            "SELECT p.id, p.owner_id, coalesce(u.display_name, u.username) AS owner, p.name,
                    p.shuffle, p.shared, p.created_at, p.updated_at,
                    (SELECT count(*) FROM playlist_entries e WHERE e.playlist_id = p.id) AS entry_count,
                    (SELECT count(DISTINCT t.id) FROM ",
            covered!("e.playlist_id = p.id"),
            " JOIN tracks t ON t.id = c.track_id WHERE ",
            playable!(),
            ") AS song_count
             FROM playlists p JOIN users u ON u.id = p.owner_id"
        )
    };
}

/// A playlist (`p`) that the person `?1` may see and play.
macro_rules! visible_to {
    () => {
        "(p.owner_id = ?1 OR p.shared = 1 OR EXISTS
           (SELECT 1 FROM playlist_shares s WHERE s.playlist_id = p.id AND s.user_id = ?1))"
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

    /// Someone's own playlists, then those shared with them, by name.
    pub async fn playlists_for(&self, user_id: i64) -> Result<Vec<PlaylistSummary>> {
        let playlists = sqlx::query_as(concat!(
            playlist_summary!(),
            " WHERE ",
            visible_to!(),
            " ORDER BY p.owner_id != ?1, p.name COLLATE NOCASE"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(playlists)
    }

    /// One playlist's summary, whoever may see it.
    pub async fn playlist_summary(&self, id: i64) -> Result<Option<PlaylistSummary>> {
        let playlist = sqlx::query_as(concat!(playlist_summary!(), " WHERE p.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(playlist)
    }

    /// The playlist if `user_id` may see it: their own or one shared with them.
    pub async fn playlist_for(&self, user_id: i64, id: i64) -> Result<Option<PlaylistDetail>> {
        let visible: bool = sqlx::query_scalar(concat!(
            "SELECT EXISTS (SELECT 1 FROM playlists p WHERE p.id = ?2 AND ",
            visible_to!(),
            ")"
        ))
        .bind(user_id)
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        if !visible {
            return Ok(None);
        }
        self.playlist(id).await
    }

    /// Whose playlist it is.
    pub async fn playlist_owner(&self, id: i64) -> Result<Option<i64>> {
        let owner = sqlx::query_scalar("SELECT owner_id FROM playlists WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(owner)
    }

    /// Shares it with everyone, or stops. False if there's no such playlist.
    pub async fn set_playlist_shared(&self, id: i64, shared: bool) -> Result<bool> {
        let done = sqlx::query("UPDATE playlists SET shared = ?, updated_at = ? WHERE id = ?")
            .bind(shared)
            .bind(now_ms())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The particular people a playlist is shared with.
    pub async fn playlist_shares(&self, id: i64) -> Result<Vec<ShareTarget>> {
        let people = sqlx::query_as(
            "SELECT u.id, coalesce(u.display_name, u.username) AS name
             FROM playlist_shares s JOIN users u ON u.id = s.user_id
             WHERE s.playlist_id = ? ORDER BY name COLLATE NOCASE",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(people)
    }

    /// Everyone a playlist of `owner_id`'s could be shared with: accounts
    /// that work, other than theirs. Guests can't have playlists.
    pub async fn share_targets(&self, owner_id: i64) -> Result<Vec<ShareTarget>> {
        let people = sqlx::query_as(
            "SELECT id, coalesce(display_name, username) AS name FROM users
             WHERE id != ? AND role != 'guest' AND status = 'active'
             ORDER BY name COLLATE NOCASE",
        )
        .bind(owner_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(people)
    }

    /// False if `user_id` isn't someone it can be shared with.
    pub async fn share_playlist(&self, id: i64, user_id: i64) -> Result<bool> {
        let done = sqlx::query(
            "INSERT OR IGNORE INTO playlist_shares (playlist_id, user_id)
             SELECT p.id, u.id FROM playlists p, users u
             WHERE p.id = ? AND u.id = ? AND u.id != p.owner_id
               AND u.role != 'guest' AND u.status = 'active'",
        )
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() > 0 {
            return Ok(true);
        }
        let already = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM playlist_shares WHERE playlist_id = ? AND user_id = ?)",
        )
        .bind(id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(already)
    }

    pub async fn unshare_playlist(&self, id: i64, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM playlist_shares WHERE playlist_id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Adds songs to the end, leaving out any already named on their own.
    pub async fn add_playlist_songs(&self, id: i64, track_ids: &[i64]) -> Result<()> {
        let now = now_ms();
        let mut tx = self.write().await?;
        for track in track_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO playlist_entries (playlist_id, kind, ref_id, position, added_at)
                 SELECT ?, 'track', t.id,
                        (SELECT coalesce(max(position), 0) + 1 FROM playlist_entries
                         WHERE playlist_id = ?), ?
                 FROM tracks t WHERE t.id = ?",
            )
            .bind(id)
            .bind(id)
            .bind(now)
            .bind(track)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Sets the playable songs of a playlist from a flat list, as GO and
    /// phone apps edit one, which is all they know of it. What the list can't
    /// express stays where it is: songs missing from the library, albums and
    /// artists, and songs that can't play right now, which the apps never see.
    /// Songs the list drops go, and new ones join at the end, unless an album
    /// or artist already covers them.
    pub async fn set_playlist_songs(&self, id: i64, track_ids: &[i64]) -> Result<()> {
        let now = now_ms();
        let mut tx = self.write().await?;
        let entries: Vec<(i64, EntryKind, i64)> = sqlx::query_as(
            "SELECT id, kind, ref_id FROM playlist_entries WHERE playlist_id = ? ORDER BY position",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let held: HashSet<i64> = sqlx::query_scalar(concat!(
            "SELECT t.id FROM playlist_entries e JOIN tracks t ON t.id = e.ref_id
             WHERE e.playlist_id = ? AND e.kind = 'track' AND ",
            unplayable!()
        ))
        .bind(id)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let covered: HashSet<i64> = sqlx::query_scalar(concat!(
            "SELECT c.track_id FROM ",
            covered!("e.playlist_id = ?1"),
            " WHERE c.track_id NOT IN (SELECT ref_id FROM playlist_entries
                                       WHERE playlist_id = ?1 AND kind = 'track')"
        ))
        .bind(id)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .collect();
        let mut seen = HashSet::new();
        let mut songs = track_ids
            .iter()
            .copied()
            .filter(|t| !held.contains(t) && !covered.contains(t) && seen.insert(*t));

        // Each editable song's place goes to the next song in the new order.
        let mut order: Vec<Slot> = Vec::with_capacity(entries.len());
        for (entry, kind, ref_id) in &entries {
            if *kind == EntryKind::Track && !held.contains(ref_id) {
                sqlx::query("DELETE FROM playlist_entries WHERE id = ?")
                    .bind(entry)
                    .execute(&mut *tx)
                    .await?;
                if let Some(song) = songs.next() {
                    order.push(Slot::Song(song));
                }
            } else {
                order.push(Slot::Kept(*entry));
            }
        }
        order.extend(songs.map(Slot::Song));
        for (position, slot) in order.iter().enumerate() {
            let position = i64::try_from(position).unwrap_or(i64::MAX) + 1;
            match slot {
                Slot::Kept(entry) => {
                    sqlx::query("UPDATE playlist_entries SET position = ? WHERE id = ?")
                        .bind(position)
                        .bind(entry)
                        .execute(&mut *tx)
                        .await?;
                }
                Slot::Song(track) => {
                    sqlx::query(
                        "INSERT OR IGNORE INTO playlist_entries
                           (playlist_id, kind, ref_id, position, added_at)
                         SELECT ?, 'track', id, ?, ? FROM tracks WHERE id = ?",
                    )
                    .bind(id)
                    .bind(position)
                    .bind(now)
                    .bind(track)
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }
        sqlx::query("UPDATE playlists SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
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
        let covered: Vec<CoveredSong> = sqlx::query_as(expanded_tracks!(
            "c.position, t.id, t.title, t.artist, t.removed_at IS NOT NULL AS removed,
             (SELECT error FROM track_problems tp WHERE tp.track_id = t.id) AS error",
            "1 = 1"
        ))
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
        let tracks = dedup_by(tracks, |t| t.id);
        let rows = playlist_rows(&entries, &tracks, covered);
        Ok(Some(PlaylistDetail {
            playlist,
            rows,
            entries,
            tracks,
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
        let mut tx = self.write().await?;
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
        let mut tx = self.write().await?;
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
        let mut tx = self.write().await?;
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

/// Lays a playlist out line by line. `tracks` is what can play, and each song
/// shows once, where it first appears.
fn playlist_rows(
    entries: &[PlaylistEntry],
    tracks: &[Track],
    covered: Vec<CoveredSong>,
) -> Vec<PlaylistRow> {
    let index: HashMap<i64, usize> = tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let mut by_position: HashMap<i64, Vec<CoveredSong>> = HashMap::new();
    for song in covered {
        by_position.entry(song.position).or_default().push(song);
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(entries.len());
    for entry in entries {
        let one_song = matches!(entry.kind, EntryKind::Track | EntryKind::Wanted);
        let songs = by_position.remove(&entry.position).unwrap_or_default();
        if songs.is_empty() && (entry.kind == EntryKind::Wanted || entry.name.is_none()) {
            let wanted = entry.kind == EntryKind::Wanted;
            rows.push(PlaylistRow {
                entry_id: entry.id,
                removable: true,
                song_index: None,
                track_id: None,
                title: entry.name.clone().unwrap_or_else(|| "Unknown".to_owned()),
                artist: entry.subtitle.clone(),
                problem: Some(if wanted {
                    RowProblem::NotInLibrary
                } else {
                    RowProblem::FileMissing
                }),
                error: None,
            });
        }
        for song in songs {
            if !seen.insert(song.id) {
                continue;
            }
            let song_index = index.get(&song.id).copied();
            let problem = match (song.removed, song_index) {
                (true, _) => Some(RowProblem::FileMissing),
                (false, None) => Some(RowProblem::FileBroken),
                (false, Some(_)) => None,
            };
            rows.push(PlaylistRow {
                entry_id: entry.id,
                removable: one_song,
                song_index,
                track_id: Some(song.id),
                title: song.title,
                artist: song.artist,
                problem,
                error: song.error,
            });
        }
    }
    rows
}

/// A place in a playlist being rearranged.
enum Slot {
    Kept(i64),
    Song(i64),
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
    async fn rows_show_everything_in_its_place() {
        let (store, owner) = setup().await;
        let id = |p: &'static str| {
            let store = store.clone();
            async move { id_of(&store, "SELECT id FROM tracks WHERE path = ?", p).await }
        };
        let (a2, c1) = (id("/m/a2").await, id("/m/c1").await);
        let a1 = id("/m/a1").await;
        let b1 = id("/m/b1").await;
        let list = store
            .import_playlist(
                owner,
                "Mix",
                &[ImportRow::Track(c1), wanted("Gap"), ImportRow::Track(a2)],
            )
            .await
            .unwrap()
            .id;
        let alpha = id_of(&store, "SELECT id FROM albums WHERE title = ?", "Alpha").await;
        store
            .add_playlist_entry(list, EntryKind::Album, alpha)
            .await
            .unwrap();
        store
            .add_playlist_entry(list, EntryKind::Track, b1)
            .await
            .unwrap();
        store.mark_removed(&[c1]).await.unwrap();
        store
            .mark_track_problem(a1, "corrupt", None, None)
            .await
            .unwrap();

        let detail = store.playlist(list).await.unwrap().unwrap();
        let rows: Vec<_> = detail
            .rows
            .iter()
            .map(|r| (r.title.as_str(), r.problem, r.removable, r.song_index))
            .collect();
        assert_eq!(
            rows,
            [
                ("c1", Some(RowProblem::FileMissing), true, None),
                ("Gap", Some(RowProblem::NotInLibrary), true, None),
                ("a2", None, true, Some(0)),
                // a2 already showed; the album's other song can only go with it.
                ("a1", Some(RowProblem::FileBroken), false, None),
                ("b1", None, true, Some(1)),
            ]
        );
        assert_eq!(detail.rows[3].error.as_deref(), Some("corrupt"));
        assert_eq!(detail.rows[4].track_id, Some(b1));
        assert_eq!(detail.tracks[1].id, b1, "song_index points into tracks");
    }

    #[tokio::test]
    async fn playlists_are_private_until_shared() {
        let (store, sam) = setup().await;
        let jo = store.create_user("jo", "h", Role::User).await.unwrap().id;
        let kim = store.create_user("kim", "h", Role::User).await.unwrap().id;
        let guest = store.create_user("g", "h", Role::Guest).await.unwrap().id;
        let list = store.create_playlist(sam, "Mine").await.unwrap().id;
        let names = |lists: Vec<PlaylistSummary>| lists.len();

        assert_eq!(names(store.playlists_for(sam).await.unwrap()), 1);
        assert_eq!(names(store.playlists_for(jo).await.unwrap()), 0);
        assert!(store.playlist_for(jo, list).await.unwrap().is_none());

        // With one person: they see it, nobody else does.
        assert!(store.share_playlist(list, jo).await.unwrap());
        assert!(store.share_playlist(list, jo).await.unwrap(), "already is");
        assert_eq!(
            store
                .playlist_for(jo, list)
                .await
                .unwrap()
                .unwrap()
                .playlist
                .owner,
            "admin"
        );
        assert!(store.playlist_for(kim, list).await.unwrap().is_none());
        let shares = store.playlist_shares(list).await.unwrap();
        assert_eq!((shares.len(), shares[0].id), (1, jo));

        // Never with guests, or its owner.
        assert!(!store.share_playlist(list, guest).await.unwrap());
        assert!(!store.share_playlist(list, sam).await.unwrap());
        let targets = store.share_targets(sam).await.unwrap();
        assert_eq!(targets.iter().map(|t| t.id).collect::<Vec<_>>(), [jo, kim]);

        store.unshare_playlist(list, jo).await.unwrap();
        assert!(store.playlist_for(jo, list).await.unwrap().is_none());

        // With everyone.
        assert!(store.set_playlist_shared(list, true).await.unwrap());
        assert!(store.playlist_for(kim, list).await.unwrap().is_some());
        store.set_playlist_shared(list, false).await.unwrap();
        assert!(store.playlist_for(kim, list).await.unwrap().is_none());
        assert_eq!(store.playlist_owner(list).await.unwrap(), Some(sam));
    }

    #[tokio::test]
    async fn editing_as_a_flat_list_keeps_what_it_cant_express() {
        let (store, owner) = setup().await;
        let id = |p: &'static str| {
            let store = store.clone();
            async move { id_of(&store, "SELECT id FROM tracks WHERE path = ?", p).await }
        };
        let (a1, b1, c1) = (id("/m/a1").await, id("/m/b1").await, id("/m/c1").await);
        let list = store
            .import_playlist(
                owner,
                "Mix",
                &[ImportRow::Track(a1), wanted("Gap"), ImportRow::Track(b1)],
            )
            .await
            .unwrap()
            .id;
        let rows = |d: PlaylistDetail| {
            d.entries
                .into_iter()
                .map(|e| e.name.unwrap_or_default())
                .collect::<Vec<_>>()
        };
        let read = || async { rows(store.playlist(list).await.unwrap().unwrap()) };

        // Reordered by an app that has never heard of the gap.
        store.set_playlist_songs(list, &[b1, a1]).await.unwrap();
        assert_eq!(read().await, ["b1", "Gap", "a1"]);

        // A song removed, another added.
        store.set_playlist_songs(list, &[a1, c1]).await.unwrap();
        assert_eq!(read().await, ["a1", "Gap", "c1"]);

        // A song that can't play isn't in the app's list but isn't dropped.
        store.mark_removed(&[a1]).await.unwrap();
        store.set_playlist_songs(list, &[c1]).await.unwrap();
        assert_eq!(read().await, ["a1", "Gap", "c1"]);
        assert_eq!(store.playlist_track_ids(list).await.unwrap(), [c1]);

        // A song an album covers needs no entry of its own.
        let beta = id_of(&store, "SELECT id FROM albums WHERE title = ?", "Beta").await;
        store
            .add_playlist_entry(list, EntryKind::Album, beta)
            .await
            .unwrap();
        store.set_playlist_songs(list, &[c1, b1, c1]).await.unwrap();
        assert_eq!(read().await, ["a1", "Gap", "c1", "Beta"]);
    }

    #[tokio::test]
    async fn counts_cover_albums_and_leave_out_what_cant_play() {
        let (store, owner) = setup().await;
        let list = store.create_playlist(owner, "Mix").await.unwrap().id;
        let alpha = id_of(&store, "SELECT id FROM albums WHERE title = ?", "Alpha").await;
        store
            .add_playlist_entry(list, EntryKind::Album, alpha)
            .await
            .unwrap();
        let summary = store.playlist_summary(list).await.unwrap().unwrap();
        assert_eq!((summary.entry_count, summary.song_count), (1, 2));
        let a1 = id_of(&store, "SELECT id FROM tracks WHERE path = ?", "/m/a1").await;
        store.mark_removed(&[a1]).await.unwrap();
        let summary = store.playlist_summary(list).await.unwrap().unwrap();
        assert_eq!(summary.song_count, 1);
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
