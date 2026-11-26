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
/// A song a worker is working on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct AiHeld {
    pub worker: String,
    pub track_id: i64,
    /// "listen" or "sync".
    pub kind: String,
    pub artist: Option<String>,
    pub title: String,
    pub taken_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, FromRow)]
pub struct AiCounts {
    pub waiting_listen: i64,
    pub waiting_sync: i64,
    pub working: i64,
    /// Vocals isolated on Main, waiting for a node.
    pub prepared: i64,
    pub done: i64,
    /// Results under the minimum score (not waiting for a second opinion).
    pub low_score: i64,
    /// Songs that failed: an error, or nothing sent back.
    pub failed: i64,
}

// Songs a worker could take: no job holding or finishing them.
macro_rules! free {
    () => {
        "NOT EXISTS (SELECT 1 FROM ai_jobs j WHERE j.track_id = t.id
            AND (j.status = 'done' OR (j.status = 'failed' AND j.attempts >= 3)
                 OR (j.status IN ('working', 'prepared') AND j.taken_at > ?1)))"
    };
}
macro_rules! listen_where {
    () => {
        "l.source = 'none' AND l.instrumental = 0 AND l.ai_removed = 0 AND t.removed_at IS NULL
         AND t.duration_ms >= 5000
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
    /// time. A song held longer than `lease_ms` counts as free again. Songs
    /// in `skip` (the genres left out of AI lyrics) are only done when an
    /// admin asks for them.
    #[allow(clippy::too_many_arguments)]
    pub async fn ai_claim(
        &self,
        worker: &str,
        listen: bool,
        sync: bool,
        lookups_off: bool,
        limit: i64,
        lease_ms: i64,
        skip: &[i64],
    ) -> Result<Vec<AiJob>> {
        let skip = crate::library::json_ids(skip);
        let now = now_ms();
        let lease_cut = now - lease_ms;
        let mut tx = self.write().await?;
        // Songs an admin asked for come first, whatever `listen` and `sync`
        // say, while they still need it and haven't failed three times.
        let mut jobs: Vec<AiJob> = sqlx::query_as(
            "SELECT t.id AS track_id, q.kind, t.artist, t.title, t.album, t.duration_ms, t.codec,
                    CASE WHEN q.kind = 'sync' THEN l.plain END AS plain
             FROM ai_asked q JOIN tracks t ON t.id = q.track_id
             LEFT JOIN lyrics l ON l.track_id = t.id
             WHERE t.removed_at IS NULL
               AND ((q.kind = 'listen' AND (l.track_id IS NULL OR (l.source = 'none' AND l.instrumental = 0)))
                    OR (q.kind = 'sync' AND l.synced IS NULL AND l.plain IS NOT NULL
                        AND l.instrumental = 0))
               AND NOT EXISTS (SELECT 1 FROM ai_jobs j WHERE j.track_id = t.id
                   AND ((j.status IN ('working', 'prepared') AND j.taken_at > ?1)
                        OR (j.status = 'failed' AND j.attempts >= 3 AND j.kind = q.kind)))
             ORDER BY q.asked_at LIMIT ?2",
        )
        .bind(lease_cut)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        let left = limit - i64::try_from(jobs.len()).unwrap_or(limit);
        if listen && left > 0 {
            let found: Vec<AiJob> = sqlx::query_as(concat!(
                "SELECT t.id AS track_id, 'listen' AS kind, t.artist, t.title, t.album, t.duration_ms,
                        t.codec, NULL AS plain
                 FROM lyrics l JOIN tracks t ON t.id = l.track_id
                 WHERE ",
                listen_where!(),
                " AND ",
                free!(),
                " AND t.id NOT IN (SELECT value FROM json_each(?4))
                 ORDER BY t.id LIMIT ?3"
            ))
            .bind(lease_cut)
            .bind(lookups_off)
            .bind(left)
            .bind(&skip)
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
                " AND t.id NOT IN (SELECT value FROM json_each(?3))
                 ORDER BY t.id LIMIT ?2"
            ))
            .bind(lease_cut)
            .bind(room)
            .bind(&skip)
            .fetch_all(&mut *tx)
            .await?;
            jobs.extend(found);
        }
        // An asked song the usual rounds found too goes once.
        let mut seen = std::collections::HashSet::new();
        jobs.retain(|job| seen.insert(job.track_id));
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

    /// Hands `limit` songs whose vocals Main has isolated to `worker` (a
    /// node), oldest first, as long as they still need lyrics.
    pub async fn ai_claim_prepared(&self, worker: &str, limit: i64) -> Result<Vec<AiJob>> {
        let now = now_ms();
        let mut tx = self.write().await?;
        let jobs: Vec<AiJob> = sqlx::query_as(
            "SELECT t.id AS track_id, j.kind, t.artist, t.title, t.album, t.duration_ms, t.codec,
                    CASE WHEN j.kind = 'sync' THEN l.plain END AS plain
             FROM ai_jobs j JOIN tracks t ON t.id = j.track_id JOIN lyrics l ON l.track_id = t.id
             WHERE j.status = 'prepared' AND t.removed_at IS NULL
               AND ((j.kind = 'listen' AND l.source = 'none')
                    OR (j.kind = 'sync' AND l.ai = 0 AND l.synced IS NULL AND l.plain IS NOT NULL))
             ORDER BY j.taken_at LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        for job in &jobs {
            sqlx::query(
                "UPDATE ai_jobs SET status = 'working', worker = ?, taken_at = ? WHERE track_id = ?",
            )
            .bind(worker)
            .bind(now)
            .bind(job.track_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(jobs)
    }

    /// Main has isolated a song's vocals: it waits for a node.
    pub async fn ai_prepared(&self, track_id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE ai_jobs SET status = 'prepared', worker = NULL, taken_at = ? WHERE track_id = ?",
        )
        .bind(now_ms())
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// How many songs wait for a node with their vocals isolated.
    pub async fn ai_prepared_count(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT count(*) FROM ai_jobs WHERE status = 'prepared'")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// The songs workers are working on now (taken since `since`), oldest
    /// first, for the status page.
    pub async fn ai_held(&self, since: i64) -> Result<Vec<AiHeld>> {
        Ok(sqlx::query_as(
            "SELECT j.worker, j.track_id, j.kind, t.artist, t.title, j.taken_at
             FROM ai_jobs j JOIN tracks t ON t.id = j.track_id
             WHERE j.status = 'working' AND j.taken_at > ? AND j.worker IS NOT NULL
             ORDER BY j.taken_at",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Songs a worker holds or that wait prepared: their isolated vocals
    /// are still wanted.
    pub async fn ai_open_jobs(&self) -> Result<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT track_id FROM ai_jobs WHERE status IN ('working', 'prepared')",
        )
        .fetch_all(&self.pool)
        .await?)
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

    /// Whether `worker` currently holds an active lease on any track job.
    pub async fn ai_worker_busy(&self, worker: &str, lease_ms: i64) -> Result<bool> {
        let cut = now_ms() - lease_ms;
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT EXISTS (SELECT 1 FROM ai_jobs WHERE worker = ?1 AND status = 'working' AND taken_at > ?2)",
        )
        .bind(worker)
        .bind(cut)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    /// Asks workers for these songs before any others: make lyrics
    /// (`listen`) or time the words they have (`sync`). Returns how many.
    pub async fn ai_ask(&self, ids: &[i64], kind: &str) -> Result<u64> {
        let list = crate::library::json_ids(ids);
        // A song asked again gets a fresh go, failed tries forgotten.
        sqlx::query(
            "DELETE FROM ai_jobs WHERE status IN ('failed', 'done') AND kind = ?2
               AND track_id IN (SELECT value FROM json_each(?1))",
        )
        .bind(&list)
        .bind(kind)
        .execute(&self.pool)
        .await?;
        let done = sqlx::query(
            "INSERT INTO ai_asked (track_id, kind, asked_at)
             SELECT t.id, ?2, ?3 FROM tracks t
             WHERE t.id IN (SELECT value FROM json_each(?1)) AND t.removed_at IS NULL
             ON CONFLICT (track_id) DO UPDATE SET kind = excluded.kind, asked_at = excluded.asked_at",
        )
        .bind(&list)
        .bind(kind)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }

    /// The worker is done with a song: it worked, or `error` says why not.
    pub async fn ai_finish(&self, track_id: i64, error: Option<&str>) -> Result<()> {
        if error.is_none() {
            sqlx::query("DELETE FROM ai_asked WHERE track_id = ?")
                .bind(track_id)
                .execute(&self.pool)
                .await?;
        }
        sqlx::query("UPDATE ai_jobs SET status = ?, finished_at = ?, error = ? WHERE track_id = ?")
            .bind(if error.is_some() { "failed" } else { "done" })
            .bind(now_ms())
            .bind(error)
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Records a failure that asking again won't change (a result that
    /// scored under the minimum): the song isn't handed out again until an
    /// admin retries failed songs.
    pub async fn ai_give_up(&self, track_id: i64, error: Option<&str>) -> Result<()> {
        sqlx::query("DELETE FROM ai_asked WHERE track_id = ?")
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        sqlx::query(
            "UPDATE ai_jobs SET status = 'failed', finished_at = ?, error = ?,
                    attempts = max(attempts, ?) WHERE track_id = ?",
        )
        .bind(now_ms())
        .bind(error)
        .bind(MAX_ATTEMPTS)
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Sets a song a smaller model wasn't sure of aside for a second opinion
    /// from this computer's listener (`ai_claim_second`); no one else takes
    /// it meanwhile.
    pub async fn ai_second_opinion(&self, track_id: i64, error: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE ai_jobs SET status = 'failed', finished_at = ?, error = ?,
                    attempts = max(attempts, ?), second_opinion = 1 WHERE track_id = ?",
        )
        .bind(now_ms())
        .bind(error)
        .bind(MAX_ATTEMPTS)
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Hands `limit` songs waiting for a second opinion to `worker`, oldest
    /// first, as long as they still need lyrics.
    pub async fn ai_claim_second(&self, worker: &str, limit: i64) -> Result<Vec<AiJob>> {
        let now = now_ms();
        let mut tx = self.write().await?;
        let jobs: Vec<AiJob> = sqlx::query_as(
            "SELECT t.id AS track_id, j.kind, t.artist, t.title, t.album, t.duration_ms, t.codec,
                    CASE WHEN j.kind = 'sync' THEN l.plain END AS plain
             FROM ai_jobs j JOIN tracks t ON t.id = j.track_id JOIN lyrics l ON l.track_id = t.id
             WHERE j.second_opinion = 1 AND j.status = 'failed' AND t.removed_at IS NULL
               AND ((j.kind = 'listen' AND l.source = 'none')
                    OR (j.kind = 'sync' AND l.ai = 0 AND l.synced IS NULL AND l.plain IS NOT NULL))
             ORDER BY j.finished_at LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        for job in &jobs {
            sqlx::query(
                "UPDATE ai_jobs SET status = 'working', worker = ?, taken_at = ?, error = NULL,
                        second_opinion = 0 WHERE track_id = ?",
            )
            .bind(worker)
            .bind(now)
            .bind(job.track_id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(jobs)
    }

    /// Songs waiting for a second opinion.
    pub async fn ai_second_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM ai_jobs WHERE second_opinion = 1 AND status = 'failed'",
        )
        .fetch_one(&self.pool)
        .await?)
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

    pub async fn ai_counts(
        &self,
        lookups_off: bool,
        lease_ms: i64,
        skip: &[i64],
    ) -> Result<AiCounts> {
        let cut = now_ms() - lease_ms;
        Ok(sqlx::query_as(concat!(
            "SELECT
               (SELECT count(*) FROM lyrics l JOIN tracks t ON t.id = l.track_id
                WHERE ",
            listen_where!(),
            " AND ",
            free!(),
            " AND t.id NOT IN (SELECT value FROM json_each(?3))) AS waiting_listen,
               (SELECT count(*) FROM lyrics l JOIN tracks t ON t.id = l.track_id
                WHERE ",
            sync_where!(),
            " AND ",
            free!(),
            " AND t.id NOT IN (SELECT value FROM json_each(?3))) AS waiting_sync,
               (SELECT count(*) FROM ai_jobs WHERE status = 'working') AS working,
               (SELECT count(*) FROM ai_jobs WHERE status = 'prepared') AS prepared,
               (SELECT count(*) FROM ai_jobs WHERE status = 'done') AS done,
               (SELECT count(*) FROM ai_jobs WHERE status = 'failed' AND second_opinion = 0
                  AND error LIKE 'scored too low%') AS low_score,
               (SELECT count(*) FROM ai_jobs WHERE status = 'failed'
                  AND (error IS NULL OR error NOT LIKE 'scored too low%')) AS failed"
        ))
        .bind(cut)
        .bind(lookups_off)
        .bind(crate::library::json_ids(skip))
        .fetch_one(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NewLyrics, ScannedTrack};

    #[tokio::test]
    async fn a_song_a_node_was_unsure_of_waits_for_a_second_opinion() {
        let store = Store::open_in_memory().await.unwrap();
        let song = |t: &str| ScannedTrack {
            path: format!("/m/{t}.flac"),
            title: t.into(),
            artist: Some("Band".into()),
            duration_ms: Some(200_000),
            ..Default::default()
        };
        store
            .save_scanned(&[song("Unsure"), song("Hum")])
            .await
            .unwrap();
        let ids: Vec<i64> = store
            .compare_rows()
            .await
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        for &id in &ids {
            store
                .save_read_lyrics(id, 1, &NewLyrics::NONE)
                .await
                .unwrap();
            store.save_looked_up_lyrics(id, None).await.unwrap();
        }
        let lease = 30 * 60 * 1000;
        let batch = store
            .ai_claim("node:a", true, false, false, 10, lease, &[])
            .await
            .unwrap();
        assert_eq!(batch.len(), 2);
        // The node wasn't sure of one: set aside for a second opinion,
        // which nobody's ordinary round takes.
        store
            .ai_second_opinion(ids[0], Some("scored too low (72%, under 85%)"))
            .await
            .unwrap();
        assert_eq!(store.ai_second_count().await.unwrap(), 1);
        assert!(store
            .ai_claim("node:b", true, false, false, 10, lease, &[])
            .await
            .unwrap()
            .iter()
            .all(|j| j.track_id != ids[0]));
        // This computer's listener takes it.
        let second = store.ai_claim_second("this computer", 5).await.unwrap();
        assert_eq!(
            second.iter().map(|j| j.track_id).collect::<Vec<_>>(),
            [ids[0]]
        );
        assert_eq!(store.ai_second_count().await.unwrap(), 0);
        let held = store.ai_held(0).await.unwrap();
        let holder = held
            .iter()
            .find(|h| h.track_id == ids[0])
            .map(|h| h.worker.as_str());
        assert_eq!(holder, Some("this computer"));

        // The other was an instrumental: marked, and no longer missing lyrics.
        store.ai_finish(ids[1], None).await.unwrap();
        store.mark_instrumental(ids[1]).await.unwrap();
        let (instrumental, source): (i64, String) =
            sqlx::query_as("SELECT instrumental, source FROM lyrics WHERE track_id = ?")
                .bind(ids[1])
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!((instrumental, source.as_str()), (1, "none"));
        assert_eq!(store.track(ids[1]).await.unwrap().unwrap().instrumental, 1);
        let missing = store
            .problem_counts()
            .await
            .unwrap()
            .into_iter()
            .find(|(k, _)| *k == crate::ProblemKind::NoLyrics)
            .map(|(_, n)| n);
        assert_eq!(missing, Some(1), "only the unsure one still has no lyrics");

        // Unsure again: a low score, counted apart from failures.
        store
            .ai_give_up(ids[0], Some("scored too low (70%, under 85%)"))
            .await
            .unwrap();
        let counts = store.ai_counts(true, lease, &[]).await.unwrap();
        assert_eq!((counts.done, counts.low_score, counts.failed), (1, 1, 0));
    }

    #[tokio::test]
    async fn songs_asked_for_go_first_whatever_the_settings() {
        let store = Store::open_in_memory().await.unwrap();
        let song = |t: &str| ScannedTrack {
            path: format!("/m/{t}.flac"),
            title: t.into(),
            artist: Some("Band".into()),
            duration_ms: Some(200_000),
            ..Default::default()
        };
        store
            .save_scanned(&[song("Silent"), song("Words")])
            .await
            .unwrap();
        let ids: Vec<i64> = store
            .compare_rows()
            .await
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        store
            .save_read_lyrics(ids[0], 1, &NewLyrics::NONE)
            .await
            .unwrap();
        let plain = NewLyrics {
            synced: None,
            plain: Some("la la"),
            instrumental: false,
            source: "file",
        };
        store.save_read_lyrics(ids[1], 1, &plain).await.unwrap();
        let lease = 30 * 60 * 1000;
        // Neither kind is on, and LRCLIB hasn't been asked: nothing.
        assert!(store
            .ai_claim("pc", false, false, false, 10, lease, &[])
            .await
            .unwrap()
            .is_empty());
        assert_eq!(store.ai_ask(&[ids[1]], "sync").await.unwrap(), 1);
        // Windows' clock moves in steps of about 15 ms: asked in the same
        // step, the two would tie.
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        assert_eq!(store.ai_ask(&[ids[0]], "listen").await.unwrap(), 1);
        let batch = store
            .ai_claim("pc", true, true, true, 10, lease, &[])
            .await
            .unwrap();
        // Asked ones in the order asked, each once.
        assert_eq!(
            batch
                .iter()
                .map(|j| (j.track_id, j.kind.as_str()))
                .collect::<Vec<_>>(),
            [(ids[1], "sync"), (ids[0], "listen")]
        );
        assert_eq!(batch[0].plain.as_deref(), Some("la la"));
        // Done: no longer asked.
        store.ai_finish(ids[1], None).await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM ai_asked")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

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
        let counts = store.ai_counts(false, lease, &[]).await.unwrap();
        assert_eq!((counts.waiting_listen, counts.waiting_sync), (1, 1));
        // Songs in a genre left out aren't counted or handed out.
        let counts = store.ai_counts(false, lease, &ids[..2]).await.unwrap();
        assert_eq!((counts.waiting_listen, counts.waiting_sync), (0, 0));
        assert!(store
            .ai_claim("pc", true, true, false, 10, lease, &ids[..2])
            .await
            .unwrap()
            .is_empty());
        // Only new songs when sync is off.
        let batch = store
            .ai_claim("pc", true, false, false, 10, lease, &[])
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
            .ai_claim("other", true, true, false, 10, lease, &[])
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
                .ai_claim("late", true, true, false, 10, 0, &[])
                .await
                .unwrap()
                .len()
                >= 2
        );
        // Done and failed songs stay out; failed ones come back on retry.
        store.ai_finish(ids[0], None).await.unwrap();
        store.ai_finish(ids[1], Some("no vocals")).await.unwrap();
        let counts = store.ai_counts(false, lease, &[]).await.unwrap();
        assert_eq!((counts.done, counts.failed, counts.working), (1, 1, 0));
        assert_eq!(store.ai_retry_failed().await.unwrap(), 1);
        // Main isolates the vocals of a song for a node: only a node's claim
        // of prepared songs takes it, until the lease runs out.
        let batch = store
            .ai_claim("main", true, true, false, 1, lease, &[])
            .await
            .unwrap();
        let id = batch[0].track_id;
        store.ai_prepared(id).await.unwrap();
        assert_eq!(store.ai_prepared_count().await.unwrap(), 1);
        assert!(store
            .ai_claim("main", true, true, false, 10, lease, &[])
            .await
            .unwrap()
            .iter()
            .all(|j| j.track_id != id));
        let taken = store.ai_claim_prepared("node-1", 5).await.unwrap();
        assert_eq!(taken.iter().map(|j| j.track_id).collect::<Vec<_>>(), [id]);
        assert_eq!(
            store.ai_holder(id).await.unwrap().map(|h| h.0),
            Some("node-1".into())
        );
        assert!(store.ai_open_jobs().await.unwrap().contains(&id));
    }
}
