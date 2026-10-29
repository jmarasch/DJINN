//! Party styles: separate memories of what each kind of party liked
//! ("Halloween", "Family BBQ"). One style is in use at a time; requests and
//! plays count toward it as they're recorded.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store, StoreError};

/// Setting: the style in use.
pub const PARTY_STYLE: &str = "party_style";

/// The style requests and plays count toward right now, for the history
/// inserts: `NULL` if the setting names a style that's gone.
macro_rules! style_in_use {
    () => {
        "(SELECT id FROM party_styles \
          WHERE id = (SELECT CAST(value AS INTEGER) FROM settings WHERE key = 'party_style'))"
    };
}
pub(crate) use style_in_use;

#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct PartyStyle {
    pub id: i64,
    pub name: String,
    /// Requests and plays that count toward it.
    pub requests: i64,
    pub plays: i64,
    pub created_at: i64,
}

/// What parties of one style did with a song.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, FromRow)]
pub struct SongHistory {
    pub track_id: i64,
    pub requests: i64,
    /// Played to the end.
    pub finished: i64,
    pub skipped: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum PartyStyleError {
    #[error("there's already a style with that name")]
    NameTaken,
    #[error("that style is in use; use another one first")]
    InUse,
    #[error(transparent)]
    Store(#[from] StoreError),
}

macro_rules! style_select {
    () => {
        "SELECT s.id, s.name, s.created_at, \
                (SELECT count(*) FROM request_events r WHERE r.style_id = s.id) AS requests, \
                (SELECT count(*) FROM play_events p WHERE p.style_id = s.id) AS plays \
         FROM party_styles s"
    };
}

impl Store {
    pub async fn party_styles(&self) -> Result<Vec<PartyStyle>> {
        let styles = sqlx::query_as(concat!(style_select!(), " ORDER BY s.name"))
            .fetch_all(&self.pool)
            .await?;
        Ok(styles)
    }

    pub async fn party_style(&self, id: i64) -> Result<Option<PartyStyle>> {
        let style = sqlx::query_as(concat!(style_select!(), " WHERE s.id = ?"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(style)
    }

    /// The style in use, if the setting names one that exists.
    pub async fn active_party_style(&self) -> Result<Option<i64>> {
        let id = sqlx::query_scalar(concat!("SELECT ", style_in_use!()))
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
    }

    /// Puts a style in use. Returns false if there's no such style.
    pub async fn use_party_style(&self, id: i64) -> Result<bool> {
        if self.party_style(id).await?.is_none() {
            return Ok(false);
        }
        self.set_setting(PARTY_STYLE, &id.to_string()).await?;
        Ok(true)
    }

    pub async fn create_party_style(&self, name: &str) -> Result<PartyStyle, PartyStyleError> {
        let inserted = sqlx::query_scalar(
            "INSERT INTO party_styles (name, created_at) VALUES (?, ?) RETURNING id",
        )
        .bind(name)
        .bind(now_ms())
        .fetch_one(&self.pool)
        .await;
        let id: i64 = match inserted {
            Ok(id) => id,
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                return Err(PartyStyleError::NameTaken)
            }
            Err(e) => return Err(StoreError::from(e).into()),
        };
        let style = sqlx::query_as(concat!(style_select!(), " WHERE s.id = ?"))
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(StoreError::from)?;
        Ok(style)
    }

    /// Returns false if there's no such style.
    pub async fn rename_party_style(&self, id: i64, name: &str) -> Result<bool, PartyStyleError> {
        let renamed = sqlx::query("UPDATE party_styles SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await;
        match renamed {
            Ok(result) => Ok(result.rows_affected() > 0),
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
                Err(PartyStyleError::NameTaken)
            }
            Err(e) => Err(StoreError::from(e).into()),
        }
    }

    /// Forgets what parties of this style did. The request log and recently
    /// played songs keep their history. Returns false if there's no such
    /// style.
    pub async fn clear_party_style(&self, id: i64) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let found = sqlx::query("SELECT 1 FROM party_styles WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        sqlx::query("UPDATE request_events SET style_id = NULL WHERE style_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE play_events SET style_id = NULL WHERE style_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(found)
    }

    /// Deletes a style and what it learned. The style in use can't be
    /// deleted. Returns false if there's no such style.
    pub async fn delete_party_style(&self, id: i64) -> Result<bool, PartyStyleError> {
        if self.active_party_style().await? == Some(id) {
            return Err(PartyStyleError::InUse);
        }
        // Its events' style_id goes back to NULL (ON DELETE SET NULL).
        let result = sqlx::query("DELETE FROM party_styles WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from)?;
        Ok(result.rows_affected() > 0)
    }

    /// What parties of a style did with each song they touched.
    pub async fn party_history(&self, style_id: i64) -> Result<Vec<SongHistory>> {
        let rows = sqlx::query_as(
            "SELECT track_id, sum(requests) AS requests, sum(finished) AS finished, \
                    sum(skipped) AS skipped FROM ( \
                 SELECT track_id, 1 AS requests, 0 AS finished, 0 AS skipped \
                 FROM request_events WHERE style_id = ? \
                 UNION ALL \
                 SELECT track_id, 0, CASE WHEN skipped = 0 AND ended_at IS NOT NULL THEN 1 ELSE 0 END, \
                        skipped FROM play_events WHERE style_id = ? \
             ) GROUP BY track_id",
        )
        .bind(style_id)
        .bind(style_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PlaySource, RequestLimits, RequestOrigin, Role, ScannedTrack};

    async fn store_with_song() -> (Store, i64, i64) {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&[ScannedTrack {
                path: "/m/a.flac".into(),
                title: "Song".into(),
                artist: Some("Artist".into()),
                album: Some("Album".into()),
                ..Default::default()
            }])
            .await
            .unwrap();
        let track = store.known_files().await.unwrap()[0].id;
        let user = store.create_user("ana", "h", Role::Admin).await.unwrap().id;
        (store, track, user)
    }

    async fn request_and_play(store: &Store, track: i64, user: i64, skipped: bool) {
        let (item, _) = store
            .add_request(
                track,
                user,
                RequestLimits::default(),
                &RequestOrigin::default(),
            )
            .await
            .unwrap();
        let play = store
            .start_play_event(track, PlaySource::Request, Some(user))
            .await
            .unwrap();
        store
            .finish_play_event(play, 1000, skipped, None)
            .await
            .unwrap();
        store.remove_request(item.id).await.unwrap();
    }

    #[tokio::test]
    async fn history_counts_toward_the_style_in_use() {
        let (store, track, user) = store_with_song().await;
        assert_eq!(store.active_party_style().await.unwrap(), Some(1));
        request_and_play(&store, track, user, false).await;

        let halloween = store.create_party_style("Halloween").await.unwrap();
        assert!(store.use_party_style(halloween.id).await.unwrap());
        request_and_play(&store, track, user, true).await;

        let everyday = store.party_history(1).await.unwrap();
        assert_eq!(
            everyday,
            vec![SongHistory {
                track_id: track,
                requests: 1,
                finished: 1,
                skipped: 0
            }]
        );
        let spooky = store.party_history(halloween.id).await.unwrap();
        assert_eq!(
            spooky,
            vec![SongHistory {
                track_id: track,
                requests: 1,
                finished: 0,
                skipped: 1
            }]
        );
        let listed = store.party_styles().await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|s| (s.name.as_str(), s.requests, s.plays))
                .collect::<Vec<_>>(),
            vec![("Everyday", 1, 1), ("Halloween", 1, 1)]
        );
    }

    #[tokio::test]
    async fn clearing_forgets_one_style_but_keeps_the_log() {
        let (store, track, user) = store_with_song().await;
        request_and_play(&store, track, user, false).await;
        let other = store.create_party_style("BBQ").await.unwrap();

        assert!(store.clear_party_style(1).await.unwrap());
        assert!(store.party_history(1).await.unwrap().is_empty());
        assert_eq!(store.recent_track_ids(10).await.unwrap(), vec![track]);
        assert!(store.party_history(other.id).await.unwrap().is_empty());
        assert!(!store.clear_party_style(999).await.unwrap());
    }

    #[tokio::test]
    async fn names_are_unique_and_the_style_in_use_stays() {
        let (store, _, _) = store_with_song().await;
        assert!(matches!(
            store.create_party_style("everyday").await,
            Err(PartyStyleError::NameTaken)
        ));
        let bbq = store.create_party_style("BBQ").await.unwrap();
        assert!(matches!(
            store.rename_party_style(bbq.id, "Everyday").await,
            Err(PartyStyleError::NameTaken)
        ));
        assert!(store
            .rename_party_style(bbq.id, "Family BBQ")
            .await
            .unwrap());
        assert!(matches!(
            store.delete_party_style(1).await,
            Err(PartyStyleError::InUse)
        ));
        assert!(store.delete_party_style(bbq.id).await.unwrap());
        assert!(!store.use_party_style(bbq.id).await.unwrap());
    }

    #[tokio::test]
    async fn deleting_a_style_keeps_the_log() {
        let (store, track, user) = store_with_song().await;
        let bbq = store.create_party_style("BBQ").await.unwrap();
        store.use_party_style(bbq.id).await.unwrap();
        request_and_play(&store, track, user, false).await;
        store.use_party_style(1).await.unwrap();

        assert!(store.delete_party_style(bbq.id).await.unwrap());
        assert_eq!(store.recent_track_ids(10).await.unwrap(), vec![track]);
        assert!(store.party_history(1).await.unwrap().is_empty());
    }
}
