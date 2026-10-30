//! Roles (what an account may do) and invite links.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{now_ms, AccountStatus, Result, Role, Store, StoreError, User};

/// The built-in role every account starts in. Editable, never deleted.
pub const LISTENER_ROLE: i64 = 1;

/// What an account may do. Admins may do everything; guests only what
/// guest mode allows, which isn't a role. The default allows nothing but
/// two GO streams, like a new role in the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, FromRow)]
#[serde(default)]
pub struct Permissions {
    /// DJINN GO: personal listening.
    pub go: bool,
    /// GO from outside the home network.
    pub go_remote: bool,
    /// GO streams at once; 0 is no limit.
    pub max_streams: i64,
    pub downloads: bool,
    pub share_playlists: bool,
    pub submit_music: bool,
    pub submit_without_review: bool,
    /// Party requests skip the per-person limit and the queue cap.
    pub unlimited_requests: bool,
    /// Must use two-step sign-in for DJINN GO.
    pub require_two_step: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            go: false,
            go_remote: false,
            max_streams: 2,
            downloads: false,
            share_playlists: false,
            submit_music: false,
            submit_without_review: false,
            unlimited_requests: false,
            require_two_step: false,
        }
    }
}

impl Permissions {
    pub fn all() -> Self {
        Self {
            go: true,
            go_remote: true,
            max_streams: 0,
            downloads: true,
            share_playlists: true,
            submit_music: true,
            submit_without_review: true,
            unlimited_requests: true,
            // Admins' requirement is the `admin_two_step` setting.
            require_two_step: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct AccountRole {
    pub id: i64,
    pub name: String,
    pub builtin: bool,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub permissions: Permissions,
    /// Accounts in this role (admins aren't counted).
    pub accounts: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum RoleError {
    #[error("a role with that name already exists")]
    Taken,
    #[error("the Listener role can't be deleted")]
    Builtin,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for RoleError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(e) if e.is_unique_violation() => Self::Taken,
            e => Self::Store(e.into()),
        }
    }
}

macro_rules! permission_columns {
    () => {
        "r.go, r.go_remote, r.max_streams, r.downloads, r.share_playlists, r.submit_music, \
         r.submit_without_review, r.unlimited_requests, r.require_two_step"
    };
}

macro_rules! role_columns {
    () => {
        concat!(
            "r.id, r.name, r.builtin, ",
            permission_columns!(),
            ", (SELECT count(*) FROM users u WHERE u.role_id = r.id AND u.role = 'user') AS accounts"
        )
    };
}

/// An invite link as the admin sees it (the token itself is never kept).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct Invite {
    pub id: i64,
    pub role_id: i64,
    pub role_name: String,
    pub note: String,
    /// None: no limit.
    pub uses_left: Option<i64>,
    pub used: i64,
    pub created_at: i64,
    /// None: never.
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct NewInvite<'a> {
    pub token_hash: &'a [u8],
    pub role_id: i64,
    pub note: &'a str,
    pub uses_left: Option<i64>,
    pub expires_at: Option<i64>,
    pub created_by: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SignupError {
    #[error("username is already taken")]
    Taken,
    #[error("this invite link has expired or been used up")]
    BadInvite,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for SignupError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::Database(e) if e.is_unique_violation() => Self::Taken,
            e => Self::Store(e.into()),
        }
    }
}

macro_rules! invite_columns {
    () => {
        "i.id, i.role_id, r.name AS role_name, i.note, i.uses_left, i.used, i.created_at, \
         i.expires_at"
    };
}

/// Invites that still work at `?`.
macro_rules! usable_invite {
    () => {
        "(i.uses_left IS NULL OR i.uses_left > 0) AND (i.expires_at IS NULL OR i.expires_at > ?)"
    };
}

impl Store {
    /// Every role, Listener first, then A to Z.
    pub async fn roles(&self) -> Result<Vec<AccountRole>> {
        let roles = sqlx::query_as(concat!(
            "SELECT ",
            role_columns!(),
            " FROM roles r ORDER BY r.builtin DESC, r.name"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(roles)
    }

    pub async fn role(&self, id: i64) -> Result<Option<AccountRole>> {
        let role = sqlx::query_as(concat!(
            "SELECT ",
            role_columns!(),
            " FROM roles r WHERE r.id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(role)
    }

    pub async fn create_role(&self, name: &str, p: &Permissions) -> Result<AccountRole, RoleError> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO roles (name, go, go_remote, max_streams, downloads, share_playlists,
                                submit_music, submit_without_review, unlimited_requests,
                                require_two_step, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(name)
        .bind(p.go)
        .bind(p.go_remote)
        .bind(p.max_streams)
        .bind(p.downloads)
        .bind(p.share_playlists)
        .bind(p.submit_music)
        .bind(p.submit_without_review)
        .bind(p.unlimited_requests)
        .bind(p.require_two_step)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await?;
        Ok(AccountRole {
            id,
            name: name.to_owned(),
            builtin: false,
            permissions: p.clone(),
            accounts: 0,
        })
    }

    /// Renames a role and replaces its permissions. False if there's no
    /// such role.
    pub async fn update_role(
        &self,
        id: i64,
        name: &str,
        p: &Permissions,
    ) -> Result<bool, RoleError> {
        let result = sqlx::query(
            "UPDATE roles SET name = ?, go = ?, go_remote = ?, max_streams = ?, downloads = ?,
                              share_playlists = ?, submit_music = ?, submit_without_review = ?,
                              unlimited_requests = ?, require_two_step = ?
             WHERE id = ?",
        )
        .bind(name)
        .bind(p.go)
        .bind(p.go_remote)
        .bind(p.max_streams)
        .bind(p.downloads)
        .bind(p.share_playlists)
        .bind(p.submit_music)
        .bind(p.submit_without_review)
        .bind(p.unlimited_requests)
        .bind(p.require_two_step)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Deletes a role, moving its accounts to Listener. Its invites go too.
    pub async fn delete_role(&self, id: i64) -> Result<bool, RoleError> {
        if id == LISTENER_ROLE {
            return Err(RoleError::Builtin);
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE users SET role_id = ? WHERE role_id = ?")
            .bind(LISTENER_ROLE)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let result = sqlx::query("DELETE FROM roles WHERE id = ? AND builtin = 0")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// What `user` may do.
    pub async fn permissions(&self, user: &User) -> Result<Permissions> {
        match user.role {
            Role::Admin => Ok(Permissions::all()),
            Role::Guest => Ok(Permissions::default()),
            Role::User => {
                let found = sqlx::query_as(concat!(
                    "SELECT ",
                    permission_columns!(),
                    " FROM roles r WHERE r.id = ?"
                ))
                .bind(user.role_id)
                .fetch_optional(&self.pool)
                .await?;
                Ok(found.unwrap_or_default())
            }
        }
    }

    pub async fn create_invite(&self, invite: &NewInvite<'_>) -> Result<Invite> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO invites (token_hash, role_id, note, uses_left, created_by, created_at,
                                  expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(invite.token_hash)
        .bind(invite.role_id)
        .bind(invite.note)
        .bind(invite.uses_left)
        .bind(invite.created_by)
        .bind(now_ms())
        .bind(invite.expires_at)
        .fetch_one(&self.pool)
        .await?;
        let invite = sqlx::query_as(concat!(
            "SELECT ",
            invite_columns!(),
            " FROM invites i JOIN roles r ON r.id = i.role_id WHERE i.id = ?"
        ))
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(invite)
    }

    /// Invites that still work, newest first.
    pub async fn invites(&self) -> Result<Vec<Invite>> {
        let invites = sqlx::query_as(concat!(
            "SELECT ",
            invite_columns!(),
            " FROM invites i JOIN roles r ON r.id = i.role_id WHERE ",
            usable_invite!(),
            " ORDER BY i.created_at DESC, i.id DESC"
        ))
        .bind(now_ms())
        .fetch_all(&self.pool)
        .await?;
        Ok(invites)
    }

    pub async fn delete_invite(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM invites WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// The invite behind a token, if it still works.
    pub async fn invite_by_token(&self, token_hash: &[u8]) -> Result<Option<Invite>> {
        let invite = sqlx::query_as(concat!(
            "SELECT ",
            invite_columns!(),
            " FROM invites i JOIN roles r ON r.id = i.role_id WHERE i.token_hash = ? AND ",
            usable_invite!()
        ))
        .bind(token_hash)
        .bind(now_ms())
        .fetch_optional(&self.pool)
        .await?;
        Ok(invite)
    }

    /// Creates an active account from an invite, using one of its uses. A
    /// taken username leaves the invite as it was.
    pub async fn sign_up_with_invite(
        &self,
        username: &str,
        password_hash: &str,
        token_hash: &[u8],
    ) -> Result<User, SignupError> {
        let mut tx = self.pool.begin().await?;
        let role_id: Option<i64> = sqlx::query_scalar(concat!(
            "UPDATE invites AS i SET used = used + 1, uses_left = uses_left - 1
             WHERE i.token_hash = ? AND ",
            usable_invite!(),
            " RETURNING role_id"
        ))
        .bind(token_hash)
        .bind(now_ms())
        .fetch_optional(&mut *tx)
        .await?;
        let role_id = role_id.ok_or(SignupError::BadInvite)?;
        let user = crate::users::insert_account(
            &mut tx,
            username,
            password_hash,
            Role::User,
            role_id,
            AccountStatus::Active,
        )
        .await?;
        tx.commit().await?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roles_permissions_and_invites() {
        let store = Store::open_in_memory().await.unwrap();
        let roles = store.roles().await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(
            (roles[0].id, roles[0].name.as_str()),
            (LISTENER_ROLE, "Listener")
        );
        assert!(roles[0].permissions.go && !roles[0].permissions.downloads);

        let dj = store
            .create_role(
                "DJ",
                &Permissions {
                    unlimited_requests: true,
                    ..Permissions::default()
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            store.create_role("dj", &Permissions::default()).await,
            Err(RoleError::Taken)
        ));

        let sam = store.create_user("sam", "h", Role::User).await.unwrap();
        assert_eq!(sam.role_id, LISTENER_ROLE);
        assert!(!store.permissions(&sam).await.unwrap().unlimited_requests);
        assert!(store.set_account_role(sam.id, dj.id).await.unwrap());
        let sam = store.user(sam.id).await.unwrap().unwrap();
        assert!(store.permissions(&sam).await.unwrap().unlimited_requests);
        assert_eq!(store.role(dj.id).await.unwrap().unwrap().accounts, 1);

        // Invites: two uses, then used up. A taken name doesn't use one.
        let admin = store.create_user("ana", "h", Role::Admin).await.unwrap();
        assert_eq!(store.permissions(&admin).await.unwrap(), Permissions::all());
        store
            .create_invite(&NewInvite {
                token_hash: b"tok",
                role_id: dj.id,
                note: "band",
                uses_left: Some(2),
                expires_at: None,
                created_by: admin.id,
            })
            .await
            .unwrap();
        assert!(matches!(
            store.sign_up_with_invite("SAM", "h", b"tok").await,
            Err(SignupError::Taken)
        ));
        assert_eq!(
            store
                .invite_by_token(b"tok")
                .await
                .unwrap()
                .unwrap()
                .uses_left,
            Some(2)
        );
        let jo = store.sign_up_with_invite("jo", "h", b"tok").await.unwrap();
        assert_eq!((jo.role_id, jo.status), (dj.id, AccountStatus::Active));
        store.sign_up_with_invite("kim", "h", b"tok").await.unwrap();
        assert!(matches!(
            store.sign_up_with_invite("lee", "h", b"tok").await,
            Err(SignupError::BadInvite)
        ));
        assert!(store.invites().await.unwrap().is_empty());

        // Deleting a role moves its accounts to Listener.
        assert!(matches!(
            store.delete_role(LISTENER_ROLE).await,
            Err(RoleError::Builtin)
        ));
        assert!(store.delete_role(dj.id).await.unwrap());
        assert_eq!(
            store.user(jo.id).await.unwrap().unwrap().role_id,
            LISTENER_ROLE
        );
    }
}
