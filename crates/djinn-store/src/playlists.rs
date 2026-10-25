//! Playlists: named collections of songs, albums, and artists. Albums and
//! artists are expanded when the playlist is played, so music added to the
//! library later joins automatically.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, Result, Store, StoreError, Track};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum EntryKind {
    Track,
    Album,
    Artist,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    /// Shuffle when played; otherwise play in order.
    pub shuffle: bool,
    /// Songs, albums, and artists in it.
    pub entries: i64,
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
}

#[derive(Clone, Debug, Serialize)]
pub struct PlaylistDetail {
    #[serde(flatten)]
    pub playlist: PlaylistSummary,
    pub entries: Vec<PlaylistEntry>,
    /// Every song the playlist currently covers, in play order.
    pub tracks: Vec<Track>,
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
                (SELECT count(*) FROM playlist_entries e WHERE e.playlist_id = p.id) AS entries \
         FROM playlists p"
    };
}

macro_rules! entry_select {
    () => {
        "SELECT e.id, e.kind, e.ref_id, e.position,
                CASE e.kind
                  WHEN 'track' THEN (SELECT title FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT title FROM albums WHERE id = e.ref_id)
                  WHEN 'artist' THEN (SELECT name FROM artists WHERE id = e.ref_id)
                END AS name,
                CASE e.kind
                  WHEN 'track' THEN (SELECT artist FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT ar.name FROM albums al JOIN artists ar
                                     ON ar.id = al.artist_id WHERE al.id = e.ref_id)
                END AS subtitle,
                CASE e.kind
                  WHEN 'track' THEN (SELECT art_hash FROM tracks WHERE id = e.ref_id)
                  WHEN 'album' THEN (SELECT art_hash FROM albums WHERE id = e.ref_id)
                  WHEN 'artist' THEN (SELECT art_hash FROM artists WHERE id = e.ref_id)
                END AS art_hash
         FROM playlist_entries e"
    };
}

/// Playable tracks covered by a playlist's entries, in play order: entries
/// in their order, albums by disc and track, artists by album year.
macro_rules! expanded_tracks {
    ($columns:expr) => {
        concat!(
            "SELECT ",
            $columns,
            " FROM playlist_entries e
             JOIN tracks t ON t.removed_at IS NULL AND (
                  (e.kind = 'track' AND t.id = e.ref_id)
               OR (e.kind = 'album' AND t.album_id = e.ref_id)
               OR (e.kind = 'artist' AND (t.artist_id = e.ref_id
                   OR t.album_id IN (SELECT id FROM albums WHERE artist_id = e.ref_id))))
             LEFT JOIN albums al ON al.id = t.album_id
             WHERE e.playlist_id = ?
             ORDER BY e.position, al.year IS NULL, al.year, t.album COLLATE NOCASE,
                      t.disc_no, t.track_no, t.title COLLATE NOCASE"
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
        let tracks: Vec<Track> = sqlx::query_as(expanded_tracks!(
            "t.id, t.title, t.artist, t.album, t.album_artist, t.artist_id, t.album_id, \
             t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.art_hash"
        ))
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(PlaylistDetail {
            playlist,
            entries,
            tracks: dedup_by(tracks, |t| t.id),
        }))
    }

    /// Song ids the playlist covers, in play order, each once.
    pub async fn playlist_track_ids(&self, id: i64) -> Result<Vec<i64>> {
        let ids: Vec<i64> = sqlx::query_scalar(expanded_tracks!("t.id"))
            .bind(id)
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
            (summary.name.as_str(), summary.shuffle, summary.entries),
            ("Dance", false, 4)
        );

        assert!(store
            .remove_playlist_entry(list.id, artist.id)
            .await
            .unwrap());
        assert!(store.delete_playlist(list.id).await.unwrap());
        assert!(store.playlists().await.unwrap().is_empty());
    }
}
