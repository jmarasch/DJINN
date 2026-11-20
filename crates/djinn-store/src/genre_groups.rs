//! Genre containers (Library settings > Genres): similar genres grouped under
//! one name. Only names are kept; the library's genres are never changed.

use serde::Serialize;
use sqlx::FromRow;

use crate::{now_ms, Result, Store};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct GenreGroup {
    pub id: i64,
    pub name: String,
    /// Made by Auto-group (replaced when it runs again).
    pub auto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, FromRow)]
pub struct GenreMember {
    pub genre: String,
    /// None: taken out of every container by hand.
    pub group_id: Option<i64>,
    /// Placed by hand.
    pub pinned: bool,
}

impl Store {
    pub async fn genre_groups(&self) -> Result<(Vec<GenreGroup>, Vec<GenreMember>)> {
        let groups = sqlx::query_as("SELECT id, name, auto FROM genre_groups ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        let members = sqlx::query_as(
            "SELECT genre, group_id, pinned FROM genre_group_members ORDER BY genre",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok((groups, members))
    }

    /// Each genre in a container, and the container's name.
    pub async fn genre_fold(&self) -> Result<Vec<(String, String)>> {
        Ok(sqlx::query_as(
            "SELECT m.genre, g.name FROM genre_group_members m
             JOIN genre_groups g ON g.id = m.group_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    /// A new, empty container someone made. None when the name is taken.
    pub async fn create_genre_group(&self, name: &str) -> Result<Option<i64>> {
        let done = sqlx::query(
            "INSERT INTO genre_groups (name, auto, created_at) VALUES (?, 0, ?)
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(name)
        .bind(now_ms())
        .execute(&self.pool)
        .await?;
        Ok((done.rows_affected() > 0).then(|| done.last_insert_rowid()))
    }

    /// Renames a container, which makes it one Auto-group keeps. False when
    /// it's gone or the name is taken.
    pub async fn rename_genre_group(&self, id: i64, name: &str) -> Result<bool> {
        let taken: Option<i64> =
            sqlx::query_scalar("SELECT id FROM genre_groups WHERE name = ? AND id != ?")
                .bind(name)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        if taken.is_some() {
            return Ok(false);
        }
        let done = sqlx::query("UPDATE genre_groups SET name = ?, auto = 0 WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Removes a container; its genres are free again.
    pub async fn delete_genre_group(&self, id: i64) -> Result<bool> {
        let done = sqlx::query("DELETE FROM genre_groups WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Puts a genre in a container by hand (or, with None, in none). The
    /// container becomes one Auto-group keeps. False when it's gone.
    pub async fn place_genre(&self, genre: &str, group: Option<i64>) -> Result<bool> {
        let mut tx = self.write().await?;
        if let Some(id) = group {
            let done = sqlx::query("UPDATE genre_groups SET auto = 0 WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            if done.rows_affected() == 0 {
                return Ok(false);
            }
        }
        sqlx::query(
            "INSERT INTO genre_group_members (genre, group_id, pinned) VALUES (?, ?, 1)
             ON CONFLICT (genre) DO UPDATE SET group_id = excluded.group_id, pinned = 1",
        )
        .bind(genre)
        .bind(group)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Lets a genre placed by hand go back to Auto-group.
    pub async fn free_genre(&self, genre: &str) -> Result<()> {
        sqlx::query("DELETE FROM genre_group_members WHERE genre = ?")
            .bind(genre)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Replaces the containers Auto-group made with `groups` (name and
    /// genres). Containers someone made or changed, and genres placed by
    /// hand, stay as they are; a name already taken gets a number.
    pub async fn replace_auto_genre_groups(&self, groups: &[(String, Vec<String>)]) -> Result<()> {
        let mut tx = self.write().await?;
        sqlx::query("DELETE FROM genre_groups WHERE auto = 1")
            .execute(&mut *tx)
            .await?;
        let now = now_ms();
        for (name, genres) in groups {
            let mut id: Option<i64> = None;
            for n in 1..100 {
                let tried = if n == 1 {
                    name.clone()
                } else {
                    format!("{name} {n}")
                };
                let done = sqlx::query(
                    "INSERT INTO genre_groups (name, auto, created_at) VALUES (?, 1, ?)
                     ON CONFLICT (name) DO NOTHING",
                )
                .bind(&tried)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                if done.rows_affected() > 0 {
                    id = Some(done.last_insert_rowid());
                    break;
                }
            }
            let Some(id) = id else { continue };
            for genre in genres {
                sqlx::query(
                    "INSERT INTO genre_group_members (genre, group_id, pinned) VALUES (?, ?, 0)
                     ON CONFLICT (genre) DO NOTHING",
                )
                .bind(genre)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            }
        }
        // An auto container that got no genres (all placed elsewhere) goes.
        sqlx::query(
            "DELETE FROM genre_groups WHERE auto = 1
               AND NOT EXISTS (SELECT 1 FROM genre_group_members m WHERE m.group_id = genre_groups.id)",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Takes every container and placement away.
    pub async fn clear_genre_groups(&self) -> Result<()> {
        let mut tx = self.write().await?;
        sqlx::query("DELETE FROM genre_group_members")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM genre_groups")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn auto_group_keeps_what_was_done_by_hand() {
        let store = Store::open_in_memory().await.unwrap();
        let g =
            |n: &str, list: &[&str]| (n.to_owned(), list.iter().map(|s| (*s).to_owned()).collect());
        store
            .replace_auto_genre_groups(&[
                g("Metal", &["metal", "black metal"]),
                g("House", &["house", "deep house"]),
            ])
            .await
            .unwrap();
        let fold = store.genre_fold().await.unwrap();
        assert_eq!(fold.len(), 4);
        // By hand: a container of one's own, polka in it, deep house taken out.
        let mine = store.create_genre_group("Odd").await.unwrap().unwrap();
        assert_eq!(
            store.create_genre_group("odd").await.unwrap(),
            None,
            "names ignore case"
        );
        store.place_genre("polka", Some(mine)).await.unwrap();
        store.place_genre("deep house", None).await.unwrap();
        // Moving a genre into an auto container makes it kept.
        let (groups, _) = store.genre_groups().await.unwrap();
        let metal = groups.iter().find(|g| g.name == "Metal").unwrap().id;
        store.place_genre("speed metal", Some(metal)).await.unwrap();
        // Auto-group again, wanting deep house and polka elsewhere.
        store
            .replace_auto_genre_groups(&[
                g("Dance", &["house", "deep house", "polka"]),
                g("Metal", &["doom metal"]),
            ])
            .await
            .unwrap();
        let mut fold = store.genre_fold().await.unwrap();
        fold.sort();
        let want: Vec<(String, String)> = [
            ("black metal", "Metal"),
            ("doom metal", "Metal 2"),
            ("house", "Dance"),
            ("metal", "Metal"),
            ("polka", "Odd"),
            ("speed metal", "Metal"),
        ]
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
        assert_eq!(fold, want);
        // Renaming keeps a container; deleting frees its genres.
        let (groups, _) = store.genre_groups().await.unwrap();
        let dance = groups.iter().find(|g| g.name == "Dance").unwrap().id;
        assert!(store.rename_genre_group(dance, "Club").await.unwrap());
        assert!(
            !store.rename_genre_group(dance, "metal").await.unwrap(),
            "taken"
        );
        store.replace_auto_genre_groups(&[]).await.unwrap();
        let names: Vec<String> = store
            .genre_groups()
            .await
            .unwrap()
            .0
            .into_iter()
            .map(|g| g.name)
            .collect();
        assert_eq!(names, ["Club", "Metal", "Odd"]);
        assert!(store.delete_genre_group(mine).await.unwrap());
        assert!(store
            .genre_fold()
            .await
            .unwrap()
            .iter()
            .all(|(g, _)| g != "polka"));
        store.clear_genre_groups().await.unwrap();
        assert!(store.genre_fold().await.unwrap().is_empty());
    }
}
