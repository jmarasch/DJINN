//! Music people upload in DJINN GO, waiting for review or already decided.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum SubmissionStatus {
    Waiting,
    Approved,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct Submission {
    pub id: i64,
    pub user_id: Option<i64>,
    /// Who sent it.
    pub uploader: Option<String>,
    pub file_name: String,
    #[serde(skip)]
    pub stored_name: String,
    pub size: i64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub duplicate_of: Option<i64>,
    /// "Artist - Title" of the song it looks like.
    pub duplicate_title: Option<String>,
    pub status: SubmissionStatus,
    pub note: Option<String>,
    #[serde(skip)]
    pub final_path: Option<String>,
    pub created_at: i64,
    pub decided_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct NewSubmission<'a> {
    pub user_id: i64,
    pub file_name: &'a str,
    pub stored_name: &'a str,
    pub size: i64,
    pub sha256: &'a [u8],
    pub title: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub duration_ms: Option<i64>,
    pub duplicate_of: Option<i64>,
}

macro_rules! submission_columns {
    () => {
        "s.id, s.user_id, coalesce(u.display_name, u.username) AS uploader, s.file_name, \
         s.stored_name, s.size, s.title, s.artist, s.album, s.duration_ms, s.duplicate_of, \
         (SELECT coalesce(t.artist || ' - ', '') || t.title FROM tracks t \
          WHERE t.id = s.duplicate_of) AS duplicate_title, \
         s.status, s.note, s.final_path, s.created_at, s.decided_at \
         FROM submissions s LEFT JOIN users u ON u.id = s.user_id"
    };
}

impl Store {
    pub async fn add_submission(&self, s: &NewSubmission<'_>) -> Result<i64> {
        let id = sqlx::query_scalar(
            "INSERT INTO submissions (user_id, file_name, stored_name, size, sha256, title,
                                      artist, album, duration_ms, duplicate_of, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(s.user_id)
        .bind(s.file_name)
        .bind(s.stored_name)
        .bind(s.size)
        .bind(s.sha256)
        .bind(s.title)
        .bind(s.artist)
        .bind(s.album)
        .bind(s.duration_ms)
        .bind(s.duplicate_of)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn submission(&self, id: i64) -> Result<Option<Submission>> {
        let found = sqlx::query_as(concat!("SELECT ", submission_columns!(), " WHERE s.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(found)
    }

    /// Someone's uploads, newest first.
    pub async fn user_submissions(&self, user_id: i64) -> Result<Vec<Submission>> {
        let list = sqlx::query_as(concat!(
            "SELECT ",
            submission_columns!(),
            " WHERE s.user_id = ? ORDER BY s.created_at DESC, s.id DESC LIMIT 500"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(list)
    }

    /// Uploads waiting for review, oldest first.
    pub async fn waiting_submissions(&self) -> Result<Vec<Submission>> {
        let list = sqlx::query_as(concat!(
            "SELECT ",
            submission_columns!(),
            " WHERE s.status = 'waiting' ORDER BY s.created_at, s.id"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(list)
    }

    /// Uploads of the same file still waiting or already in.
    pub async fn submission_with_hash(&self, sha256: &[u8]) -> Result<Option<i64>> {
        let id = sqlx::query_scalar(
            "SELECT id FROM submissions WHERE sha256 = ? AND status != 'declined' LIMIT 1",
        )
        .bind(sha256)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// How many of someone's uploads are waiting.
    pub async fn waiting_count(&self, user_id: i64) -> Result<i64> {
        let n = sqlx::query_scalar(
            "SELECT count(*) FROM submissions WHERE user_id = ? AND status = 'waiting'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    /// A playable song with this title and artist (ignoring case) and about
    /// this length, if the library has one.
    pub async fn same_song(
        &self,
        title: &str,
        artist: &str,
        duration_ms: Option<i64>,
    ) -> Result<Option<i64>> {
        let id = sqlx::query_scalar(
            "SELECT t.id FROM tracks t
             WHERE t.removed_at IS NULL AND t.title = ? COLLATE NOCASE
               AND t.artist = ? COLLATE NOCASE
               AND (? IS NULL OR t.duration_ms IS NULL OR abs(t.duration_ms - ?) <= 3000)
             LIMIT 1",
        )
        .bind(title)
        .bind(artist)
        .bind(duration_ms)
        .bind(duration_ms)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    /// Marks a waiting upload approved (with where it went) or turned down.
    /// False if it wasn't waiting.
    pub async fn decide_submission(
        &self,
        id: i64,
        status: SubmissionStatus,
        decided_by: Option<i64>,
        note: Option<&str>,
        final_path: Option<&str>,
    ) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE submissions SET status = ?, decided_by = ?, decided_at = ?, note = ?,
                                    final_path = ?
             WHERE id = ? AND status = 'waiting'",
        )
        .bind(status)
        .bind(decided_by)
        .bind(now_ms())
        .bind(note)
        .bind(final_path)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Forgets a waiting upload (the uploader took it back).
    pub async fn withdraw_submission(&self, user_id: i64, id: i64) -> Result<Option<String>> {
        let stored = sqlx::query_scalar(
            "DELETE FROM submissions WHERE id = ? AND user_id = ? AND status = 'waiting'
             RETURNING stored_name",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    #[tokio::test]
    async fn submissions_wait_then_get_decided() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.flac".into(),
                title: "Hello".into(),
                artist: Some("Band".into()),
                duration_ms: Some(200_000),
                ..ScannedTrack::default()
            }])
            .await
            .unwrap();
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;

        let dup = store
            .same_song("HELLO", "band", Some(201_500))
            .await
            .unwrap();
        assert!(dup.is_some());
        assert_eq!(
            store
                .same_song("Hello", "Band", Some(260_000))
                .await
                .unwrap(),
            None,
            "a different length is a different recording"
        );

        let id = store
            .add_submission(&NewSubmission {
                user_id: sam,
                file_name: "hello.flac",
                stored_name: "x.flac",
                size: 10,
                sha256: b"hash",
                title: Some("Hello"),
                artist: Some("Band"),
                album: None,
                duration_ms: Some(200_000),
                duplicate_of: dup,
            })
            .await
            .unwrap();
        assert_eq!(store.submission_with_hash(b"hash").await.unwrap(), Some(id));
        assert_eq!(store.waiting_count(sam).await.unwrap(), 1);
        let waiting = store.waiting_submissions().await.unwrap();
        assert_eq!(waiting[0].uploader.as_deref(), Some("sam"));
        assert_eq!(waiting[0].duplicate_title.as_deref(), Some("Band - Hello"));

        assert!(store
            .decide_submission(id, SubmissionStatus::Declined, None, Some("have it"), None)
            .await
            .unwrap());
        assert!(!store
            .decide_submission(id, SubmissionStatus::Approved, None, None, None)
            .await
            .unwrap());
        assert_eq!(store.submission_with_hash(b"hash").await.unwrap(), None);
        let mine = store.user_submissions(sam).await.unwrap();
        assert_eq!(
            (mine[0].status, mine[0].note.as_deref()),
            (SubmissionStatus::Declined, Some("have it"))
        );
        assert_eq!(store.withdraw_submission(sam, id).await.unwrap(), None);
    }
}
