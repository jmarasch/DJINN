//! Albums brought into the library editor to work on before they go into
//! the library. The files wait in DJINN's own folder; these rows say which
//! import each belongs to.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// A file waiting in an import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct ImportFile {
    pub id: i64,
    pub batch: i64,
    /// The name it was brought in with.
    pub name: String,
    /// Where it waits.
    pub path: String,
    pub size: i64,
    pub added_at: i64,
}

/// An import: how many files it holds, and when it was started.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct ImportBatch {
    pub batch: i64,
    pub files: i64,
    pub started_at: i64,
}

/// Ids as a JSON list, for `json_each`.
fn id_list(ids: &[i64]) -> String {
    format!(
        "[{}]",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    )
}

impl Store {
    /// A number for a new import.
    pub async fn next_import_batch(&self) -> Result<i64> {
        Ok(
            sqlx::query_scalar("SELECT coalesce(max(batch), 0) + 1 FROM import_files")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    /// Notes a file brought into `batch`; returns its id.
    pub async fn add_import_file(
        &self,
        batch: i64,
        name: &str,
        path: &str,
        size: i64,
        added_by: Option<i64>,
    ) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "INSERT INTO import_files (batch, name, path, size, added_at, added_by)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(batch)
        .bind(name)
        .bind(path)
        .bind(size)
        .bind(now_ms())
        .bind(added_by)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Every import with files in it, newest first.
    pub async fn import_batches(&self) -> Result<Vec<ImportBatch>> {
        Ok(sqlx::query_as(
            "SELECT batch, count(*) AS files, min(added_at) AS started_at
             FROM import_files GROUP BY batch ORDER BY batch DESC",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// The files of an import, in the order they came.
    pub async fn import_files(&self, batch: i64) -> Result<Vec<ImportFile>> {
        Ok(sqlx::query_as(
            "SELECT id, batch, name, path, size, added_at FROM import_files
             WHERE batch = ? ORDER BY id",
        )
        .bind(batch)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Files waiting in imports, by id (those that exist).
    pub async fn import_files_by_id(&self, ids: &[i64]) -> Result<Vec<ImportFile>> {
        let ids = id_list(ids);
        Ok(sqlx::query_as(
            "SELECT id, batch, name, path, size, added_at FROM import_files
             WHERE id IN (SELECT value FROM json_each(?)) ORDER BY id",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Forgets files (added to the library, or thrown away).
    pub async fn remove_import_files(&self, ids: &[i64]) -> Result<()> {
        let ids = id_list(ids);
        sqlx::query("DELETE FROM import_files WHERE id IN (SELECT value FROM json_each(?))")
            .bind(ids)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[tokio::test]
    async fn imports_keep_their_files_apart_until_removed() {
        let store = Store::open_in_memory().await.unwrap();
        let first = store.next_import_batch().await.unwrap();
        let a = store
            .add_import_file(first, "01 One.flac", "/data/imports/1/a.flac", 10, None)
            .await
            .unwrap();
        let b = store
            .add_import_file(first, "02 Two.flac", "/data/imports/1/b.flac", 20, None)
            .await
            .unwrap();
        let second = store.next_import_batch().await.unwrap();
        assert_eq!(second, first + 1);
        store
            .add_import_file(second, "x.mp3", "/data/imports/2/x.mp3", 5, None)
            .await
            .unwrap();
        let batches = store.import_batches().await.unwrap();
        assert_eq!(
            batches
                .iter()
                .map(|b| (b.batch, b.files))
                .collect::<Vec<_>>(),
            [(second, 1), (first, 2)]
        );
        let files = store.import_files(first).await.unwrap();
        assert_eq!(files.iter().map(|f| f.id).collect::<Vec<_>>(), [a, b]);
        assert_eq!(store.import_files_by_id(&[b, 999]).await.unwrap().len(), 1);
        store.remove_import_files(&[a, b]).await.unwrap();
        assert!(store.import_files(first).await.unwrap().is_empty());
        assert_eq!(store.import_batches().await.unwrap().len(), 1);
    }
}
