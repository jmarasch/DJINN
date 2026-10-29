//! Request queue, auto pool, and listening history.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::library::track_columns;
use crate::party::style_in_use;
use crate::{now_ms, Result, Store, StoreError, Track};

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct QueueItem {
    #[sqlx(rename = "queue_id")]
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub added_at: i64,
    /// Others who asked for it too, in the order they did.
    #[sqlx(json)]
    pub backers: Vec<Backer>,
    #[sqlx(flatten)]
    pub track: Track,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backer {
    pub user_id: i64,
    pub name: String,
}

/// What a request did.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RequestOutcome {
    /// A new song in the queue.
    Added,
    /// The song was queued already; this person backs it now.
    Backed,
}

/// What withdrawing from a request did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Withdrawal {
    /// A backer stepped back; the song stays.
    Unbacked,
    /// The requester stepped back and the first backer took it over.
    HandedOver,
    /// The requester stepped back and nobody else wanted it.
    Removed,
    /// Neither the requester nor a backer.
    NotInvolved,
}

#[derive(Debug, thiserror::Error)]
pub enum AddRequestError {
    #[error("track not found")]
    NotFound,
    #[error("that song is already in the queue")]
    AlreadyQueued,
    #[error("you already asked for that song")]
    AlreadyYours,
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

/// How many people back a queued request (`q`), besides its requester.
macro_rules! backer_count {
    () => {
        "(SELECT count(*) FROM request_backers rb WHERE rb.queue_id = q.id)"
    };
}

/// Play order: songs more people asked for first, then first come first
/// served (`position`, which an admin can change within the same support).
/// Worked out on every read, so it never depends on the order people
/// backed or stepped back in.
macro_rules! queue_order {
    () => {
        concat!(backer_count!(), " DESC, q.position")
    };
}

macro_rules! queue_select {
    () => {
        concat!(
            "SELECT q.id AS queue_id, q.user_id, \
                    coalesce(u.display_name, u.username) AS username, q.added_at, \
                    (SELECT json_group_array(json_object('user_id', b.user_id, 'name', b.name)) \
                     FROM (SELECT rb.user_id, coalesce(bu.display_name, bu.username) AS name \
                           FROM request_backers rb JOIN users bu ON bu.id = rb.user_id \
                           WHERE rb.queue_id = q.id ORDER BY rb.added_at, rb.user_id) b) AS backers, ",
            track_columns!(),
            " FROM request_queue q \
             JOIN users u ON u.id = q.user_id \
             JOIN tracks t ON t.id = q.track_id"
        )
    };
}

impl Store {
    /// Appends a request, enforcing `limits` and recording where it came
    /// from. A song already in the queue gets this person as a backer
    /// instead (using one of their slots) and moves up one place.
    pub async fn add_request(
        &self,
        track_id: i64,
        user_id: i64,
        limits: RequestLimits,
        origin: &RequestOrigin,
    ) -> Result<(QueueItem, RequestOutcome), AddRequestError> {
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
        let existing: Option<(i64, i64)> =
            sqlx::query_as("SELECT id, user_id FROM request_queue WHERE track_id = ?")
                .bind(track_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(StoreError::from)?;
        if let Some((queue_id, owner)) = existing {
            let backing: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM request_backers WHERE queue_id = ? AND user_id = ?)",
            )
            .bind(queue_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(StoreError::from)?;
            if owner == user_id || backing {
                return Err(AddRequestError::AlreadyYours);
            }
        } else if let Some(cap) = limits.queue_cap {
            let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM request_queue")
                .fetch_one(&mut *tx)
                .await
                .map_err(StoreError::from)?;
            if queued >= cap {
                return Err(AddRequestError::QueueFull(cap));
            }
        }
        if let Some(limit) = limits.per_user {
            let queued: i64 = sqlx::query_scalar(
                "SELECT (SELECT count(*) FROM request_queue WHERE user_id = ?1) \
                      + (SELECT count(*) FROM request_backers WHERE user_id = ?1)",
            )
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(StoreError::from)?;
            if queued >= limit {
                return Err(AddRequestError::LimitReached(limit));
            }
        }
        let now = now_ms();
        let (id, outcome) = match existing {
            Some((queue_id, _)) => {
                // More backers puts it further up the queue (see QUEUE_ORDER).
                sqlx::query(
                    "INSERT INTO request_backers (queue_id, user_id, added_at) VALUES (?, ?, ?)",
                )
                .bind(queue_id)
                .bind(user_id)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(StoreError::from)?;
                (queue_id, RequestOutcome::Backed)
            }
            None => {
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
                match inserted {
                    Ok(id) => (id, RequestOutcome::Added),
                    Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                        return Err(AddRequestError::AlreadyQueued)
                    }
                    Err(e) => return Err(StoreError::from(e).into()),
                }
            }
        };
        sqlx::query(concat!(
            "INSERT INTO request_events (track_id, user_id, at, name, mac, ip, style_id)              VALUES (?, ?, ?, ?, ?, ?, ",
            style_in_use!(),
            ")"
        ))
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
        Ok((item, outcome))
    }

    /// Takes `user_id` off a request: a backer just steps back; the
    /// requester hands it to the first backer, or removes it if there's
    /// none.
    pub async fn withdraw_request(&self, id: i64, user_id: i64) -> Result<Withdrawal> {
        let mut tx = self.pool.begin().await?;
        let unbacked =
            sqlx::query("DELETE FROM request_backers WHERE queue_id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
        if unbacked.rows_affected() > 0 {
            tx.commit().await?;
            return Ok(Withdrawal::Unbacked);
        }
        let owner: Option<i64> =
            sqlx::query_scalar("SELECT user_id FROM request_queue WHERE id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        if owner != Some(user_id) {
            return Ok(Withdrawal::NotInvolved);
        }
        let next: Option<i64> = sqlx::query_scalar(
            "SELECT user_id FROM request_backers WHERE queue_id = ? \
             ORDER BY added_at, user_id LIMIT 1",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let outcome = match next {
            Some(heir) => {
                sqlx::query("DELETE FROM request_backers WHERE queue_id = ? AND user_id = ?")
                    .bind(id)
                    .bind(heir)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE request_queue SET user_id = ? WHERE id = ?")
                    .bind(heir)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                Withdrawal::HandedOver
            }
            None => {
                sqlx::query("DELETE FROM request_queue WHERE id = ?")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                Withdrawal::Removed
            }
        };
        tx.commit().await?;
        Ok(outcome)
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
            " WHERE t.removed_at IS NULL ORDER BY ",
            queue_order!()
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

    /// Moves a request to `index` (0-based) in the queue, as far as it can
    /// go among songs with as many backers: a song with more support stays
    /// ahead, one with less behind (see `queue_order!`).
    pub async fn move_request(&self, id: i64, index: usize) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let rows: Vec<(i64, i64, i64)> = sqlx::query_as(concat!(
            "SELECT q.id, q.position, ",
            backer_count!(),
            " FROM request_queue q ORDER BY ",
            queue_order!()
        ))
        .fetch_all(&mut *tx)
        .await?;
        let Some(from) = rows.iter().position(|&(i, _, _)| i == id) else {
            return Ok(false);
        };
        let support = rows[from].2;
        // The songs with the same support sit together in play order.
        let first = rows.iter().position(|r| r.2 == support).unwrap_or(from);
        let last = rows.iter().rposition(|r| r.2 == support).unwrap_or(from);
        let to = index.clamp(first, last);
        let mut tier: Vec<i64> = rows[first..=last].iter().map(|r| r.0).collect();
        let positions: Vec<i64> = rows[first..=last].iter().map(|r| r.1).collect();
        let moved = tier.remove(from - first);
        tier.insert(to - first, moved);
        for (id, position) in tier.iter().zip(positions) {
            sqlx::query("UPDATE request_queue SET position = ? WHERE id = ?")
                .bind(position)
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

    /// Songs that started since `since` (Unix ms) and have ended, newest
    /// first, with whether each was skipped.
    pub async fn plays_since(&self, since: i64) -> Result<Vec<(i64, bool)>> {
        let rows = sqlx::query_as(
            "SELECT track_id, skipped FROM play_events \
             WHERE started_at >= ? AND ended_at IS NOT NULL \
             ORDER BY started_at DESC, id DESC LIMIT 200",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn start_play_event(
        &self,
        track_id: i64,
        source: PlaySource,
        requested_by: Option<i64>,
    ) -> Result<i64> {
        let id = sqlx::query_scalar(concat!(
            "INSERT INTO play_events (track_id, source, requested_by, started_at, style_id)              VALUES (?, ?, ?, ?, ",
            style_in_use!(),
            ") RETURNING id"
        ))
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
    async fn the_queue_follows_how_many_back_each_song() {
        let (store, ana, ids) = setup().await;
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let jo = store.create_user("jo", "h", Role::User).await.unwrap().id;
        let mut queued = Vec::new();
        for &id in &ids[..4] {
            let (item, _) = store
                .add_request(id, ana, per_user(None), &origin())
                .await
                .unwrap();
            queued.push(item.id);
        }
        let order = |store: Store| async move {
            store
                .queue()
                .await
                .unwrap()
                .iter()
                .map(|q| q.track.id)
                .collect::<Vec<_>>()
        };
        let original = [ids[0], ids[1], ids[2], ids[3]];
        let back = |track, user| {
            let store = store.clone();
            async move {
                store
                    .add_request(track, user, per_user(None), &origin())
                    .await
                    .unwrap();
            }
        };

        // Backed songs go ahead of unbacked ones, in the order they were
        // asked for, whatever order they were backed in.
        back(ids[3], sam).await;
        back(ids[1], sam).await;
        assert_eq!(order(store.clone()).await, [ids[1], ids[3], ids[0], ids[2]]);
        // Two backers beat one.
        back(ids[3], jo).await;
        assert_eq!(order(store.clone()).await, [ids[3], ids[1], ids[0], ids[2]]);

        // Stepping back, in any order, restores the original order.
        for (queue_id, user) in [(queued[1], sam), (queued[3], jo), (queued[3], sam)] {
            assert_eq!(
                store.withdraw_request(queue_id, user).await.unwrap(),
                Withdrawal::Unbacked
            );
        }
        assert_eq!(order(store.clone()).await, original);
    }

    #[tokio::test]
    async fn admin_moves_stay_among_songs_with_as_much_support() {
        let (store, ana, ids) = setup().await;
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let mut queued = Vec::new();
        for &id in &ids[..4] {
            let (item, _) = store
                .add_request(id, ana, per_user(None), &origin())
                .await
                .unwrap();
            queued.push(item.id);
        }
        store
            .add_request(ids[3], sam, per_user(None), &origin())
            .await
            .unwrap();
        let order = || async {
            store
                .queue()
                .await
                .unwrap()
                .iter()
                .map(|q| q.track.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(order().await, [ids[3], ids[0], ids[1], ids[2]]);
        // Moving the last song to the top stops below the backed one.
        assert!(store.move_request(queued[2], 0).await.unwrap());
        assert_eq!(order().await, [ids[3], ids[2], ids[0], ids[1]]);
        // And the backed one can't be pushed below the others.
        assert!(store.move_request(queued[3], 3).await.unwrap());
        assert_eq!(order().await[0], ids[3]);
        // Once Sam steps back, everyone keeps the admin's order.
        store.withdraw_request(queued[3], sam).await.unwrap();
        assert_eq!(order().await, [ids[2], ids[0], ids[1], ids[3]]);
    }

    #[tokio::test]
    async fn asking_for_a_queued_song_backs_it() {
        let (store, ana, ids) = setup().await;
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let jo = store.create_user("jo", "h", Role::User).await.unwrap().id;
        let limit = per_user(Some(2));
        for &id in &ids[..3] {
            store
                .add_request(id, ana, per_user(None), &origin())
                .await
                .unwrap();
        }
        let order = |store: Store| async move {
            store
                .queue()
                .await
                .unwrap()
                .iter()
                .map(|q| (q.track.id, q.backers.len()))
                .collect::<Vec<_>>()
        };

        // Sam backs the last song: backed songs go first.
        let (item, outcome) = store
            .add_request(ids[2], sam, limit, &origin())
            .await
            .unwrap();
        assert_eq!(outcome, RequestOutcome::Backed);
        assert_eq!(item.backers[0].name, "sam");
        assert_eq!(
            order(store.clone()).await,
            [(ids[2], 1), (ids[0], 0), (ids[1], 0)]
        );
        // Jo backs it too.
        store
            .add_request(ids[2], jo, limit, &origin())
            .await
            .unwrap();
        assert_eq!(order(store.clone()).await[0], (ids[2], 2));
        // Asking again is refused, and a backing uses up a slot.
        assert!(matches!(
            store.add_request(ids[2], sam, limit, &origin()).await,
            Err(AddRequestError::AlreadyYours)
        ));
        store
            .add_request(ids[3], sam, limit, &origin())
            .await
            .unwrap();
        assert!(matches!(
            store.add_request(ids[4], sam, limit, &origin()).await,
            Err(AddRequestError::LimitReached(2))
        ));

        // Ana steps back: Sam asked first, so the request is his now.
        let queue_id = item.id;
        assert_eq!(
            store.withdraw_request(queue_id, ana).await.unwrap(),
            Withdrawal::HandedOver
        );
        let item = store.queue_item(queue_id).await.unwrap().unwrap();
        assert_eq!((item.user_id, item.backers.len()), (sam, 1));
        assert_eq!(
            store.withdraw_request(queue_id, jo).await.unwrap(),
            Withdrawal::Unbacked
        );
        assert_eq!(
            store.withdraw_request(queue_id, jo).await.unwrap(),
            Withdrawal::NotInvolved
        );
        assert_eq!(
            store.withdraw_request(queue_id, sam).await.unwrap(),
            Withdrawal::Removed
        );
        assert!(store.queue_item(queue_id).await.unwrap().is_none());
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
            Err(AddRequestError::AlreadyYours)
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
