//! Request queue, auto pool, and listening history.

use serde::Serialize;
use sqlx::FromRow;

use crate::library::track_columns;
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
    #[error("the queue is full ({0} songs). Try again after a few play")]
    QueueFull(i64),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Request limits for one user. `None` means no limit (admins and users
/// marked unlimited).
#[derive(Copy, Clone, Debug, Default)]
pub struct RequestLimits {
    /// Unplayed requests this user may have queued.
    pub per_user: Option<i64>,
    /// Requests the whole queue may hold.
    pub queue_cap: Option<i64>,
}

/// Who made a request and from where, for the request log.
#[derive(Clone, Debug, Default)]
pub struct RequestOrigin {
    pub name: String,
    pub mac: Option<String>,
    pub ip: Option<String>,
}

/// One entry in the request log.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct RequestLogEntry {
    pub at: i64,
    pub track_id: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub user_id: Option<i64>,
    pub name: Option<String>,
    pub mac: Option<String>,
    pub ip: Option<String>,
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
        concat!(
            "SELECT q.id AS queue_id, q.user_id, \
                    coalesce(u.display_name, u.username) AS username, q.added_at, ",
            track_columns!(),
            " FROM request_queue q \
             JOIN users u ON u.id = q.user_id \
             JOIN tracks t ON t.id = q.track_id"
        )
    };
}

impl Store {
    /// Appends a request, enforcing `limits` and recording where it came
    /// from.
    pub async fn add_request(
        &self,
        track_id: i64,
        user_id: i64,
        limits: RequestLimits,
        origin: &RequestOrigin,
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
        if let Some(cap) = limits.queue_cap {
            let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM request_queue")
                .fetch_one(&mut *tx)
                .await
                .map_err(StoreError::from)?;
            if queued >= cap {
                return Err(AddRequestError::QueueFull(cap));
            }
        }
        if let Some(limit) = limits.per_user {
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
        sqlx::query(
            "INSERT INTO request_events (track_id, user_id, at, name, mac, ip) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(track_id)
        .bind(user_id)
        .bind(now)
        .bind(&origin.name)
        .bind(&origin.mac)
        .bind(&origin.ip)
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

    /// Most recent requests first, from `since` (Unix ms) on.
    pub async fn request_log(&self, since: i64, limit: i64) -> Result<Vec<RequestLogEntry>> {
        let entries = sqlx::query_as(
            "SELECT r.at, r.track_id, t.title, t.artist, r.user_id,
                    coalesce(r.name, u.display_name, u.username) AS name, r.mac, r.ip
             FROM request_events r
             LEFT JOIN tracks t ON t.id = r.track_id
             LEFT JOIN users u ON u.id = r.user_id
             WHERE r.at >= ?
             ORDER BY r.at DESC, r.id DESC LIMIT ?",
        )
        .bind(since)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(entries)
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

    fn per_user(limit: Option<i64>) -> RequestLimits {
        RequestLimits {
            per_user: limit,
            queue_cap: None,
        }
    }

    fn origin() -> RequestOrigin {
        RequestOrigin {
            name: "u".into(),
            mac: Some("aa:bb:cc:00:11:22".into()),
            ip: Some("192.168.0.7".into()),
        }
    }

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
        store
            .add_request(ids[0], user, per_user(Some(2)), &origin())
            .await
            .unwrap();
        assert!(matches!(
            store
                .add_request(ids[0], user, per_user(Some(2)), &origin())
                .await,
            Err(AddRequestError::AlreadyQueued)
        ));
        store
            .add_request(ids[1], user, per_user(Some(2)), &origin())
            .await
            .unwrap();
        assert!(matches!(
            store
                .add_request(ids[2], user, per_user(Some(2)), &origin())
                .await,
            Err(AddRequestError::LimitReached(2))
        ));
        // No limit (admin).
        store
            .add_request(ids[2], user, per_user(None), &origin())
            .await
            .unwrap();
        assert!(matches!(
            store
                .add_request(9999, user, per_user(None), &origin())
                .await,
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

    #[tokio::test]
    async fn queue_cap_and_request_log() {
        let (store, user, ids) = setup().await;
        let capped = RequestLimits {
            per_user: None,
            queue_cap: Some(2),
        };
        store
            .add_request(ids[0], user, capped, &origin())
            .await
            .unwrap();
        store
            .add_request(ids[1], user, capped, &origin())
            .await
            .unwrap();
        assert!(matches!(
            store.add_request(ids[2], user, capped, &origin()).await,
            Err(AddRequestError::QueueFull(2))
        ));
        store
            .add_request(ids[2], user, RequestLimits::default(), &origin())
            .await
            .unwrap();

        let log = store.request_log(0, 10).await.unwrap();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].track_id, ids[2]);
        assert_eq!(log[0].title.as_deref(), Some("Song 3"));
        assert_eq!(log[0].mac.as_deref(), Some("aa:bb:cc:00:11:22"));
        assert_eq!(log[0].ip.as_deref(), Some("192.168.0.7"));
        assert_eq!(log[0].name.as_deref(), Some("u"));
    }

    #[tokio::test]
    async fn artist_top_tracks_rank_requests_then_plays() {
        let (store, user, ids) = setup().await;
        // Songs 1 to 3 are by artist A. Song 2 is requested twice, song 3
        // played once.
        for _ in 0..2 {
            store
                .add_request(ids[1], user, RequestLimits::default(), &origin())
                .await
                .unwrap();
            let id: i64 = sqlx::query_scalar("SELECT id FROM request_queue")
                .fetch_one(store.pool())
                .await
                .unwrap();
            store.remove_request(id).await.unwrap();
        }
        store
            .start_play_event(ids[2], PlaySource::Pool, None)
            .await
            .unwrap();
        let artist: i64 = sqlx::query_scalar("SELECT id FROM artists WHERE name = 'A'")
            .fetch_one(store.pool())
            .await
            .unwrap();
        let top = store.artist_top_tracks(artist, 10).await.unwrap();
        let ranked: Vec<(i64, i64, i64)> = top
            .iter()
            .map(|t| (t.track.id, t.requests, t.plays))
            .collect();
        assert_eq!(ranked, [(ids[1], 2, 0), (ids[2], 0, 1), (ids[0], 0, 0)]);
    }
}
