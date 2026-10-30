//! Two-step sign-in (TOTP secrets and recovery codes) and the sign-in log.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// How long sign-ins are kept.
const SIGN_IN_DAYS: i64 = 90;

/// One sign-in attempt, for the log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct SignIn {
    pub id: i64,
    pub at: i64,
    pub user_id: Option<i64>,
    pub username: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    /// "party" or "go".
    pub via: String,
    /// "ok", or why it failed (see the migration).
    pub outcome: String,
}

#[derive(Clone, Debug)]
pub struct NewSignIn<'a> {
    pub user_id: Option<i64>,
    pub username: &'a str,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub via: &'a str,
    pub outcome: &'a str,
}

impl Store {
    /// Records a sign-in attempt, and forgets ones older than 90 days.
    pub async fn log_sign_in(&self, s: &NewSignIn<'_>) -> Result<()> {
        let now = now_ms();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO sign_ins (at, user_id, username, ip, user_agent, via, outcome)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(now)
        .bind(s.user_id)
        .bind(s.username)
        .bind(s.ip)
        .bind(s.user_agent)
        .bind(s.via)
        .bind(s.outcome)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM sign_ins WHERE at < ?")
            .bind(now - SIGN_IN_DAYS * 24 * 60 * 60 * 1000)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The latest sign-in attempts, newest first.
    pub async fn sign_ins(&self, limit: i64) -> Result<Vec<SignIn>> {
        let rows = sqlx::query_as(
            "SELECT id, at, user_id, username, ip, user_agent, via, outcome
             FROM sign_ins ORDER BY at DESC, id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Keeps a new two-step secret until the user confirms a code from it.
    pub async fn begin_two_step(&self, user_id: i64, secret: &str) -> Result<()> {
        sqlx::query("UPDATE users SET totp_pending = ? WHERE id = ?")
            .bind(secret)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The secret waiting for confirmation, if any.
    pub async fn pending_two_step(&self, user_id: i64) -> Result<Option<String>> {
        let secret = sqlx::query_scalar("SELECT totp_pending FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten();
        Ok(secret)
    }

    /// Turns two-step sign-in on with the pending secret, `step` as the
    /// code already used, and new recovery codes (their hashes). Signs out
    /// every session but `keep`.
    pub async fn confirm_two_step(
        &self,
        user_id: i64,
        step: i64,
        recovery_hashes: &[Vec<u8>],
        keep: Option<i64>,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "UPDATE users SET totp_secret = totp_pending, totp_pending = NULL, totp_last_step = ?
             WHERE id = ? AND totp_pending IS NOT NULL",
        )
        .bind(step)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        for hash in recovery_hashes {
            sqlx::query("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?, ?)")
                .bind(user_id)
                .bind(hash)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id IS NOT ?")
            .bind(user_id)
            .bind(keep)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// The account's two-step secret, when it's on.
    pub async fn two_step_secret(&self, user_id: i64) -> Result<Option<String>> {
        let secret = sqlx::query_scalar("SELECT totp_secret FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?
            .flatten();
        Ok(secret)
    }

    /// Accepts the code for time step `step` once: false if that step (or
    /// a later one) was already used.
    pub async fn use_two_step_code(&self, user_id: i64, step: i64) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE users SET totp_last_step = ?
             WHERE id = ? AND (totp_last_step IS NULL OR totp_last_step < ?)",
        )
        .bind(step)
        .bind(user_id)
        .bind(step)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Uses up a recovery code. False if it's wrong or already used.
    pub async fn use_recovery_code(&self, user_id: i64, code_hash: &[u8]) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE recovery_codes SET used_at = ?
             WHERE user_id = ? AND code_hash = ? AND used_at IS NULL",
        )
        .bind(now_ms())
        .bind(user_id)
        .bind(code_hash)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Recovery codes not used yet.
    pub async fn recovery_codes_left(&self, user_id: i64) -> Result<i64> {
        let left = sqlx::query_scalar(
            "SELECT count(*) FROM recovery_codes WHERE user_id = ? AND used_at IS NULL",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(left)
    }

    /// Turns two-step sign-in off and forgets its recovery codes.
    pub async fn clear_two_step(&self, user_id: i64) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "UPDATE users SET totp_secret = NULL, totp_pending = NULL, totp_last_step = NULL
             WHERE id = ?",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = ?")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    #[tokio::test]
    async fn two_step_codes_and_sign_ins() {
        let store = Store::open_in_memory().await.unwrap();
        let sam = store.create_user("sam", "h", Role::User).await.unwrap();
        assert!(!sam.two_step);

        store.begin_two_step(sam.id, "SECRET").await.unwrap();
        assert_eq!(
            store.pending_two_step(sam.id).await.unwrap().as_deref(),
            Some("SECRET")
        );
        assert_eq!(store.two_step_secret(sam.id).await.unwrap(), None);
        let codes = [b"a".to_vec(), b"b".to_vec()];
        assert!(store
            .confirm_two_step(sam.id, 100, &codes, None)
            .await
            .unwrap());
        assert!(store.user(sam.id).await.unwrap().unwrap().two_step);
        assert_eq!(
            store.two_step_secret(sam.id).await.unwrap().as_deref(),
            Some("SECRET")
        );

        // Each time step once, and never an older one.
        assert!(!store.use_two_step_code(sam.id, 100).await.unwrap());
        assert!(store.use_two_step_code(sam.id, 101).await.unwrap());
        assert!(!store.use_two_step_code(sam.id, 99).await.unwrap());

        assert!(store.use_recovery_code(sam.id, b"a").await.unwrap());
        assert!(!store.use_recovery_code(sam.id, b"a").await.unwrap());
        assert_eq!(store.recovery_codes_left(sam.id).await.unwrap(), 1);

        assert!(store.clear_two_step(sam.id).await.unwrap());
        assert!(!store.user(sam.id).await.unwrap().unwrap().two_step);
        assert_eq!(store.recovery_codes_left(sam.id).await.unwrap(), 0);

        store
            .log_sign_in(&NewSignIn {
                user_id: Some(sam.id),
                username: "sam",
                ip: Some("10.0.0.2"),
                user_agent: None,
                via: "go",
                outcome: "wrong_password",
            })
            .await
            .unwrap();
        let log = store.sign_ins(10).await.unwrap();
        assert_eq!(
            (log[0].username.as_str(), log[0].outcome.as_str()),
            ("sam", "wrong_password")
        );
    }
}
