//! Reads music requests with the local language model:
//! `cargo run --release -p djinn-ai --example ask -- <model folder> "request"...`.

use djinn_ai::Reader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or("usage: ask <model folder> <request>...")?;
    let started = std::time::Instant::now();
    let mut reader = Reader::load(std::path::Path::new(&dir))?;
    println!("loaded in {:.1}s", started.elapsed().as_secs_f32());
    for request in args {
        let started = std::time::Instant::now();
        let reading = reader.read(&request)?;
        println!(
            "\n{request}\n  {:.1}s: {reading:?}",
            started.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
