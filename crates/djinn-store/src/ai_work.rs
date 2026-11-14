//! Work for AI workers: songs that need lyrics made or timed, handed out in
//! batches with a lease, and what came of them.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// Failed this many times, a song is left alone until an admin retries.
pub const MAX_ATTEMPTS: i64 = 3;

/// A song handed to a worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct AiJob {
    pub track_id: i64,
    /// "listen" (make the lyrics) or "sync" (time the lyrics it already has).
    pub kind: String,
    pub artist: Option<String>,
    pub title: String,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    /// The file's format (mpeg, mp4, flac, ...), so the worker can name it.
    pub codec: Option<String>,
    /// For "sync": the words to time.
    pub plain: Option<String>,
}

/// How the work stands.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, FromRow)]
pub struct AiCounts {
    pub waiting_listen: i64,
    pub waiting_sync: i64,
    pub working: i64,
    pub done: i64,
    pub failed: i64,
}

// Songs a worker could take: no job holding or finishing them.
macro_rules! free {
    () => {
        "NOT EXISTS (SELECT 1 FROM ai_jobs j WHERE j.track_id = t.id
            AND (j.status = 'done' OR (j.status = 'failed' AND j.attempts >= 3)
                 OR (j.status = 'working' AND j.taken_at > ?1)))"
    };
}
macro_rules! listen_where {
    () => {
        "l.source = 'none' AND t.removed_at IS NULL AND t.duration_ms >= 5000
         AND (l.online_checked_at IS NOT NULL OR ?2)"
    };
}
macro_rules! sync_where {
    () => {
        "l.ai = 0 AND l.synced IS NULL AND l.plain IS NOT NULL AND l.instrumental = 0
         AND t.removed_at IS NULL AND t.duration_ms >= 5000"
    };
}

impl Store {
    /// Hands `limit` songs to `worker`: songs with no lyrics (once LRCLIB has
    /// been asked, or when `lookups_off`), then songs with plain lyrics to
    /// time. A song held longer than `lease_ms` counts as free again.
    pub async fn ai_claim(
        &self,
        worker: &str,
        listen: bool,
        sync: bool,
        lookups_off: bool,
        limit: i64,
        lease_ms: i64,
    ) -> Result<Vec<AiJob>> {
        let now = now_ms();
        let lease_cut = now - lease_ms;
        let mut tx = self.write().await?;
        let mut jobs: Vec<AiJob> = Vec::new();
        if listen {
            let found: Vec<AiJob> = sqlx::query_as(concat!(
                "SELECT t.id AS track_id, 'listen' AS kind, t.artist, t.title, t.album, t.duration_ms,
                        t.codec, NULL AS plain
                 FROM lyrics l JOIN tracks t ON t.id = l.track_id
                 WHERE ",
                listen_where!(),
                " AND ",
                free!(),
                " ORDER BY t.id LIMIT ?3"
            ))
            .bind(lease_cut)
            .bind(lookups_off)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await?;
            jobs.extend(found);
        }
        let room = limit - i64::try_from(jobs.len()).unwrap_or(limit);
        if sync && room > 0 {
            let found: Vec<AiJob> = sqlx::query_as(concat!(
                "SELECT t.id AS track_id, 'sync' AS kind, t.artist, t.title, t.album, t.duration_ms,
                        t.codec, l.plain AS plain
                 FROM lyrics l JOIN tracks t ON t.id = l.track_id
                 WHERE ",
                sync_where!(),
                " AND ",
                free!(),
                " ORDER BY t.id LIMIT ?2"
            ))
            .bind(lease_cut)
            .bind(room)
            .fetch_all(&mut *tx)
            .await?;
            jobs.extend(found);
        }
        for job in &jobs {
            sqlx::query(
                "INSERT INTO ai_jobs (track_id, kind, status, worker, taken_at, attempts)
                 VALUES (?, ?, 'working', ?, ?, 1)
                 ON CONFLICT (track_id) DO UPDATE SET kind = excluded.kind, status = 'working',
                   worker = excluded.worker, taken_at = excluded.taken_at, error = NULL,
                   attempts = attempts + 1",
            )
            .bind(job.track_id)
            .bind(&job.kind)
            .bind(worker)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(jobs)
    }

    /// Who holds a song, and its kind, while the lease stands.
    pub async fn ai_holder(&self, track_id: i64) -> Result<Option<(String, String)>> {
        Ok(sqlx::query_as(
            "SELECT coalesce(worker, ''), kind FROM ai_jobs
             WHERE track_id = ? AND status = 'working'",
        )
        .bind(track_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// The worker is done with a song: it worked, or `error` says why not.
    pub async fn ai_finish(&self, track_id: i64, error: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE ai_jobs SET status = ?, finished_at = ?, error = ? WHERE track_id = ?")
            .bind(if error.is_some() { "failed" } else { "done" })
            .bind(now_ms())
            .bind(error)
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Gives failed songs another go.
    pub async fn ai_retry_failed(&self) -> Result<u64> {
        let done = sqlx::query("DELETE FROM ai_jobs WHERE status = 'failed'")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// Forgets what workers did (finished songs come back only if they still
    /// need lyrics).
    pub async fn ai_clear_jobs(&self) -> Result<u64> {
        let done = sqlx::query("DELETE FROM ai_jobs")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    pub async fn ai_counts(&self, lookups_off: bool, lease_ms: i64) -> Result<AiCounts> {
        let cut = now_ms() - lease_ms;
        Ok(sqlx::query_as(concat!(
            "SELECT
               (SELECT count(*) FROM lyrics l JOIN tracks t ON t.id = l.track_id
                WHERE ",
            listen_where!(),
            " AND ",
            free!(),
            ") AS waiting_listen,
               (SELECT count(*) FROM lyrics l JOIN tracks t ON t.id = l.track_id
                WHERE ",
            sync_where!(),
            " AND ",
            free!(),
            ") AS waiting_sync,
               (SELECT count(*) FROM ai_jobs WHERE status = 'working') AS working,
               (SELECT count(*) FROM ai_jobs WHERE status = 'done') AS done,
               (SELECT count(*) FROM ai_jobs WHERE status = 'failed') AS failed"
        ))
        .bind(cut)
        .bind(lookups_off)
        .fetch_one(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewLyrics, ScannedTrack};

    #[tokio::test]
    async fn songs_are_handed_out_once_with_a_lease_and_can_be_retried() {
        let store = Store::open_in_memory().await.unwrap();
        let song = |t: &str| ScannedTrack {
            path: format!("/m/{t}.flac"),
            title: t.into(),
            artist: Some("Band".into()),
            duration_ms: Some(200_000),
            ..Default::default()
        };
        store
            .save_scanned(&[song("Silent"), song("Words"), song("Done")])
            .await
            .unwrap();
        let ids: Vec<i64> = store
            .compare_rows()
            .await
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        // "Silent" has no lyrics (LRCLIB asked), "Words" has plain ones, "Done" has timed.
        store
            .save_read_lyrics(ids[0], 1, &NewLyrics::NONE)
            .await
            .unwrap();
        store.save_looked_up_lyrics(ids[0], None).await.unwrap();
        let plain = NewLyrics {
            synced: None,
            plain: Some("la la"),
            instrumental: false,
            source: "file",
        };
        store.save_read_lyrics(ids[1], 1, &plain).await.unwrap();
        let timed = NewLyrics {
            synced: Some("[00:01.00]la"),
            plain: Some("la"),
            instrumental: false,
            source: "file",
        };
        store.save_read_lyrics(ids[2], 1, &timed).await.unwrap();
        let lease = 30 * 60 * 1000;
        let counts = store.ai_counts(false, lease).await.unwrap();
        assert_eq!((counts.waiting_listen, counts.waiting_sync), (1, 1));
        // Only new songs when sync is off.
        let batch = store
            .ai_claim("pc", true, false, false, 10, lease)
            .await
            .unwrap();
        assert_eq!(
            batch
                .iter()
                .map(|j| (j.track_id, j.kind.as_str()))
                .collect::<Vec<_>>(),
            [(ids[0], "listen")]
        );
        // Held: nobody else gets it. Sync hands over the words to time.
        let more = store
            .ai_claim("other", true, true, false, 10, lease)
            .await
            .unwrap();
        assert_eq!(more.len(), 1);
        assert_eq!(
            (more[0].track_id, more[0].plain.as_deref()),
            (ids[1], Some("la la"))
        );
        assert_eq!(
            store.ai_holder(ids[0]).await.unwrap().map(|h| h.0),
            Some("pc".into())
        );
        // The worker dies: after the lease the song is free again.
        assert!(
            store
                .ai_claim("late", true, true, false, 10, 0)
                .await
                .unwrap()
                .len()
                >= 2
        );
        // Done and failed songs stay out; failed ones come back on retry.
        store.ai_finish(ids[0], None).await.unwrap();
        store.ai_finish(ids[1], Some("no vocals")).await.unwrap();
        let counts = store.ai_counts(false, lease).await.unwrap();
        assert_eq!((counts.done, counts.failed, counts.working), (1, 1, 0));
        assert_eq!(store.ai_retry_failed().await.unwrap(), 1);
    }
}
