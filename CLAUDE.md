# DJINN

Self-hosted, headless party jukebox in Rust. Audio plays on the server's own output devices; guests queue songs from a phone-first web UI.

**`djinn-spec.md` is the source of truth.** Read it before starting new work. Build in phase order and do not start a later phase until the current one meets its acceptance criteria. This file only summarizes spec sections 4, 5, and 16.

## Commands

```sh
# Web UI (required before a release build; debug builds read web/dist from disk)
cd web && npm ci && npm run build      # npm run dev proxies /api to localhost:8066
cd web && npm run check                # svelte-check + tsc

# Rust (same checks CI runs on Linux and Windows)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p djinn-server -- --config config.example.toml
cargo build --release                  # single binary: target/release/djinn
```

Linux build prerequisite: `libasound2-dev` and `pkg-config`.

## Tech stack (spec section 4)

- Rust stable, 2021 edition. tokio runtime, axum for REST + Server-Sent Events.
- Audio: **cpal** for output (WASAPI on Windows, ALSA on Linux), **symphonia** for decoding (mp3, flac, aac, isomp4, vorbis, wav), **rubato** for resampling and drift correction.
- **Do not use rodio.** The engine needs dual-deck decoding, a custom mixer, and fan-out to multiple devices.
- lofty for tags and cover art. SQLite via sqlx with migrations, WAL mode. FTS5 for search. argon2 for passwords.
- tracing + tracing-subscriber for logs. Config is a TOML file plus `DJINN_*` env overrides (figment).
- Frontend: Svelte + Vite + TypeScript, built to static files and embedded in the binary with rust-embed.
- Later phases: notify (fs watching, Phase 2), bliss-audio (similarity, Phase 6).

## Architecture (spec section 5)

```
crates/
  djinn-engine/    audio: decode, mix, outputs. No web, DB, or async runtime deps.
  djinn-library/   scanner, tags, cover art, search
  djinn-store/     SQLite access, migrations, models
  djinn-server/    axum app, auth, API, SSE. Builds the `djinn` binary.
web/               Svelte app (built to web/dist, embedded by djinn-server)
migrations/        sqlx migrations
```

- `djinn-engine` must be testable alone via a CLI harness: `cargo run -p djinn-engine --example play -- file.flac`.
- **Threading:** cpal audio callbacks never allocate, lock a mutex, or touch the DB; they only read lock-free ring buffers. A dedicated engine thread decodes and mixes to fill those buffers.
- The tokio side talks to the engine over channels: commands in (`Play`, `Pause`, `Skip`, `SetVolume`, `SetCrossfade`, `AddOutput`, ...) and events out (`TrackStarted`, `TrackEnded`, `Position`, `OutputError`, ...).
- Queue logic lives in the server, not the engine. The engine asks "what's next?" and is told.

## Coding rules (spec section 16)

- No `unwrap()` or `expect()` outside tests and startup config validation. Enforced by workspace clippy lints (`clippy.toml` allows them in tests). Use `anyhow` in binaries and `thiserror` in library crates.
- Nothing on the audio callback thread may allocate, block, or log.
- Every DB change goes through a migration.
- Keep `djinn-engine` free of web, DB, and async runtime dependencies.
- Write tests for queue logic, shuffle bag, and transition math. Gapless needs a golden-sample test: two test tracks concatenated through the engine must have no gap or click at the boundary.
- Do not use em dashes in UI copy, comments, or docs.
- Exit 66 Jukebox (Go + Svelte rewrite) has no license: use it for ideas only, never copy code.
