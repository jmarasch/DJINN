//! Users, guests, and login sessions.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use sqlx::{Sqlite, Transaction};

use crate::{now_ms, Result, Store, StoreError, LISTENER_ROLE};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
    /// Joined with just a name while guest mode was on.
    Guest,
}

/// Whether an account can sign in.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum AccountStatus {
    Active,
    /// Signed up and waiting for the admin.
    Pending,
    /// Turned off by the admin.
    Disabled,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    /// Shown instead of `username` when set (guests).
    pub display_name: Option<String>,
    pub role: Role,
    /// Exempt from request limits.
    pub unlimited: bool,
    /// A guest device's hardware address, when known.
    pub mac: Option<String>,
    pub created_at: i64,
    /// The account's role (see `roles`); only used for `Role::User`.
    pub role_id: i64,
    pub status: AccountStatus,
    /// Two-step sign-in is on.
    pub two_step: bool,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }

    /// The name to show for this user.
    pub fn name(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.username)
    }
}

/// Account sessions end after this long unused.
const SESSION_IDLE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

macro_rules! user_columns {
    () => {
        "u.id, u.username, u.display_name, u.role, u.unlimited, u.mac, u.created_at, u.role_id, \
         u.status, u.totp_secret IS NOT NULL AS two_step"
    };
}

/// The same columns for RETURNING, which can't take table-qualified names.
macro_rules! returning_user {
    () => {
        " RETURNING id, username, display_name, role, unlimited, mac, created_at, role_id, status, \n         totp_secret IS NOT NULL AS two_step"
    };
}

/// A guest device as the admin sees it.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct GuestSummary {
    pub id: i64,
    /// Current name.
    pub name: String,
    pub mac: Option<String>,
    /// Every name this guest has used, most recent first, comma separated.
    pub names: String,
    pub last_ip: Option<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub requests: i64,
    pub unlimited: bool,
}

/// The user behind a session, and which session it is.
#[derive(Clone, Debug, PartialEq, FromRow)]
pub struct SessionUser {
    #[sqlx(flatten)]
    pub user: User,
    pub session_id: i64,
    /// When the session was last used (or started), Unix ms.
    pub last_seen_at: i64,
}

/// A signed-in device.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct SessionInfo {
    pub id: i64,
    pub created_at: i64,
    pub last_seen_at: i64,
    pub expires_at: i64,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateUserError {
    #[error("username is already taken")]
    Taken,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl Store {
    pub async fn create_user(
        &self,
        username: &str,
        password_hash: &str,
        role: Role,
    ) -> Result<User, CreateUserError> {
        self.create_account(
            username,
            password_hash,
            role,
            LISTENER_ROLE,
            AccountStatus::Active,
        )
        .await
    }

    /// Creates an account in a role, active or waiting for approval.
    pub async fn create_account(
        &self,
        username: &str,
        password_hash: &str,
        role: Role,
        role_id: i64,
        status: AccountStatus,
    ) -> Result<User, CreateUserError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from)?;
        match insert_account(&mut tx, username, password_hash, role, role_id, status).await {
            Ok(user) => {
                tx.commit().await.map_err(StoreError::from)?;
                Ok(user)
            }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(CreateUserError::Taken),
            Err(e) => Err(StoreError::from(e).into()),
        }
    }

    /// An account and its password hash, for login. Guests have no password
    /// and are never returned.
    pub async fn user_credentials(&self, username: &str) -> Result<Option<(User, String)>> {
        #[derive(FromRow)]
        struct Row {
            #[sqlx(flatten)]
            user: User,
            password_hash: String,
        }
        let row: Option<Row> = sqlx::query_as(concat!(
            "SELECT ",
            user_columns!(),
            ", u.password_hash FROM users u WHERE u.username = ? AND u.role != 'guest'"
        ))
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.user, r.password_hash)))
    }

    /// Accounts (not guests), A to Z.
    pub async fn users(&self) -> Result<Vec<User>> {
        let users = sqlx::query_as(concat!(
            "SELECT ",
            user_columns!(),
            " FROM users u WHERE u.role != 'guest' ORDER BY u.username"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(users)
    }

    pub async fn user(&self, id: i64) -> Result<Option<User>> {
        let user = sqlx::query_as(concat!(
            "SELECT ",
            user_columns!(),
            " FROM users u WHERE u.id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn admin_exists(&self) -> Result<bool> {
        let exists = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM users WHERE role = 'admin' AND status = 'active')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Deletes a user and, through cascades, their sessions and requests.
    pub async fn delete_user(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM users WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Moves an account to another role (see `roles`).
    pub async fn set_account_role(&self, id: i64, role_id: i64) -> Result<bool> {
        let result = sqlx::query("UPDATE users SET role_id = ? WHERE id = ? AND role != 'guest'")
            .bind(role_id)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Approves, turns off or turns back on an account. Anything but active
    /// signs it out everywhere.
    pub async fn set_status(&self, id: i64, status: AccountStatus) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query("UPDATE users SET status = ? WHERE id = ? AND role != 'guest'")
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if status != AccountStatus::Active {
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_role(&self, id: i64, role: Role) -> Result<bool> {
        let result = sqlx::query("UPDATE users SET role = ? WHERE id = ? AND role != 'guest'")
            .bind(role)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Sets a new password and signs the user out everywhere.
    pub async fn set_password(&self, id: i64, password_hash: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let result =
            sqlx::query("UPDATE users SET password_hash = ? WHERE id = ? AND role != 'guest'")
                .bind(password_hash)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        sqlx::query("DELETE FROM sessions WHERE user_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn admin_count(&self) -> Result<i64> {
        let count = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE role = 'admin' AND status = 'active'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn set_unlimited(&self, id: i64, unlimited: bool) -> Result<bool> {
        let result = sqlx::query("UPDATE users SET unlimited = ? WHERE id = ?")
            .bind(unlimited)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Signs in a guest by name. A device seen before (same MAC address)
    /// gets its old identity back under the new name; otherwise a new guest
    /// is created. Every join is recorded.
    pub async fn join_as_guest(
        &self,
        name: &str,
        mac: Option<&str>,
        ip: Option<&str>,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let now = now_ms();
        let existing: Option<i64> = match mac {
            Some(mac) => {
                sqlx::query_scalar("SELECT id FROM users WHERE mac = ? AND role = 'guest'")
                    .bind(mac)
                    .fetch_optional(&mut *tx)
                    .await?
            }
            None => None,
        };
        let user: User = match existing {
            Some(id) => {
                sqlx::query_as(concat!(
                    "UPDATE users SET display_name = ? WHERE id = ?",
                    returning_user!()
                ))
                .bind(name)
                .bind(id)
                .fetch_one(&mut *tx)
                .await?
            }
            // ':' can't appear in account usernames, so these never collide.
            None => {
                sqlx::query_as(concat!(
                    "INSERT INTO users (username, password_hash, role, created_at, display_name, mac)
                     VALUES ('guest:' || (SELECT coalesce(max(id), 0) + 1 FROM users), '', 'guest', ?, ?, ?)",
                    returning_user!()
                ))
                .bind(now)
                .bind(name)
                .bind(mac)
                .fetch_one(&mut *tx)
                .await?
            }
        };
        sqlx::query("INSERT INTO guest_joins (user_id, name, mac, ip, at) VALUES (?, ?, ?, ?, ?)")
            .bind(user.id)
            .bind(name)
            .bind(mac)
            .bind(ip)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(user)
    }

    /// The guest who last used this device, if any.
    pub async fn guest_by_mac(&self, mac: &str) -> Result<Option<User>> {
        let user = sqlx::query_as(concat!(
            "SELECT ",
            user_columns!(),
            " FROM users u WHERE u.mac = ? AND u.role = 'guest'"
        ))
        .bind(mac)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    /// Guest devices, most recently seen first.
    pub async fn guests(&self) -> Result<Vec<GuestSummary>> {
        let guests = sqlx::query_as(
            "SELECT u.id, coalesce(u.display_name, u.username) AS name, u.mac, u.unlimited,
                    coalesce((SELECT group_concat(name, ', ') FROM (
                        SELECT name, max(at) AS last, max(id) AS newest FROM guest_joins
                        WHERE user_id = u.id
                        GROUP BY name ORDER BY last DESC, newest DESC)), '') AS names,
                    (SELECT ip FROM guest_joins WHERE user_id = u.id ORDER BY at DESC, id DESC
                        LIMIT 1) AS last_ip,
                    u.created_at AS first_seen,
                    coalesce((SELECT max(at) FROM guest_joins WHERE user_id = u.id), u.created_at)
                        AS last_seen,
                    (SELECT count(*) FROM request_events WHERE user_id = u.id) AS requests
             FROM users u WHERE u.role = 'guest'
             ORDER BY last_seen DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(guests)
    }

    pub async fn create_session(
        &self,
        user_id: i64,
        token_hash: &[u8],
        expires_at: i64,
    ) -> Result<()> {
        self.create_session_from(user_id, token_hash, expires_at, None, None)
            .await
    }

    /// Starts a session, noting the device it's for.
    pub async fn create_session_from(
        &self,
        user_id: i64,
        token_hash: &[u8],
        expires_at: i64,
        ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        let now = now_ms();
        sqlx::query(
            "INSERT INTO sessions (user_id, token_hash, created_at, expires_at, last_seen_at, ip,
                                   user_agent)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(now)
        .bind(expires_at)
        .bind(now)
        .bind(ip)
        .bind(user_agent)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The active account owning an unexpired session.
    pub async fn session_user(&self, token_hash: &[u8]) -> Result<Option<User>> {
        Ok(self.session(token_hash).await?.map(|s| s.user))
    }

    /// The active account owning an unexpired session, with the session.
    pub async fn session(&self, token_hash: &[u8]) -> Result<Option<SessionUser>> {
        let found = sqlx::query_as(concat!(
            "SELECT ",
            user_columns!(),
            ", s.id AS session_id, coalesce(s.last_seen_at, s.created_at) AS last_seen_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > ? AND u.status = 'active'"
        ))
        .bind(token_hash)
        .bind(now_ms())
        .fetch_optional(&self.pool)
        .await?;
        Ok(found)
    }

    /// Notes that a session was just used, and from where. Account sessions
    /// last 30 days from their last use.
    pub async fn touch_session(
        &self,
        session_id: i64,
        ip: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET last_seen_at = ?, ip = coalesce(?, ip),
                                 user_agent = coalesce(?, user_agent),
                                 expires_at = max(expires_at, ?)
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(ip)
        .bind(user_agent)
        .bind(now_ms() + SESSION_IDLE_MS)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// An account's signed-in devices, most recently used first.
    pub async fn user_sessions(&self, user_id: i64) -> Result<Vec<SessionInfo>> {
        let sessions = sqlx::query_as(
            "SELECT id, created_at, coalesce(last_seen_at, created_at) AS last_seen_at,
                    expires_at, ip, user_agent
             FROM sessions WHERE user_id = ? AND expires_at > ?
             ORDER BY last_seen_at DESC, id DESC",
        )
        .bind(user_id)
        .bind(now_ms())
        .fetch_all(&self.pool)
        .await?;
        Ok(sessions)
    }

    /// Signs one device out. False if the account has no such session.
    pub async fn end_session(&self, user_id: i64, session_id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
            .bind(session_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Signs an account out everywhere except the session `keep`.
    pub async fn end_other_sessions(&self, user_id: i64, keep: Option<i64>) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id IS NOT ?")
            .bind(user_id)
            .bind(keep)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn extend_session(&self, token_hash: &[u8], expires_at: i64) -> Result<()> {
        sqlx::query("UPDATE sessions SET expires_at = max(expires_at, ?) WHERE token_hash = ?")
            .bind(expires_at)
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, token_hash: &[u8]) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(now_ms())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Inserts an account inside a transaction.
pub(crate) async fn insert_account(
    tx: &mut Transaction<'_, Sqlite>,
    username: &str,
    password_hash: &str,
    role: Role,
    role_id: i64,
    status: AccountStatus,
) -> std::result::Result<User, sqlx::Error> {
    sqlx::query_as(concat!(
        "INSERT INTO users (username, password_hash, role, created_at, role_id, status)
         VALUES (?, ?, ?, ?, ?, ?)",
        returning_user!()
    ))
    .bind(username)
    .bind(password_hash)
    .bind(role)
    .bind(now_ms())
    .bind(role_id)
    .bind(status)
    .fetch_one(&mut **tx)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_and_disabled_accounts_cannot_use_sessions() {
        let store = Store::open_in_memory().await.unwrap();
        let jo = store
            .create_account("jo", "h", Role::User, LISTENER_ROLE, AccountStatus::Pending)
            .await
            .unwrap();
        let soon = now_ms() + 60_000;
        store
            .create_session_from(jo.id, b"jo", soon, Some("10.0.0.2"), Some("Firefox"))
            .await
            .unwrap();
        assert_eq!(store.session(b"jo").await.unwrap(), None, "pending");
        assert!(store
            .set_status(jo.id, AccountStatus::Active)
            .await
            .unwrap());
        store
            .create_session_from(jo.id, b"jo2", soon, None, None)
            .await
            .unwrap();
        let found = store.session(b"jo2").await.unwrap().unwrap();
        assert_eq!(found.user.id, jo.id);
        assert_eq!(store.user_sessions(jo.id).await.unwrap().len(), 2);
        assert_eq!(
            store
                .end_other_sessions(jo.id, Some(found.session_id))
                .await
                .unwrap(),
            1
        );
        assert!(store
            .set_status(jo.id, AccountStatus::Disabled)
            .await
            .unwrap());
        assert!(store.user_sessions(jo.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn users_and_sessions() {
        let store = Store::open_in_memory().await.unwrap();
        assert!(!store.admin_exists().await.unwrap());
        let admin = store.create_user("Root", "h1", Role::Admin).await.unwrap();
        assert!(store.admin_exists().await.unwrap());
        assert!(matches!(
            store.create_user("root", "h2", Role::User).await,
            Err(CreateUserError::Taken)
        ));

        // Username lookup ignores case.
        let (found, hash) = store.user_credentials("ROOT").await.unwrap().unwrap();
        assert_eq!(
            (found.id, hash.as_str(), found.role),
            (admin.id, "h1", Role::Admin)
        );

        store
            .create_session(admin.id, b"live", now_ms() + 60_000)
            .await
            .unwrap();
        store
            .create_session(admin.id, b"old", now_ms() - 1)
            .await
            .unwrap();
        assert_eq!(
            store.session_user(b"live").await.unwrap().unwrap().id,
            admin.id
        );
        assert_eq!(store.session_user(b"old").await.unwrap(), None);
        assert_eq!(store.delete_expired_sessions().await.unwrap(), 1);

        assert!(store.set_unlimited(admin.id, true).await.unwrap());
        let user = store.create_user("dj", "old", Role::User).await.unwrap();
        store
            .create_session(user.id, b"dj-session", now_ms() + 60_000)
            .await
            .unwrap();
        assert!(store.set_role(user.id, Role::Admin).await.unwrap());
        assert_eq!(store.admin_count().await.unwrap(), 2);
        assert!(store.set_password(user.id, "new").await.unwrap());
        assert_eq!(
            store.session_user(b"dj-session").await.unwrap(),
            None,
            "signed out"
        );
        let (_, hash) = store.user_credentials("dj").await.unwrap().unwrap();
        assert_eq!(hash, "new");
        assert!(store.user(admin.id).await.unwrap().unwrap().unlimited);

        assert!(store.delete_user(admin.id).await.unwrap());
        assert_eq!(store.session_user(b"live").await.unwrap(), None);
    }

    #[tokio::test]
    async fn guests_are_remembered_by_mac() {
        let store = Store::open_in_memory().await.unwrap();
        let mac = "aa:bb:cc:dd:ee:ff";
        let first = store
            .join_as_guest("Sam", Some(mac), Some("192.168.0.5"))
            .await
            .unwrap();
        assert_eq!((first.role, first.name()), (Role::Guest, "Sam"));
        assert!(first.username.starts_with("guest:"));

        // Same device, new name: same identity.
        let again = store
            .join_as_guest("DJ Sam", Some(mac), Some("192.168.0.9"))
            .await
            .unwrap();
        assert_eq!(again.id, first.id);
        assert_eq!(again.name(), "DJ Sam");

        // No MAC (or a different one): a separate guest.
        let other = store
            .join_as_guest("Sam", None, Some("10.0.0.2"))
            .await
            .unwrap();
        assert_ne!(other.id, first.id);

        let guests = store.guests().await.unwrap();
        let sam = guests.iter().find(|g| g.id == first.id).unwrap();
        assert_eq!(sam.names, "DJ Sam, Sam");
        assert_eq!(sam.last_ip.as_deref(), Some("192.168.0.9"));
        assert_eq!(sam.mac.as_deref(), Some(mac));

        // Guests never log in with a password and aren't listed as accounts.
        assert!(store
            .user_credentials(&first.username)
            .await
            .unwrap()
            .is_none());
        assert!(store.users().await.unwrap().is_empty());
    }
}
