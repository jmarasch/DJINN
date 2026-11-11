//! Nodes: other DJINN boxes that play a room on their own speakers, taking
//! everything from this server. Pairing (ask, accept with a password, kick)
//! is in `djinn-server::nodes`; this keeps the list.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

/// How many join requests may wait at once, so nobody can flood Admin.
pub const MAX_PENDING_NODES: i64 = 5;

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Node {
    pub id: i64,
    pub node_id: String,
    pub name: String,
    #[serde(skip)]
    pub secret_hash: String,
    /// "pending" or "approved".
    pub status: String,
    pub room_id: i64,
    pub address: Option<String>,
    pub version: Option<String>,
    pub platform: Option<String>,
    pub requested_at: i64,
    pub approved_at: Option<i64>,
    pub last_seen: Option<i64>,
    /// Speaker device id; None for the system default.
    pub device: Option<String>,
    pub delay_ms: i64,
    pub trim: f64,
    /// Equalizer settings as JSON; None for off.
    pub equalizer: Option<String>,
    /// "stop" or "cached": what to do once Main is gone and what was
    /// loaded has played.
    pub offline: String,
    /// The speakers the node last reported, as JSON.
    pub devices: Option<String>,
    /// Plays straight through instead of correcting itself against Main.
    pub free_run: bool,
    /// What a screen plugged into it shows: "status", "off", or the pages to
    /// try in order ("lyrics", "visual", "tv"), joined by commas.
    pub screen: String,
    /// How its visualizer draws: "bars", "ring" or "mirror".
    pub visual: String,
    /// How many songs, playing and coming up, it keeps copies of.
    pub cache_ahead: i64,
}

/// A node's speaker settings.
pub struct NodeAudio<'a> {
    pub device: Option<&'a str>,
    pub delay_ms: i64,
    pub trim: f64,
    pub equalizer: Option<&'a str>,
    pub offline: &'a str,
    pub free_run: bool,
    pub screen: &'a str,
    pub visual: &'a str,
    pub cache_ahead: i64,
}

/// A node asking to join.
pub struct JoinRequest<'a> {
    pub node_id: &'a str,
    pub name: &'a str,
    pub secret_hash: &'a str,
    pub address: Option<&'a str>,
    pub version: Option<&'a str>,
    pub platform: Option<&'a str>,
}

/// What became of a join request.
#[derive(Debug, PartialEq, Eq)]
pub enum Joined {
    /// Waiting for an admin (asking again with the same secret is fine).
    Pending,
    /// Already accepted; the node just needs to carry on.
    Approved,
    /// The id is taken by a node with another secret.
    Taken,
    /// Too many requests already waiting.
    Full,
}

impl Store {
    pub async fn nodes(&self) -> Result<Vec<Node>> {
        Ok(sqlx::query_as(
            "SELECT id, node_id, name, secret_hash, status, room_id, address, version, platform,
                    requested_at, approved_at, last_seen, device, delay_ms, trim, equalizer, offline, devices, free_run, screen, visual, cache_ahead
             FROM nodes ORDER BY status = 'approved', name, id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn node_by_node_id(&self, node_id: &str) -> Result<Option<Node>> {
        Ok(sqlx::query_as(
            "SELECT id, node_id, name, secret_hash, status, room_id, address, version, platform,
                    requested_at, approved_at, last_seen, device, delay_ms, trim, equalizer, offline, devices, free_run, screen, visual, cache_ahead
             FROM nodes WHERE node_id = ?",
        )
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn node(&self, id: i64) -> Result<Option<Node>> {
        Ok(sqlx::query_as(
            "SELECT id, node_id, name, secret_hash, status, room_id, address, version, platform,
                    requested_at, approved_at, last_seen, device, delay_ms, trim, equalizer, offline, devices, free_run, screen, visual, cache_ahead
             FROM nodes WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Records a join request, or refreshes one already waiting.
    pub async fn request_node_join(&self, request: &JoinRequest<'_>) -> Result<Joined> {
        let mut tx = self.write().await?;
        let existing: Option<(String, String)> =
            sqlx::query_as("SELECT secret_hash, status FROM nodes WHERE node_id = ?")
                .bind(request.node_id)
                .fetch_optional(&mut *tx)
                .await?;
        let now = now_ms();
        let joined = match existing {
            Some((hash, _)) if hash != request.secret_hash => Joined::Taken,
            Some((_, status)) => {
                sqlx::query(
                    "UPDATE nodes SET name = ?, address = ?, version = ?, platform = ?, last_seen = ?
                     WHERE node_id = ?",
                )
                .bind(request.name)
                .bind(request.address)
                .bind(request.version)
                .bind(request.platform)
                .bind(now)
                .bind(request.node_id)
                .execute(&mut *tx)
                .await?;
                if status == "approved" {
                    Joined::Approved
                } else {
                    Joined::Pending
                }
            }
            None => {
                let waiting: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM nodes WHERE status = 'pending'")
                        .fetch_one(&mut *tx)
                        .await?;
                if waiting >= MAX_PENDING_NODES {
                    Joined::Full
                } else {
                    sqlx::query(
                        "INSERT INTO nodes (node_id, name, secret_hash, status, address, version, platform,
                                            requested_at, last_seen)
                         VALUES (?, ?, ?, 'pending', ?, ?, ?, ?, ?)",
                    )
                    .bind(request.node_id)
                    .bind(request.name)
                    .bind(request.secret_hash)
                    .bind(request.address)
                    .bind(request.version)
                    .bind(request.platform)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
                    Joined::Pending
                }
            }
        };
        tx.commit().await?;
        Ok(joined)
    }

    /// Accepts a waiting node. False if there's no such request.
    pub async fn approve_node(&self, id: i64, by: i64) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE nodes SET status = 'approved', approved_at = ?, approved_by = ?
             WHERE id = ? AND status = 'pending'",
        )
        .bind(now_ms())
        .bind(by)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Declines a request or kicks a node: either way it's gone.
    pub async fn remove_node(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM nodes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn rename_node(&self, id: i64, name: &str) -> Result<bool> {
        let result = sqlx::query("UPDATE nodes SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_node_room(&self, id: i64, room: i64) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE nodes SET room_id = ? WHERE id = ? AND EXISTS (SELECT 1 FROM rooms WHERE id = ?)",
        )
        .bind(room)
        .bind(id)
        .bind(room)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Notes that a node checked in, from where, running what.
    pub async fn node_seen(
        &self,
        id: i64,
        address: Option<&str>,
        version: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE nodes SET last_seen = ?, address = coalesce(?, address), version = coalesce(?, version)
             WHERE id = ?",
        )
        .bind(now_ms())
        .bind(address)
        .bind(version)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_node_audio(&self, id: i64, audio: &NodeAudio<'_>) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE nodes SET device = ?, delay_ms = ?, trim = ?, equalizer = ?, offline = ?, free_run = ?, screen = ?, visual = ?, cache_ahead = ? WHERE id = ?",
        )
        .bind(audio.device)
        .bind(audio.delay_ms.clamp(0, 2000))
        .bind(audio.trim.clamp(0.0, 1.0))
        .bind(audio.equalizer)
        .bind(if audio.offline == "cached" { "cached" } else { "stop" })
        .bind(audio.free_run)
        .bind(audio.screen)
        .bind(audio.visual)
        .bind(audio.cache_ahead.clamp(1, 50))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Sets only a node's delay (the room timing tool), 0 to 2000 ms.
    pub async fn set_node_delay(&self, id: i64, delay_ms: i64) -> Result<bool> {
        let result = sqlx::query("UPDATE nodes SET delay_ms = ? WHERE id = ?")
            .bind(delay_ms.clamp(0, 2000))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_node_devices(&self, id: i64, devices: &str) -> Result<()> {
        sqlx::query("UPDATE nodes SET devices = ? WHERE id = ?")
            .bind(devices)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// True if any node has joined or is asking to.
    pub async fn has_nodes(&self) -> Result<bool> {
        Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM nodes)")
            .fetch_one(&self.pool)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(node_id: &'a str, hash: &'a str) -> JoinRequest<'a> {
        JoinRequest {
            node_id,
            name: "Kitchen box",
            secret_hash: hash,
            address: Some("192.168.1.20"),
            version: Some("2026.11.7"),
            platform: Some("linux-aarch64"),
        }
    }

    #[tokio::test]
    async fn a_node_asks_waits_and_is_accepted_or_kicked() {
        let store = Store::open_in_memory().await.unwrap();
        let admin = store
            .create_user("admin", "h", crate::Role::Admin)
            .await
            .unwrap()
            .id;
        assert!(!store.has_nodes().await.unwrap());
        assert_eq!(
            store.request_node_join(&request("n1", "aa")).await.unwrap(),
            Joined::Pending
        );
        // Asking again is fine; someone else's secret isn't.
        assert_eq!(
            store.request_node_join(&request("n1", "aa")).await.unwrap(),
            Joined::Pending
        );
        assert_eq!(
            store.request_node_join(&request("n1", "bb")).await.unwrap(),
            Joined::Taken
        );
        let node = store.node_by_node_id("n1").await.unwrap().unwrap();
        assert_eq!(node.status, "pending");
        assert!(store.approve_node(node.id, admin).await.unwrap());
        assert!(
            !store.approve_node(node.id, admin).await.unwrap(),
            "only once"
        );
        assert_eq!(
            store.request_node_join(&request("n1", "aa")).await.unwrap(),
            Joined::Approved
        );
        assert!(store.remove_node(node.id).await.unwrap());
        assert!(store.node_by_node_id("n1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn only_a_few_requests_wait_at_once() {
        let store = Store::open_in_memory().await.unwrap();
        for i in 0..MAX_PENDING_NODES {
            let id = format!("n{i}");
            assert_eq!(
                store.request_node_join(&request(&id, "aa")).await.unwrap(),
                Joined::Pending
            );
        }
        assert_eq!(
            store
                .request_node_join(&request("late", "aa"))
                .await
                .unwrap(),
            Joined::Full
        );
    }
}
