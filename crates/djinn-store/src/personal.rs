//! DJINN GO: each person's likes, history, where they left off, and their
//! own playlists. None of it feeds the party's learning.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::library::{album_columns, track_columns};
use crate::{now_ms, Album, ArtistSummary, Result, Store, Track};

/// What a heart is on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum LikeKind {
    Track,
    Album,
    Artist,
}

/// Ids of everything a person likes, for showing hearts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LikedIds {
    pub tracks: Vec<i64>,
    pub albums: Vec<i64>,
    pub artists: Vec<i64>,
}

/// Everything a person likes, newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Liked {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<ArtistSummary>,
}

/// A song from someone's history.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct Played {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub track: Track,
    pub played_at: i64,
}

/// Where someone left off.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resume {
    pub queue: Vec<i64>,
    pub position: i64,
    pub position_ms: i64,
    #[serde(default)]
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct GoPlaylist {
    pub id: i64,
    pub user_id: i64,
    /// Who made it, for shared ones.
    pub owner: String,
    pub name: String,
    pub shared: bool,
    pub tracks: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GoPlaylistDetail {
    #[serde(flatten)]
    pub playlist: GoPlaylist,
    pub songs: Vec<Track>,
}

macro_rules! go_playlist_columns {
    () => {
        "p.id, p.user_id, coalesce(u.display_name, u.username) AS owner, p.name, p.shared, \
         (SELECT count(*) FROM go_playlist_tracks pt WHERE pt.playlist_id = p.id) AS tracks, \
         p.updated_at"
    };
}

/// A play counts as skipped when less than this much was heard.
pub const SKIP_BEFORE_MS: i64 = 30_000;

/// What tapping a song in GO does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum TapAction {
    /// Play it now, in place of what's playing.
    #[default]
    Play,
    /// Add it to the queue, after what you added before.
    Queue,
}

/// Someone's DJINN GO preferences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct GoPrefs {
    pub tap: TapAction,
}

impl Store {
    pub async fn set_like(&self, user_id: i64, kind: LikeKind, id: i64, on: bool) -> Result<()> {
        if on {
            sqlx::query(
                "INSERT OR IGNORE INTO go_likes (user_id, kind, ref_id, at) VALUES (?, ?, ?, ?)",
            )
            .bind(user_id)
            .bind(kind)
            .bind(id)
            .bind(now_ms())
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("DELETE FROM go_likes WHERE user_id = ? AND kind = ? AND ref_id = ?")
                .bind(user_id)
                .bind(kind)
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    async fn like_ids(&self, user_id: i64, kind: LikeKind) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(
            "SELECT ref_id FROM go_likes WHERE user_id = ? AND kind = ? ORDER BY at DESC",
        )
        .bind(user_id)
        .bind(kind)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    pub async fn liked_ids(&self, user_id: i64) -> Result<LikedIds> {
        Ok(LikedIds {
            tracks: self.like_ids(user_id, LikeKind::Track).await?,
            albums: self.like_ids(user_id, LikeKind::Album).await?,
            artists: self.like_ids(user_id, LikeKind::Artist).await?,
        })
    }

    pub async fn liked(&self, user_id: i64) -> Result<Liked> {
        let ids = self.liked_ids(user_id).await?;
        let albums: Vec<Album> = sqlx::query_as(concat!(
            "SELECT ",
            album_columns!(),
            " FROM go_likes l JOIN albums al ON al.id = l.ref_id
             JOIN artists ar ON ar.id = al.artist_id
             WHERE l.user_id = ? AND l.kind = 'album' ORDER BY l.at DESC"
        ))
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Liked {
            tracks: self.tracks_by_ids(&ids.tracks).await?,
            albums,
            artists: self.artists_by_ids(&ids.artists).await?,
        })
    }

    /// Records a song played in GO.
    pub async fn record_go_play(&self, user_id: i64, track_id: i64, ms_played: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO go_plays (user_id, track_id, at, ms_played, skipped)
             SELECT ?, id, ?, ?, ? FROM tracks WHERE id = ?",
        )
        .bind(user_id)
        .bind(now_ms())
        .bind(ms_played.max(0))
        .bind(ms_played < SKIP_BEFORE_MS)
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Someone's history, newest first.
    pub async fn go_history(&self, user_id: i64, offset: i64, limit: i64) -> Result<Vec<Played>> {
        let played = sqlx::query_as(concat!(
            "SELECT ",
            track_columns!(),
            ", gp.at AS played_at FROM go_plays gp JOIN tracks t ON t.id = gp.track_id
             WHERE gp.user_id = ? AND t.removed_at IS NULL
             ORDER BY gp.at DESC, gp.id DESC LIMIT ? OFFSET ?"
        ))
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(played)
    }

    /// Different songs played lately, most recent first.
    pub async fn recently_played(&self, user_id: i64, limit: i64) -> Result<Vec<Track>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT track_id FROM go_plays WHERE user_id = ?
             GROUP BY track_id ORDER BY max(at) DESC, max(id) DESC LIMIT ?",
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        self.tracks_by_ids(&ids).await
    }

    /// Songs played or skipped since `since`: (played through, skipped).
    pub async fn go_listening_since(
        &self,
        user_id: i64,
        since: i64,
    ) -> Result<(Vec<i64>, Vec<i64>)> {
        let rows: Vec<(i64, bool)> =
            sqlx::query_as("SELECT track_id, skipped FROM go_plays WHERE user_id = ? AND at >= ?")
                .bind(user_id)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
        let (skipped, played): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(_, s)| *s);
        Ok((
            played.into_iter().map(|(id, _)| id).collect(),
            skipped.into_iter().map(|(id, _)| id).collect(),
        ))
    }

    pub async fn save_resume(&self, user_id: i64, resume: &Resume) -> Result<()> {
        let ids: Vec<String> = resume.queue.iter().map(i64::to_string).collect();
        let queue = format!("[{}]", ids.join(","));
        sqlx::query(
            "INSERT INTO go_resume (user_id, queue, position, position_ms, updated_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (user_id) DO UPDATE SET queue = excluded.queue,
                position = excluded.position, position_ms = excluded.position_ms,
                updated_at = excluded.updated_at",
        )
        .bind(user_id)
        .bind(queue)
        .bind(resume.position)
        .bind(resume.position_ms)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Someone's GO preferences; the defaults if they haven't set any.
    pub async fn go_prefs(&self, user_id: i64) -> Result<GoPrefs> {
        let prefs = sqlx::query_as("SELECT tap FROM go_prefs WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(prefs.unwrap_or_default())
    }

    pub async fn set_go_prefs(&self, user_id: i64, prefs: &GoPrefs) -> Result<()> {
        sqlx::query(
            "INSERT INTO go_prefs (user_id, tap) VALUES (?, ?) \
             ON CONFLICT (user_id) DO UPDATE SET tap = excluded.tap",
        )
        .bind(user_id)
        .bind(prefs.tap)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn resume(&self, user_id: i64) -> Result<Option<Resume>> {
        let row: Option<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT queue, position, position_ms, updated_at FROM go_resume WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(queue, position, position_ms, updated_at)| Resume {
                queue: queue
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .filter_map(|id| id.trim().parse().ok())
                    .collect(),
                position,
                position_ms,
                updated_at,
            }),
        )
    }

    /// Someone's own playlists, then ones others share, by name.
    pub async fn go_playlists(&self, user_id: i64) -> Result<Vec<GoPlaylist>> {
        let lists = sqlx::query_as(concat!(
            "SELECT ",
            go_playlist_columns!(),
            " FROM go_playlists p JOIN users u ON u.id = p.user_id
             WHERE p.user_id = ? OR p.shared = 1
             ORDER BY p.user_id != ?, p.name COLLATE NOCASE"
        ))
        .bind(user_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(lists)
    }

    /// A playlist `user_id` may see: their own, or a shared one.
    pub async fn go_playlist(&self, user_id: i64, id: i64) -> Result<Option<GoPlaylistDetail>> {
        let playlist: Option<GoPlaylist> = sqlx::query_as(concat!(
            "SELECT ",
            go_playlist_columns!(),
            " FROM go_playlists p JOIN users u ON u.id = p.user_id
             WHERE p.id = ? AND (p.user_id = ? OR p.shared = 1)"
        ))
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(playlist) = playlist else {
            return Ok(None);
        };
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT track_id FROM go_playlist_tracks WHERE playlist_id = ? ORDER BY position",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        let found = self.tracks_by_ids(&ids).await?;
        // Keep repeats and order: tracks_by_ids drops duplicates.
        let by_id: std::collections::HashMap<i64, &Track> =
            found.iter().map(|t| (t.id, t)).collect();
        let songs = ids
            .iter()
            .filter_map(|i| by_id.get(i).map(|t| (*t).clone()))
            .collect();
        Ok(Some(GoPlaylistDetail { playlist, songs }))
    }

    pub async fn create_go_playlist(&self, user_id: i64, name: &str) -> Result<i64> {
        let now = now_ms();
        let id = sqlx::query_scalar(
            "INSERT INTO go_playlists (user_id, name, created_at, updated_at)
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(user_id)
        .bind(name)
        .bind(now)
        .bind(now)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    /// Renames or shares one of `user_id`'s playlists. False if it isn't
    /// theirs.
    pub async fn update_go_playlist(
        &self,
        user_id: i64,
        id: i64,
        name: Option<&str>,
        shared: Option<bool>,
    ) -> Result<bool> {
        let done = sqlx::query(
            "UPDATE go_playlists SET name = coalesce(?, name), shared = coalesce(?, shared),
                                     updated_at = ?
             WHERE id = ? AND user_id = ?",
        )
        .bind(name)
        .bind(shared)
        .bind(now_ms())
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn delete_go_playlist(&self, user_id: i64, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM go_playlists WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Replaces the songs of one of `user_id`'s playlists. False if it
    /// isn't theirs.
    pub async fn set_go_playlist_tracks(
        &self,
        user_id: i64,
        id: i64,
        track_ids: &[i64],
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let owned: Option<i64> =
            sqlx::query_scalar("SELECT id FROM go_playlists WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&mut *tx)
                .await?;
        if owned.is_none() {
            return Ok(false);
        }
        sqlx::query("DELETE FROM go_playlist_tracks WHERE playlist_id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for (position, track_id) in track_ids.iter().enumerate() {
            sqlx::query(
                "INSERT INTO go_playlist_tracks (playlist_id, position, track_id)
                 SELECT ?, ?, id FROM tracks WHERE id = ?",
            )
            .bind(id)
            .bind(i64::try_from(position).unwrap_or(i64::MAX))
            .bind(track_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE go_playlists SET updated_at = ? WHERE id = ?")
            .bind(now_ms())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Adds songs to the end of one of `user_id`'s playlists.
    pub async fn add_to_go_playlist(
        &self,
        user_id: i64,
        id: i64,
        track_ids: &[i64],
    ) -> Result<bool> {
        let Some(current) = self.go_playlist(user_id, id).await? else {
            return Ok(false);
        };
        if current.playlist.user_id != user_id {
            return Ok(false);
        }
        let mut ids: Vec<i64> = current.songs.iter().map(|t| t.id).collect();
        ids.extend_from_slice(track_ids);
        self.set_go_playlist_tracks(user_id, id, &ids).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    async fn library(store: &Store) -> Vec<i64> {
        let songs: Vec<ScannedTrack> = (0..3)
            .map(|i| ScannedTrack {
                path: format!("/m/{i}.flac"),
                title: format!("Song {i}"),
                artist: Some("Band".into()),
                album: Some("Record".into()),
                ..ScannedTrack::default()
            })
            .collect();
        store.save_scanned(&songs).await.unwrap();
        let mut ids: Vec<i64> = store
            .known_files()
            .await
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    #[tokio::test]
    async fn likes_history_resume_and_playlists() {
        let store = Store::open_in_memory().await.unwrap();
        let ids = library(&store).await;
        let sam = store.create_user("sam", "h", Role::User).await.unwrap().id;
        let jo = store.create_user("jo", "h", Role::User).await.unwrap().id;

        store
            .set_like(sam, LikeKind::Track, ids[1], true)
            .await
            .unwrap();
        store
            .set_like(sam, LikeKind::Track, ids[1], true)
            .await
            .unwrap();
        let liked = store.liked(sam).await.unwrap();
        assert_eq!(liked.tracks.len(), 1);
        store
            .set_like(sam, LikeKind::Track, ids[1], false)
            .await
            .unwrap();
        assert!(store.liked_ids(sam).await.unwrap().tracks.is_empty());

        store.record_go_play(sam, ids[0], 200_000).await.unwrap();
        store.record_go_play(sam, ids[2], 5_000).await.unwrap();
        store.record_go_play(sam, ids[0], 200_000).await.unwrap();
        let recent = store.recently_played(sam, 10).await.unwrap();
        assert_eq!(
            recent.iter().map(|t| t.id).collect::<Vec<_>>(),
            [ids[0], ids[2]]
        );
        assert_eq!(store.go_history(sam, 0, 10).await.unwrap().len(), 3);
        let (played, skipped) = store.go_listening_since(sam, 0).await.unwrap();
        assert_eq!((played.len(), skipped), (2, vec![ids[2]]));

        let resume = Resume {
            queue: ids.clone(),
            position: 1,
            position_ms: 42_000,
            updated_at: 0,
        };
        store.save_resume(sam, &resume).await.unwrap();
        let back = store.resume(sam).await.unwrap().unwrap();
        assert_eq!((back.queue, back.position_ms), (ids.clone(), 42_000));

        let list = store.create_go_playlist(sam, "Mix").await.unwrap();
        assert!(store
            .add_to_go_playlist(sam, list, &[ids[2], ids[0], ids[2]])
            .await
            .unwrap());
        let detail = store.go_playlist(sam, list).await.unwrap().unwrap();
        assert_eq!(
            detail.songs.iter().map(|t| t.id).collect::<Vec<_>>(),
            [ids[2], ids[0], ids[2]],
            "order and repeats kept"
        );
        // Jo can't see or change it until it's shared.
        assert!(store.go_playlist(jo, list).await.unwrap().is_none());
        assert!(!store.add_to_go_playlist(jo, list, &[ids[1]]).await.unwrap());
        assert!(store
            .update_go_playlist(sam, list, None, Some(true))
            .await
            .unwrap());
        assert_eq!(store.go_playlists(jo).await.unwrap()[0].owner, "sam");
        assert!(!store.add_to_go_playlist(jo, list, &[ids[1]]).await.unwrap());
        assert!(!store.delete_go_playlist(jo, list).await.unwrap());
        assert!(store.delete_go_playlist(sam, list).await.unwrap());
    }
}
