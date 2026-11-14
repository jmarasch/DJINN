//! Locked accounts and the host recovery command (`djinn recover`).
//!
//! An account is locked after [`CODE_FAILURE_LIMIT`] wrong two-step codes in
//! a row (counted only after a right password, so a stranger can't lock
//! someone out). A locked account can't sign in until another admin unlocks
//! it. When no admin is left who can sign in, the host's recovery command
//! may unlock one, and its own tries are limited and kept in the database,
//! so restarting DJINN doesn't clear them.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, AccountStatus, Result, Store};

/// Wrong codes in a row that lock an account.
pub const CODE_FAILURE_LIMIT: i64 = 10;
/// Wrong usernames at the recovery command that lock it.
pub const RECOVERY_TRIES: i64 = 3;
/// How long the recovery command stays locked: the first time, the second,
/// and every time after.
pub const RECOVERY_LOCKS_MIN: [i64; 3] = [15, 30, 60];

const FAILED: &str = "recovery_failed";
const STRIKES: &str = "recovery_strikes";
const LOCKED_UNTIL: &str = "recovery_locked_until";
/// Setting key: when and for whom recovery was last used, for the banner.
pub const RECOVERY_USED: &str = "recovery_used";

/// An admin account and whether it can sign in.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct AdminAccount {
    pub id: i64,
    pub username: String,
    pub status: AccountStatus,
    pub locked: bool,
}

impl AdminAccount {
    /// Can't sign in: locked, or turned off or waiting.
    pub fn shut_out(&self) -> bool {
        self.locked || self.status != AccountStatus::Active
    }
}

/// Who the recovery command may help: the account named exactly, when it is
/// shut out and no other admin can sign in.
pub fn recoverable<'a>(admins: &'a [AdminAccount], username: &str) -> Option<&'a AdminAccount> {
    let target = admins.iter().find(|a| a.username == username)?;
    let others_can_sign_in = admins.iter().any(|a| a.id != target.id && !a.shut_out());
    (target.shut_out() && !others_can_sign_in).then_some(target)
}

/// When recovery was used last.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryUsed {
    pub at: i64,
    pub username: String,
}

impl Store {
    /// Counts a wrong two-step code for an account. True when that locks it.
    pub async fn note_code_failure(&self, user_id: i64) -> Result<bool> {
        let mut tx = self.write().await?;
        sqlx::query(
            "INSERT INTO account_security (user_id, failures) VALUES (?, 1)
             ON CONFLICT (user_id) DO UPDATE SET failures = failures + 1",
        )
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        let locked = sqlx::query(
            "UPDATE account_security SET locked_at = ?
             WHERE user_id = ? AND locked_at IS NULL AND failures >= ?",
        )
        .bind(now_ms())
        .bind(user_id)
        .bind(CODE_FAILURE_LIMIT)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(locked.rows_affected() > 0)
    }

    /// A right code: the count starts over.
    pub async fn clear_code_failures(&self, user_id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE account_security SET failures = 0 WHERE user_id = ? AND locked_at IS NULL",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn is_locked(&self, user_id: i64) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM account_security WHERE user_id = ? AND locked_at IS NOT NULL)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    /// Lets a locked account sign in again. False if it wasn't locked.
    pub async fn unlock_account(&self, user_id: i64) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE account_security SET failures = 0, locked_at = NULL
             WHERE user_id = ? AND (locked_at IS NOT NULL OR failures > 0)",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Every admin account, with whether it's locked.
    pub async fn admin_accounts(&self) -> Result<Vec<AdminAccount>> {
        Ok(sqlx::query_as(
            "SELECT u.id, u.username, u.status,
                    coalesce(s.locked_at IS NOT NULL, 0) AS locked
             FROM users u LEFT JOIN account_security s ON s.user_id = u.id
             WHERE u.role = 'admin' ORDER BY u.id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn must_change_password(&self, user_id: i64) -> Result<bool> {
        Ok(sqlx::query_scalar(
            "SELECT coalesce((SELECT must_change_password FROM account_security WHERE user_id = ?), 0)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn set_must_change_password(&self, user_id: i64, must: bool) -> Result<()> {
        sqlx::query(
            "INSERT INTO account_security (user_id, must_change_password) VALUES (?, ?)
             ON CONFLICT (user_id) DO UPDATE SET must_change_password = excluded.must_change_password",
        )
        .bind(user_id)
        .bind(must)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Milliseconds until the recovery command may be tried again; None if now.
    pub async fn recovery_wait(&self) -> Result<Option<i64>> {
        let until: i64 = self
            .get_setting(LOCKED_UNTIL)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok((until > now_ms()).then(|| until - now_ms()))
    }

    /// A wrong try at the recovery command. Returns how many minutes it is
    /// locked for when that was the last try allowed.
    pub async fn recovery_failed(&self) -> Result<Option<i64>> {
        let failed: i64 = self
            .get_setting(FAILED)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
            + 1;
        if failed < RECOVERY_TRIES {
            self.set_setting(FAILED, &failed.to_string()).await?;
            return Ok(None);
        }
        let strikes: i64 = self
            .get_setting(STRIKES)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let index = usize::try_from(strikes)
            .unwrap_or(usize::MAX)
            .min(RECOVERY_LOCKS_MIN.len() - 1);
        let minutes = RECOVERY_LOCKS_MIN[index];
        self.set_setting(FAILED, "0").await?;
        self.set_setting(STRIKES, &(strikes + 1).to_string())
            .await?;
        self.set_setting(LOCKED_UNTIL, &(now_ms() + minutes * 60_000).to_string())
            .await?;
        Ok(Some(minutes))
    }

    /// Recovery worked: unlocks the account, makes it active, gives it a new
    /// password that must be changed, turns its two-step off (to be set up
    /// again), signs it out everywhere, and notes the use for the banner.
    pub async fn recover_account(
        &self,
        user_id: i64,
        username: &str,
        password_hash: &str,
    ) -> Result<()> {
        self.set_password(user_id, password_hash).await?;
        self.clear_two_step(user_id).await?;
        self.set_status(user_id, AccountStatus::Active).await?;
        self.unlock_account(user_id).await?;
        self.set_must_change_password(user_id, true).await?;
        self.set_setting(FAILED, "0").await?;
        self.set_setting(STRIKES, "0").await?;
        self.set_setting(LOCKED_UNTIL, "0").await?;
        let used = RecoveryUsed {
            at: now_ms(),
            username: username.to_owned(),
        };
        if let Ok(json) = note_text(&used) {
            self.set_setting(RECOVERY_USED, &json).await?;
        }
        Ok(())
    }

    pub async fn recovery_used(&self) -> Result<Option<RecoveryUsed>> {
        Ok(self
            .get_setting(RECOVERY_USED)
            .await?
            .and_then(|v| parse_note(&v)))
    }

    pub async fn dismiss_recovery_note(&self) -> Result<()> {
        sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(RECOVERY_USED)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

// The banner note is two plain fields; written by hand so this crate needs no JSON dependency.
fn note_text(used: &RecoveryUsed) -> std::result::Result<String, ()> {
    if used.username.contains(['\t', '\n']) {
        return Err(());
    }
    Ok(format!("{}\t{}", used.at, used.username))
}

fn parse_note(text: &str) -> Option<RecoveryUsed> {
    let (at, username) = text.split_once('\t')?;
    Some(RecoveryUsed {
        at: at.parse().ok()?,
        username: username.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    fn admin(id: i64, name: &str, status: AccountStatus, locked: bool) -> AdminAccount {
        AdminAccount {
            id,
            username: name.into(),
            status,
            locked,
        }
    }

    #[test]
    fn only_a_shut_out_admin_with_no_other_way_in_can_be_recovered() {
        use AccountStatus::{Active, Disabled};
        let admins = [
            admin(1, "ann", Active, true),
            admin(2, "bob", Active, false),
        ];
        // Bob can still sign in: the command refuses everyone.
        assert!(recoverable(&admins, "ann").is_none());
        let admins = [
            admin(1, "ann", Active, true),
            admin(2, "bob", Disabled, false),
        ];
        assert_eq!(recoverable(&admins, "ann").map(|a| a.id), Some(1));
        assert_eq!(recoverable(&admins, "bob").map(|a| a.id), Some(2));
        // The name must be typed exactly, and the account must be shut out.
        assert!(recoverable(&admins, "Ann").is_none());
        assert!(recoverable(&admins, "nobody").is_none());
        let fine = [admin(1, "ann", Active, false)];
        assert!(recoverable(&fine, "ann").is_none());
        // A single locked admin qualifies.
        let alone = [admin(1, "ann", Active, true)];
        assert!(recoverable(&alone, "ann").is_some());
    }

    #[tokio::test]
    async fn wrong_codes_lock_an_account_until_unlocked() {
        let store = Store::open_in_memory().await.unwrap();
        let user = store.create_user("ann", "h", Role::Admin).await.unwrap();
        for _ in 1..CODE_FAILURE_LIMIT {
            assert!(!store.note_code_failure(user.id).await.unwrap());
        }
        assert!(store.note_code_failure(user.id).await.unwrap());
        assert!(store.is_locked(user.id).await.unwrap());
        assert!(store.admin_accounts().await.unwrap()[0].locked);
        assert!(store.unlock_account(user.id).await.unwrap());
        assert!(!store.is_locked(user.id).await.unwrap());
        // A right code in between starts the count over.
        for _ in 1..CODE_FAILURE_LIMIT {
            store.note_code_failure(user.id).await.unwrap();
        }
        store.clear_code_failures(user.id).await.unwrap();
        assert!(!store.note_code_failure(user.id).await.unwrap());
    }

    #[tokio::test]
    async fn the_recovery_command_locks_after_three_wrong_tries_and_for_longer_each_time() {
        let store = Store::open_in_memory().await.unwrap();
        assert_eq!(store.recovery_wait().await.unwrap(), None);
        assert_eq!(store.recovery_failed().await.unwrap(), None);
        assert_eq!(store.recovery_failed().await.unwrap(), None);
        assert_eq!(store.recovery_failed().await.unwrap(), Some(15));
        assert!(store.recovery_wait().await.unwrap().is_some());
        // Later rounds last longer, then stay at the longest.
        for expected in [30, 60, 60] {
            store.set_setting(LOCKED_UNTIL, "0").await.unwrap();
            store.recovery_failed().await.unwrap();
            store.recovery_failed().await.unwrap();
            assert_eq!(store.recovery_failed().await.unwrap(), Some(expected));
        }
    }

    #[tokio::test]
    async fn recovering_resets_the_account_and_leaves_a_note() {
        let store = Store::open_in_memory().await.unwrap();
        let user = store.create_user("ann", "old", Role::Admin).await.unwrap();
        store
            .set_status(user.id, AccountStatus::Disabled)
            .await
            .unwrap();
        for _ in 0..CODE_FAILURE_LIMIT {
            store.note_code_failure(user.id).await.unwrap();
        }
        store
            .recover_account(user.id, "ann", "new-hash")
            .await
            .unwrap();
        assert!(!store.is_locked(user.id).await.unwrap());
        assert!(store.must_change_password(user.id).await.unwrap());
        let (found, hash) = store.user_credentials("ann").await.unwrap().unwrap();
        assert_eq!(
            (found.status, hash.as_str()),
            (AccountStatus::Active, "new-hash")
        );
        let used = store.recovery_used().await.unwrap().unwrap();
        assert_eq!(used.username, "ann");
        store.dismiss_recovery_note().await.unwrap();
        assert!(store.recovery_used().await.unwrap().is_none());
    }
}
