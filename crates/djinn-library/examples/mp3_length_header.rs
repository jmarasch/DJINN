//! Puts a length header in a variable bit rate MP3 that has none (or takes
//! it out with `--remove`): `cargo run -p djinn-library --example
//! mp3_length_header -- COPY.mp3`. It changes the file, so use a copy.

use std::path::Path;

use lofty::file::AudioFile;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let remove = args.iter().any(|a| a == "--remove");
    for arg in args.iter().filter(|a| *a != "--remove") {
        let path = Path::new(arg);
        let length = || {
            lofty::read_from_path(path)
                .ok()
                .map(|f| f.properties().duration().as_millis())
        };
        let before = length();
        let done = if remove {
            djinn_library::mpeg::remove_length_header(path)
        } else {
            djinn_library::mpeg::add_length_header(path)
        };
        println!("{before:?} -> {:?}\t{done:?}\t{arg}", length());
    }
}
