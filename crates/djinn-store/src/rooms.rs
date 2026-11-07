//! Rooms: areas with their own speakers, each its own party (queue, rules,
//! skip votes), unless linked to another room to play what it plays. Room 1,
//! "Main", always exists; everything starts there.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store, StoreError};

/// The room that always exists, where everything starts.
pub const MAIN_ROOM: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct Room {
    pub id: i64,
    pub name: String,
    /// The room this one plays along with; None plays its own.
    pub linked_to: Option<i64>,
    pub position: i64,
}

/// Why a room change wasn't made.
#[derive(Debug, thiserror::Error)]
pub enum RoomError {
    #[error("a room with that name already exists")]
    NameTaken,
    #[error("the room needs a name")]
    NoName,
    #[error("Main can't be deleted")]
    MainStays,
    #[error("there's no such room")]
    NotFound,
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<sqlx::Error> for RoomError {
    fn from(e: sqlx::Error) -> Self {
        RoomError::Store(e.into())
    }
}

impl Store {
    /// Every room, in the admin's order.
    pub async fn rooms(&self) -> Result<Vec<Room>> {
        Ok(
            sqlx::query_as("SELECT id, name, linked_to, position FROM rooms ORDER BY position, id")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn room(&self, id: i64) -> Result<Option<Room>> {
        Ok(
            sqlx::query_as("SELECT id, name, linked_to, position FROM rooms WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn create_room(&self, name: &str) -> std::result::Result<Room, RoomError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RoomError::NoName);
        }
        let inserted = sqlx::query_as(
            "INSERT INTO rooms (name, position, created_at)
             VALUES (?, (SELECT coalesce(max(position), 0) + 1 FROM rooms), ?)
             RETURNING id, name, linked_to, position",
        )
        .bind(name)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await;
        match inserted {
            Ok(room) => Ok(room),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(RoomError::NameTaken),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn rename_room(&self, id: i64, name: &str) -> std::result::Result<(), RoomError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(RoomError::NoName);
        }
        let result = sqlx::query("UPDATE rooms SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await;
        match result {
            Ok(r) if r.rows_affected() == 0 => Err(RoomError::NotFound),
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(RoomError::NameTaken),
            Err(e) => Err(e.into()),
        }
    }

    /// Deletes a room. Its speakers and the people in it move to Main, and
    /// so do its requests (one already queued in Main just stays there).
    /// Rooms linked to it play their own again.
    pub async fn delete_room(&self, id: i64) -> std::result::Result<(), RoomError> {
        if id == MAIN_ROOM {
            return Err(RoomError::MainStays);
        }
        let mut tx = self.write().await?;
        for sql in [
            "UPDATE outputs SET room_id = 1 WHERE room_id = ?",
            "UPDATE users SET room_id = 1 WHERE room_id = ?",
            "UPDATE OR IGNORE request_queue SET room_id = 1,
                position = position + (SELECT coalesce(max(position), 0) FROM request_queue WHERE room_id = 1)
             WHERE room_id = ?",
            "DELETE FROM request_queue WHERE room_id = ?",
        ] {
            sqlx::query(sql).bind(id).execute(&mut *tx).await?;
        }
        let result = sqlx::query("DELETE FROM rooms WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            return Err(RoomError::NotFound);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Links `id` to play along with `to`, or (None) unlinks it. Links
    /// don't chain: linking to a room that's itself linked links to the room
    /// it follows (or, if that's `id`, `to` leads instead), and rooms that
    /// followed `id` follow the new leader as well.
    pub async fn link_room(&self, id: i64, to: Option<i64>) -> std::result::Result<(), RoomError> {
        let mut tx = self.write().await?;
        let exists = |room: i64| {
            sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM rooms WHERE id = ?)")
                .bind(room)
        };
        if !exists(id).fetch_one(&mut *tx).await? {
            return Err(RoomError::NotFound);
        }
        let leader = match to {
            Some(to) => {
                if !exists(to).fetch_one(&mut *tx).await? {
                    return Err(RoomError::NotFound);
                }
                let followed: Option<i64> =
                    sqlx::query_scalar("SELECT linked_to FROM rooms WHERE id = ?")
                        .bind(to)
                        .fetch_one(&mut *tx)
                        .await?;
                if followed == Some(id) {
                    // Joining a room that followed this one: it leads now.
                    sqlx::query("UPDATE rooms SET linked_to = NULL WHERE id = ?")
                        .bind(to)
                        .execute(&mut *tx)
                        .await?;
                    Some(to)
                } else {
                    let leader = followed.unwrap_or(to);
                    (leader != id).then_some(leader)
                }
            }
            None => None,
        };
        sqlx::query("UPDATE rooms SET linked_to = ? WHERE id = ?")
            .bind(leader)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if let Some(leader) = leader {
            sqlx::query("UPDATE rooms SET linked_to = ? WHERE linked_to = ?")
                .bind(leader)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The room a person is in.
    pub async fn user_room(&self, user_id: i64) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT coalesce((SELECT u.room_id FROM users u JOIN rooms r ON r.id = u.room_id WHERE u.id = ?), 1)",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?)
    }

    pub async fn set_user_room(
        &self,
        user_id: i64,
        room: i64,
    ) -> std::result::Result<(), RoomError> {
        if self.room(room).await?.is_none() {
            return Err(RoomError::NotFound);
        }
        sqlx::query("UPDATE users SET room_id = ? WHERE id = ?")
            .bind(room)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_output_room(
        &self,
        output: i64,
        room: i64,
    ) -> std::result::Result<(), RoomError> {
        if self.room(room).await?.is_none() {
            return Err(RoomError::NotFound);
        }
        sqlx::query("UPDATE outputs SET room_id = ? WHERE id = ?")
            .bind(room)
            .bind(output)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn main_is_there_and_rooms_come_and_go() {
        let store = Store::open_in_memory().await.unwrap();
        let rooms = store.rooms().await.unwrap();
        assert_eq!(rooms.len(), 1);
        assert_eq!((rooms[0].id, rooms[0].name.as_str()), (MAIN_ROOM, "Main"));
        let patio = store.create_room(" Patio ").await.unwrap();
        assert_eq!(patio.name, "Patio");
        assert!(matches!(
            store.create_room("patio").await,
            Err(RoomError::NameTaken)
        ));
        assert!(matches!(
            store.create_room("  ").await,
            Err(RoomError::NoName)
        ));
        store.rename_room(patio.id, "Deck").await.unwrap();
        assert!(matches!(
            store.delete_room(MAIN_ROOM).await,
            Err(RoomError::MainStays)
        ));

        let sam = store
            .create_user("sam", "h", crate::Role::User)
            .await
            .unwrap()
            .id;
        assert_eq!(store.user_room(sam).await.unwrap(), MAIN_ROOM);
        store.set_user_room(sam, patio.id).await.unwrap();
        assert_eq!(store.user_room(sam).await.unwrap(), patio.id);
        store.delete_room(patio.id).await.unwrap();
        assert_eq!(
            store.user_room(sam).await.unwrap(),
            MAIN_ROOM,
            "back in Main"
        );
        assert!(matches!(
            store.set_user_room(sam, patio.id).await,
            Err(RoomError::NotFound)
        ));
    }

    #[tokio::test]
    async fn links_never_chain() {
        let store = Store::open_in_memory().await.unwrap();
        let a = store.create_room("A").await.unwrap().id;
        let b = store.create_room("B").await.unwrap().id;
        store.link_room(a, Some(MAIN_ROOM)).await.unwrap();
        store.link_room(b, Some(a)).await.unwrap();
        let linked = |id: i64| {
            let store = &store;
            async move { store.room(id).await.unwrap().unwrap().linked_to }
        };
        assert_eq!(linked(b).await, Some(MAIN_ROOM), "B follows what A follows");
        // Main joins B: B's followers can't follow a follower.
        store.link_room(MAIN_ROOM, Some(b)).await.unwrap();
        assert_eq!(linked(MAIN_ROOM).await, Some(b));
        assert_eq!(linked(a).await, Some(b));
        assert_eq!(linked(b).await, None);
        store.link_room(a, None).await.unwrap();
        assert_eq!(linked(a).await, None);
        store.link_room(b, Some(b)).await.unwrap();
        assert_eq!(linked(b).await, None, "a room can't follow itself");
    }

    #[tokio::test]
    async fn each_room_has_its_own_queue_and_limits() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[crate::ScannedTrack {
                path: "/m/a.flac".into(),
                size: 1,
                mtime: 1,
                title: "A".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let track: i64 = sqlx::query_scalar("SELECT id FROM tracks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let patio = store.create_room("Patio").await.unwrap().id;
        let sam = store
            .create_user("sam", "h", crate::Role::User)
            .await
            .unwrap()
            .id;
        let one_each = crate::RequestLimits {
            per_user: Some(1),
            queue_cap: None,
        };
        let origin = crate::RequestOrigin::default();
        store
            .add_request(MAIN_ROOM, track, sam, one_each, &origin)
            .await
            .unwrap();
        // The same song, and another slot, in another room.
        store
            .add_request(patio, track, sam, one_each, &origin)
            .await
            .unwrap();
        assert_eq!(store.queue(MAIN_ROOM).await.unwrap().len(), 1);
        assert_eq!(store.queue(patio).await.unwrap().len(), 1);
        // Deleting the patio leaves Main's copy alone.
        store.delete_room(patio).await.unwrap();
        assert_eq!(store.queue(MAIN_ROOM).await.unwrap().len(), 1);
    }
}
