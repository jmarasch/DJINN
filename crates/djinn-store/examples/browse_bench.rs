//! Times the library's browse queries on a made-up library.
//!
//! cargo run --release -p djinn-store --example browse_bench -- <db file> [songs] [blobs]
//!
//! Makes the database if it isn't there. `blobs` fills in sound features and
//! AI embeddings, as on a library that has been analyzed.

use std::path::Path;
use std::time::{Duration, Instant};

use djinn_store::{AlbumFilter, AlbumSort, Store, TrackSort};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

const PER_ALBUM: i64 = 12;
const PER_ARTIST: i64 = 3;
const RUNS: usize = 3;

async fn seed(path: &Path, songs: i64, blobs: bool) -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePool::connect_with(SqliteConnectOptions::new().filename(path)).await?;
    let mut tx = pool.begin().await?;
    let albums = songs / PER_ALBUM + 1;
    let artists = albums / PER_ARTIST + 1;
    for id in 0..artists {
        let name = match id % 10 {
            0 => format!("The Artist {:05}", id.wrapping_mul(7919) % artists),
            _ => format!("Artist {:05}", id.wrapping_mul(7919) % artists),
        };
        sqlx::query("INSERT INTO artists (id, name, art_hash) VALUES (?, ?, ?)")
            .bind(id + 1)
            .bind(format!("{name} {id}"))
            .bind(format!("{id:032x}"))
            .execute(&mut *tx)
            .await?;
    }
    for id in 0..albums {
        sqlx::query(
            "INSERT INTO albums (id, title, artist_id, year, art_hash) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id + 1)
        .bind(format!("Album {:06}", id.wrapping_mul(104_729) % albums))
        .bind(id / PER_ARTIST + 1)
        .bind(1960 + id % 65)
        .bind(format!("{id:032x}"))
        .execute(&mut *tx)
        .await?;
    }
    let features = vec![7_u8; 37 * 4];
    let heard = vec![9_u8; 512 * 4];
    for id in 0..songs {
        let album = id / PER_ALBUM;
        let artist = album / PER_ARTIST;
        let title = format!("Song {:07}", id.wrapping_mul(15_485_863) % songs);
        sqlx::query(
            "INSERT INTO tracks (id, path, size, mtime, duration_ms, title, artist, album,
                album_artist, track_no, disc_no, year, genre, art_hash, artist_id, album_id,
                added_at)
             VALUES (?, ?, 5000000, 0, 200000, ?, ?, ?, ?, ?, 1, ?, 'Rock', NULL, ?, ?, ?)",
        )
        .bind(id + 1)
        .bind(format!(
            "D:/Music/Artist {artist}/Album {album}/{title}.flac"
        ))
        .bind(&title)
        .bind(format!("Artist {artist}"))
        .bind(format!("Album {album}"))
        .bind(format!("Artist {artist}"))
        .bind(id % PER_ALBUM + 1)
        .bind(1960 + album % 65)
        .bind(artist + 1)
        .bind(album + 1)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if blobs {
            sqlx::query(
                "INSERT INTO track_sound (track_id, features, features_version, heard, heard_version)
                 VALUES (?, ?, 1, ?, 1)",
            )
            .bind(id + 1)
            .bind(&features)
            .bind(&heard)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO track_artists (track_id, artist_id, position, featured) VALUES (?, ?, 0, 0)")
            .bind(id + 1)
            .bind(artist + 1)
            .execute(&mut *tx)
            .await?;
        if id % 10 == 0 {
            sqlx::query("INSERT INTO track_artists (track_id, artist_id, position, featured) VALUES (?, ?, 1, 1)")
                .bind(id + 1)
                .bind((artist + 17) % artists + 1)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    pool.close().await;
    Ok(())
}

async fn time<T, E, F, Fut>(label: &str, run: F)
where
    E: std::fmt::Display,
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut took = Vec::new();
    for _ in 0..RUNS {
        let started = Instant::now();
        if let Err(e) = run().await {
            println!("{label:<34} FAILED after {:?}: {e}", started.elapsed());
            return;
        }
        took.push(started.elapsed());
    }
    took.sort();
    let median: Duration = took[RUNS / 2];
    println!("{label:<34} {:>8.1} ms", median.as_secs_f64() * 1000.0);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: browse_bench <db file> [songs] [blobs]")?;
    let songs: i64 = args.next().map_or(Ok(36_000), |s| s.parse())?;
    let blobs = args.next().as_deref() == Some("blobs");
    let path = Path::new(&path);
    let fresh = !path.exists();
    let store = Store::open(path).await?;
    if fresh {
        let started = Instant::now();
        seed(path, songs, blobs).await?;
        println!(
            "seeded {songs} songs (blobs: {blobs}) in {:?}",
            started.elapsed()
        );
    }
    println!(
        "database file: {} MB",
        std::fs::metadata(path)?.len() / 1_000_000
    );

    // The first of each is the cost after the library changes; the rest of
    // the runs (the median) are what every other request costs.
    let deep = songs / 2;
    time("artists page", || {
        store.artists_page_filtered(djinn_store::ArtistSort::Name, true, 0, 50)
    })
    .await;
    time("artists page, hide collabs", || {
        store.artists_page_filtered(djinn_store::ArtistSort::Name, false, 0, 50)
    })
    .await;
    time("artist jumps", || {
        store.artist_jumps_filtered(djinn_store::ArtistSort::Name, true)
    })
    .await;
    time("artist jumps, hide collabs", || {
        store.artist_jumps_filtered(djinn_store::ArtistSort::Name, false)
    })
    .await;
    time("albums page, name", || {
        store.albums_page_filtered(AlbumSort::Name, AlbumFilter::All, 0, 50)
    })
    .await;
    time("albums page, year", || {
        store.albums_page_filtered(AlbumSort::Year, AlbumFilter::All, 0, 50)
    })
    .await;
    time("albums page, various", || {
        store.albums_page_filtered(AlbumSort::Name, AlbumFilter::Various, 0, 50)
    })
    .await;
    time("album jumps, name", || {
        store.album_jumps_filtered(AlbumSort::Name, AlbumFilter::All)
    })
    .await;
    time("album jumps, various", || {
        store.album_jumps_filtered(AlbumSort::Name, AlbumFilter::Various)
    })
    .await;
    time("songs page, title", || {
        store.tracks_page(TrackSort::Title, 0, 50)
    })
    .await;
    time("songs page, title, deep", || {
        store.tracks_page(TrackSort::Title, deep, 50)
    })
    .await;
    time("songs page, artist", || {
        store.tracks_page(TrackSort::Artist, 0, 50)
    })
    .await;
    time("songs page, year", || {
        store.tracks_page(TrackSort::Year, 0, 50)
    })
    .await;
    time("songs page, added", || {
        store.tracks_page(TrackSort::Added, 0, 50)
    })
    .await;
    time("songs page, popular", || {
        store.tracks_page(TrackSort::Popular, 0, 50)
    })
    .await;
    time("song jumps, title", || store.track_jumps(TrackSort::Title)).await;
    time("song jumps, artist", || {
        store.track_jumps(TrackSort::Artist)
    })
    .await;
    time("track count", || store.track_count()).await;
    time("search 'song 12'", || store.search("song 12", 20)).await;
    Ok(())
}
