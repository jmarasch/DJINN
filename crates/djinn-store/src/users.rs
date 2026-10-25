//! Users and login sessions.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, Result, Store, StoreError};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
    /// Party guest (Phase 4); not issued yet.
    Guest,
}

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
    pub created_at: i64,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == Role::Admin
    }
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
        let result = sqlx::query_as(
            "INSERT INTO users (username, password_hash, role, created_at) VALUES (?, ?, ?, ?)
             RETURNING id, username, role, created_at",
        )
        .bind(username)
        .bind(password_hash)
        .bind(role)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await;
        match result {
            Ok(user) => Ok(user),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(CreateUserError::Taken),
            Err(e) => Err(StoreError::from(e).into()),
        }
    }

    /// The user and their password hash, for login.
    pub async fn user_credentials(&self, username: &str) -> Result<Option<(User, String)>> {
        #[derive(FromRow)]
        struct Row {
            #[sqlx(flatten)]
            user: User,
            password_hash: String,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT id, username, role, created_at, password_hash FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.user, r.password_hash)))
    }

    pub async fn users(&self) -> Result<Vec<User>> {
        let users =
            sqlx::query_as("SELECT id, username, role, created_at FROM users ORDER BY username")
                .fetch_all(&self.pool)
                .await?;
        Ok(users)
    }

    pub async fn admin_exists(&self) -> Result<bool> {
        let exists = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE role = 'admin')")
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

    pub async fn create_session(
        &self,
        user_id: i64,
        token_hash: &[u8],
        expires_at: i64,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (user_id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(token_hash)
        .bind(now_ms())
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The user owning an unexpired session.
    pub async fn session_user(&self, token_hash: &[u8]) -> Result<Option<User>> {
        let user = sqlx::query_as(
            "SELECT u.id, u.username, u.role, u.created_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(token_hash)
        .bind(now_ms())
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
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

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(store.delete_user(admin.id).await.unwrap());
        assert_eq!(store.session_user(b"live").await.unwrap(), None);
    }
}
