//! The playlist check: what search by description and radio would queue,
//! song by song, with what each song is known by (year, genres, the AI's
//! styles, the artist's genres, tempo) and its score, for grading by ear
//! and tuning the ranking. It runs the server's own code over a library.
//!
//! ```sh
//! DJINN_CHECK_DB=<a database> DJINN_CHECK_MODELS=<models folder> \
//! DJINN_CHECK='80s hair metal; song:kickstart my heart; artist:kream; genre:glam metal' \
//! cargo test --release -p djinn-server playlist_check -- --ignored --nocapture
//! ```
//!
//! The database is copied to a temporary folder first (opening one applies
//! migrations), so the file named is never changed. `DJINN_CHECK_MODELS` is
//! DJINN's models folder; without it the AI match and the AI reader are left
//! out. `DJINN_CHECK_COUNT` is how many songs to list (30). Checks are
//! separated by `;`: a description, or `song:`, `artist:` (found by name, the
//! first hit) or `genre:` for a radio. `ai genres` shows how often the
//! AI's best genre for a song agrees with the song's own, album's and
//! artist's genres, by how sure the AI was.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::jukebox::{Mixer, RadioSeed};
use crate::state::AppState;

const CHECKS: &str = "80s hair metal; 80s hair; glam rock; chill cooking music; dinner music";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a library database; see the module's notes"]
async fn playlist_check() {
    let Ok(db) = std::env::var("DJINN_CHECK_DB") else {
        println!("set DJINN_CHECK_DB to a database to check");
        return;
    };
    let count: usize = std::env::var("DJINN_CHECK_COUNT")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(30);
    let checks = std::env::var("DJINN_CHECK").unwrap_or_else(|_| CHECKS.to_owned());

    let work = std::env::temp_dir().join(format!("djinn-playlist-check-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::copy(&db, work.join("djinn.db")).unwrap();
    let mut state = AppState::for_tests_in(&work).await;
    if let Ok(models) = std::env::var("DJINN_CHECK_MODELS") {
        let models = PathBuf::from(models);
        state.ai = std::sync::Arc::new(crate::ai::AiState::new(&models));
        match state.ai.load_now() {
            Ok(()) => println!("AI match loaded"),
            Err(e) => println!("AI match not loaded: {e}"),
        }
        match state.ai.reader.load_now() {
            Ok(()) => println!("AI reader loaded"),
            Err(e) => println!("AI reader not loaded: {e}"),
        }
    }
    super::refresh(&state).await;

    for check in checks.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let report = match check.split_once(':') {
            Some(("song", name)) => song_radio(&state, name.trim(), count).await,
            Some(("artist", name)) => artist_radio(&state, name.trim(), count).await,
            Some(("genre", name)) => {
                let seed = RadioSeed::Genre {
                    name: name.trim().to_owned(),
                };
                radio(&state, &format!("genre {}", name.trim()), &seed, count).await
            }
            _ if check == "ai genres" => ai_genres(&state),
            _ => description(&state, check, count).await,
        };
        println!("{report}");
    }
    let _ = std::fs::remove_dir_all(&work);
}

async fn song_radio(state: &AppState, name: &str, count: usize) -> String {
    let found = state.store.search(name, 1).await.unwrap();
    let Some(track) = found.tracks.first() else {
        return format!("== radio from song \"{name}\": no such song\n");
    };
    let label = format!(
        "song {} - {} (#{})",
        track.artist.as_deref().unwrap_or("?"),
        track.title,
        track.id
    );
    radio(state, &label, &RadioSeed::Track { id: track.id }, count).await
}

async fn artist_radio(state: &AppState, name: &str, count: usize) -> String {
    let found = state.store.search(name, 5).await.unwrap();
    let Some(artist) = found.artists.first() else {
        return format!("== radio from artist \"{name}\": no such artist\n");
    };
    let label = format!("artist {} (#{})", artist.name, artist.id);
    radio(state, &label, &RadioSeed::Artist { id: artist.id }, count).await
}

/// Radio's list, best first, then a play order drawn from it.
async fn radio(state: &AppState, label: &str, seed: &RadioSeed, count: usize) -> String {
    let ranked = super::SmartMixer::new(state).radio(seed, &[], &[]);
    let mut out = format!(
        "== radio from {label}: {} songs in its pool\n-- best first (weight)\n",
        ranked.len()
    );
    let best: Vec<(i64, String)> = ranked
        .iter()
        .take(count)
        .map(|(id, w)| (*id, format!("{w:.3}")))
        .collect();
    out.push_str(&rows(state, &best).await);
    // A fixed draw, so reruns compare; the jukebox's real draw also weighs
    // what the party liked and holds back what played lately.
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let order = super::draw_in_order(ranked, move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 40) as f32 / (1u64 << 24) as f32
    });
    let played: Vec<(i64, String)> = order
        .into_iter()
        .take(count)
        .enumerate()
        .map(|(n, id)| (id, format!("#{}", n + 1)))
        .collect();
    out.push_str("-- a play order drawn from it\n");
    out.push_str(&rows(state, &played).await);
    out
}

/// What search shows for a description: the phrase reader at once, then
/// the AI reader's reading when search would ask for it.
async fn description(state: &AppState, query: &str, count: usize) -> String {
    let found = state.store.search(query, 60).await.unwrap();
    let plain_found =
        !found.tracks.is_empty() || !found.albums.is_empty() || !found.artists.is_empty();
    let mut out = format!(
        "== \"{query}\": by name {} songs, {} albums, {} artists\n",
        found.tracks.len(),
        found.albums.len(),
        found.artists.len()
    );
    // The description's sound, as `describe` makes it (without the reader's
    // extra words), to show each song's AI match.
    let sound = state
        .ai
        .model()
        .and_then(|m| m.embed_text(&query.trim().to_lowercase()).ok());
    let ask = super::worth_asking(state, query, plain_found);
    let described = super::describe(state, query, plain_found, None)
        .await
        .unwrap();
    out.push_str(&described_rows(state, "phrase reader", described, sound.as_deref(), count).await);
    if ask {
        let started = std::time::Instant::now();
        match crate::reader::read(state, query).await {
            Some(read) => {
                let _ = writeln!(
                    out,
                    "-- AI reader ({:.1} s): {read:?}",
                    started.elapsed().as_secs_f32()
                );
                let described = super::describe(state, query, false, Some(&read))
                    .await
                    .unwrap();
                out.push_str(
                    &described_rows(state, "AI reader", described, sound.as_deref(), count).await,
                );
            }
            None => out.push_str("-- AI reader: no reading\n"),
        }
    } else if state.ai.reader.ready() {
        out.push_str("-- search wouldn't ask the AI reader\n");
    }
    out
}

async fn described_rows(
    state: &AppState,
    by: &str,
    described: Option<super::Described>,
    sound: Option<&[f32]>,
    count: usize,
) -> String {
    let Some(d) = described else {
        return format!("-- {by}: not read as a description\n");
    };
    let mut out = format!(
        "-- {by}: [{}], unknown [{}], {} fit, showing {}\n",
        d.summary.join(" · "),
        d.unknown.join(", "),
        d.total,
        d.tracks.len()
    );
    let heard = state
        .catalog
        .read()
        .map(|c| c.heard.clone())
        .unwrap_or_default();
    let listed: Vec<(i64, String)> = d
        .tracks
        .iter()
        .take(count)
        .map(|t| {
            let score = sound
                .and_then(|s| heard.get(&t.id).map(|v| djinn_ai::similarity(v, s)))
                .map_or_else(|| "-".to_owned(), |s| format!("ai {s:.3}"));
            (t.id, score)
        })
        .collect();
    out.push_str(&rows(state, &listed).await);
    out
}

/// One line per song: score, artist - title, year, its genres, the AI's
/// styles, the artist's genres, tempo.
async fn rows(state: &AppState, songs: &[(i64, String)]) -> String {
    let ids: Vec<i64> = songs.iter().map(|(id, _)| *id).collect();
    let tracks: HashMap<i64, djinn_store::Track> = state
        .store
        .tracks_by_ids(&ids)
        .await
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t))
        .collect();
    let Ok(catalog) = state.catalog.read() else {
        return String::new();
    };
    let facts: HashMap<i64, &djinn_library::vibe::SongFacts> =
        catalog.songs.iter().map(|s| (s.id, s)).collect();
    let mut out = String::new();
    for (n, (id, score)) in songs.iter().enumerate() {
        let Some(t) = tracks.get(id) else { continue };
        let fact = facts.get(id);
        let genres = fact.map(|f| f.genres.join(", ")).unwrap_or_default();
        let styles = catalog
            .styles
            .get(id)
            .map(|s| s.join(", "))
            .unwrap_or_default();
        let mut artist_genres: Vec<&String> = t
            .artist_id
            .and_then(|a| catalog.artist_genres.get(&a))
            .map(|g| g.iter().collect())
            .unwrap_or_default();
        artist_genres.sort();
        let bpm = fact
            .and_then(|f| f.bpm)
            .map_or_else(String::new, |b| format!(" {b:.0} bpm"));
        let feel = fact.map_or_else(String::new, |f| {
            let energy = f.energy.map_or_else(|| "-".into(), |e| format!("{e:+.2}"));
            let mood = f.mood.map_or_else(|| "-".into(), |m| format!("{m:+.3}"));
            format!(" energy {energy} mood {mood}")
        });
        let _ = writeln!(
            out,
            "{:>3} {score:>9}  {} - {}  ({})  [{genres}] ai [{styles}] artist [{}]{bpm}{feel}",
            n + 1,
            t.artist.as_deref().unwrap_or("?"),
            t.title,
            t.year
                .map_or_else(|| "no year".to_owned(), |y| y.to_string()),
            artist_genres
                .iter()
                .map(|g| g.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    out
}

/// How often the AI's best genre for a song agrees with the genres the song
/// has from tags and MusicBrainz, grouped by its score and by how far
/// ahead of the next genre it was.
fn ai_genres(state: &AppState) -> String {
    let genres = state.ai.genre_embeddings();
    let Ok(catalog) = state.catalog.read() else {
        return String::new();
    };
    if genres.is_empty() {
        return "== ai genres: the AI model isn't loaded\n".into();
    }
    let names: Vec<String> = genres
        .iter()
        .map(|(n, _)| djinn_library::vibe::normalize(n))
        .collect();
    // (score, lead over the next, agrees)
    let mut rows: Vec<(f32, f32, bool)> = Vec::new();
    for song in &catalog.songs {
        let Some(v) = catalog.heard.get(&song.id) else {
            continue;
        };
        let known: Vec<&String> = song
            .own_genres
            .iter()
            .chain(&song.album_genres)
            .chain(&song.artist_genres)
            .collect();
        if known.is_empty() {
            continue;
        }
        let mut scored: Vec<(f32, usize)> = genres
            .iter()
            .enumerate()
            .map(|(i, (_, g))| (djinn_ai::similarity(v, g), i))
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let (best, i) = scored[0];
        let lead = best - scored[1].0;
        let name = &names[i];
        let agrees = known.iter().any(|g| {
            djinn_library::vibe::genre_fits(g, name) || djinn_library::vibe::genre_fits(name, g)
        });
        rows.push((best, lead, agrees));
    }
    let mut out = format!(
        "== ai genres: {} songs with genres of their own\n",
        rows.len()
    );
    let share = |pick: &dyn Fn(&(f32, f32, bool)) -> bool| {
        let chosen: Vec<&(f32, f32, bool)> = rows.iter().filter(|r| pick(r)).collect();
        let right = chosen.iter().filter(|r| r.2).count();
        (
            chosen.len(),
            right as f32 * 100.0 / chosen.len().max(1) as f32,
        )
    };
    out.push_str("-- by score: songs, % agreeing\n");
    for low in [0.30, 0.35, 0.40, 0.45, 0.50, 0.55, 0.60] {
        let (n, pct) = share(&|r| r.0 >= low && r.0 < low + 0.05);
        let _ = writeln!(out, "   {low:.2}..{:.2}  {n:>6}  {pct:5.1}%", low + 0.05);
    }
    out.push_str("-- by lead over the next genre\n");
    for low in [0.0, 0.01, 0.02, 0.03, 0.04, 0.05, 0.07, 0.10] {
        let (n, pct) = share(&|r| r.1 >= low);
        let _ = writeln!(out, "   lead >= {low:.2}  {n:>6}  {pct:5.1}%");
    }
    out.push_str("-- both\n");
    for (s, l) in [
        (0.40, 0.03),
        (0.45, 0.03),
        (0.45, 0.05),
        (0.50, 0.03),
        (0.50, 0.05),
        (0.40, 0.07),
    ] {
        let (n, pct) = share(&|r| r.0 >= s && r.1 >= l);
        let _ = writeln!(
            out,
            "   score >= {s:.2}, lead >= {l:.2}  {n:>6}  {pct:5.1}%"
        );
    }
    out
}
