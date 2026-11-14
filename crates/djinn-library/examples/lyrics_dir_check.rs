//! Tries a folder of lyrics files the way the zip import reads them, against
//! a COPY of a database (opening one applies migrations): what matches, which
//! are AI-made, and how many look like code. Read only on the files.
//! `lyrics_dir_check <db copy> <folder>`.

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};

use djinn_library::lyrics_import::{content, header, suspicious, Content};
use djinn_library::playlist_match::SongIndex;
use djinn_store::{Store, WantedSong};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "lrc" || e == "txt") {
            out.push(path);
        }
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let db = PathBuf::from(args.next().expect("db copy"));
    let folder = PathBuf::from(args.next().expect("folder"));
    let store = Store::open(&db).await.expect("open");
    let index = SongIndex::new(store.song_facts().await.expect("songs"));
    let mut files = Vec::new();
    walk(&folder, &mut files);
    files.sort();
    let (mut matched, mut ai, mut timed, mut plain, mut unmatched, mut flagged) =
        (0, 0, 0, 0, 0, 0);
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        if suspicious(&text).is_some() {
            flagged += 1;
            continue;
        }
        let head = header(&text);
        let found = head
            .title
            .clone()
            .zip(head.artist.clone())
            .and_then(|(title, artist)| {
                index.find(&WantedSong {
                    title,
                    artist: Some(artist),
                    album: head.album.clone(),
                    duration_ms: head.length_ms,
                    path: None,
                    either_way: false,
                })
            });
        match (found, content(&text)) {
            (Some(_), Content::Lyrics(l)) => {
                matched += 1;
                if head.ai_model().is_some() {
                    ai += 1;
                }
                if l.synced.is_some() {
                    timed += 1;
                } else {
                    plain += 1;
                }
                let first = l
                    .plain
                    .as_deref()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("");
                println!(
                    "{}\t{}\t{first:.70}",
                    if head.ai_model().is_some() {
                        "AI "
                    } else {
                        "web"
                    },
                    file.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                );
            }
            (None, _) => unmatched += 1,
            _ => {}
        }
    }
    println!("{} files: {matched} matched ({timed} timed, {plain} plain, {ai} AI), {unmatched} not matched, {flagged} thrown out", files.len());
}
