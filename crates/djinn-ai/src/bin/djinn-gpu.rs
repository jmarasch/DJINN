//! The AI reader on an NVIDIA GPU, as a helper program. DJINN itself runs
//! on the CPU everywhere; this one needs NVIDIA's CUDA libraries, which
//! come with it in DJINN's optional GPU pack, so it lives in its own
//! program that DJINN starts when the pack is installed.
//!
//! `djinn-gpu <model folder>` loads the model, then speaks JSON lines:
//! first `{"ready": true, "gpu": true}` (or `{"error": "..."}`), then for
//! each `{"read": "request"}` line in, a `{"reading": {...}}` or
//! `{"error": "..."}` line out. It ends when its input closes.

use std::io::{BufRead, Write};

use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Request {
    read: String,
}

fn main() {
    let mut out = std::io::stdout().lock();
    let reply = |out: &mut std::io::StdoutLock, value: serde_json::Value| {
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    };
    let Some(dir) = std::env::args_os().nth(1) else {
        reply(
            &mut out,
            json!({ "error": "usage: djinn-gpu <model folder>" }),
        );
        return;
    };
    let mut reader = match djinn_ai::Reader::load(std::path::Path::new(&dir)) {
        Ok(reader) => reader,
        Err(e) => {
            reply(&mut out, json!({ "error": e.to_string() }));
            return;
        }
    };
    reply(&mut out, json!({ "ready": true, "gpu": reader.on_gpu() }));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let answer = match serde_json::from_str::<Request>(&line) {
            Ok(request) => match reader.read(&request.read) {
                Ok(reading) => json!({ "reading": reading }),
                Err(e) => json!({ "error": e.to_string() }),
            },
            Err(e) => json!({ "error": format!("bad request: {e}") }),
        };
        reply(&mut out, answer);
    }
}
