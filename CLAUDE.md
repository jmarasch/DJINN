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
- Later phases: notify (fs watching, Phase 2), bliss-audio (similarity, Phase 5).

## Architecture (spec section 5)

```
crates/
  djinn-engine/    audio: decode, mix, outputs. No web, DB, or async runtime deps.
  djinn-library/   scanner, tags, cover art, search
  djinn-store/     SQLite access, migrations, models
  djinn-server/    axum app, auth, API, SSE. Builds the `djinn` binary.
  djinn-platform/  OS integration: tray icon, MAC lookup, Windows console. No C deps,
                   so `cargo check -p djinn-platform --target x86_64-pc-windows-gnu` works here.
web/               Svelte app (built to web/dist, embedded by djinn-server)
migrations/        sqlx migrations
```

- `djinn-engine` must be testable alone via a CLI harness: `cargo run -p djinn-engine --example play -- file.flac`.
- **Threading:** cpal audio callbacks never allocate, lock a mutex, or touch the DB; they only read lock-free ring buffers. A dedicated engine thread decodes and mixes to fill those buffers.
- The tokio side talks to the engine over channels: commands in (`Play`, `Pause`, `Skip`, `SetVolume`, `SetCrossfade`, `SetOutputs`, ...) and events out (`TrackStarted`, `TrackEnded`, `Position`, `Outputs`, ...).
- Queue logic lives in the server, not the engine. The engine asks "what's next?" and is told.

## Where things live

- Engine: `player.rs` sequences tracks and shares one resampling stage across same-rate tracks (that is what keeps resampled albums gapless); with a crossfade it holds back each track's last N seconds and blends them into the next with equal-power curves (tracks marked `gapless`, e.g. the next song on the same album, skip it); `output.rs` owns one device's cpal stream and realtime callback; `outputs.rs` fans audio out to every device: the first open one is the clock the player paces to, the rest follow it through `drift.rs` (a PI controller nudging each follower's resampling ratio by ppm to hold its buffered time, less its delay, equal to the clock's). Per-output delay is applied by padding or skipping audio. On Linux cpal uses its PulseAudio host (covers PipeWire and Bluetooth sinks). `loudness.rs` measures EBU R128 loudness for ReplayGain; `Track.gain` is applied right after decoding. `engine.rs` is the thread and command loop. Try outputs with `--example devices` and `--example outputs`. `NextNeeded { after }` names the track the engine just took.
- Server: `desktop.rs` runs the tray (skipped without a display or with `--no-tray` / `server.tray = false`); `banner.rs` picks the LAN URL and prints the terminal QR; the library check runs in the background after the server is listening. `jukebox/` decides what plays (requests first, then the active playlist: shuffle bag or in order), tracks queued, in-flight, and playing tracks, and counts skip votes (in memory, reset per song). Asking for a song already queued makes you a backer (`request_backers`): it uses one of your slots and moves the song up one place; a requester who steps back hands the request to the first backer; `playlist_api.rs` manages playlists; `tv_api.rs` serves the public, read-only TV page feed (`/tv`, gated by the `tv_page` setting); `audio_api.rs` manages outputs and ReplayGain, and measures loudness of untagged songs in the background after each library check while ReplayGain is on; `jukebox/gain.rs` turns tags or measurements into a track's gain; `folders.rs` handles library folders (config.toml plus ones added in the app) and the folder browser; `events.rs` + `sse.rs` push live state; `*_api.rs` are the REST routes.
- Library: `online.rs` looks up artist photos (Deezer, then Wikimedia Commons with credit) and bios (Wikipedia) when the admin turns it on, plus MusicBrainz (exact artist/album matches, genres, Cover Art Archive covers) once a contact is set; strict matching, MusicBrainz paced to 1 request/s, retries, 30-day recheck. Everything found is cached in `<db dir>/art/` as `<hash>.<ext>`; the music library itself is read-only.
- Store: one module per area (`library`, `playlists`, `users`, `queue`, `outputs`, `loudness`); all SQL is static strings (sqlx 0.9 rejects dynamic SQL), with shared column lists as macros used through `concat!`.
- Golden-sample audio fixtures: `crates/djinn-engine/tests/fixtures/` (README says how to regenerate). Acceptance checklists: `docs/phase1-test-party.md`, `docs/phase3-outputs.md`.

## Coding rules (spec section 16)

- No `unwrap()` or `expect()` outside tests and startup config validation. Enforced by workspace clippy lints (`clippy.toml` allows them in tests). Use `anyhow` in binaries and `thiserror` in library crates.
- Nothing on the audio callback thread may allocate, block, or log.
- Every DB change goes through a migration.
- Keep `djinn-engine` free of web, DB, and async runtime dependencies.
- Write tests for queue logic, shuffle bag, and transition math. Gapless needs a golden-sample test: two test tracks concatenated through the engine must have no gap or click at the boundary.
- Do not use em dashes in UI copy, comments, or docs.
- Exit 66 Jukebox (Go + Svelte rewrite) has no license: use it for ideas only, never copy code.
