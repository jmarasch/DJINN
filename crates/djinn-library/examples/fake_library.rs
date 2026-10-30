//! Makes a large fake music library for trying DJINN with many songs:
//! copies of the tagged test file, each with its own artist, album, title,
//! track number, year and genre, in Artist/Album folders.
//!
//! cargo run --release -p djinn-library --example fake_library -- <dir> [songs]

use std::path::{Path, PathBuf};

use lofty::config::WriteOptions;
use lofty::prelude::*;

const GENRES: [&str; 12] = [
    "Rock",
    "Pop",
    "Electronic",
    "Hip Hop",
    "Jazz",
    "Classical",
    "Metal",
    "Folk",
    "Soul",
    "House",
    "Country",
    "Reggae",
];
/// Songs per album, and albums per artist.
const PER_ALBUM: usize = 12;
const PER_ARTIST: usize = 3;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("usage: fake_library <dir> [songs]")?);
    let songs: usize = args.next().map_or(Ok(36_000), |n| n.parse())?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tagged.flac");
    let template = std::fs::read(&source)?;

    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let per_thread = songs.div_ceil(threads);
    std::thread::scope(|s| {
        for t in 0..threads {
            let (dir, template) = (&dir, &template);
            s.spawn(move || {
                let end = ((t + 1) * per_thread).min(songs);
                for i in t * per_thread..end {
                    if let Err(e) = make(dir, template, i) {
                        eprintln!("song {i}: {e}");
                    }
                }
            });
        }
    });
    println!("{songs} songs in {}", dir.display());
    Ok(())
}

fn make(dir: &Path, template: &[u8], i: usize) -> Result<(), Box<dyn std::error::Error>> {
    let album = i / PER_ALBUM;
    let artist = album / PER_ARTIST;
    let track = i % PER_ALBUM + 1;
    let folder = dir
        .join(format!("Artist {artist:04}"))
        .join(format!("Album {album:05}"));
    std::fs::create_dir_all(&folder)?;
    let path = folder.join(format!("{track:02} Song {i}.flac"));
    std::fs::write(&path, template)?;

    let mut file = lofty::read_from_path(&path)?;
    let tag = file.primary_tag_mut().ok_or("no tag")?;
    tag.set_title(format!("Song {i}"));
    tag.set_artist(format!("Artist {artist:04}"));
    tag.set_album(format!("Album {album:05}"));
    tag.insert_text(ItemKey::AlbumArtist, format!("Artist {artist:04}"));
    tag.set_track(u32::try_from(track)?);
    tag.set_genre(GENRES[artist % GENRES.len()].to_owned());
    tag.insert_text(ItemKey::Year, (1960 + album % 60).to_string());
    file.save_to_path(&path, WriteOptions::default())?;
    Ok(())
}
