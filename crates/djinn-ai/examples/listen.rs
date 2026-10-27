//! Labels songs with the downloaded CLAP model:
//! `cargo run --release -p djinn-ai --example listen -- <model dir> <file>...`.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: listen <model dir> <file>...");
        return;
    };
    let clap = match djinn_ai::Clap::load(dir.as_ref()) {
        Ok(clap) => clap,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let labels = [
        "rock",
        "glam metal",
        "synth pop",
        "gothic rock",
        "house",
        "pop",
        "hip hop",
        "jazz",
        "sad",
        "energetic",
        "calm",
        "dark",
    ];
    let label_vecs: Vec<Vec<f32>> = labels
        .iter()
        .filter_map(|l| clap.embed_text(&format!("This is a {l} song.")).ok())
        .collect();
    for path in args {
        let started = std::time::Instant::now();
        let samples =
            match djinn_engine::decode_mono(path.as_ref(), djinn_ai::SAMPLE_RATE as u32, 720) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{e}");
                    continue;
                }
            };
        let Ok(Some(song)) = clap.embed_song(&samples) else {
            continue;
        };
        let mut scores: Vec<(f32, &str)> = label_vecs
            .iter()
            .zip(labels)
            .map(|(v, l)| (djinn_ai::similarity(v, &song), l))
            .collect();
        scores.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top: Vec<String> = scores
            .iter()
            .take(4)
            .map(|(s, l)| format!("{l} {s:.2}"))
            .collect();
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!("{:.1}s  {:?}  {name}", started.elapsed().as_secs_f32(), top);
    }
}
