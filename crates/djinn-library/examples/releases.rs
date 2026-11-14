//! Searches each source for an album and shows the first release's tracks:
//! `cargo run -p djinn-library --example releases -- <MusicBrainz contact> "artist" "album"`.

use djinn_library::releases::{cover, release, search, Source};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for source in [Source::MusicBrainz, Source::Deezer, Source::Itunes] {
        let hits = match search(source, Some(&args[0]), &args[1], &args[2]).await {
            Ok(hits) => hits,
            Err(e) => {
                println!("{source:?}: {e}");
                continue;
            }
        };
        println!("{source:?}: {} found", hits.len());
        for hit in hits.iter().take(3) {
            println!(
                "  {} / {} ({:?}, {:?}, {} tracks, {:?})",
                hit.artist, hit.title, hit.date, hit.format, hit.tracks, hit.note
            );
        }
        if let Some(first) = hits.first() {
            if let Ok(Some(r)) = release(source, Some(&args[0]), &first.id).await {
                let names: Vec<String> = r
                    .tracks
                    .iter()
                    .take(4)
                    .map(|t| format!("{}.{} {}", t.disc, t.number, t.title))
                    .collect();
                println!(
                    "  tracks: {} ({}...), year {:?}",
                    r.tracks.len(),
                    names.join(", "),
                    r.year
                );
            }
            match cover(source, &first.id).await {
                Ok(Some(bytes)) => println!("  cover: {} KB", bytes.len() / 1024),
                other => println!("  cover: {other:?}"),
            }
        }
    }
}
