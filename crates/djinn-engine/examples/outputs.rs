//! Plays a file on several outputs at once and prints their sync figures.
//!
//! cargo run -p djinn-engine --example outputs -- <file> <secs> <device[@trim[@delay_ms]]>...
//!
//! A device is `default` or an id from the `devices` example. The first
//! output to open is the clock; the others follow it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use djinn_engine::{
    Command, Engine, EngineConfig, Event, OutputConfig, OutputDevice, Track, TrackId,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [file, secs, devices @ ..] = args.as_slice() else {
        eprintln!("usage: outputs <file> <secs> <device[@trim[@delay_ms]]>...");
        return ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,symphonia=warn".into()),
        )
        .init();

    let outputs = devices
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let mut parts = spec.split('@');
            let device = match parts.next() {
                Some("default") | None => OutputDevice::Default,
                Some(id) => OutputDevice::Id(id.to_owned()),
            };
            OutputConfig {
                id: i as i64,
                device,
                enabled: true,
                trim: parts.next().and_then(|t| t.parse().ok()).unwrap_or(1.0),
                delay_ms: parts.next().and_then(|d| d.parse().ok()).unwrap_or(0),
            }
        })
        .collect();
    let (tx, events) = mpsc::channel();
    let engine = match Engine::start(
        EngineConfig {
            outputs,
            ..EngineConfig::default()
        },
        move |e| {
            let _ = tx.send(e);
        },
    ) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let track = Track {
        id: TrackId(0),
        path: PathBuf::from(file),
        gapless: false,
        gain: 1.0,
        start_ms: 0,
    };
    let end = Instant::now() + Duration::from_secs(secs.parse().unwrap_or(60));
    while Instant::now() < end {
        match events.recv_timeout(Duration::from_millis(200)) {
            // Loop the file so the outputs keep streaming.
            Ok(Event::NextNeeded { .. }) => {
                let _ = engine.send(Command::SetNext(Some(track.clone())));
            }
            Ok(Event::Outputs(list)) => {
                for o in list {
                    println!(
                        "output {} {:?} clock={} {:?} {:?} drift={:+.1}ppm offset={:+.2}ms {}",
                        o.id,
                        o.state,
                        o.clock,
                        o.device.unwrap_or_default(),
                        o.sample_rate,
                        o.drift_ppm,
                        o.offset_ms,
                        o.error.unwrap_or_default()
                    );
                }
            }
            Ok(Event::TrackError { error, .. }) => eprintln!("track error: {error}"),
            _ => {}
        }
    }
    ExitCode::SUCCESS
}
