//! Plays files back to back through the default output device.
//!
//! cargo run -p djinn-engine --example play -- a.flac b.flac ...
//!
//! Type a command and press Enter: p (pause/resume), s (skip),
//! + / - (volume), q (quit).

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use djinn_engine::{Command, Engine, EngineConfig, Event, PlayState, Track, TrackId};

fn main() -> ExitCode {
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if files.is_empty() {
        eprintln!("usage: play <file>...");
        return ExitCode::FAILURE;
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,symphonia=warn".into()),
        )
        .init();

    let (event_tx, events) = mpsc::channel();
    let engine = match Engine::start(EngineConfig::default(), move |e| {
        let _ = event_tx.send(e);
    }) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let (input_tx, input) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if input_tx.send(line).is_err() {
                break;
            }
        }
    });

    println!("controls: p = pause/resume, s = skip, +/- = volume, q = quit");
    let name = |id: TrackId| {
        usize::try_from(id.0)
            .ok()
            .and_then(|i| files.get(i))
            .map_or_else(|| "?".to_owned(), |p| p.display().to_string())
    };
    let mut queue = files.iter().cloned().enumerate();
    let mut exhausted = false;
    let mut paused = false;
    let mut volume = 1.0f32;

    loop {
        match events.recv_timeout(Duration::from_millis(50)) {
            Ok(Event::NextNeeded { .. }) => match queue.next() {
                Some((i, path)) => {
                    let track = Track {
                        id: TrackId(i as i64),
                        path,
                        gapless: false,
                        gain: 1.0,
                    };
                    let _ = engine.send(Command::SetNext(Some(track)));
                }
                None => exhausted = true,
            },
            Ok(Event::TrackStarted { id }) => println!("\n> {}", name(id)),
            Ok(Event::Position { ms, .. }) => {
                print!("\r  {}:{:02}  ", ms / 60_000, ms / 1000 % 60);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            Ok(Event::StateChanged(PlayState::Idle)) if exhausted => break,
            Ok(event) => println!("\n  {event:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(line) = input.try_recv() {
            let command = match line.trim() {
                "p" => {
                    paused = !paused;
                    if paused {
                        Command::Pause
                    } else {
                        Command::Play
                    }
                }
                "s" => Command::Skip,
                "+" => {
                    volume = (volume + 0.1).min(1.0);
                    Command::SetVolume(volume)
                }
                "-" => {
                    volume = (volume - 0.1).max(0.0);
                    Command::SetVolume(volume)
                }
                "q" => return ExitCode::SUCCESS,
                _ => continue,
            };
            let _ = engine.send(command);
        }
    }
    println!();
    ExitCode::SUCCESS
}
