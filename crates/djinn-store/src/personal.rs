//! DJINN GO: each person's likes, history, where they left off, and their
//! own playlists. None of it feeds the party's learning.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::library::{album_columns, track_columns};
use crate::playlists::{
    PlaylistDetail, PlaylistEntry, PlaylistRow, PlaylistSummary, UnplayableSong,
};
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

/// Changes when a song's file does.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct TrackVersion {
    pub id: i64,
    pub version: String,
    /// The original file's size in bytes.
    pub size: i64,
}

/// Plays reported later than this are taken as happening now (ms).
const OLDEST_REPORT_MS: i64 = 90 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct GoPlaylist {
    pub id: i64,
    pub user_id: i64,
    /// Who made it, for shared ones.
    pub owner: String,
    pub name: String,
    pub shared: bool,
    pub tracks: i64,
    /// It's the viewer's own, so they can change it.
    pub mine: bool,
    pub updated_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GoPlaylistDetail {
    #[serde(flatten)]
    pub playlist: GoPlaylist,
    pub songs: Vec<Track>,
    /// The list as made, including songs that can't play.
    pub entries: Vec<PlaylistEntry>,
    pub unplayable: Vec<UnplayableSong>,
    /// Everything in it, in order, flagged where it can't play.
    pub rows: Vec<PlaylistRow>,
}

impl From<PlaylistSummary> for GoPlaylist {
    fn from(p: PlaylistSummary) -> Self {
        Self {
            id: p.id,
            user_id: p.owner_id,
            owner: p.owner,
            name: p.name,
            shared: p.shared,
            tracks: p.song_count,
            mine: false,
            updated_at: p.updated_at,
        }
    }
}

impl From<PlaylistDetail> for GoPlaylistDetail {
    fn from(d: PlaylistDetail) -> Self {
        Self {
            playlist: d.playlist.into(),
            songs: d.tracks,
            entries: d.entries,
            unplayable: d.unplayable,
            rows: d.rows,
        }
    }
}

/// A play counts as skipped when less than this much was heard.
pub const SKIP_BEFORE_MS: i64 = 30_000;

/// Skipping a song you heard all the way through this recently says you
/// just heard it, not that you don't like it, so it isn't recorded (radio
/// from a song that just ended starts with it, for one). Migration 43
/// cleared earlier skips like that with the same window.
pub const HEARD_RECENTLY_MS: i64 = 6 * 60 * 60 * 1000;

/// What tapping a song in GO does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum TapAction {
    /// Play it now, in place of what's playing.
    #[default]
    Play,
    /// Play it next: after the song playing and what you lined up before.
    Queue,
    /// Add it to the very end of the queue.
    End,
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
        self.record_go_play_at(user_id, track_id, ms_played, None)
            .await
    }

    /// Records a play that may have happened earlier, offline. A time in the
    /// future, or too long ago to be real, counts as now.
    pub async fn record_go_play_at(
        &self,
        user_id: i64,
        track_id: i64,
        ms_played: i64,
        at: Option<i64>,
    ) -> Result<()> {
        let now = now_ms();
        let at = at
            .filter(|at| (now - OLDEST_REPORT_MS..=now).contains(at))
            .unwrap_or(now);
        let skipped = ms_played < SKIP_BEFORE_MS;
        if skipped {
            let heard: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM go_plays WHERE user_id = ? AND track_id = ?
                                AND skipped = 0 AND at BETWEEN ? AND ?)",
            )
            .bind(user_id)
            .bind(track_id)
            .bind(at - HEARD_RECENTLY_MS)
            .bind(at)
            .fetch_one(&self.pool)
            .await?;
            if heard {
                return Ok(());
            }
        }
        sqlx::query(
            "INSERT INTO go_plays (user_id, track_id, at, ms_played, skipped)
             SELECT ?, id, ?, ?, ? FROM tracks WHERE id = ?",
        )
        .bind(user_id)
        .bind(at)
        .bind(ms_played.max(0))
        .bind(skipped)
        .bind(track_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The songs someone would most miss offline: most played through, then
    /// liked.
    pub async fn worth_keeping(&self, user_id: i64, limit: i64) -> Result<Vec<Track>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT track_id FROM (
                SELECT track_id, count(*) AS plays, max(at) AS last FROM go_plays
                WHERE user_id = ?1 AND NOT skipped GROUP BY track_id
                UNION ALL
                SELECT ref_id, 0, at FROM go_likes WHERE user_id = ?1 AND kind = 'track'
             ) GROUP BY track_id ORDER BY max(plays) DESC, max(last) DESC LIMIT ?2",
        )
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        self.tracks_by_ids(&ids).await
    }

    /// Each song's version, for those still in the library.
    pub async fn track_versions(&self, ids: &[i64]) -> Result<Vec<TrackVersion>> {
        let list = crate::library::json_ids(ids);
        let rows = sqlx::query_as(
            "SELECT id, size || '-' || mtime AS version, size FROM tracks
             WHERE removed_at IS NULL AND id IN (SELECT value FROM json_each(?))",
        )
        .bind(list)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
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

    /// Someone's own playlists, then those shared with them, by name.
    pub async fn go_playlists(&self, user_id: i64) -> Result<Vec<GoPlaylist>> {
        let lists = self.playlists_for(user_id).await?;
        Ok(lists
            .into_iter()
            .map(|list| {
                let mut list = GoPlaylist::from(list);
                list.mine = list.user_id == user_id;
                list
            })
            .collect())
    }

    /// A playlist `user_id` may see: their own, or one shared with them.
    pub async fn go_playlist(&self, user_id: i64, id: i64) -> Result<Option<GoPlaylistDetail>> {
        let detail = self.playlist_for(user_id, id).await?;
        Ok(detail.map(|detail| {
            let mut detail = GoPlaylistDetail::from(detail);
            detail.playlist.mine = detail.playlist.user_id == user_id;
            detail
        }))
    }

    pub async fn create_go_playlist(&self, user_id: i64, name: &str) -> Result<i64> {
        Ok(self.create_playlist(user_id, name).await?.id)
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
        if self.playlist_owner(id).await? != Some(user_id) {
            return Ok(false);
        }
        self.update_playlist(id, name, None).await?;
        if let Some(shared) = shared {
            self.set_playlist_shared(id, shared).await?;
        }
        Ok(true)
    }

    pub async fn delete_go_playlist(&self, user_id: i64, id: i64) -> Result<bool> {
        if self.playlist_owner(id).await? != Some(user_id) {
            return Ok(false);
        }
        self.delete_playlist(id).await
    }

    /// Sets the songs of one of `user_id`'s playlists. False if it isn't
    /// theirs.
    pub async fn set_go_playlist_tracks(
        &self,
        user_id: i64,
        id: i64,
        track_ids: &[i64],
    ) -> Result<bool> {
        if self.playlist_owner(id).await? != Some(user_id) {
            return Ok(false);
        }
        self.set_playlist_songs(id, track_ids).await?;
        Ok(true)
    }

    /// Adds songs to the end of one of `user_id`'s playlists.
    pub async fn add_to_go_playlist(
        &self,
        user_id: i64,
        id: i64,
        track_ids: &[i64],
    ) -> Result<bool> {
        if self.playlist_owner(id).await? != Some(user_id) {
            return Ok(false);
        }
        self.add_playlist_songs(id, track_ids).await?;
        Ok(true)
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
            [ids[2], ids[0]],
            "order kept, a repeat holds its first place"
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
