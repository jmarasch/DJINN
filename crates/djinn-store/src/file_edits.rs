//! What the library editor wrote into music files, so each approval can be
//! undone, and so a file DJINN only retagged keeps its sound analysis (the
//! `tracks_file_changed` trigger skips a file whose size and change time
//! are the ones recorded here).

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// One file written.
#[derive(Clone, Debug)]
pub struct NewFileEdit<'a> {
    pub batch: i64,
    pub track_id: Option<i64>,
    pub path: &'a str,
    /// "tags", "cover" or "rename".
    pub kind: &'a str,
    pub before: &'a str,
    pub after: &'a str,
    pub size: i64,
    pub mtime: i64,
    pub made_by: Option<i64>,
}

/// A file written, as recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct FileEdit {
    pub id: i64,
    pub batch: i64,
    pub track_id: Option<i64>,
    pub path: String,
    pub kind: String,
    pub before: String,
    pub after: String,
    pub made_at: i64,
    pub undone_at: Option<i64>,
    pub final_at: Option<i64>,
}

/// One approval: when, by whom, how many files, and whether it was undone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct FileBatch {
    pub batch: i64,
    pub made_at: i64,
    pub made_by: Option<String>,
    pub files: i64,
    pub kinds: String,
    pub undone_at: Option<i64>,
    /// Its files went out with the trash: it can't be undone.
    pub final_at: Option<i64>,
}

impl Store {
    /// A number for a new approval.
    pub async fn next_file_batch(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT coalesce(max(batch), 0) + 1 FROM file_edits")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Whether `batch` is the newest approval and holds only `kind` changes by
    /// `user`, none undone: a change sent in parts may join it.
    pub async fn can_add_to_batch(&self, batch: i64, kind: &str, user: i64) -> Result<bool> {
        let ok: bool = sqlx::query_scalar(
            "SELECT ? = (SELECT max(batch) FROM file_edits)
                AND EXISTS (SELECT 1 FROM file_edits WHERE batch = ?)
                AND NOT EXISTS (SELECT 1 FROM file_edits WHERE batch = ?
                    AND (kind != ? OR made_by IS NOT ? OR undone_at IS NOT NULL))",
        )
        .bind(batch)
        .bind(batch)
        .bind(batch)
        .bind(kind)
        .bind(user)
        .fetch_one(&self.pool)
        .await?;
        Ok(ok)
    }

    pub async fn record_file_edit(&self, edit: &NewFileEdit<'_>) -> Result<()> {
        sqlx::query(
            "INSERT INTO file_edits (batch, track_id, path, kind, before, after, size, mtime, made_at, made_by)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(edit.batch)
        .bind(edit.track_id)
        .bind(edit.path)
        .bind(edit.kind)
        .bind(edit.before)
        .bind(edit.after)
        .bind(edit.size)
        .bind(edit.mtime)
        .bind(now_ms())
        .bind(edit.made_by)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Recent approvals, newest first.
    pub async fn file_batches(&self, limit: i64) -> Result<Vec<FileBatch>> {
        Ok(sqlx::query_as(
            "SELECT f.batch, min(f.made_at) AS made_at, u.username AS made_by, count(*) AS files,
                    group_concat(DISTINCT f.kind) AS kinds, max(f.undone_at) AS undone_at,
                    max(f.final_at) AS final_at
             FROM file_edits f LEFT JOIN users u ON u.id = f.made_by
             GROUP BY f.batch ORDER BY f.batch DESC LIMIT ?",
        )
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?)
    }

    /// The files of one approval, last written first (the order to undo in).
    pub async fn file_batch(&self, batch: i64) -> Result<Vec<FileEdit>> {
        Ok(sqlx::query_as(
            "SELECT id, batch, track_id, path, kind, before, after, made_at, undone_at, final_at
             FROM file_edits WHERE batch = ? ORDER BY id DESC",
        )
        .bind(batch)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn mark_file_batch_undone(&self, batch: i64) -> Result<()> {
        sqlx::query("UPDATE file_edits SET undone_at = ? WHERE batch = ? AND undone_at IS NULL")
            .bind(now_ms())
            .bind(batch)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The trash under `trash` (a library folder's) was emptied: what went
    /// there can't be undone any more.
    pub async fn trash_emptied(&self, trash: &str) -> Result<()> {
        sqlx::query(
            "UPDATE file_edits SET final_at = ?1
             WHERE final_at IS NULL AND undone_at IS NULL AND kind IN ('delete', 'folder')
               AND substr(after, 1, length(?2)) = ?2",
        )
        .bind(now_ms())
        .bind(trash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The song at `path`, if the library has one there.
    pub async fn track_id_at(&self, path: &str) -> Result<Option<i64>> {
        Ok(sqlx::query_scalar("SELECT id FROM tracks WHERE path = ?")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// A song set aside comes back.
    pub async fn mark_restored(&self, track_id: i64) -> Result<()> {
        sqlx::query("UPDATE tracks SET removed_at = NULL WHERE id = ?")
            .bind(track_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A song's file moved (a rename): its record follows.
    ///
    /// Called once the file is at `path`, so another song still claiming that
    /// path is out of date (its file was deleted or moved away and no check
    /// has noticed yet): it's marked removed and its path set aside, so the
    /// song moved there keeps its own id, plays and likes.
    pub async fn move_track_path(&self, track_id: i64, path: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE tracks SET path = path || ' (gone, replaced by song ' || ? || ')',
                removed_at = coalesce(removed_at, ?)
             WHERE path = ? AND id != ?",
        )
        .bind(track_id)
        .bind(now_ms())
        .bind(path)
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE tracks SET path = ? WHERE id = ?")
            .bind(path)
            .bind(track_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScannedTrack;

    #[tokio::test]
    async fn a_file_only_retagged_by_djinn_keeps_its_analysis() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.flac".into(),
                title: "A".into(),
                size: 100,
                mtime: 1,
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = store.compare_rows().await.unwrap()[0].id;
        sqlx::query("INSERT INTO track_heard (track_id, heard, version) VALUES (?, NULL, 1)")
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        let heard = || async {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM track_heard")
                .fetch_one(&store.pool)
                .await
                .unwrap()
        };
        let batch = store.next_file_batch().await.unwrap();
        store
            .record_file_edit(&NewFileEdit {
                batch,
                track_id: Some(id),
                path: "/m/a.flac",
                kind: "tags",
                before: "{}",
                after: "[]",
                size: 120,
                mtime: 2,
                made_by: None,
            })
            .await
            .unwrap();
        // The scan sees the size and time DJINN left: nothing to redo.
        sqlx::query("UPDATE tracks SET size = 120, mtime = 2 WHERE id = ?")
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(heard().await, 1);
        // Something else changes the file: listened to again.
        sqlx::query("UPDATE tracks SET size = 130, mtime = 3 WHERE id = ?")
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(heard().await, 0);
        let batches = store.file_batches(10).await.unwrap();
        assert_eq!((batches[0].batch, batches[0].files), (batch, 1));
        store.mark_file_batch_undone(batch).await.unwrap();
        assert!(store.file_batch(batch).await.unwrap()[0]
            .undone_at
            .is_some());
    }

    #[tokio::test]
    async fn a_move_onto_a_gone_songs_path_sets_the_old_entry_aside() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[
                ScannedTrack {
                    path: "/a/x.mp3".into(),
                    title: "X".into(),
                    size: 1,
                    mtime: 1,
                    ..Default::default()
                },
                ScannedTrack {
                    path: "/b/x.mp3".into(),
                    title: "X copy".into(),
                    size: 1,
                    mtime: 1,
                    ..Default::default()
                },
            ])
            .await
            .unwrap();
        let id_of = |p: &'static str| {
            let store = store.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT id FROM tracks WHERE path = ?")
                    .bind(p)
                    .fetch_one(&store.pool)
                    .await
                    .unwrap()
            }
        };
        let (moving, stale) = (id_of("/a/x.mp3").await, id_of("/b/x.mp3").await);
        store.move_track_path(moving, "/b/x.mp3").await.unwrap();
        assert_eq!(id_of("/b/x.mp3").await, moving);
        let (path, removed): (String, Option<i64>) =
            sqlx::query_as("SELECT path, removed_at FROM tracks WHERE id = ?")
                .bind(stale)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_ne!(path, "/b/x.mp3");
        assert!(removed.is_some());
    }

    #[tokio::test]
    async fn only_the_newest_batch_of_the_same_change_can_be_added_to() {
        let store = Store::open_in_memory().await.unwrap();
        let sam = store
            .create_user("sam", "h", crate::Role::User)
            .await
            .unwrap()
            .id;
        let kim = store
            .create_user("kim", "h", crate::Role::User)
            .await
            .unwrap()
            .id;
        let edit = |batch, kind| NewFileEdit {
            batch,
            track_id: None,
            path: "/m/a.mp3",
            kind,
            before: "{}",
            after: "{}",
            size: 1,
            mtime: 1,
            made_by: Some(sam),
        };
        store.record_file_edit(&edit(1, "tags")).await.unwrap();
        store
            .record_file_edit(&edit(2, "mp3-length"))
            .await
            .unwrap();
        assert!(store.can_add_to_batch(2, "mp3-length", sam).await.unwrap());
        assert!(!store.can_add_to_batch(2, "mp3-length", kim).await.unwrap());
        assert!(!store.can_add_to_batch(1, "tags", sam).await.unwrap());
        assert!(!store.can_add_to_batch(2, "tags", sam).await.unwrap());
        assert!(!store.can_add_to_batch(3, "mp3-length", sam).await.unwrap());
    }
}
