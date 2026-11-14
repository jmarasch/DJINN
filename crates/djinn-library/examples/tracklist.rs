//! Asks MusicBrainz for an album's track list and matches songs to it:
//! `cargo run -p djinn-library --example tracklist -- <contact> <release group id> "Title|Title|..."`.

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let songs: Vec<(i64, String, Option<i64>)> = args[2]
        .split('|')
        .enumerate()
        .map(|(i, t)| (i as i64, t.to_owned(), None))
        .collect();
    match djinn_library::online::album_tracklist(&args[0], &args[1], songs.len()).await {
        Ok(Some(list)) => {
            println!("{} ({} tracks)", list.release, list.tracks.len());
            for (id, disc, number) in djinn_library::online::match_tracks(&list, &songs) {
                println!("  {} -> disc {disc} track {number}", songs[id as usize].1);
            }
        }
        other => println!("{other:?}"),
    }
}
