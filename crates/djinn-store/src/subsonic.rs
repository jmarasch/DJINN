//! What the Subsonic API on DJINN GO needs beyond the rest of the store:
//! app passwords, albums with their song counts, and song files.

use serde::Serialize;
use sqlx::FromRow;

use crate::library::artist_counts;
use crate::{now_ms, Result, Store, User};

/// An album as Subsonic lists it.
#[derive(Clone, Debug, PartialEq, Serialize, FromRow)]
pub struct SsAlbum {
    pub id: i64,
    pub name: String,
    pub artist: String,
    pub artist_id: i64,
    pub year: Option<i64>,
    pub art_hash: Option<String>,
    pub song_count: i64,
    pub duration_ms: i64,
    /// When its first song was added, Unix ms.
    pub created: i64,
    pub genre: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct SsArtist {
    pub id: i64,
    pub name: String,
    pub art_hash: Option<String>,
    pub album_count: i64,
}

/// A song's file, for sizes, types and streaming.
#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct TrackFile {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub added_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, FromRow)]
pub struct Genre {
    pub name: String,
    pub songs: i64,
    pub albums: i64,
}

/// Which albums `getAlbumList2` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlbumList {
    Random,
    Newest,
    ByName,
    ByArtist,
    /// Inclusive; newest first when `from` is the later year.
    ByYear {
        from: i64,
        to: i64,
    },
    ByGenre(String),
    Starred(i64),
    /// The user's most recently played.
    Recent(i64),
    /// The user's most played.
    Frequent(i64),
}

macro_rules! ss_album {
    () => {
        "SELECT al.id, al.title AS name, ar.name AS artist, al.artist_id, al.year, al.art_hash, \
         count(t.id) AS song_count, coalesce(sum(t.duration_ms), 0) AS duration_ms, \
         min(t.added_at) AS created, \
         (SELECT t2.genre FROM tracks t2 WHERE t2.album_id = al.id AND t2.genre IS NOT NULL \
          AND t2.removed_at IS NULL LIMIT 1) AS genre \
         FROM albums al JOIN artists ar ON ar.id = al.artist_id \
         JOIN tracks t ON t.album_id = al.id AND t.removed_at IS NULL "
    };
}

impl Store {
    pub async fn ss_albums(
        &self,
        list: &AlbumList,
        offset: i64,
        size: i64,
    ) -> Result<Vec<SsAlbum>> {
        let q = match list {
            AlbumList::Random => sqlx::query_as(concat!(
                ss_album!(),
                "GROUP BY al.id ORDER BY random() LIMIT ? OFFSET ?"
            )),
            AlbumList::Newest => sqlx::query_as(concat!(
                ss_album!(),
                "GROUP BY al.id ORDER BY created DESC, al.id DESC LIMIT ? OFFSET ?"
            )),
            AlbumList::ByName => sqlx::query_as(concat!(
                ss_album!(),
                "GROUP BY al.id ORDER BY al.title COLLATE NOCASE LIMIT ? OFFSET ?"
            )),
            AlbumList::ByArtist => sqlx::query_as(concat!(
                ss_album!(),
                "GROUP BY al.id ORDER BY ar.name COLLATE NOCASE, al.year, al.title COLLATE NOCASE
                 LIMIT ? OFFSET ?"
            )),
            AlbumList::ByYear { from, to } if from <= to => sqlx::query_as(concat!(
                ss_album!(),
                "WHERE al.year BETWEEN ? AND ? GROUP BY al.id ORDER BY al.year, al.title
                 LIMIT ? OFFSET ?"
            ))
            .bind(*from)
            .bind(*to),
            AlbumList::ByYear { from, to } => sqlx::query_as(concat!(
                ss_album!(),
                "WHERE al.year BETWEEN ? AND ? GROUP BY al.id ORDER BY al.year DESC, al.title
                 LIMIT ? OFFSET ?"
            ))
            .bind(*to)
            .bind(*from),
            AlbumList::ByGenre(genre) => sqlx::query_as(concat!(
                ss_album!(),
                "WHERE t.genre = ? COLLATE NOCASE GROUP BY al.id ORDER BY al.title COLLATE NOCASE
                 LIMIT ? OFFSET ?"
            ))
            .bind(genre.clone()),
            AlbumList::Starred(user) => sqlx::query_as(concat!(
                ss_album!(),
                "WHERE al.id IN (SELECT ref_id FROM go_likes WHERE user_id = ? AND kind = 'album')
                 GROUP BY al.id ORDER BY al.title COLLATE NOCASE LIMIT ? OFFSET ?"
            ))
            .bind(*user),
            AlbumList::Recent(user) => sqlx::query_as(concat!(
                ss_album!(),
                "JOIN (SELECT t3.album_id, max(gp.at) AS last FROM go_plays gp
                       JOIN tracks t3 ON t3.id = gp.track_id WHERE gp.user_id = ?
                       GROUP BY t3.album_id) p ON p.album_id = al.id
                 GROUP BY al.id ORDER BY max(p.last) DESC LIMIT ? OFFSET ?"
            ))
            .bind(*user),
            AlbumList::Frequent(user) => sqlx::query_as(concat!(
                ss_album!(),
                "JOIN (SELECT t3.album_id, count(*) AS plays FROM go_plays gp
                       JOIN tracks t3 ON t3.id = gp.track_id WHERE gp.user_id = ?
                       GROUP BY t3.album_id) p ON p.album_id = al.id
                 GROUP BY al.id ORDER BY max(p.plays) DESC LIMIT ? OFFSET ?"
            ))
            .bind(*user),
        };
        let albums = q.bind(size).bind(offset).fetch_all(&self.pool).await?;
        Ok(albums)
    }

    pub async fn ss_album(&self, id: i64) -> Result<Option<SsAlbum>> {
        let album = sqlx::query_as(concat!(ss_album!(), "WHERE al.id = ? GROUP BY al.id"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(album)
    }

    /// Artists with playable songs, A to Z, with how many albums they have.
    pub async fn ss_artists(&self) -> Result<Vec<SsArtist>> {
        let artists = sqlx::query_as(concat!(
            "SELECT c.id, c.name, c.art_hash,
                    (SELECT count(*) FROM albums al WHERE al.artist_id = c.id
                       AND EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = al.id
                                   AND t.removed_at IS NULL)) AS album_count
             FROM (",
            artist_counts!(),
            ") c WHERE c.track_count > 0 ORDER BY c.name COLLATE NOCASE"
        ))
        .fetch_all(&self.pool)
        .await?;
        Ok(artists)
    }

    /// File details for songs, in no particular order.
    pub async fn track_files(&self, ids: &[i64]) -> Result<Vec<TrackFile>> {
        let list = format!(
            "[{}]",
            ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
        );
        let files = sqlx::query_as(
            "SELECT id, path, size, added_at FROM tracks
             WHERE id IN (SELECT value FROM json_each(?)) AND removed_at IS NULL",
        )
        .bind(list)
        .fetch_all(&self.pool)
        .await?;
        Ok(files)
    }

    /// Genres as tagged, with how many songs and albums have each.
    pub async fn ss_genres(&self) -> Result<Vec<Genre>> {
        let genres = sqlx::query_as(
            "SELECT t.genre AS name, count(*) AS songs, count(DISTINCT t.album_id) AS albums
             FROM tracks t WHERE t.removed_at IS NULL AND t.genre IS NOT NULL AND t.genre != ''
             GROUP BY t.genre COLLATE NOCASE ORDER BY t.genre COLLATE NOCASE",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(genres)
    }

    pub async fn song_ids_by_genre(
        &self,
        genre: &str,
        offset: i64,
        count: i64,
    ) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(
            "SELECT t.id FROM tracks t
             WHERE t.removed_at IS NULL AND t.genre = ? COLLATE NOCASE
             ORDER BY t.artist COLLATE NOCASE, t.album COLLATE NOCASE, t.disc_no, t.track_no
             LIMIT ? OFFSET ?",
        )
        .bind(genre)
        .bind(count)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    /// Random songs, optionally of a genre and between years.
    pub async fn random_song_ids(
        &self,
        count: i64,
        genre: Option<&str>,
        from_year: Option<i64>,
        to_year: Option<i64>,
    ) -> Result<Vec<i64>> {
        let ids = sqlx::query_scalar(
            "SELECT t.id FROM tracks t
             WHERE t.removed_at IS NULL
               AND (? IS NULL OR t.genre = ? COLLATE NOCASE)
               AND (? IS NULL OR t.year >= ?)
               AND (? IS NULL OR t.year <= ?)
             ORDER BY random() LIMIT ?",
        )
        .bind(genre)
        .bind(genre)
        .bind(from_year)
        .bind(from_year)
        .bind(to_year)
        .bind(to_year)
        .bind(count)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids)
    }

    /// The account's Subsonic app password, if it has one.
    pub async fn subsonic_key(&self, user_id: i64) -> Result<Option<String>> {
        let key = sqlx::query_scalar("SELECT secret FROM subsonic_keys WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(key)
    }

    /// Sets (or replaces) the account's app password.
    pub async fn set_subsonic_key(&self, user_id: i64, secret: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO subsonic_keys (user_id, secret, created_at) VALUES (?, ?, ?)
             ON CONFLICT (user_id) DO UPDATE SET secret = excluded.secret,
                created_at = excluded.created_at, last_used_at = NULL",
        )
        .bind(user_id)
        .bind(secret)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_subsonic_key(&self, user_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM subsonic_keys WHERE user_id = ?")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The active account with this username and its app password.
    pub async fn subsonic_account(&self, username: &str) -> Result<Option<(User, String)>> {
        #[derive(FromRow)]
        struct Row {
            #[sqlx(flatten)]
            user: User,
            secret: String,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT u.id, u.username, u.display_name, u.role, u.unlimited, u.mac, u.created_at,
                    u.role_id, u.status, u.totp_secret IS NOT NULL AS two_step, k.secret
             FROM users u JOIN subsonic_keys k ON k.user_id = u.id
             WHERE u.username = ? AND u.status = 'active'",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| (r.user, r.secret)))
    }

    /// The active account an app password (as an API key) belongs to.
    pub async fn subsonic_key_account(&self, secret: &str) -> Result<Option<User>> {
        let user = sqlx::query_as(
            "SELECT u.id, u.username, u.display_name, u.role, u.unlimited, u.mac, u.created_at,
                    u.role_id, u.status, u.totp_secret IS NOT NULL AS two_step
             FROM users u JOIN subsonic_keys k ON k.user_id = u.id
             WHERE k.secret = ? AND u.status = 'active'",
        )
        .bind(secret)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn touch_subsonic_key(&self, user_id: i64) -> Result<()> {
        sqlx::query("UPDATE subsonic_keys SET last_used_at = ? WHERE user_id = ?")
            .bind(now_ms())
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, ScannedTrack};

    #[tokio::test]
    async fn albums_genres_and_keys() {
        let store = Store::open_in_memory().await.unwrap();
        let songs: Vec<ScannedTrack> = (0..4)
            .map(|i| ScannedTrack {
                path: format!("/m/{i}.flac"),
                size: 1000 + i,
                title: format!("Song {i}"),
                artist: Some(if i < 2 { "Abba" } else { "Zed" }.into()),
                album: Some(if i < 2 { "First" } else { "Second" }.into()),
                year: Some(1970 + i),
                genre: Some("Pop".into()),
                duration_ms: Some(60_000),
                ..ScannedTrack::default()
            })
            .collect();
        store.save_scanned(&songs).await.unwrap();

        let by_name = store.ss_albums(&AlbumList::ByName, 0, 10).await.unwrap();
        assert_eq!(by_name.len(), 2);
        assert_eq!(
            (by_name[0].name.as_str(), by_name[0].song_count),
            ("First", 2)
        );
        assert_eq!(by_name[0].duration_ms, 120_000);
        let years = store
            .ss_albums(
                &AlbumList::ByYear {
                    from: 1980,
                    to: 1960,
                },
                0,
                10,
            )
            .await
            .unwrap();
        assert_eq!(years[0].name, "Second", "newest first");
        assert_eq!(
            store
                .ss_albums(&AlbumList::ByGenre("pop".into()), 0, 10)
                .await
                .unwrap()
                .len(),
            2
        );
        let artists = store.ss_artists().await.unwrap();
        assert_eq!(
            artists
                .iter()
                .map(|a| (a.name.as_str(), a.album_count))
                .collect::<Vec<_>>(),
            [("Abba", 1), ("Zed", 1)]
        );
        let genres = store.ss_genres().await.unwrap();
        assert_eq!((genres[0].songs, genres[0].albums), (4, 2));
        assert_eq!(
            store
                .random_song_ids(10, Some("pop"), Some(1972), None)
                .await
                .unwrap()
                .len(),
            2
        );
        let ids = store.song_ids_by_genre("Pop", 0, 10).await.unwrap();
        assert_eq!(store.track_files(&ids).await.unwrap().len(), 4);

        let sam = store.create_user("Sam", "h", Role::User).await.unwrap();
        assert_eq!(store.subsonic_account("sam").await.unwrap(), None);
        store.set_subsonic_key(sam.id, "s3cret").await.unwrap();
        let (user, secret) = store.subsonic_account("SAM").await.unwrap().unwrap();
        assert_eq!((user.id, secret.as_str()), (sam.id, "s3cret"));
        assert_eq!(
            store
                .subsonic_key_account("s3cret")
                .await
                .unwrap()
                .unwrap()
                .id,
            sam.id
        );
        store.delete_subsonic_key(sam.id).await.unwrap();
        assert_eq!(store.subsonic_key(sam.id).await.unwrap(), None);
    }
}
