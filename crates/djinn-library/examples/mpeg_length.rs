//! Compares the tag reader's length with the counted one for MP3 files:
//! `cargo run -p djinn-library --example mpeg_length -- file.mp3 ...` (read only).

use std::path::Path;

use lofty::file::AudioFile;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let guessed = lofty::read_from_path(path)
            .ok()
            .map(|f| f.properties().duration().as_millis());
        let counted = djinn_library::basics(path).and_then(|b| b.duration_ms);
        println!("{guessed:?}\t{counted:?}\t{arg}");
    }
}
