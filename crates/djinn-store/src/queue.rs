//! Request queue, auto pool, and listening history.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, Result, Store, StoreError, Track};

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct QueueItem {
    #[sqlx(rename = "queue_id")]
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub added_at: i64,
    #[sqlx(flatten)]
    pub track: Track,
}

#[derive(Debug, thiserror::Error)]
pub enum AddRequestError {
    #[error("track not found")]
    NotFound,
    #[error("that song is already in the queue")]
    AlreadyQueued,
    #[error("you already have {0} songs in the queue")]
    LimitReached(i64),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum PoolKind {
    Track,
    Album,
    Artist,
    Playlist,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct PoolEntry {
    pub id: i64,
    pub kind: PoolKind,
    pub ref_id: i64,
    /// Display name of the referenced item, if it still exists.
    pub name: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum AddPoolError {
    #[error("not found")]
    NotFound,
    #[error("already in the pool")]
    AlreadyInPool,
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum PlaySource {
    Request,
    Pool,
}

macro_rules! queue_select {
    () => {
        "SELECT q.id AS queue_id, q.user_id, u.username, q.added_at, \
                t.id, t.title, t.artist, t.album, t.album_artist, t.artist_id, t.album_id, \
                t.track_no, t.disc_no, t.year, t.genre, t.duration_ms, t.art_hash \
         FROM request_queue q \
         JOIN users u ON u.id = q.user_id \
         JOIN tracks t ON t.id = q.track_id"
    };
}

impl Store {
    /// Appends a request. `limit` caps the user's unplayed requests; pass
    /// `None` for admins.
    pub async fn add_request(
        &self,
        track_id: i64,
        user_id: i64,
        limit: Option<i64>,
    ) -> Result<QueueItem, AddRequestError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from)?;
        let available: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ? AND removed_at IS NULL)",
        )
        .bind(track_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(StoreError::from)?;
        if !available {
            return Err(AddRequestError::NotFound);
        }
        if let Some(limit) = limit {
            let queued: i64 =
                sqlx::query_scalar("SELECT count(*) FROM request_queue WHERE user_id = ?")
                    .bind(user_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(StoreError::from)?;
            if queued >= limit {
                return Err(AddRequestError::LimitReached(limit));
            }
        }
        let now = now_ms();
        let inserted = sqlx::query_scalar(
            "INSERT INTO request_queue (track_id, user_id, added_at, position)
             VALUES (?, ?, ?, (SELECT coalesce(max(position), 0) + 1 FROM request_queue))
             RETURNING id",
        )
        .bind(track_id)
        .bind(user_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await;
        let id: i64 = match inserted {
            Ok(id) => id,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(AddRequestError::AlreadyQueued)
            }
            Err(e) => return Err(StoreError::from(e).into()),
        };
        sqlx::query("INSERT INTO request_events (track_id, user_id, at) VALUES (?, ?, ?)")
            .bind(track_id)
            .bind(user_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from)?;
        let item = sqlx::query_as(concat!(queue_select!(), " WHERE q.id = ?"))
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(StoreError::from)?;
        tx.commit().await.map_err(StoreError::from)?;
        Ok(item)
    }

    /// The request queue in play order. Requests for files that have since
    /// gone missing are left out.
    pub async fn queue(&self) -> Result<Vec<QueueItem>> {
        let items = sqlx::query_as(concat!(
            queue_select!(),
            " WHERE t.removed_at IS NULL ORDER BY q.position"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(items)
    }

    pub async fn queue_item(&self, id: i64) -> Result<Option<QueueItem>> {
        let item = sqlx::query_as(concat!(queue_select!(), " WHERE q.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(item)
    }

    pub async fn remove_request(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM request_queue WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Moves a request to `index` (0-based, clamped) in the queue.
    pub async fn move_request(&self, id: i64, index: usize) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let mut ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM request_queue ORDER BY position")
                .fetch_all(&mut *tx)
                .await?;
        let Some(from) = ids.iter().position(|&i| i == id) else {
            return Ok(false);
        };
        ids.remove(from);
        ids.insert(index.min(ids.len()), id);
        for (position, id) in ids.iter().enumerate() {
            sqlx::query("UPDATE request_queue SET position = ? WHERE id = ?")
                .bind(i64::try_from(position).unwrap_or(i64::MAX) + 1)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    pub async fn pool_entries(&self) -> Result<Vec<PoolEntry>> {
        let entries = sqlx::query_as(
            "SELECT p.id, p.kind, p.ref_id, p.added_at,
                    CASE p.kind
                      WHEN 'track' THEN (SELECT title FROM tracks WHERE id = p.ref_id)
                      WHEN 'album' THEN (SELECT title FROM albums WHERE id = p.ref_id)
                      WHEN 'artist' THEN (SELECT name FROM artists WHERE id = p.ref_id)
                      WHEN 'playlist' THEN (SELECT name FROM playlists WHERE id = p.ref_id)
                    END AS name
             FROM pool_entries p ORDER BY p.added_at, p.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
    }

    pub async fn add_pool_entry(
        &self,
        kind: PoolKind,
        ref_id: i64,
    ) -> Result<PoolEntry, AddPoolError> {
        let exists: bool = sqlx::query_scalar(match kind {
            PoolKind::Track => "SELECT EXISTS (SELECT 1 FROM tracks WHERE id = ?)",
            PoolKind::Album => "SELECT EXISTS (SELECT 1 FROM albums WHERE id = ?)",
            PoolKind::Artist => "SELECT EXISTS (SELECT 1 FROM artists WHERE id = ?)",
            PoolKind::Playlist => "SELECT EXISTS (SELECT 1 FROM playlists WHERE id = ?)",
        })
        .bind(ref_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)?;
        if !exists {
            return Err(AddPoolError::NotFound);
        }
        let inserted = sqlx::query_scalar(
            "INSERT INTO pool_entries (kind, ref_id, added_at) VALUES (?, ?, ?) RETURNING id",
        )
        .bind(kind)
        .bind(ref_id)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await;
        let id: i64 = match inserted {
            Ok(id) => id,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(AddPoolError::AlreadyInPool)
            }
            Err(e) => return Err(StoreError::from(e).into()),
        };
        let entries = self.pool_entries().await?;
        entries
            .into_iter()
            .find(|e| e.id == id)
            .ok_or(AddPoolError::NotFound)
    }

    pub async fn remove_pool_entry(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM pool_entries WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Every playable track the pool currently covers. Album, artist, and
    /// playlist entries are expanded now, so new additions are included.
    pub async fn pool_track_ids(&self) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(
            "SELECT t.id FROM tracks t
             WHERE t.removed_at IS NULL AND (
               t.id IN (SELECT ref_id FROM pool_entries WHERE kind = 'track')
               OR t.album_id IN (SELECT ref_id FROM pool_entries WHERE kind = 'album')
               OR t.artist_id IN (SELECT ref_id FROM pool_entries WHERE kind = 'artist')
               OR t.album_id IN (
                 SELECT al.id FROM albums al
                 JOIN pool_entries p ON p.kind = 'artist' AND p.ref_id = al.artist_id)
               OR t.id IN (
                 SELECT pt.track_id FROM playlist_tracks pt
                 JOIN pool_entries p ON p.kind = 'playlist' AND p.ref_id = pt.playlist_id)
             )
             ORDER BY t.id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    /// Most recently started tracks, newest first.
    pub async fn recent_track_ids(&self, limit: i64) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(
            "SELECT track_id FROM play_events ORDER BY started_at DESC, id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    pub async fn start_play_event(
        &self,
        track_id: i64,
        source: PlaySource,
        requested_by: Option<i64>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO play_events (track_id, source, requested_by, started_at)
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(track_id)
        .bind(source)
        .bind(requested_by)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn finish_play_event(
        &self,
        id: i64,
        played_ms: i64,
        skipped: bool,
        skipped_by: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE play_events SET ended_at = ?, played_ms = ?, skipped = ?, skipped_by = ?
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(played_ms)
        .bind(skipped)
        .bind(skipped_by)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    async fn setup() -> (Store, i64, Vec<i64>) {
        let store = Store::open_in_memory().await.unwrap();
        let user = store.create_user("u", "h", Role::User).await.unwrap().id;
        let tracks: Vec<ScannedTrack> = (1..=5)
            .map(|i| ScannedTrack {
                path: format!("/m/{i}.flac"),
                title: format!("Song {i}"),
                artist: Some(if i <= 3 { "A" } else { "B" }.to_owned()),
                album: Some(if i <= 3 { "First" } else { "Second" }.to_owned()),
                ..Default::default()
            })
            .collect();
        store.save_scanned(&tracks).await.unwrap();
        let ids = store
            .known_files()
            .await
            .unwrap()
            .iter()
            .map(|k| k.id)
            .collect();
        (store, user, ids)
    }

    #[tokio::test]
    async fn request_rules() {
        let (store, user, ids) = setup().await;
        store.add_request(ids[0], user, Some(2)).await.unwrap();
        assert!(matches!(
            store.add_request(ids[0], user, Some(2)).await,
            Err(AddRequestError::AlreadyQueued)
        ));
        store.add_request(ids[1], user, Some(2)).await.unwrap();
        assert!(matches!(
            store.add_request(ids[2], user, Some(2)).await,
            Err(AddRequestError::LimitReached(2))
        ));
        // No limit (admin).
        store.add_request(ids[2], user, None).await.unwrap();
        assert!(matches!(
            store.add_request(9999, user, None).await,
            Err(AddRequestError::NotFound)
        ));

        let queue = store.queue().await.unwrap();
        let order: Vec<i64> = queue.iter().map(|q| q.track.id).collect();
        assert_eq!(order, [ids[0], ids[1], ids[2]]);
        assert_eq!(queue[0].username, "u");

        store.move_request(queue[2].id, 0).await.unwrap();
        let order: Vec<i64> = store
            .queue()
            .await
            .unwrap()
            .iter()
            .map(|q| q.track.id)
            .collect();
        assert_eq!(order, [ids[2], ids[0], ids[1]]);

        assert!(store.remove_request(queue[0].id).await.unwrap());
        assert_eq!(store.queue().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn pool_expands_live() {
        let (store, _, ids) = setup().await;
        let album = store.search("second", 5).await.unwrap().albums[0].id;
        let artist_a = store.search("a", 5).await.unwrap().artists[0].id;

        store.add_pool_entry(PoolKind::Album, album).await.unwrap();
        assert_eq!(store.pool_track_ids().await.unwrap(), [ids[3], ids[4]]);
        assert!(matches!(
            store.add_pool_entry(PoolKind::Album, album).await,
            Err(AddPoolError::AlreadyInPool)
        ));
        let entry = store
            .add_pool_entry(PoolKind::Artist, artist_a)
            .await
            .unwrap();
        assert_eq!(entry.name.as_deref(), Some("A"));
        assert_eq!(store.pool_track_ids().await.unwrap(), ids);

        // A new track on a pooled album joins automatically.
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/6.flac".into(),
                title: "Song 6".into(),
                artist: Some("B".into()),
                album: Some("Second".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        assert_eq!(store.pool_track_ids().await.unwrap().len(), 6);

        store.remove_pool_entry(entry.id).await.unwrap();
        assert_eq!(store.pool_track_ids().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn play_history() {
        let (store, user, ids) = setup().await;
        let first = store
            .start_play_event(ids[0], PlaySource::Pool, None)
            .await
            .unwrap();
        store
            .finish_play_event(first, 1000, false, None)
            .await
            .unwrap();
        store
            .start_play_event(ids[1], PlaySource::Request, Some(user))
            .await
            .unwrap();
        assert_eq!(store.recent_track_ids(10).await.unwrap(), [ids[1], ids[0]]);
    }
}
