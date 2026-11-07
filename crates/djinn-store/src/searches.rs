//! Search history and the wishlist: what people looked for, what found
//! nothing, and songs they asked the admin to get.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// Keystrokes this close together, each extending the last, are one search.
const TYPING_MS: i64 = 15_000;
/// A request this soon after a search came from it.
const REQUEST_AFTER_MS: i64 = 5 * 60 * 1000;

/// A query and how people fared with it.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct SearchCount {
    pub query: String,
    pub searches: i64,
    /// Different people who searched it.
    pub people: i64,
    /// Results the latest time.
    pub results: i64,
    /// Times a request followed.
    pub requested: i64,
    pub last_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Wish {
    pub id: i64,
    pub artist: String,
    pub title: String,
    pub status: String,
    pub track_id: Option<i64>,
    /// People who asked.
    pub askers: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Lower case, letters and digits only, single spaces: "AC/DC" and "ac dc"
/// are one wish.
pub fn fold_wish(text: &str) -> String {
    let mut out = String::new();
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

impl Store {
    /// Notes a search. Typing grows one search, so a query that extends or
    /// trims this person's last one, moments ago, replaces it.
    pub async fn log_search(&self, user_id: Option<i64>, query: &str, results: i64) -> Result<()> {
        let now = now_ms();
        let last: Option<(i64, String)> = sqlx::query_as(
            "SELECT id, query FROM search_log WHERE user_id IS ? AND at > ?
             ORDER BY at DESC, id DESC LIMIT 1",
        )
        .bind(user_id)
        .bind(now - TYPING_MS)
        .fetch_optional(&self.pool)
        .await?;
        let lower = query.to_lowercase();
        match last {
            Some((id, before))
                if lower.starts_with(&before.to_lowercase())
                    || before.to_lowercase().starts_with(&lower) =>
            {
                sqlx::query("UPDATE search_log SET query = ?, results = ?, at = ? WHERE id = ?")
                    .bind(query)
                    .bind(results)
                    .bind(now)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
            }
            _ => {
                sqlx::query(
                    "INSERT INTO search_log (user_id, query, results, at) VALUES (?, ?, ?, ?)",
                )
                .bind(user_id)
                .bind(query)
                .bind(results)
                .bind(now)
                .execute(&self.pool)
                .await?;
            }
        }
        Ok(())
    }

    /// Marks this person's latest search, if recent, as having led to a
    /// request.
    pub async fn note_request_after_search(&self, user_id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE search_log SET requested = 1 WHERE id = (
                SELECT id FROM search_log WHERE user_id = ? AND at > ?
                ORDER BY at DESC, id DESC LIMIT 1)",
        )
        .bind(user_id)
        .bind(now_ms() - REQUEST_AFTER_MS)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Queries searched most from `since` on; with `nothing_found`, only
    /// ones whose latest search found nothing.
    pub async fn search_counts(
        &self,
        since: i64,
        nothing_found: bool,
        limit: i64,
    ) -> Result<Vec<SearchCount>> {
        Ok(sqlx::query_as(
            "SELECT s.query, count(*) AS searches, count(DISTINCT s.user_id) AS people,
                    (SELECT l.results FROM search_log l WHERE lower(l.query) = lower(s.query)
                     ORDER BY l.at DESC LIMIT 1) AS results,
                    sum(s.requested) AS requested, max(s.at) AS last_at
             FROM search_log s WHERE s.at >= ?
             GROUP BY lower(s.query)
             HAVING NOT ? OR results = 0
             ORDER BY searches DESC, last_at DESC LIMIT ?",
        )
        .bind(since)
        .bind(nothing_found)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn clear_search_log(&self) -> Result<u64> {
        let done = sqlx::query("DELETE FROM search_log")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// Adds a wish (or this person to an existing one, opening it again if
    /// it was ignored). Returns its id.
    pub async fn add_wish(&self, user_id: i64, artist: &str, title: &str) -> Result<i64> {
        let wanted = format!("{}\u{1f}{}", fold_wish(artist), fold_wish(title));
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO wishes (artist, title, wanted, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (wanted) DO UPDATE SET updated_at = excluded.updated_at,
                 status = CASE WHEN status = 'ignored' THEN 'open' ELSE status END
             RETURNING id",
        )
        .bind(artist.trim())
        .bind(title.trim())
        .bind(&wanted)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("INSERT OR IGNORE INTO wish_askers (wish_id, user_id, at) VALUES (?, ?, ?)")
            .bind(id)
            .bind(user_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Wishes with `status` ("open", "got", "ignored"), most asked first.
    pub async fn wishes(&self, status: &str) -> Result<Vec<Wish>> {
        Ok(sqlx::query_as(
            "SELECT w.id, w.artist, w.title, w.status, w.track_id,
                    (SELECT count(*) FROM wish_askers a WHERE a.wish_id = w.id) AS askers,
                    w.created_at, w.updated_at
             FROM wishes w WHERE w.status = ?
             ORDER BY askers DESC, w.updated_at DESC",
        )
        .bind(status)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Sets a wish's status (and the song that granted it). False if
    /// there's no such wish.
    pub async fn set_wish(&self, id: i64, status: &str, track_id: Option<i64>) -> Result<bool> {
        let done =
            sqlx::query("UPDATE wishes SET status = ?, track_id = ?, updated_at = ? WHERE id = ?")
                .bind(status)
                .bind(track_id)
                .bind(now_ms())
                .bind(id)
                .execute(&self.pool)
                .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn delete_wish(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM wishes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    #[tokio::test]
    async fn typing_is_one_search_and_requests_follow() {
        let store = Store::open_in_memory().await.unwrap();
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        for (q, n) in [("bea", 4), ("beat", 2), ("beatl", 0), ("beatles", 9)] {
            store.log_search(Some(sam), q, n).await.unwrap();
        }
        store.log_search(Some(sam), "zappa", 0).await.unwrap();
        store.note_request_after_search(sam).await.unwrap();
        store.log_search(None, "Zappa", 0).await.unwrap();

        let all = store.search_counts(0, false, 10).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            (all[0].query.to_lowercase().as_str(), all[0].searches),
            ("zappa", 2)
        );
        assert_eq!(all[0].requested, 1);
        assert_eq!((all[1].query.as_str(), all[1].results), ("beatles", 9));
        let nothing = store.search_counts(0, true, 10).await.unwrap();
        assert_eq!(nothing.len(), 1);
        assert_eq!(store.clear_search_log().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn wishes_merge_by_name_and_count_askers() {
        let store = Store::open_in_memory().await.unwrap();
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let kim = store.create_user("kim", "h", Role::User).await.unwrap().id;
        let a = store.add_wish(sam, "AC/DC", "Thunderstruck").await.unwrap();
        let b = store
            .add_wish(kim, "ac dc", "thunderstruck!")
            .await
            .unwrap();
        assert_eq!(a, b);
        store.add_wish(sam, "AC/DC", "Thunderstruck").await.unwrap();
        let open = store.wishes("open").await.unwrap();
        assert_eq!((open.len(), open[0].askers), (1, 2));

        assert!(store.set_wish(a, "ignored", None).await.unwrap());
        assert!(store.wishes("open").await.unwrap().is_empty());
        // Asked for again: open again.
        store.add_wish(kim, "AC/DC", "Thunderstruck").await.unwrap();
        assert_eq!(store.wishes("open").await.unwrap().len(), 1);
        assert!(store.delete_wish(a).await.unwrap());
        assert_eq!(fold_wish("  Guns N' Roses "), "guns n roses");
    }
}
