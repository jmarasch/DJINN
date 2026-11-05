//! Albums with one name that are really different releases. Folders alone
//! don't tell: most albums across several folders are one album's discs. What
//! does is two different songs at one place on the album, such as two "track
//! 1"s of disc 1, in different folders.

use super::*;
use std::collections::{BTreeMap, HashSet};

/// Whether albums with one name stay apart by folder. On unless turned off.
pub const ALBUM_FOLDERS_KEY: &str = "album_folders";

/// One song of an album, as far as telling releases apart goes.
#[derive(Clone, Debug)]
struct AlbumSong {
    id: i64,
    path: String,
    disc: Option<i64>,
    track: Option<i64>,
}

/// One release of an album: its songs, and the folder it's named for.
#[derive(Debug, PartialEq)]
struct Part {
    folder: String,
    songs: Vec<i64>,
}

/// "CD1", "Disc 2", "disk-03", "2": a folder that's only a disc of what's around it.
fn is_disc_folder(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let rest = ["disc", "disk", "cd"]
        .iter()
        .find_map(|word| name.strip_prefix(word))
        .map_or_else(
            // Bare numbers only when short: "1984" is a name.
            || if name.len() <= 2 { name.as_str() } else { "" },
            |rest| rest.trim_start_matches([' ', '-', '_', '.']),
        );
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
}

/// A folder's name without any "CD 1" or "(Disc 2)" in it, for comparing.
fn without_disc_mark(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut out = String::new();
    let mut rest = lower.as_str();
    while let Some(c) = rest.chars().next() {
        let word_start = !out.chars().last().is_some_and(char::is_alphanumeric);
        let marked = word_start
            .then(|| {
                ["disc", "disk", "cd"]
                    .iter()
                    .find_map(|w| rest.strip_prefix(w))
            })
            .flatten()
            .and_then(|after| {
                let number = after.trim_start_matches([' ', '-', '_', '.']);
                let tail = number.trim_start_matches(|c: char| c.is_ascii_digit());
                (tail.len() < number.len()).then_some(tail)
            });
        if let Some(tail) = marked {
            rest = tail;
        } else {
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out.trim_matches(|c: char| !c.is_alphanumeric()).to_owned()
}

/// The folder a song's release lives in: what to compare by, and its name to show.
fn release_folder(path: &str) -> (String, String) {
    let parent = |p: &str| p.rfind(['/', '\\']).map(|at| p[..at].to_owned());
    let name = |p: &str| p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned();
    let mut folder = parent(path).unwrap_or_default();
    if is_disc_folder(&name(&folder)) {
        if let Some(above) = parent(&folder) {
            folder = above;
        }
    }
    let shown = name(&folder);
    let above = parent(&folder).unwrap_or_default();
    (format!("{above}/{}", without_disc_mark(&shown)), shown)
}

/// Sorts an album's songs into its releases, biggest first. Each folder joins
/// the first release it has no clash with; a folder of one song stays with the
/// biggest, since one file in the wrong place isn't a release.
fn parts(songs: &[AlbumSong]) -> Vec<Part> {
    let mut folders: BTreeMap<String, (String, Vec<&AlbumSong>)> = BTreeMap::new();
    for song in songs {
        let (key, shown) = release_folder(&song.path);
        folders
            .entry(key)
            .or_insert_with(|| (shown, Vec::new()))
            .1
            .push(song);
    }
    let mut folders: Vec<(String, Vec<&AlbumSong>)> = folders.into_values().collect();
    // Stable, so folders of one size stay in name order.
    folders.sort_by_key(|(_, songs)| std::cmp::Reverse(songs.len()));

    let place = |song: &AlbumSong| song.track.map(|track| (song.disc.unwrap_or(1), track));
    let mut found: Vec<(Part, HashSet<(i64, i64)>)> = Vec::new();
    for (folder, songs) in folders {
        let places: HashSet<(i64, i64)> = songs.iter().filter_map(|s| place(s)).collect();
        let join = if songs.len() == 1 && !found.is_empty() {
            Some(0)
        } else {
            found
                .iter()
                .position(|(_, taken)| taken.is_disjoint(&places))
        };
        let ids = songs.iter().map(|s| s.id);
        if let Some(at) = join {
            found[at].0.songs.extend(ids);
            found[at].1.extend(places);
        } else {
            let songs = ids.collect();
            found.push((Part { folder, songs }, places));
        }
    }
    found.into_iter().map(|(part, _)| part).collect()
}

type Row = (
    i64,
    String,
    Option<i64>,
    Option<i64>,
    i64,
    String,
    i64,
    String,
    Option<String>,
);

/// Puts each album's songs in the right release: the biggest keeps the album
/// (and its likes and playlist entries), each other gets one of its own.
/// Scanning files them all under the name again, so this runs after each scan.
pub(super) async fn sort_editions(tx: &mut Transaction<'_, Sqlite>, apart: bool) -> Result<()> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT t.id, t.path, t.disc_no, t.track_no, al.id, al.title, al.artist_id, al.edition, al.label
         FROM tracks t JOIN albums al ON al.id = t.album_id
         WHERE t.removed_at IS NULL",
    )
    .fetch_all(&mut **tx)
    .await?;
    let mut albums: BTreeMap<(String, i64), Vec<Row>> = BTreeMap::new();
    for row in rows {
        // The table compares names this way too.
        albums
            .entry((row.5.to_ascii_lowercase(), row.6))
            .or_default()
            .push(row);
    }
    for rows in albums.values() {
        let songs: Vec<AlbumSong> = rows
            .iter()
            .map(|r| AlbumSong {
                id: r.0,
                path: r.1.clone(),
                disc: r.2,
                track: r.3,
            })
            .collect();
        let found = if apart {
            parts(&songs)
        } else {
            vec![Part {
                folder: String::new(),
                songs: songs.iter().map(|s| s.id).collect(),
            }]
        };
        let whole = found.len() == 1;
        // Nearly every album: one release, as it already is.
        if whole && rows.iter().all(|r| r.7.is_empty() && r.8.is_none()) {
            continue;
        }
        let (title, artist) = (&rows[0].5, rows[0].6);
        for (i, part) in found.iter().enumerate() {
            let edition = if i == 0 { "" } else { part.folder.as_str() };
            let album: i64 = sqlx::query_scalar(
                "INSERT INTO albums (title, artist_id, edition) VALUES (?, ?, ?)
                 ON CONFLICT (title, artist_id, edition) DO UPDATE SET edition = excluded.edition
                 RETURNING id",
            )
            .bind(title)
            .bind(artist)
            .bind(edition)
            .fetch_one(&mut **tx)
            .await?;
            let label = (!whole).then_some(part.folder.as_str());
            sqlx::query("UPDATE albums SET label = ? WHERE id = ? AND label IS NOT ?")
                .bind(label)
                .bind(album)
                .bind(label)
                .execute(&mut **tx)
                .await?;
            for song in &part.songs {
                sqlx::query("UPDATE tracks SET album_id = ? WHERE id = ? AND album_id IS NOT ?")
                    .bind(album)
                    .bind(song)
                    .bind(album)
                    .execute(&mut **tx)
                    .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Songs numbered from one, in a folder.
    fn folder(first_id: i64, folder: &str, songs: i64, disc: Option<i64>) -> Vec<AlbumSong> {
        (0..songs)
            .map(|i| AlbumSong {
                id: first_id + i,
                path: format!("/m/{folder}/{i}.mp3"),
                disc,
                track: Some(i + 1),
            })
            .collect()
    }

    fn found(lists: &[Vec<AlbumSong>]) -> Vec<(String, usize)> {
        parts(&lists.concat())
            .into_iter()
            .map(|p| (p.folder, p.songs.len()))
            .collect()
    }

    #[test]
    fn different_releases_with_one_name_come_apart() {
        let blue = folder(1, "Weezer/(1994) Weezer (Blue Album)", 10, Some(1));
        let green = folder(20, "Weezer/(2001) Weezer (Green Album)", 10, None);
        assert_eq!(
            found(&[blue, green]),
            [
                ("(1994) Weezer (Blue Album)".to_owned(), 10),
                ("(2001) Weezer (Green Album)".to_owned(), 10)
            ]
        );
    }

    #[test]
    fn three_releases_make_three() {
        let lists = [
            folder(1, "NIN/Halo 20 - Only", 3, Some(1)),
            folder(10, "NIN/Halo 21a - Only", 3, None),
            folder(20, "NIN/Halo 21b - Only (Enhanced)", 3, Some(1)),
        ];
        assert_eq!(found(&lists).len(), 3);
    }

    #[test]
    fn folders_with_their_own_disc_numbers_are_one_album() {
        let lists = [
            folder(1, "NIN/Halo 14 - The Fragile - Left", 12, Some(1)),
            folder(20, "NIN/Halo 14 - The Fragile - Right", 11, Some(2)),
        ];
        assert_eq!(found(&lists).len(), 1);
    }

    #[test]
    fn disc_folders_are_one_album_without_disc_tags() {
        for (one, two) in [
            ("Band/Album/CD1", "Band/Album/CD2"),
            ("Band/Album/Disc 1", "Band/Album/Disc 2"),
            ("Band/Album/1", "Band/Album/2"),
            ("Band/Album (CD 1) [FLAC]", "Band/Album (CD 2) [FLAC]"),
            ("Band/(1993) Show Disk-01", "Band/(1993) Show Disk-02"),
        ] {
            let lists = [folder(1, one, 9, None), folder(20, two, 9, None)];
            assert_eq!(found(&lists).len(), 1, "{one} and {two}");
        }
    }

    #[test]
    fn a_year_or_a_name_with_a_number_isnt_a_disc() {
        assert!(!is_disc_folder("1984"));
        assert!(!is_disc_folder("CDs"));
        assert!(!is_disc_folder("Discovery"));
        assert_eq!(without_disc_mark("Discovery 2"), "discovery 2");
        assert_eq!(without_disc_mark("Abcd 1"), "abcd 1");
    }

    #[test]
    fn songs_that_interlock_across_folders_are_one_album() {
        let mut a = folder(1, "NIN/Halo 09 - Closer to God", 2, Some(1));
        let mut b = folder(10, "NIN/Halo 09 - Further Away", 2, Some(1));
        a[1].track = Some(4);
        b[0].track = Some(2);
        b[1].track = Some(3);
        assert_eq!(found(&[a, b]).len(), 1);
    }

    #[test]
    fn a_lone_stray_stays_with_the_album_it_names() {
        let lists = [
            folder(1, "Band/Do or Die", 16, Some(1)),
            folder(30, "Band/The Gang's All Here", 1, Some(1)),
        ];
        assert_eq!(found(&lists), [("Do or Die".to_owned(), 17)]);
    }

    #[test]
    fn songs_scattered_one_to_a_folder_stay_together() {
        let lists = [
            folder(1, "Hits/a", 1, None),
            folder(2, "Hits/b", 1, None),
            folder(3, "Hits/c", 1, None),
        ];
        assert_eq!(found(&lists).len(), 1);
    }

    #[test]
    fn songs_without_numbers_dont_clash() {
        let mut a = folder(1, "Band/One", 3, None);
        let mut b = folder(10, "Band/Two", 3, None);
        for song in a.iter_mut().chain(b.iter_mut()) {
            song.track = None;
        }
        assert_eq!(found(&[a, b]).len(), 1);
    }

    #[test]
    fn windows_paths_work() {
        let (key, shown) = release_folder(r"D:\Music\Band\Album\CD1\01.flac");
        assert_eq!(shown, "Album");
        assert_eq!(key, r"D:\Music\Band/album");
    }

    async fn album_rows(store: &Store) -> Vec<(String, String, Option<String>, i64, Option<i64>)> {
        sqlx::query_as(
            "SELECT al.title, al.edition, al.label, count(t.id), al.year FROM albums al
             JOIN tracks t ON t.album_id = al.id GROUP BY al.id ORDER BY al.edition",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap()
    }

    fn scanned(lists: &[(&str, i64, i64)]) -> Vec<ScannedTrack> {
        lists
            .iter()
            .flat_map(|&(folder, songs, year)| {
                (1..=songs).map(move |i| ScannedTrack {
                    path: format!("/m/Weezer/{folder}/{i}.flac"),
                    size: 1,
                    mtime: 1,
                    title: format!("{folder} {i}"),
                    artist: Some("Weezer".into()),
                    album: Some("Weezer".into()),
                    album_artist: Some("Weezer".into()),
                    credits: vec![("Weezer".into(), false)],
                    track_no: Some(i),
                    year: Some(year),
                    ..Default::default()
                })
            })
            .collect()
    }

    #[tokio::test]
    async fn a_scan_splits_them_keeps_them_split_and_gives_each_its_year() {
        let store = Store::open_in_memory().await.unwrap();
        let songs = scanned(&[("Blue", 10, 1994), ("Green", 9, 2001)]);
        store.save_scanned(&songs).await.unwrap();
        let original: i64 = sqlx::query_scalar("SELECT id FROM albums")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        let expected = vec![
            (
                "Weezer".to_owned(),
                String::new(),
                Some("Blue".to_owned()),
                10,
                Some(1994),
            ),
            (
                "Weezer".to_owned(),
                "Green".to_owned(),
                Some("Green".to_owned()),
                9,
                Some(2001),
            ),
        ];
        assert_eq!(album_rows(&store).await, expected);
        let kept: i64 = sqlx::query_scalar("SELECT id FROM albums WHERE edition = ''")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            kept, original,
            "the biggest part keeps the album, so likes stay"
        );

        // Reading the files again files them under the name, until sorted.
        store.save_scanned(&songs).await.unwrap();
        store.refresh_artwork().await.unwrap();
        assert_eq!(album_rows(&store).await, expected);
    }

    #[tokio::test]
    async fn turning_it_off_puts_them_back_together() {
        let store = Store::open_in_memory().await.unwrap();
        store
            .save_scanned(&scanned(&[("Blue", 10, 1994), ("Green", 9, 2001)]))
            .await
            .unwrap();
        store.refresh_artwork().await.unwrap();
        store.set_setting(ALBUM_FOLDERS_KEY, "0").await.unwrap();
        store.refresh_artwork().await.unwrap();
        assert_eq!(
            album_rows(&store).await,
            [("Weezer".to_owned(), String::new(), None, 19, Some(1994))]
        );
        let albums: i64 = sqlx::query_scalar("SELECT count(*) FROM albums")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(albums, 1, "the empty part is gone");
    }
}
