use super::browse::fts_query;
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
