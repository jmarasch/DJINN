//! Prints what the analyzer hears in some files:
//! `cargo run --release -p djinn-engine --example analyze -- a.flac b.mp3`.

use std::time::Instant;

fn main() {
    let names = djinn_engine::describe();
    for path in std::env::args_os().skip(1) {
        let started = Instant::now();
        match djinn_engine::analyze(path.as_ref()) {
            Ok(analysis) => {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match analysis.features {
                    Some(f) => println!(
                        "{:>6.1} bpm  {:>5.2}s  {}={:.2} {}={:.2} {}={:.3} {}={:.2}  {}",
                        f.bpm,
                        started.elapsed().as_secs_f32(),
                        names[2],
                        f.values[2],
                        names[1],
                        f.values[1],
                        names[6],
                        f.values[6],
                        names[11],
                        f.values[11],
                        name
                    ),
                    None => println!("no features: {name}"),
                }
            }
            Err(e) => eprintln!("{e}"),
        }
    }
}
