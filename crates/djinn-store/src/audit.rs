//! The audit trail of protected areas (who unlocked what, and what changed).

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// Most records kept, so a busy or hostile admin session can't fill the disk.
const AUDIT_ROWS: i64 = 200_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub at: i64,
    pub user_id: Option<i64>,
    pub username: String,
    pub ip: Option<String>,
    pub kind: String,
    pub detail: String,
}

pub struct NewAudit<'a> {
    pub user_id: Option<i64>,
    pub username: &'a str,
    pub ip: Option<&'a str>,
    pub kind: &'a str,
    pub detail: &'a str,
}

impl Store {
    pub async fn log_audit(&self, entry: &NewAudit<'_>) -> Result<()> {
        let mut tx = self.write().await?;
        sqlx::query(
            "INSERT INTO audit_log (at, user_id, username, ip, kind, detail)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(now_ms())
        .bind(entry.user_id)
        .bind(entry.username)
        .bind(entry.ip)
        .bind(entry.kind)
        .bind(entry.detail)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM audit_log WHERE id <= (SELECT max(id) FROM audit_log) - ?")
            .bind(AUDIT_ROWS)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// A page of the trail, newest first, and how many records there are.
    pub async fn audit_page(&self, offset: i64, limit: i64) -> Result<(Vec<AuditEntry>, i64)> {
        let rows = sqlx::query_as(
            "SELECT id, at, user_id, username, ip, kind, detail
             FROM audit_log ORDER BY at DESC, id DESC LIMIT ? OFFSET ?",
        )
        .bind(limit.max(0))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        Ok((rows, self.audit_count().await?))
    }

    pub async fn audit_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM audit_log")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Deletes every record; how many there were.
    pub async fn purge_audit(&self) -> Result<u64> {
        let done = sqlx::query("DELETE FROM audit_log")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_are_kept_newest_first_and_can_be_purged() {
        let store = Store::open_in_memory().await.unwrap();
        for kind in ["entered", "left"] {
            store
                .log_audit(&NewAudit {
                    user_id: None,
                    username: "admin",
                    ip: Some("10.0.0.2"),
                    kind,
                    detail: "",
                })
                .await
                .unwrap();
        }
        let (page, total) = store.audit_page(0, 10).await.unwrap();
        assert_eq!((page.len(), total), (2, 2));
        assert_eq!(page[0].kind, "left");
        assert_eq!(store.purge_audit().await.unwrap(), 2);
        assert_eq!(store.audit_count().await.unwrap(), 0);
    }
}
