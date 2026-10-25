# Phase 1 test party checklist

Phase 1 is done when, at a test party, phones can queue songs, the server
plays them gaplessly through the default device, and it falls back to the
auto pool when the queue empties (spec section 15).

## Before the party

1. Build: `cd web && npm ci && npm run build`, then `cargo build --release`.
   The binary is `target/release/djinn`.
2. Write `config.toml` next to the binary (Windows) or in
   `~/.config/djinn/` (Linux). Set `library.roots` to the music folders.
   Relative paths resolve against the config file's folder.
3. Start `djinn`. The log prints a setup link the first time:
   `Create one at http://localhost:8066/setup?token=...`. Open it on the
   server machine and create the admin account.
4. Wait for `library check finished` in the log (or watch Admin > Library).
5. Either turn on **guest mode** (Admin > Rules) so guests join with just
   a name, or create an account for each guest in Admin > Users.
6. Build the auto pool: search for albums or artists, open them, and tap
   "Add to auto pool". Add enough that the history window (default 25
   songs) does not exhaust it in a night.
7. Set the volume in Admin. It is remembered across restarts.
8. Find the server's LAN address (`ip addr` or `ipconfig`). Guests open
   `http://<address>:8066`. Allow port 8066 through the firewall.

## During the party

Tick each one off:

- [ ] Guests sign in from their phones.
- [ ] A guest searches, taps +, and the song shows in Queue on every
      phone within a second or two, without refreshing.
- [ ] Requested songs play in queue order, before pool songs.
- [ ] Now Playing shows who requested the current song, and "Up next".
- [ ] A fourth request from the same guest is refused (default limit 3).
- [ ] A guest can remove their own request but not someone else's.
- [ ] Admin can reorder the queue, skip, pause, and resume.
- [ ] When the queue empties, pool songs start with no silence between.
- [ ] Pool songs do not repeat within the history window.
- [ ] Gapless: play an album with continuous tracks (live album, DJ mix,
      or a concept album). There is no gap or click between tracks.
- [ ] A phone that sleeps or loses Wi-Fi catches up when it comes back.
- [ ] Restarting the server keeps accounts, queue, pool, and volume.

## Afterwards

Listening history is in the `play_events` table (source, requester,
played time, who skipped). A quick look:

```sh
sqlite3 djinn.db "SELECT t.title, p.source, p.played_ms, p.skipped
                  FROM play_events p JOIN tracks t ON t.id = p.track_id
                  ORDER BY p.started_at DESC LIMIT 30;"
```

## Known limits in Phase 1

- **No crossfade yet.** Transitions are gapless only. Crossfade is Phase 2.
- **Only the system default output.** Multiple outputs are Phase 3.
- **No "Add to Home Screen" app install over plain HTTP.** Browsers only
  allow installing a web app over HTTPS (or on localhost). Over plain
  HTTP on the LAN, iOS Safari's "Add to Home Screen" still makes a
  shortcut.
- **Cover art is served at full size.** Thumbnails are Phase 2.
- **Rescans are manual.** Changes to the music folders are picked up at
  startup or by pressing Rescan; the file watcher is Phase 2.
- **Guest devices are recognized by MAC address** only on the server's own
  network segment, and phones use per-network private addresses. The
  year-long guest cookie is the main way a phone is remembered.
