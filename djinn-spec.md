# DJINN: Project Spec

**DJINN**: Dynamic Jukebox with Intelligent Neural Navigation.

A self-hosted, headless party jukebox written in Rust. Audio plays on the central server through its own output devices. Guests use a web UI on their phones to search the library and queue songs. When the guest queue is empty, the server plays from an approved pool.

This file is the source of truth for the project. Read it fully before writing code. Build in the phase order below and do not start a later phase until the current phase meets its acceptance criteria.

---

## 1. Goals

- Headless server that plays audio directly on the host machine. No browser or display needed on the host.
- Web UI (phone first) to browse, search, and queue songs, with live updates.
- Library built from local music files: tags, cover art, full-text search.
- User accounts, saved playlists.
- Queue logic: user requests first, then auto-fill from an approved pool.
- Gapless playback and configurable crossfade.
- Multiple simultaneous outputs (for example optical plus Bluetooth) with per-output delay and drift correction.
- Runs on Linux and Windows from a single binary per OS.
- Logs listening events from day one to feed a future smart playlist feature.

## 2. Non-goals (for now)

- Streaming services (Spotify, Apple Music, etc.). Local files only.
- Native mobile apps. Phones use the web player or existing Subsonic apps (Phase 7).
- Transcoding. Not needed while audio plays on the host.

## 3. Platforms

- Linux (primary target: Debian/Ubuntu, likely running in a Proxmox LXC container with the sound device passed through). ALSA backend, which routes through PipeWire/PulseAudio when present.
- Windows 10/11. WASAPI backend.
- Default behavior: use the OS's currently selected default output device. Additional outputs are opt-in from the admin page.

## 4. Tech stack

| Concern | Choice | Notes |
|---|---|---|
| Language | Rust (stable, 2021 edition) | |
| Async runtime | tokio | |
| Web framework | axum | REST + Server-Sent Events |
| Audio output | cpal | WASAPI on Windows, ALSA on Linux |
| Decoding | symphonia | Pure Rust. Enable mp3, flac, aac, isomp4, vorbis, wav features |
| Resampling | rubato | Sample-rate conversion and drift correction |
| Tags and cover art | lofty | |
| Database | SQLite via sqlx (with migrations) | WAL mode |
| Search | SQLite FTS5 | |
| Password hashing | argon2 | |
| Logging | tracing + tracing-subscriber | |
| Config | TOML file + env overrides | |
| Frontend | Svelte + Vite (TypeScript) | Built to static files |
| Embedding UI | rust-embed | UI ships inside the binary |
| Filesystem watching | notify | Phase 2+ |
| Similarity (future) | bliss-audio (bliss-rs) | Phase 6 |

Linux build prerequisite: `libasound2-dev` and `pkg-config`.

Do not use rodio. The engine needs dual-deck decoding, a custom mixer, and fan-out to multiple devices, which rodio's sink model does not fit.

## 5. Architecture

Cargo workspace:

```
djinn/
  Cargo.toml              (workspace)
  crates/
    djinn-engine/         audio: decode, mix, outputs. No web or DB deps.
    djinn-library/        scanner, tags, cover art, search
    djinn-store/          SQLite access, migrations, models
    djinn-server/         axum app, auth, API, SSE. Builds the `djinn` binary
  web/                    Svelte app
  migrations/
  config.example.toml
  CLAUDE.md               (copy of the key rules from this spec)
```

The `djinn-engine` crate must be testable on its own with a CLI harness (`cargo run -p djinn-engine --example play -- file.flac`).

### Threading model

- Audio threads (cpal callbacks) never allocate, lock a mutex, or touch the DB. They only read from lock-free ring buffers.
- A dedicated engine thread runs decode and mix, filling the buffers.
- The tokio side communicates with the engine through channels: commands in (`Play`, `Pause`, `Skip`, `SetVolume`, `SetCrossfade`, `AddOutput`, ...) and events out (`TrackStarted`, `TrackEnded`, `Position`, `OutputError`, ...).
- The queue logic lives in the server, not the engine. The engine only asks "what's next?" and is told.

## 6. Audio engine

### 6.1 Pipeline

```
[Deck A decoder] ─┐
                  ├─> [Mixer + gain curves] ─> [Master f32 stereo stream @ 48 kHz]
[Deck B decoder] ─┘                                  │
                                                     ├─> [Output 1: delay + rubato + ring buffer] ─> cpal device
                                                     └─> [Output 2: delay + rubato + ring buffer] ─> cpal device
```

- Decode to interleaved f32 stereo. Resample every track to the internal master rate (48 kHz) at decode time.
- Two decks. While deck A plays, deck B pre-decodes the next track so it's ready before A ends.
- Mono sources are upmixed to stereo; multichannel sources are downmixed to stereo.

### 6.2 Transitions

- One code path for both modes: a transition of length N seconds with an equal-power gain curve.
- Gapless = N of 0. The next track's first sample immediately follows the previous track's last sample.
- Crossfade = N of 1 to 12 seconds, configurable at runtime.
- Respect encoder delay and padding where symphonia exposes it (MP3/AAC gapless info) so gapless is truly gapless.
- Optional later: trim leading and trailing silence.

### 6.3 Outputs

- Output 0 is always "system default device" and follows the OS default.
- Additional outputs are selected by device name from the admin page and persisted in the DB.
- Each output has: enabled flag, volume trim, delay offset in ms (0 to 1000), and its own native sample rate. rubato converts from master rate to device rate.
- Drift correction: each output monitors its ring buffer fill level and adjusts its resampling ratio by tiny amounts (parts per million) to hold a target fill. This keeps multiple devices in sync over hours.
- A device disappearing (Bluetooth disconnects) must not stop playback on other outputs. Log it, emit `OutputError`, retry reconnection on an interval.

### 6.4 Volume

- Master volume plus per-output trim. Apply in the f32 domain with a smoothed ramp to avoid clicks.
- ReplayGain support (track gain from tags) behind a setting. Phase 3.

## 7. Library

- Configurable music roots (multiple allowed).
- Scanner walks roots, reads tags with lofty, and upserts tracks keyed by path plus file size and mtime. Unchanged files are skipped.
- Cover art: use embedded art first, then `cover.jpg`, `folder.jpg`, `front.jpg` in the track's folder. Store a hash and serve resized thumbnails (cache to disk, sizes 128 and 512).
- Search: FTS5 over title, artist, album, album artist, genre. Prefix matching for search-as-you-type.
- Phase 2: filesystem watcher for incremental updates. Phase 1: manual rescan button plus scan on startup.

## 8. Queue logic

Two sources:

1. **Request queue**: songs added by users, first come first served, except that songs more people asked for play first (see backing below).
2. **Auto pool**: the "approved" list, managed by admin.

When the current track ends (or the transition starts):

1. If the request queue is non-empty, take the head.
2. Otherwise draw from the auto pool using a **shuffle bag**: shuffle the full pool, play through it in order, reshuffle when exhausted. Skip any track played within the last `history_window` tracks (default 25).

Rules:
- Per-user limit on queued, unplayed requests (default 3, configurable). Admin is exempt.
- Same track cannot be in the request queue twice. Asking for a queued song backs it instead: it uses one of your request slots, and the queue is ordered by how many people back each song, then by request order. Stepping back restores the order exactly.
- Admin can reorder (among songs with as many backers), remove, skip, and pause.
- Users can remove their own requests.
- The approved pool can be built from: individual tracks, whole albums, whole artists, or saved playlists.

## 9. Users and auth

- Roles: `admin`, `user`. First run prints a one-time bootstrap URL to the log to create the admin account.
- Accounts: username plus password (argon2). Session cookie, HttpOnly, SameSite=Lax.
- Admin can create invite links.
- **Open decision:** whether party guests need a full account or can join via a QR code with just a display name (a `guest` role with a session tied to a party code). Build Phase 1 with real accounts only, and keep the role model ready to add `guest`.

## 10. Data model (initial)

```sql
users(id, username UNIQUE, password_hash, role, created_at)
sessions(id, user_id, token_hash, expires_at)

tracks(id, path UNIQUE, size, mtime, duration_ms, title, artist, album,
       album_artist, track_no, disc_no, year, genre, art_hash,
       sample_rate, channels, codec, added_at)
tracks_fts  -- FTS5 virtual table over title, artist, album, album_artist, genre

playlists(id, owner_id, name, created_at, updated_at)
playlist_tracks(playlist_id, track_id, position)

pool_entries(id, kind, ref_id)        -- kind: track | album | artist | playlist

request_queue(id, track_id, user_id, added_at, position)

outputs(id, device_name, enabled, volume_trim, delay_ms)
settings(key PRIMARY KEY, value)      -- crossfade_secs, history_window, per_user_limit, ...

-- Event log for the future smart playlist feature. Write from Phase 1.
play_events(id, track_id, source,     -- source: request | pool
            requested_by, started_at, ended_at,
            played_ms, skipped, skipped_by)
request_events(id, track_id, user_id, at)
```

## 11. API

REST under `/api`, JSON. Live state over SSE at `/api/events`.

- `POST /api/auth/login`, `POST /api/auth/logout`, `GET /api/me`
- `GET /api/search?q=` (tracks, albums, artists)
- `GET /api/tracks/:id`, `GET /api/albums/:id`, `GET /api/artists/:id`
- `GET /api/art/:hash?size=128|512`
- `GET /api/queue`, `POST /api/queue` `{track_id}`, `DELETE /api/queue/:id`, `PATCH /api/queue/:id` (admin reorder)
- `GET /api/now-playing`
- Player (admin): `POST /api/player/play|pause|skip`, `PUT /api/player/volume`, `PUT /api/player/crossfade`
- Playlists: CRUD under `/api/playlists`
- Pool (admin): CRUD under `/api/pool`
- Outputs (admin): `GET /api/outputs/devices`, CRUD under `/api/outputs`
- Library (admin): `POST /api/library/rescan`, `GET /api/library/status`

SSE event types: `now_playing`, `position` (every 1 s), `queue_changed`, `player_state`, `output_status`, `scan_progress`.

## 12. Web UI

Phone first, works on desktop. Installable PWA.

Screens:
- **Now Playing**: art, title, artist, progress, up next.
- **Search**: search-as-you-type, tap to queue, album and artist drill-down.
- **Queue**: requests with who added them; your own items removable.
- **Playlists**: create, edit, add from search.
- **Admin**: player controls, volume, crossfade slider, outputs (enable, delay slider, trim), approved pool, users and invites, rescan.

Dark theme default. Large tap targets. Optimistic UI updates, reconciled by SSE.

## 13. Config

`config.toml` (path via `--config`, defaults next to binary on Windows, `~/.config/djinn/` on Linux):

```toml
[server]
listen = "0.0.0.0:8066"

[library]
roots = ["/srv/music"]

[database]
path = "djinn.db"

[audio]
master_rate = 48000
crossfade_secs = 0
```

Runtime-tunable settings live in the `settings` table, not the config file.

## 14. Deployment

- Linux: systemd service (user or system), install script in `packaging/linux/`.
- Windows: run as a tray app or as a service. Start with a plain console app; service wrapper later.
- Release builds: `cargo build --release` on each OS. Embedded UI means one file to copy.

## 15. Phases

### Phase 0: Skeleton
- Workspace, crates, CI (`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`) on Linux and Windows.
- Config loading, tracing, empty axum server serving the embedded Svelte placeholder.
- **Done when:** binary builds and runs on both OSes and serves a page.

### Phase 1: It plays music (MVP)
- Engine: single output (system default), decode via symphonia, gapless track-to-track.
- Library scan, tags, cover art, FTS search.
- Accounts (admin bootstrap, login, user creation by admin).
- Request queue + shuffle-bag auto pool + history window + per-user limit.
- SSE now playing and queue.
- UI: Now Playing, Search, Queue, basic Admin.
- `play_events` and `request_events` logged.
- **Done when:** at a test party, phones can queue songs, the server plays them gaplessly through the default device, and falls back to the pool when the queue empties.

### Phase 2: Crossfade and library polish
- Dual-deck mixer, equal-power crossfade, runtime setting.
- Filesystem watcher. Thumbnail cache.
- Playlists CRUD and adding playlists to the pool.

### Phase 3: Multiple outputs
- Device enumeration, add/remove outputs, per-output delay and trim.
- rubato drift correction, Bluetooth disconnect resilience.
- ReplayGain.
- **Done when:** optical and Bluetooth play together for 3+ hours without audible drift after calibration.

### Phase 4: Guests and party features
- Resolve the guest access decision (QR + display name vs accounts).
- Optional: request voting, "skip" votes, now-playing display page for a TV.

### Phase 5: Casting
- DJINN streams the party mix over HTTP as a network output, so the music can play on devices that aren't wired to the server. Several devices at once.
- Targets: Chromecast / Google TV (Google Cast, Default Media Receiver), DLNA/UPnP media renderers (smart TVs, AV receivers, network speakers), and Sonos. AirPlay is out of scope for now.
- Discover devices on the LAN (mDNS for Cast, SSDP for UPnP). Start, stop, and watch each cast from Admin > Audio.
- The TV page can play the stream itself, so a TV or tablet browser gets picture and sound.
- Works with no local output at all (cast-only), paced by the system clock.
- Network devices buffer seconds of audio and report no timing, so casts are not sample-synced with local outputs; document this rather than promise it.
- **Done when:** a Chromecast, a DLNA TV or receiver, and a Sonos speaker play the party at the same time for a full evening, with skips and song changes reaching them.

### Phase 6: Smart playlists ("tiny AI")
- Run bliss-rs analysis during scan; store feature vectors.
- Similarity search: "more like this track/artist", seeded radio for the auto pool.
- Use play/skip history to weight results.
- Optional: small local language model that turns plain-language prompts ("chill 90s, nothing too slow") into structured queries over tags, features, and history. Must run fully offline.
- Optional AI listening: a local audio-text model (CLAP) hears genre and mood in songs with no tags and matches free descriptions against the audio. Downloaded on request, runs on the CPU (a GPU only speeds it up). Results live in DJINN's database; music folders stay read-only.
- Identify songs with missing or wrong tags by audio fingerprint (AcoustID, then MusicBrainz), opt-in like other online lookups; corrections are stored in DJINN, not written to files.
- Browse by genre: Library > Genres shows every genre as tags, or as a cloud where bigger genres have more songs and lines join related genres (shared songs, similar sound); each opens its genre page.

### Phase 7: DJINN GO (personal streaming)

Personal listening, apart from the party, on the same library and database. Same binary and process; its own listener (default port 8067, `[go]` in the config, off until the admin turns it on) so it can be reached from away from home without exposing the party or Admin. Released in steps: 0.4.0, 0.4.1, 0.4.2, 0.4.3.

**Accounts and roles (0.4.0)**
- One set of users for the party and GO. Built-in roles `admin` and `guest` stay; every other role is admin-defined, with permissions. Existing `user` accounts move to a built-in, editable "Listener" role.
- Permissions include: use GO, use GO from outside the home network, streams at once, download songs (off by default), share playlists, submit music, submitted music added without review (0.4.1), party request limits.
- Sign-up modes: closed (admin adds people), needs admin review, or open, each into a chosen role. Invite links can name a role.
- Admin can add, approve, disable and delete accounts, reset passwords, and see each account's signed-in devices.

**Security (0.4.0)**
- The GO listener serves only GO: no Admin API, no bootstrap, no guest join.
- Login slows down and then locks out per address and per account. Minimum password length, common passwords refused.
- Optional two-step sign-in (TOTP) with one-time recovery codes; admin can require it for GO per role and for admins. Once an account has it on, every sign-in asks for a code, on either port.
- Remote access is a role permission; without it, GO only accepts addresses on the home network (private ranges, Tailscale).
- `trusted_proxies` setting so lockouts and remote checks see the real client address behind Cloudflare Tunnel or a reverse proxy.
- Session cookie `Secure` when reached over HTTPS; idle expiry; per-device sign-out.
- Stream URLs carry short-lived signed tokens.
- Security headers (CSP, frame-ancestors, HSTS over HTTPS).
- Sign-in log in Admin (successes, failures, lockouts, address).
- CI runs `cargo audit`.

**Listening (0.4.0)**
- Web player: browse and search (shared with the party UI), your own queue with shuffle and repeat, next song preloaded for near-gapless albums, Media Session controls.
- **The device does the work, not the server.** GO sends the original file (HTTP range requests, ETag and caching headers) and the browser or app decodes and plays it. The server never decodes or transcodes for GO, so many listeners cost only disk reads and upload bandwidth.
- The player checks `canPlayType` for each song's format; songs the device can't play are marked and skipped rather than converted.
- ReplayGain is applied on the device (Web Audio gain from the track's gain, sent with the song).
- Admin > GO shows what streaming uses, so the admin can see when the home upload speed, not the server, is the limit.
- Playback position follows you across devices.
- Likes (songs, albums, artists), your own playlists (private or shared), history, "continue listening", personal radio (the smart mixer, learning from your likes and skips).
- Personal listening doesn't feed party learning. "Send to the party" requests a song.

**Remote access (0.4.0 guides, 0.4.x button)**
- Admin > Remote access explains three ways in: Tailscale (recommended; DJINN shows its Tailscale address and QR), Cloudflare Tunnel (public HTTPS, no port forwarding), or port forwarding.
- 0.4.x: an "Open it for me" button maps the port with UPnP-IGD, detects carrier-grade NAT, and only opens with HTTPS: DJINN gets and renews a Let's Encrypt certificate for a dynamic DNS name.

**Submitting music (0.4.1)**
- Users with the permission upload songs. Uploads wait in `<db dir>/uploads/` (size limit, audio formats only, checked by decoding), get fingerprinted for duplicates and checked with identify, then go to admin review or straight in, per role.
- Approved songs move into a dedicated, writable Uploads library folder. Other music folders stay read-only.

**Phones (0.4.2)**
- Subsonic/OpenSubsonic API on the GO listener so existing apps (Symfonium, DSub, play:Sub, Substreamer, Feishin) work, including their offline downloads where the role allows downloads.
- The web player installs as an app (PWA).

**Converting (0.4.3)**
- Optional and off by default: the admin can let GO convert songs for devices that can't play the original, or to save data on phones. Settings choose which formats get converted, the target (MP3 with the encoder casting already uses), the bitrate, and how many conversions run at once so the party never stutters. Roles choose who may ask for it.
- Converted copies are cached on disk (size-capped, oldest removed first) so a popular song is only converted once.
- Subsonic apps that ask for a lower bitrate get it from here when allowed, the original otherwise.

**Later:** lyrics, ListenBrainz/Last.fm scrobbling, expiring share links.

### Phase 8: Library backup and import
- Back up everything DJINN knows (settings, users, playlists, request history, outputs, casts, artwork cache, analysis) to one file, and restore or import it on another machine, remapping music folder paths. The music files themselves are not part of it.

## 16. Coding rules

- No `unwrap()` or `expect()` outside tests and startup config validation. Use `anyhow` in binaries and `thiserror` in library crates.
- Nothing on the audio callback thread may allocate, block, or log.
- Every DB change goes through a migration.
- Keep the `djinn-engine` crate free of web, DB, and async runtime dependencies.
- Write tests for queue logic, shuffle bag, and transition math. Add a golden-sample test for gapless: concatenating two test tracks through the engine must produce no gap or click at the boundary.
- Do not use em dashes in UI copy, comments, or docs.

## 17. References

- Exit 66 Jukebox (original Java, GPLv2): https://sourceforge.net/projects/exit66jukebox.berlios/ . The headless "server plays to its own sound card" model this project follows.
- Exit 66 Jukebox (current Go + Svelte rewrite): https://github.com/andybarilla/exit66jukebox . No license file at time of writing, so treat it as reference for ideas only. Do not copy code.

## 18. First task for Claude Code

1. Read this whole file.
2. Create `CLAUDE.md` summarizing sections 4, 5, and 16 for future sessions.
3. Complete Phase 0.
4. Propose a Phase 1 task breakdown and wait for approval before starting it.
