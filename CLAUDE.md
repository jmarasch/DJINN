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

Windows build prerequisites: rustup (MSVC toolchain), Visual Studio Build Tools with the C++ workload (SQLite and the exe icon compile with it), Node.js LTS, and Inno Setup 6 for the installer. With winget: `winget install Rustlang.Rustup OpenJS.NodeJS.LTS JRSoftware.InnoSetup` and `winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"` (without the override it installs no C++ compiler). Use the commands above from PowerShell.

Installers (current user, no admin; see `docs/releasing.md`):

```sh
iscc packaging\windows\djinn.iss     # Windows, after the release build: target\installer\djinn-setup-windows-x86_64.exe
packaging/linux/install.sh            # Linux, after the release build (the release tarball ships the same script)
```

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
  djinn-cast/      casting: live MP3 stream of the mix, device discovery and control.
  djinn-ai/        optional local models: CLAP (tract, CPU) for song and text embeddings,
                   Qwen2.5 reader (candle) for free-form searches; `djinn-gpu` helper
                   (feature `cuda`, needs the CUDA toolkit) runs it on NVIDIA cards.
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

- Engine: `player.rs` sequences tracks and shares one resampling stage across same-rate tracks (that is what keeps resampled albums gapless); with a crossfade it holds back each track's last N seconds and blends them into the next with equal-power curves (tracks marked `gapless`, e.g. the next song on the same album, skip it); `output.rs` owns one device's cpal stream and realtime callback; `outputs.rs` fans audio out to every device: the first open one is the clock the player paces to, the rest follow it through `drift.rs` (a PI controller nudging each follower's resampling ratio by ppm to hold its buffered time, less its delay, equal to the clock's). Per-output delay is applied by padding or skipping audio. On Linux cpal uses its PulseAudio host (covers PipeWire and Bluetooth sinks). `loudness.rs` measures EBU R128 loudness for ReplayGain; `features.rs` describes how a song sounds (tempo, punch, brightness, noisiness, mode, mel cepstrum, chroma: 37 numbers, `analyze` does both in one decode); `Track.gain` is applied right after decoding. `engine.rs` is the thread and command loop. Try outputs with `--example devices` and `--example outputs`. `tap.rs` copies the master stream for casting; with no output open but a tap listening, `outputs.rs` paces playback by the system clock (cast-only). `NextNeeded { after }` names the track the engine just took.
- Server: `desktop.rs` runs the tray (skipped without a display or with `--no-tray` / `server.tray = false`); `banner.rs` picks the LAN URL and prints the terminal QR; the library check runs in the background after the server is listening. `jukebox/` decides what plays (requests first, then the auto mix in `jukebox/auto.rs`: the active playlist by shuffle bag or in order, wild play, or radio), tracks queued, in-flight, and playing tracks, and counts skip votes (in memory, reset per song); its tests are in `jukebox/tests.rs`. Asking for a song already queued makes you a backer (`request_backers`): it uses one of your slots, and the queue plays songs with more backers first, then in request order (`queue_order!` in `djinn-store::queue`, worked out on read; admin moves stay among songs with as many backers); a requester who steps back hands the request to the first backer; `playlist_api.rs` manages playlists; `cast_api.rs` serves the live stream at `/cast/<token>/live.mp3` (ICY "Artist - Title" for players that ask; a token instead of a session, since devices fetch it themselves; `djinn-cast::stream` encodes it and fills pauses with silence), Play here (`/api/listen` hands the stream URL to anyone signed in while the `play_here` setting is on; `web/src/lib/playHere.svelte.ts` plays it, reconnects, and sets the lock-screen Media Session) and the admin cast API; `djinn-cast` finds devices (`discover.rs`: mDNS for Google Cast, SSDP on every interface for DLNA/Sonos, since a VPN steals default multicast), plays to them (`cast.rs`: CASTV2 with the Default Media Receiver for audio, or DashCast to show the TV page (`/tv?cast=1`, which plays the stream itself); `upnp.rs`: AVTransport SOAP, Sonos via `x-rincon-mp3radio`), and `caster.rs` keeps each cast alive with retries; active casts resume after a restart (`cast_targets` setting); try `--example discover` and `--example probe <ip>` (both read only); `tv_api.rs` serves the public, read-only TV page feed (`/tv`, gated by the `tv_page` setting); `audio_api.rs` manages outputs and ReplayGain, and measures loudness of untagged songs in the background after each library check while ReplayGain is on; `jukebox/gain.rs` turns tags or measurements into a track's gain; `folders.rs` handles library folders (config.toml plus ones added in the app) and the folder browser; `events.rs` + `sse.rs` push live state; `*_api.rs` are the REST routes; `settings.rs` reads and writes admin settings (`number`, `flag`, `set_flag`), and each setting's name lives with its feature.
- Library: `online.rs` looks up artist photos (Deezer, then Wikimedia Commons with credit) and bios (Wikipedia) when the admin turns it on, plus MusicBrainz (exact artist/album matches, genres, Cover Art Archive covers) once a contact is set; strict matching, MusicBrainz paced to 1 request/s, retries. Automatic lookups (after a library check that found changes) only cover artists and albums not looked up yet; "Recheck all" redoes everything. Everything found is cached in `<db dir>/art/` as `<hash>.<ext>`; the music library itself is read-only.
- Smart playlists: the background analysis job (`audio_api::start_analysis`) stores features per track; `djinn-library::sound::SoundIndex` (weighted z-scores, tuned on a real library) answers "more like this" and similar artists; `smart.rs` keeps it in memory with song facts (years, genres from tags plus MusicBrainz, popularity) and serves `/api/tracks/{id}/similar` and `/api/artists/{id}/similar` (genre overlap plus sound). `djinn-library::vibe` reads descriptions ("80's hair metal", "chill house", "upbeat like KREAM") into an intent and ranks songs; search returns them as `described`.
- Radio and learning from the party: `jukebox/mix.rs` holds `RadioSeed` (song, artist, genre), the `Mixer` trait, `party_weight` (requests up, skips down), `Learned` (what one party style liked, per song, artist and genre, from `SongLinks` the mixer supplies) and the weighted draw; wild play and radio draw by weight, playlists keep the shuffle bag (ordered by weight with `smart_playlists`). Party styles (`djinn-store::party`, `party_api.rs`, Admin > Smart play): requests and plays record the style in use (`style_in_use!`), clearing a style detaches its events from learning but keeps the request log; `smart_play`, `smart_playlists` and `smart_strength` settings. Radio is active "playlist" `RADIO` (-2) with the `radio` setting; `smart::SmartMixer` ranks songs with `djinn-library::radio::rank` (sound around the seed, drift toward songs played through, away from skipped ones, genre overlap, artist spread). `smart.rs` also keeps per-song styles (AI-heard genres), artist genres from songs when MusicBrainz has none, and serves `/api/genres/{name}` and `/api/tracks/{id}/tags`. Library > Genres lists every genre (`/api/genres`, built by `djinn-library::genres::map`: genres linked by shared songs and by average sound, each keeping its strongest few links) as tags or as a cloud (`GenreCloud.svelte`, a small force layout drawn once).
- AI listening: `ai.rs` downloads the pinned CLAP model (sizes and SHA-256 checked) into `<db dir>/models/`, loads it when switched on, and listens to songs in the background (`tracks.heard`); `smart.rs` uses it to tag songs without genres and to match descriptions by sound (`describe`). Admin > AI holds both models with this computer's specs (`djinn-platform::system::memory`), stop buttons, and options (`ai_listen_speed`, `ai_genre_tags`, `ai_reader_for`). `cargo run -p djinn-ai --example listen -- <model dir> <files>` tries it.
- AI reader: `djinn_ai::Reader` runs Qwen2.5 1.5B Instruct (pinned GGUF in `models::READER`) with candle on the CPU (`cuda`/`metal` features for a GPU). `qwen2.rs` is candle's quantized Qwen2 plus `rewind`, so the system prompt is read once at load and each request costs about 2 s. `reader.rs` (server) loads it, caches answers, and serves `/search/ask`; search sets `ask` when the phrase reader left words it didn't know, and the web page asks after typing stops. `vibe::merge` combines the model's reading with the phrase reader's (model genres replace phrase genres; artists and exclusions must be named in the request). `cargo run --release -p djinn-ai --example ask -- <model dir> "request"` tries it.
- GPU pack: `djinn-ai`'s `djinn-gpu` binary (feature `cuda`) is the reader as a JSON-lines helper; `gpu.rs` downloads this version's pack (`gpu-<os>-<arch>.<file>.zst` release assets, checked against the signed SHA256SUMS, unpacked with ruzstd) into `<db dir>/models/gpu-<version>/` and runs it; `reader.rs` tries it first when an NVIDIA driver is present and falls back to the in-process CPU reader on any failure. The release workflow's `gpu` job builds it with the CUDA toolkit.
- Identifying songs: `djinn_engine::fingerprint` (Chromaprint via rusty-chromaprint), `djinn-library::identify` (AcoustID lookup, then MusicBrainz for year and genres; a match must agree with existing title/artist tags), `identify_api.rs` (job, review, apply). Applied matches live in `track_ids` and fill gaps whenever `save_track` saves the file; files are never changed. Tag corrections made in the app (Edit tags, Edit album) live in `tag_edits` (`djinn-store::tag_edits`) and replace the file's tags as `djinn-library::tags::read` reads it (before artists split); `library_api::reread` re-reads the files and refreshes everything after either. Needs an AcoustID key and the MusicBrainz contact (Rules).
- Backups: `backup.rs` zips a database snapshot (`Store::snapshot`, VACUUM INTO), the art cache (not thumbnails) and a manifest of music folders; an uploaded backup waits in `<db dir>/restore/` with the admin's folder moves, and at the next start `backup::put_in_place` swaps the database before it opens (the old one kept as `<name>.before-restore`, put back if the new one won't open), then `backup::finish` moves song paths (`Store::move_library`) and library folders.
- Updates: `update.rs` serves Admin > About (version, commit from `build.rs`, links) and checks `jmarasch/DJINN` releases daily; installs verify a signed `SHA256SUMS` (ed25519, `PUBLIC_KEY`), replace the binary, and restart (`update::restart` after shutdown). Releases come from `.github/workflows/release.yml` on a `v*` tag; see `docs/releasing.md`.
- Library lists: artists, albums and songs have an A to Z (or year) jump strip (`JumpStrip.svelte`) fed by `/api/{artists,albums,tracks}/jumps` (where each letter or year starts, computed with the same ORDER BY macros as the pages in `djinn-store::library`); `InfiniteList` can start mid-list after a jump and loads the stretch above as you scroll up.
- Store: one module per area (`library`, `playlists`, `users`, `queue`, `outputs`, `loudness`); all SQL is static strings (sqlx 0.9 rejects dynamic SQL), with shared column lists as macros used through `concat!`.
- Golden-sample audio fixtures: `crates/djinn-engine/tests/fixtures/` (README says how to regenerate). Acceptance checklists and test notes are kept locally in `notes/`, which is not in the repository.

## Coding rules (spec section 16)

- No `unwrap()` or `expect()` outside tests and startup config validation. Enforced by workspace clippy lints (`clippy.toml` allows them in tests). Use `anyhow` in binaries and `thiserror` in library crates.
- Nothing on the audio callback thread may allocate, block, or log.
- Every DB change goes through a migration.
- Keep `djinn-engine` free of web, DB, and async runtime dependencies.
- Write tests for queue logic, shuffle bag, and transition math. Gapless needs a golden-sample test: two test tracks concatenated through the engine must have no gap or click at the boundary.
- Do not use em dashes in UI copy, comments, or docs.
- Exit 66 Jukebox (Go + Svelte rewrite) has no license: use it for ideas only, never copy code.
