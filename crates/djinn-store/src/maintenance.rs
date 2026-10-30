//! Clearing things out from Admin > System. Each one forgets something
//! DJINN worked out or was told; none of them touch the music files.

use crate::{Result, Store};

impl Store {
    /// Empties the request queue (backers and boosts go with it).
    pub async fn clear_queue(&self) -> Result<u64> {
        let done = sqlx::query("DELETE FROM request_queue")
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected())
    }

    /// Forgets how every song sounds and how loud it is, so the background
    /// analysis measures them all again.
    pub async fn clear_analysis(&self) -> Result<()> {
        sqlx::query(
            "UPDATE tracks SET features = NULL, features_version = NULL, bpm = NULL, \
                    loudness_lufs = NULL, loudness_peak = NULL, loudness_checked_at = NULL",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Forgets what the AI heard in every song, so it listens again.
    pub async fn clear_listening(&self) -> Result<()> {
        sqlx::query("UPDATE tracks SET heard = NULL, heard_version = NULL")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Forgets every play and request: the history, top songs, and what the
    /// auto mix learned from them, for every party style.
    pub async fn clear_history(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM play_events")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM request_events")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Drops every tag correction. Returns the songs' file paths, to read
    /// again so their own tags show.
    pub async fn clear_tag_edits(&self) -> Result<Vec<String>> {
        let mut tx = self.pool.begin().await?;
        let paths =
            sqlx::query_scalar("SELECT t.path FROM tag_edits e JOIN tracks t ON t.id = e.track_id")
                .fetch_all(&mut *tx)
                .await?;
        sqlx::query("DELETE FROM tag_edits")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(paths)
    }

    /// Removes these settings, so each goes back to its default.
    pub async fn reset_settings(&self, keys: &[&str]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for key in keys {
            sqlx::query("DELETE FROM settings WHERE key = ?")
                .bind(key)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Role, ScannedTrack, Store, TagChange};

    #[tokio::test]
    async fn clearing_forgets_only_what_it_says() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.flac".into(),
                title: "A".into(),
                ..Default::default()
            }])
            .await
            .unwrap();
        let id = store.library_track_ids().await.unwrap()[0];
        let user = store.create_user("sam", "h", Role::User).await.unwrap();
        store
            .edit_tags(
                &[id],
                &TagChange {
                    title: Some(Some("B".into())),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        store.set_setting("volume", "40").await.unwrap();
        store.set_setting("library_roots", "[]").await.unwrap();

        assert_eq!(store.clear_tag_edits().await.unwrap(), ["/m/a.flac"]);
        assert!(store.tag_edits_by_path().await.unwrap().is_empty());

        store.reset_settings(&["volume"]).await.unwrap();
        assert_eq!(store.get_setting("volume").await.unwrap(), None);
        assert!(store.get_setting("library_roots").await.unwrap().is_some());

        store.clear_history().await.unwrap();
        store.clear_queue().await.unwrap();
        store.clear_analysis().await.unwrap();
        store.clear_listening().await.unwrap();
        assert_eq!(store.track_count().await.unwrap(), 1, "songs stay");
        assert!(
            store.user(user.id).await.unwrap().is_some(),
            "accounts stay"
        );
    }
}
