use super::browse::{fts_query, fts_starts_with};
use super::*;

fn scanned(path: &str, title: &str, artist: &str, album: &str) -> ScannedTrack {
    ScannedTrack {
        path: path.to_owned(),
        size: 1,
        mtime: 1,
        title: title.to_owned(),
        artist: Some(artist.to_owned()),
        album: Some(album.to_owned()),
        ..Default::default()
    }
}

#[test]
fn fts_query_quotes_prefix_terms() {
    assert_eq!(fts_query("beat it").as_deref(), Some("\"beat\"* \"it\"*"));
    assert_eq!(fts_query("AC/DC").as_deref(), Some("\"AC\"* \"DC\"*"));
    assert_eq!(fts_query("\" OR *").as_deref(), Some("\"OR\"*"));
    assert_eq!(fts_query("  -- "), None);
}

#[test]
fn days_land_in_the_right_year() {
    for (days, year) in [
        (0, 1970),
        (20818, 2026),
        (20819, 2027),
        (21243, 2028),
        (21244, 2028),
        (47482, 2100),
    ] {
        assert_eq!(year_of_day(days), year, "day {days}");
    }
}

#[test]
fn a_year_can_be_next_year_at_the_latest() {
    let latest = latest_year();
    // Whenever this runs, and this was written in 2026.
    assert!(latest >= 2027);
    assert_eq!(real_year(1997), Some(1997));
    assert_eq!(real_year(latest), Some(latest));
    assert_eq!(real_year(latest + 1), None);
    assert_eq!(real_year(9999), None);
    assert_eq!(real_year(0), None);
}

#[tokio::test]
async fn impossible_years_are_never_saved() {
    let store = Store::open_in_memory().await.unwrap();
    let mut song = scanned("/m/1.flac", "Song", "Band", "Album");
    song.year = Some(latest_year() + 1);
    store.save_scanned(&[song]).await.unwrap();
    let track: (Option<i64>,) = sqlx::query_as("SELECT year FROM tracks")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let album: (Option<i64>,) = sqlx::query_as("SELECT year FROM albums")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!((track.0, album.0), (None, None));
}

/// The jukebox, GO on the web, the app and Subsonic apps all count, and a song
/// skipped early doesn't.
#[tokio::test]
async fn every_play_counts_toward_whats_popular() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[
            scanned("/m/a.flac", "Listened", "Band", "Album"),
            scanned("/m/b.flac", "Skipped", "Band", "Album"),
        ])
        .await
        .unwrap();
    let id = |title: &'static str| {
        let store = &store;
        async move {
            sqlx::query_scalar::<_, i64>("SELECT id FROM tracks WHERE title = ?")
                .bind(title)
                .fetch_one(&store.pool)
                .await
                .unwrap()
        }
    };
    let (listened, skipped) = (id("Listened").await, id("Skipped").await);
    let user = store
        .create_user("sam", "h", crate::Role::User)
        .await
        .unwrap()
        .id;
    store
        .record_go_play(user, listened, crate::SKIP_BEFORE_MS)
        .await
        .unwrap();
    store.record_go_play(user, skipped, 1).await.unwrap();
    let plays = store.song_popularity().await.unwrap();
    assert_eq!(plays.get(&listened), Some(&1));
    assert_eq!(plays.get(&skipped), None);
    let facts = store.track_facts().await.unwrap();
    let popularity = |id: i64| facts.iter().find(|f| f.id == id).unwrap().popularity;
    assert_eq!((popularity(listened), popularity(skipped)), (1, 0));
    let page = store.tracks_page(TrackSort::Popular, 0, 2).await.unwrap();
    assert_eq!(page.items[0].id, listened);
}

#[tokio::test]
async fn an_album_takes_the_year_most_of_its_songs_give() {
    let store = Store::open_in_memory().await.unwrap();
    let mut songs = Vec::new();
    for (i, year) in [2015, 2005, 2005].into_iter().enumerate() {
        let mut song = scanned(&format!("/m/{i}.flac"), "Song", "Band", "Album");
        song.year = Some(year);
        songs.push(song);
    }
    store.save_scanned(&songs).await.unwrap();
    let year = || async {
        sqlx::query_scalar::<_, Option<i64>>("SELECT year FROM albums")
            .fetch_one(&store.pool)
            .await
            .unwrap()
    };
    assert_eq!(
        year().await,
        Some(2015),
        "the first song seen, until sorted"
    );
    store.refresh_artwork().await.unwrap();
    assert_eq!(year().await, Some(2005));

    // Songs that stop giving a year leave the album's alone.
    for song in &mut songs {
        song.year = None;
    }
    store.save_scanned(&songs).await.unwrap();
    store.refresh_artwork().await.unwrap();
    assert_eq!(year().await, Some(2005));
}

/// A placeholder year stretched the year strips from 1955 to 9999.
#[tokio::test]
async fn impossible_years_already_stored_are_cleared() {
    let store = Store::open_in_memory().await.unwrap();
    let song = |path: &str| {
        let mut track = scanned(path, "Song", "Band", path);
        track.year = Some(1997);
        track
    };
    store
        .save_scanned(&[song("/m/1.flac"), song("/m/2.flac"), song("/m/3.flac")])
        .await
        .unwrap();
    // What an older version could have stored.
    for table in ["tracks", "albums"] {
        for (year, like) in [(9999, "1"), (latest_year() + 1, "2")] {
            let column = if table == "tracks" { "path" } else { "title" };
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "UPDATE {table} SET year = {year} WHERE {column} LIKE '/m/{like}%'"
            )))
            .execute(&store.pool)
            .await
            .unwrap();
        }
    }
    for migration in [
        include_str!("../../../../migrations/0033_impossible_years.sql"),
        include_str!("../../../../migrations/0034_years_not_past_next_year.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&store.pool).await.unwrap();
    }
    let labels: Vec<_> = store
        .track_jumps(TrackSort::Year)
        .await
        .unwrap()
        .into_iter()
        .map(|j| j.label)
        .collect();
    assert_eq!(labels, ["1997"]);
    let albums: Vec<Option<i64>> = sqlx::query_scalar("SELECT year FROM albums ORDER BY title")
        .fetch_all(&store.pool)
        .await
        .unwrap();
    assert_eq!(albums, [None, None, Some(1997)]);
}

#[test]
fn fts_starts_with_is_one_phrase() {
    assert_eq!(fts_starts_with("S&M").as_deref(), Some("^ \"S M\""));
    assert_eq!(fts_starts_with("\" OR *").as_deref(), Some("^ \"OR\""));
    assert_eq!(fts_starts_with("  -- "), None);
}

/// Short names made of common letters used to lose to anything with words
/// starting with them, far past the end of the results.
#[tokio::test]
async fn a_name_typed_in_full_comes_first() {
    let store = Store::open_in_memory().await.unwrap();
    let mut songs: Vec<_> = (0..30)
        .map(|i| {
            scanned(
                &format!("/m/{i}.flac"),
                &format!("Slow Motion Slow Motion {i}"),
                "Mister Sister",
                "Some More Songs",
            )
        })
        .collect();
    songs.push(scanned(
        "/m/a.flac",
        "S.M.D. Medley",
        "Carnivore",
        "Retaliation",
    ));
    songs.push(scanned("/m/b.flac", "S&M", "Rihanna", "Loud"));
    songs.push(scanned("/m/c.flac", "Master", "Band", "S&M"));
    store.save_scanned(&songs).await.unwrap();

    let found = store.search("S&M", 5).await.unwrap();
    let titles: Vec<_> = found.tracks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(titles[..2], ["S&M", "S.M.D. Medley"]);
    assert_eq!(found.albums[0].title, "S&M");

    // Plain searches keep working, and a query that's only punctuation is empty.
    assert_eq!(
        store.search("loud", 5).await.unwrap().tracks[0].title,
        "S&M"
    );
    assert!(store.search("&&", 5).await.unwrap().tracks.is_empty());
}

#[tokio::test]
async fn scan_save_builds_albums_and_artists() {
    let store = Store::open_in_memory().await.unwrap();
    let mut feat = scanned("/m/2.flac", "Guest Spot", "Guest", "Debut");
    feat.album_artist = Some("Band".to_owned());
    store
        .save_scanned(&[scanned("/m/1.flac", "Opener", "Band", "Debut"), feat])
        .await
        .unwrap();

    let results = store.search("debut", 10).await.unwrap();
    assert_eq!(results.albums.len(), 1);
    let album = store.album(results.albums[0].id).await.unwrap().unwrap();
    assert_eq!(album.album.artist, "Band");
    assert_eq!(album.tracks.len(), 2);

    // The featured artist has the track but not the album.
    let guest = store.search("guest", 10).await.unwrap().artists;
    let detail = store.artist(guest[0].id).await.unwrap().unwrap();
    assert!(detail.albums.is_empty());
    assert_eq!(detail.other_tracks.len(), 1);
}

#[tokio::test]
async fn rescanning_updates_in_place_and_removal_hides() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[scanned("/m/1.flac", "Old Title", "A", "X")])
        .await
        .unwrap();
    store
        .save_scanned(&[scanned("/m/1.flac", "New Title", "A", "X")])
        .await
        .unwrap();
    let known = store.known_files().await.unwrap();
    assert_eq!(known.len(), 1);
    assert!(store.search("old", 10).await.unwrap().tracks.is_empty());
    assert_eq!(store.search("new", 10).await.unwrap().tracks.len(), 1);

    store.mark_removed(&[known[0].id]).await.unwrap();
    let results = store.search("new", 10).await.unwrap();
    assert!(results.tracks.is_empty());
    assert!(results.albums.is_empty());
    assert!(results.artists.is_empty());
    assert_eq!(store.track(known[0].id).await.unwrap(), None);

    // Coming back clears the removal and keeps the id.
    store
        .save_scanned(&[scanned("/m/1.flac", "New Title", "A", "X")])
        .await
        .unwrap();
    assert_eq!(store.known_files().await.unwrap()[0].id, known[0].id);
    assert_eq!(store.track_count().await.unwrap(), 1);
}

#[tokio::test]
async fn browse_pages() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[
            scanned("/m/1.flac", "beta", "Zed", "Second"),
            scanned("/m/2.flac", "Alpha", "zed", "Second"),
            scanned("/m/3.flac", "gamma", "Abba", "First"),
        ])
        .await
        .unwrap();

    let artists = store.artists_page(0, 10).await.unwrap();
    let names: Vec<_> = artists
        .items
        .iter()
        .map(|a| (a.name.as_str(), a.track_count))
        .collect();
    assert_eq!(names, [("Abba", 1), ("Zed", 2)]);
    assert_eq!(artists.total, 2);

    let albums = store.albums_page(AlbumSort::Name, 0, 1).await.unwrap();
    assert_eq!((albums.items[0].title.as_str(), albums.total), ("First", 2));

    let titles = |page: Page<Track>| page.items.into_iter().map(|t| t.title).collect::<Vec<_>>();
    assert_eq!(
        titles(store.tracks_page(TrackSort::Title, 0, 10).await.unwrap()),
        ["Alpha", "beta", "gamma"]
    );
    assert_eq!(
        titles(store.tracks_page(TrackSort::Title, 1, 1).await.unwrap()),
        ["beta"]
    );
    assert_eq!(
        titles(store.tracks_page(TrackSort::Artist, 0, 10).await.unwrap()),
        ["gamma", "Alpha", "beta"]
    );

    // Removed files drop out, and so do artists left with nothing.
    let abba = store
        .known_files()
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.path == "/m/3.flac")
        .unwrap();
    store.mark_removed(&[abba.id]).await.unwrap();
    assert_eq!(store.artists_page(0, 10).await.unwrap().total, 1);
    assert_eq!(
        store
            .albums_page(AlbumSort::Name, 0, 10)
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        store
            .tracks_page(TrackSort::Added, 0, 10)
            .await
            .unwrap()
            .total,
        2
    );
}

#[tokio::test]
async fn artwork_rules_and_tidy() {
    let store = Store::open_in_memory().await.unwrap();
    let track = |path: &str, no: i64, embedded: Option<&str>, folder: Option<&str>| ScannedTrack {
        path: path.into(),
        title: path.into(),
        artist: Some("Band".into()),
        album: Some("Record".into()),
        track_no: Some(no),
        embedded_art: embedded.map(Into::into),
        folder_art: folder.map(Into::into),
        artist_art: Some("artistpic".into()),
        ..Default::default()
    };
    // Track 2 is scanned first, but track 1's embedded cover wins.
    store
        .save_scanned(&[
            track("/m/b.flac", 2, Some("second"), Some("folder")),
            track("/m/a.flac", 1, Some("first"), Some("folder")),
        ])
        .await
        .unwrap();
    store.refresh_artwork().await.unwrap();
    let album = &store
        .albums_page(AlbumSort::Name, 0, 5)
        .await
        .unwrap()
        .items[0];
    assert_eq!(album.art_hash.as_deref(), Some("first"));
    let artist = &store.artists_page(0, 5).await.unwrap().items[0];
    assert_eq!(artist.art_hash.as_deref(), Some("artistpic"));

    // Without an embedded cover on the first song, the folder image is used.
    store
        .save_scanned(&[track("/m/a.flac", 1, None, Some("folder"))])
        .await
        .unwrap();
    store.refresh_artwork().await.unwrap();
    let album = &store
        .albums_page(AlbumSort::Name, 0, 5)
        .await
        .unwrap()
        .items[0];
    assert_eq!(album.art_hash.as_deref(), Some("folder"));

    // Renamed artist: the old one disappears once nothing uses it.
    let mut renamed = track("/m/a.flac", 1, None, None);
    renamed.artist = Some("New Band".into());
    let mut renamed_b = track("/m/b.flac", 2, None, None);
    renamed_b.artist = Some("New Band".into());
    store.save_scanned(&[renamed, renamed_b]).await.unwrap();
    store.refresh_artwork().await.unwrap();
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM artists")
        .fetch_all(store.pool())
        .await
        .unwrap();
    assert_eq!(names, ["New Band"]);
}

#[tokio::test]
async fn songs_without_art_show_the_album_cover() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[scanned("/m/1.flac", "One", "A", "Record")])
        .await
        .unwrap();
    let album = store
        .albums_page(AlbumSort::Name, 0, 1)
        .await
        .unwrap()
        .items[0]
        .id;
    store
        .save_album_info(album, Some("onlinecover"), None, None, true)
        .await
        .unwrap();
    store.refresh_artwork().await.unwrap();
    let track = &store.search("one", 5).await.unwrap().tracks[0];
    assert_eq!(track.art_hash.as_deref(), Some("onlinecover"));
}

#[test]
fn jumps_mark_where_each_letter_or_year_starts() {
    let keys = |list: &[Option<&str>]| list.iter().map(|k| k.map(str::to_owned)).collect();
    let at = |found: Vec<Jump>| -> Vec<(String, i64)> {
        found.into_iter().map(|j| (j.label, j.offset)).collect()
    };
    assert_eq!(
        at(jumps(
            keys(&[
                Some("99 Problems"),
                Some("abba"),
                Some("ACDC"),
                Some("Été"),
                None
            ]),
            false
        )),
        [("#".into(), 0), ("A".into(), 1), ("E".into(), 3)]
    );
    assert_eq!(
        at(jumps(
            keys(&[Some("1967"), Some("1967"), Some("1984"), None]),
            true
        )),
        [("1967".into(), 0), ("1984".into(), 2)]
    );
}

#[tokio::test]
async fn jumps_follow_the_list_order() {
    let store = Store::open_in_memory().await.unwrap();
    let mut tracks = vec![
        scanned("/m/1.flac", "Blinding Lights", "The Weeknd", "After Hours"),
        scanned(
            "/m/2.flac",
            "Can I Kick It?",
            "A Tribe Called Quest",
            "Low End Theory",
        ),
        scanned("/m/3.flac", "Uptown Funk", "Bruno Mars", "Uptown Special"),
        scanned(
            "/m/4.flac",
            "Ballroom Blitz",
            "Sweet",
            "Desolation Boulevard",
        ),
    ];
    tracks[0].year = Some(2020);
    tracks[1].year = Some(1991);
    tracks[2].year = Some(2015);
    tracks[3].year = Some(1974);
    store.save_scanned(&tracks).await.unwrap();

    let labels = |found: Vec<Jump>| -> Vec<(String, i64)> {
        found.into_iter().map(|j| (j.label, j.offset)).collect()
    };
    // Bruno Mars, Sweet, A Tribe Called Quest, The Weeknd.
    assert_eq!(
        labels(store.artist_jumps().await.unwrap()),
        [
            ("B".into(), 0),
            ("S".into(), 1),
            ("T".into(), 2),
            ("W".into(), 3)
        ]
    );
    assert_eq!(
        labels(store.album_jumps(AlbumSort::Oldest).await.unwrap()),
        [
            ("1974".into(), 0),
            ("1991".into(), 1),
            ("2015".into(), 2),
            ("2020".into(), 3)
        ]
    );
    let newest = store.tracks_page(TrackSort::Year, 0, 10).await.unwrap();
    assert_eq!(newest.items[0].title, "Blinding Lights");
    assert_eq!(
        labels(store.track_jumps(TrackSort::Year).await.unwrap())[0],
        ("2020".into(), 0)
    );
    assert_eq!(
        labels(store.track_jumps(TrackSort::Title).await.unwrap()),
        [("B".into(), 0), ("C".into(), 2), ("U".into(), 3)]
    );
    assert!(store
        .track_jumps(TrackSort::Added)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn artists_and_albums_sort_by_what_was_added_last() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[
            scanned("/m/1.flac", "Old One", "Abba", "Arrival"),
            scanned("/m/2.flac", "Old Two", "Zappa", "Apostrophe"),
        ])
        .await
        .unwrap();
    let added = |path: &'static str, at: i64| {
        let store = store.clone();
        async move {
            sqlx::query("UPDATE tracks SET added_at = ? WHERE path = ?")
                .bind(at)
                .bind(path)
                .execute(&store.pool)
                .await
                .unwrap();
        }
    };
    added("/m/1.flac", 1_000).await;
    added("/m/2.flac", 2_000).await;
    let artists = |sort| {
        let store = store.clone();
        async move {
            let page = store
                .artists_page_filtered(sort, true, 0, 10)
                .await
                .unwrap();
            page.items.into_iter().map(|a| a.name).collect::<Vec<_>>()
        }
    };
    let albums = |sort| {
        let store = store.clone();
        async move {
            let page = store.albums_page(sort, 0, 10).await.unwrap();
            page.items.into_iter().map(|a| a.title).collect::<Vec<_>>()
        }
    };
    assert_eq!(artists(ArtistSort::Name).await, ["Abba", "Zappa"]);
    assert_eq!(artists(ArtistSort::Added).await, ["Zappa", "Abba"]);
    assert_eq!(albums(AlbumSort::Name).await, ["Apostrophe", "Arrival"]);
    assert_eq!(albums(AlbumSort::Added).await, ["Apostrophe", "Arrival"]);

    // A new song on an old album brings the album and its artist back up.
    store
        .save_scanned(&[scanned("/m/3.flac", "New Bonus", "Abba", "Arrival")])
        .await
        .unwrap();
    added("/m/3.flac", 3_000).await;
    assert_eq!(artists(ArtistSort::Added).await, ["Abba", "Zappa"]);
    assert_eq!(albums(AlbumSort::Added).await, ["Arrival", "Apostrophe"]);

    // A removed song no longer counts.
    sqlx::query("UPDATE tracks SET removed_at = 1 WHERE path = '/m/3.flac'")
        .execute(&store.pool)
        .await
        .unwrap();
    assert_eq!(artists(ArtistSort::Added).await, ["Zappa", "Abba"]);
    assert_eq!(albums(AlbumSort::Added).await, ["Apostrophe", "Arrival"]);

    // No letters or years to jump to in an order like this.
    assert!(store
        .artist_jumps_filtered(ArtistSort::Added, true)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .album_jumps(AlbumSort::Added)
        .await
        .unwrap()
        .is_empty());
    assert!(!store.album_jumps(AlbumSort::Name).await.unwrap().is_empty());
}

#[tokio::test]
async fn sorting_skips_leading_articles_and_albums_sort_by_year() {
    let store = Store::open_in_memory().await.unwrap();
    let mut tracks = vec![
        scanned("/m/1.flac", "Blinding Lights", "The Weeknd", "After Hours"),
        scanned(
            "/m/2.flac",
            "Can I Kick It?",
            "A Tribe Called Quest",
            "Low End Theory",
        ),
        scanned("/m/3.flac", "Uptown Funk", "Bruno Mars", "Uptown Special"),
        scanned("/m/4.flac", "Theme", "Theory of a Deadman", "Gasoline"),
    ];
    tracks[0].year = Some(2020);
    tracks[1].year = Some(1991);
    tracks[2].year = Some(2015);
    store.save_scanned(&tracks).await.unwrap();

    let artists = store.artists_page(0, 10).await.unwrap();
    let names: Vec<&str> = artists.items.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Bruno Mars",
            "Theory of a Deadman",
            "A Tribe Called Quest",
            "The Weeknd"
        ],
        "\"Theory\" is a word, not \"The\""
    );
    let by_year = store.albums_page(AlbumSort::Year, 0, 10).await.unwrap();
    let titles: Vec<&str> = by_year.items.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "After Hours",
            "Uptown Special",
            "Low End Theory",
            "Gasoline"
        ],
        "newest first, unknown years last"
    );
    assert_eq!(store.library_track_ids().await.unwrap().len(), 4);

    // Popular: requests first, then plays, then title.
    let ids = store.library_track_ids().await.unwrap();
    let user = store
        .create_user("u", "h", crate::Role::User)
        .await
        .unwrap()
        .id;
    let origin = crate::RequestOrigin::default();
    store
        .add_request(ids[2], user, crate::RequestLimits::default(), &origin)
        .await
        .unwrap();
    store
        .start_play_event(ids[3], crate::PlaySource::Pool, None)
        .await
        .unwrap();
    let popular = store.tracks_page(TrackSort::Popular, 0, 10).await.unwrap();
    let order: Vec<i64> = popular.items.iter().map(|t| t.id).collect();
    assert_eq!(&order[..2], &[ids[2], ids[3]]);
}

#[tokio::test]
async fn collaborations_count_for_every_artist() {
    let store = Store::open_in_memory().await.unwrap();
    let mut collab = scanned(
        "/m/v.flac",
        "Volcano",
        "Zara Larsson & David Guetta",
        "Volcano",
    );
    collab.credits = vec![
        ("Zara Larsson".into(), false),
        ("David Guetta".into(), false),
        ("KREAM".into(), true),
    ];
    collab.album_owner = Some("Zara Larsson".into());
    let solo = scanned(
        "/m/t.flac",
        "Titanium",
        "David Guetta",
        "Nothing but the Beat",
    );
    store.save_scanned(&[collab, solo]).await.unwrap();

    let artists = store.artists_page(0, 10).await.unwrap();
    let names: Vec<_> = artists
        .items
        .iter()
        .map(|a| (a.name.as_str(), a.track_count))
        .collect();
    assert_eq!(
        names,
        [("David Guetta", 2), ("KREAM", 1), ("Zara Larsson", 1)],
        "no combined artist"
    );
    let visible = store
        .artists_page_filtered(ArtistSort::Name, false, 0, 10)
        .await
        .unwrap();
    let visible_names: Vec<_> = visible.items.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(visible_names, ["David Guetta", "Zara Larsson"]);
    assert_eq!(
        store
            .artist_jumps_filtered(ArtistSort::Name, false)
            .await
            .unwrap()
            .len(),
        2
    );
    let hidden = store.search("KREAM", 10).await.unwrap().artists;
    assert_eq!(hidden.len(), 1);
    assert!(store.artist(hidden[0].id).await.unwrap().is_some());

    let track = &store.search("volcano", 5).await.unwrap().tracks[0];
    let credits: Vec<_> = track
        .credits
        .iter()
        .map(|c| (c.name.as_str(), c.featured))
        .collect();
    assert_eq!(
        credits,
        [
            ("Zara Larsson", false),
            ("David Guetta", false),
            ("KREAM", true)
        ]
    );
    assert_eq!(
        track.artist.as_deref(),
        Some("Zara Larsson & David Guetta"),
        "display text kept"
    );

    // The single belongs to Zara and appears on David's page.
    let guetta = artists
        .items
        .iter()
        .find(|a| a.name == "David Guetta")
        .unwrap()
        .id;
    let detail = store.artist(guetta).await.unwrap().unwrap();
    assert_eq!(detail.albums.len(), 1, "his own album");
    assert_eq!(detail.appears_on.len(), 1, "Zara's single");
    assert_eq!(detail.top_tracks.len(), 2);
}

#[tokio::test]
async fn albumless_songs_stay_in_the_primary_artist_list() {
    let store = Store::open_in_memory().await.unwrap();
    let mut song = scanned("/m/single.flac", "Single", "Solo", "Unused");
    song.album = None;
    store.save_scanned(&[song]).await.unwrap();

    let page = store
        .artists_page_filtered(ArtistSort::Name, false, 0, 10)
        .await
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].name, "Solo");
    assert_eq!(page.items[0].track_count, 1);
    assert_eq!(
        store
            .artist_jumps_filtered(ArtistSort::Name, false)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn compilation_albums_are_grouped_and_filterable() {
    let store = Store::open_in_memory().await.unwrap();
    let mut songs = vec![
        scanned("/music/mix/a.flac", "One", "Ada", "Weekend Mix"),
        scanned("/music/mix/b.flac", "Two", "Bea", "Weekend Mix"),
        scanned("/music/mix/c.flac", "Three", "Cyd", "Weekend Mix"),
        scanned("/music/film/a.flac", "Scene One", "Dee", "Film Soundtrack"),
        scanned("/music/film/b.flac", "Scene Two", "Eli", "Film Soundtrack"),
        scanned("/music/solo/a.flac", "Solo", "Finn", "Solo Album"),
    ];
    songs[3].genre = Some("Soundtrack".into());
    store.save_scanned(&songs).await.unwrap();
    store.refresh_artwork().await.unwrap();

    let various = store
        .albums_page_filtered(AlbumSort::Name, AlbumFilter::Various, 0, 10)
        .await
        .unwrap();
    assert_eq!(various.total, 1);
    assert_eq!(various.items[0].title, "Weekend Mix");
    assert_eq!(various.items[0].artist, "Various Artists");
    assert_eq!(
        store
            .album(various.items[0].id)
            .await
            .unwrap()
            .unwrap()
            .tracks
            .len(),
        3
    );
    let soundtracks = store
        .albums_page_filtered(AlbumSort::Name, AlbumFilter::Soundtracks, 0, 10)
        .await
        .unwrap();
    assert_eq!(soundtracks.total, 1);
    assert_eq!(soundtracks.items[0].artist, "Soundtrack");
    assert_eq!(
        store
            .album_jumps_filtered(AlbumSort::Name, AlbumFilter::Various)
            .await
            .unwrap()[0]
            .offset,
        0
    );
    let artists = store
        .artists_page_filtered(ArtistSort::Name, false, 0, 20)
        .await
        .unwrap();
    assert_eq!(
        artists
            .items
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["Finn"]
    );
    assert_eq!(store.search("Ada", 10).await.unwrap().artists.len(), 1);
    let removed = store
        .known_files()
        .await
        .unwrap()
        .into_iter()
        .find(|file| file.path.ends_with("/mix/c.flac"))
        .unwrap()
        .id;
    store.mark_removed(&[removed]).await.unwrap();
    store.refresh_artwork().await.unwrap();
    assert_eq!(
        store
            .albums_page_filtered(AlbumSort::Name, AlbumFilter::Various, 0, 10)
            .await
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn library_moves_to_a_new_folder() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[
            scanned(r"C:\Music\Björk\Jóga.flac", "Jóga", "Björk", "Homogenic"),
            scanned(r"C:\Musicals\Cats.flac", "Memory", "Cast", "Cats"),
        ])
        .await
        .unwrap();

    let moved = store
        .move_library(r"C:\Music", "/home/ana/music", '/')
        .await
        .unwrap();
    assert_eq!(moved, 1, "a folder that only starts the same isn't moved");
    let mut paths: Vec<String> = store
        .known_files()
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.path)
        .collect();
    paths.sort();
    assert_eq!(
        paths,
        ["/home/ana/music/Björk/Jóga.flac", r"C:\Musicals\Cats.flac"]
    );
}

#[tokio::test]
async fn snapshots_open_as_a_database() {
    let dir = std::env::temp_dir().join(format!("djinn-snapshot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::open(&dir.join("live.db")).await.unwrap();
    store
        .save_scanned(&[scanned("/m/a.flac", "A", "B", "C")])
        .await
        .unwrap();
    let copy = dir.join("copy.db");
    let _ = std::fs::remove_file(&copy);
    store.snapshot(&copy).await.unwrap();

    let restored = Store::open(&copy).await.unwrap();
    assert_eq!(restored.library_track_ids().await.unwrap().len(), 1);
    drop((restored, store));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn corrected_genres_replace_looked_up_ones() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[ScannedTrack {
            path: "/m/a.flac".into(),
            title: "Open Wound".into(),
            artist: Some("Kream".into()),
            album: Some("Ours".into()),
            credits: vec![("Kream".into(), false)],
            ..Default::default()
        }])
        .await
        .unwrap();
    sqlx::query("UPDATE albums SET genres = 'ebm, electro'")
        .execute(&store.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE artists SET genres = 'ebm'")
        .execute(&store.pool)
        .await
        .unwrap();
    let id = store.library_track_ids().await.unwrap()[0];
    let track = store.track(id).await.unwrap().unwrap();
    let (album, artist) = (track.album_id.unwrap(), track.artist_id.unwrap());
    let genres = |store: Store| async move {
        (
            store.album(album).await.unwrap().unwrap().album.genres,
            store.artist(artist).await.unwrap().unwrap().genres,
            store.artist_genres().await.unwrap()[0]
                .genres
                .split(',')
                .filter(|g| !g.is_empty())
                .collect::<Vec<_>>()
                .join(","),
        )
    };
    assert_eq!(
        genres(store.clone()).await,
        (
            Some("ebm, electro".into()),
            Some("ebm".into()),
            "ebm".into()
        )
    );

    store
        .edit_tags(
            &[id],
            &crate::TagChange {
                genre: Some(Some("Darkwave, Synth-pop".into())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    // As the scanner reads it with the correction applied.
    sqlx::query("UPDATE tracks SET genre = 'Darkwave, Synth-pop'")
        .execute(&store.pool)
        .await
        .unwrap();
    let corrected = Some("Darkwave, Synth-pop".to_owned());
    assert_eq!(
        genres(store.clone()).await,
        (corrected.clone(), corrected, "darkwave, synth-pop".into())
    );
    let facts = store.track_facts().await.unwrap();
    assert_eq!(
        facts[0].genres, "darkwave, synth-pop",
        "not the album's or artist's"
    );
}

#[tokio::test]
async fn artist_pictures_come_from_their_category() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[scanned("/m/1.flac", "Opener", "Band", "Debut")])
        .await
        .unwrap();
    let id = store.search("band", 10).await.unwrap().artists[0].id;
    assert_eq!(
        store.picture_source(id).await.unwrap(),
        Some(PictureSource {
            name: "Band".to_owned(),
            mbid: None,
            commons_category: None,
            checked: false
        })
    );
    assert_eq!(store.picture_source(9999).await.unwrap(), None);

    // A lookup that finds the category keeps it; a later one without keeps it too.
    let found = ArtistInfo {
        commons_category: Some("Band (band)".to_owned()),
        checked: true,
        ..Default::default()
    };
    store.save_artist_info(id, &found).await.unwrap();
    store
        .save_artist_info(id, &ArtistInfo::default())
        .await
        .unwrap();
    let source = store.picture_source(id).await.unwrap().unwrap();
    assert_eq!(source.commons_category.as_deref(), Some("Band (band)"));

    let picture = |hash: &str| ArtistPicture {
        art_hash: hash.to_owned(),
        credit: "Someone (CC BY-SA 4.0)".to_owned(),
        credit_url: format!("https://commons.wikimedia.org/wiki/File:{hash}.jpg"),
        source: "commons".to_owned(),
        remote_url: None,
    };
    store
        .save_artist_pictures(id, &[picture("a"), picture("b")])
        .await
        .unwrap();
    assert!(store.picture_source(id).await.unwrap().unwrap().checked);
    store
        .save_artist_pictures(id, &[picture("c")])
        .await
        .unwrap();
    assert_eq!(
        store.artist_pictures(id).await.unwrap(),
        [picture("c")],
        "replaced, not added"
    );
}

#[tokio::test]
async fn host_pictures_survive_commons_rechecks_and_can_be_managed() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[scanned("/m/host.flac", "Opener", "Host Band", "Debut")])
        .await
        .unwrap();
    let id = store.search("host band", 10).await.unwrap().artists[0].id;
    let manual = ArtistPicture {
        art_hash: "manual".into(),
        credit: "Host licensed".into(),
        credit_url: "".into(),
        source: "manual".into(),
        remote_url: None,
    };
    let commons = ArtistPicture {
        art_hash: "commons".into(),
        credit: "Photographer".into(),
        credit_url: "https://commons.wikimedia.org/wiki/File:Example.jpg".into(),
        source: "commons".into(),
        remote_url: None,
    };
    store.add_artist_picture(id, &manual).await.unwrap();
    store
        .save_artist_pictures(id, std::slice::from_ref(&commons))
        .await
        .unwrap();
    assert_eq!(
        store.artist_pictures(id).await.unwrap(),
        [manual.clone(), commons.clone()]
    );
    assert!(store.art_hash_in_use(&manual.art_hash).await.unwrap());
    assert!(store
        .reorder_artist_pictures(id, &[commons.art_hash.clone(), manual.art_hash.clone()])
        .await
        .unwrap());
    assert_eq!(
        store.artist_pictures(id).await.unwrap(),
        [commons.clone(), manual.clone()]
    );
    store.save_artist_pictures(id, &[]).await.unwrap();
    assert_eq!(
        store.artist_pictures(id).await.unwrap(),
        std::slice::from_ref(&manual)
    );
    assert!(store
        .remove_artist_picture(id, &manual.art_hash)
        .await
        .unwrap());
    assert!(!store.art_hash_in_use(&manual.art_hash).await.unwrap());
}

#[tokio::test]
async fn kept_lists_follow_every_change() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[scanned("/m/1.flac", "First", "Band", "Debut")])
        .await
        .unwrap();
    let titles =
        |page: Page<Track>| -> Vec<String> { page.items.into_iter().map(|t| t.title).collect() };
    let songs = store.tracks_page(TrackSort::Title, 0, 10).await.unwrap();
    assert_eq!(titles(songs), ["First"]);
    assert_eq!(
        store.artists_page(0, 10).await.unwrap().items[0].track_count,
        1
    );

    // A new song, without any call telling the lists about it.
    store
        .save_scanned(&[scanned("/m/2.flac", "Another", "Band", "Debut")])
        .await
        .unwrap();
    let songs = store.tracks_page(TrackSort::Title, 0, 10).await.unwrap();
    assert_eq!(titles(songs), ["Another", "First"]);
    assert_eq!(
        store.artists_page(0, 10).await.unwrap().items[0].track_count,
        2
    );
    let jumps = store.track_jumps(TrackSort::Title).await.unwrap();
    assert_eq!(jumps.iter().map(|j| j.offset).collect::<Vec<_>>(), [0, 1]);

    // Direct writes count too: a file going away, and a new picture.
    sqlx::query("UPDATE tracks SET removed_at = 1 WHERE path = '/m/2.flac'")
        .execute(store.pool())
        .await
        .unwrap();
    let songs = store.tracks_page(TrackSort::Title, 0, 10).await.unwrap();
    assert_eq!(
        (titles(songs.clone()), songs.total),
        (vec!["First".to_owned()], 1)
    );
    assert_eq!(
        store.artists_page(0, 10).await.unwrap().items[0].track_count,
        1
    );
    sqlx::query("UPDATE artists SET art_hash = 'new'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store.artists_page(0, 10).await.unwrap().items[0]
            .art_hash
            .as_deref(),
        Some("new")
    );
    sqlx::query("UPDATE albums SET art_hash = 'cover'")
        .execute(store.pool())
        .await
        .unwrap();
    assert_eq!(
        store
            .albums_page(AlbumSort::Name, 0, 10)
            .await
            .unwrap()
            .items[0]
            .art_hash
            .as_deref(),
        Some("cover")
    );

    // Past the end is an empty page with the right total.
    let past = store.tracks_page(TrackSort::Title, 50, 10).await.unwrap();
    assert!(past.items.is_empty());
    assert_eq!(past.total, 1);
}

#[tokio::test]
async fn a_changed_file_is_analyzed_and_heard_again() {
    let store = Store::open_in_memory().await.unwrap();
    let song = scanned("/m/1.flac", "Song", "Band", "Debut");
    store
        .save_scanned(std::slice::from_ref(&song))
        .await
        .unwrap();
    let id = store
        .tracks_page(TrackSort::Title, 0, 1)
        .await
        .unwrap()
        .items[0]
        .id;
    store
        .save_analysis(
            id,
            1,
            1,
            Some((-14.0, 0.9)),
            Some((&[1.0, 2.0], 120.0)),
            None,
        )
        .await
        .unwrap();
    store.save_heard(id, 1, Some(&[3.0])).await.unwrap();
    assert!(store.tracks_to_analyze(1, 1, 10).await.unwrap().is_empty());
    assert!(store.tracks_to_hear(1, 10).await.unwrap().is_empty());
    assert_eq!(store.heard_progress(1).await.unwrap(), (1, 0));
    assert_eq!(store.sound_rows(1).await.unwrap().len(), 1);
    assert_eq!(store.heard_rows(1).await.unwrap()[0].1, [3.0]);

    // Read again unchanged: nothing to redo.
    store
        .save_scanned(std::slice::from_ref(&song))
        .await
        .unwrap();
    assert!(store.tracks_to_analyze(1, 1, 10).await.unwrap().is_empty());

    let mut changed = song;
    changed.mtime = 2;
    store.save_scanned(&[changed]).await.unwrap();
    assert_eq!(store.tracks_to_analyze(1, 1, 10).await.unwrap().len(), 1);
    assert_eq!(store.tracks_to_hear(1, 10).await.unwrap().len(), 1);
    assert_eq!(store.heard_progress(1).await.unwrap(), (0, 1));

    store.clear_analysis().await.unwrap();
    store.clear_listening().await.unwrap();
    assert!(store.sound_rows(1).await.unwrap().is_empty());
    assert!(store.heard_rows(1).await.unwrap().is_empty());
}

#[tokio::test]
async fn compaction_runs_once() {
    let store = Store::open_in_memory().await.unwrap();
    assert_eq!(store.get_setting("compact_database").await.unwrap(), None);
}

/// The library lists' counts: party and GO plays (skipped ones too, as on
/// the Stats page) and favorites, for songs, their album and their artist.
#[tokio::test]
async fn plays_and_favorites_are_counted_per_song_album_and_artist() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .save_scanned(&[
            scanned("/m/a.flac", "One", "Band", "Album"),
            scanned("/m/b.flac", "Two", "Band", "Album"),
        ])
        .await
        .unwrap();
    let (one, two, album, artist): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT a.id, b.id, a.album_id, a.artist_id FROM tracks a, tracks b
         WHERE a.title = 'One' AND b.title = 'Two'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    let sam = store
        .create_user("sam", "h", crate::Role::User)
        .await
        .unwrap()
        .id;
    let kim = store
        .create_user("kim", "h", crate::Role::User)
        .await
        .unwrap()
        .id;
    for track in [one, one, two] {
        sqlx::query("INSERT INTO play_events (track_id, source, started_at) VALUES (?, 'pool', 1)")
            .bind(track)
            .execute(&store.pool)
            .await
            .unwrap();
    }
    store
        .record_go_play(sam, one, crate::SKIP_BEFORE_MS)
        .await
        .unwrap();
    store.record_go_play(sam, two, 1).await.unwrap();
    for user in [sam, kim] {
        store
            .set_like(user, crate::LikeKind::Track, one, true)
            .await
            .unwrap();
    }
    store
        .set_like(kim, crate::LikeKind::Album, album, true)
        .await
        .unwrap();
    store
        .set_like(sam, crate::LikeKind::Artist, artist, true)
        .await
        .unwrap();

    let songs = store.track_counts(&[one, two, 999]).await.unwrap();
    assert_eq!(songs[&one], Counts { plays: 3, likes: 2 });
    assert_eq!(songs[&two], Counts { plays: 2, likes: 0 });
    assert_eq!(songs[&999], Counts::default());
    let albums = store.album_counts(&[album]).await.unwrap();
    assert_eq!(albums[&album], Counts { plays: 5, likes: 1 });
    let artists = store.artist_counts(&[artist]).await.unwrap();
    assert_eq!(artists[&artist], Counts { plays: 5, likes: 1 });
    assert!(store.track_counts(&[]).await.unwrap().is_empty());
}
