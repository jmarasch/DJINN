# DJINN: Dynamic Jukebox with Intelligent Neural Navigation

A party jukebox for your own music. DJINN runs on a Windows or Linux computer
and plays through that computer's speakers (or several at once). Guests scan a
QR code and pick songs from their phones, no app or account needed. When nobody
has asked for anything, it keeps the music going on its own.

## What it does

- **Guests queue songs from their phones.** They search or browse the library
  in a browser and tap a song to queue it. Asking for a song that's already
  queued backs it, and songs more people want play first. Hosts can turn on
  skip votes, limit requests per guest, and choose what plays when nobody has
  requested anything.
- **Sounds right.** Gapless albums, crossfades (also when skipping), ReplayGain
  so songs play at the same loudness, and several speakers at once (sound card,
  HDMI, Bluetooth) kept in sync.
- **Casts** the party to Chromecast, DLNA and Sonos speakers, and shows a
  now-playing page on a TV.
- **Smart mixes.** It listens to your songs to find ones that sound alike:
  "more like this", radio from a song, artist or genre, and a mix that learns
  from what the party requests and skips. Optional local AI (downloaded on
  request, runs on your computer, faster with an NVIDIA graphics card) tags
  songs by genre and understands searches like "chill songs for a dinner".
- **Respects your library.** DJINN never changes your music files. Artist
  photos, bios, covers and song identification come from online services only
  if you turn them on, and are kept in DJINN's own folder.
- **Updates itself** from signed releases on GitHub.

How to use it: the [DJINN guide](docs/guide/README.md). What's in each release
and what's planned next: [ROADMAP.md](ROADMAP.md).

## Install

Downloads are on the [Releases page](https://github.com/jmarasch/DJINN/releases/latest).
Both installs are for your user only and need no admin rights.

### Windows 10 and 11

Download and run `djinn-setup-windows-x86_64.exe`. That's the installer; the
other `.exe` on the page, `djinn-windows-x86_64.exe`, is the program on its
own, which DJINN downloads when it updates itself.

- The installer isn't code signed yet, so Windows may say "Windows protected
  your PC". Click **More info**, then **Run anyway**.
- On first start Windows asks whether DJINN may use the network. Allow
  **private networks** so phones can reach it. On a network Windows calls
  Public, guests can't connect; DJINN tells you how to switch it to Private.

DJINN starts in the system tray and opens your browser to create the admin
account. Uninstall it from Settings > Apps.

### Linux

On a 64-bit PC (x86_64) with Ubuntu 22.04 or newer, Debian 12 or newer, Fedora,
Mint, Arch or similar:

```sh
curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh | sh
```

Add `-s -- --autostart` after `sh` to start DJINN when you log in. The script
checks the download against the release's checksums and installs to
`~/.local/bin`, with an app menu entry. Prefer to look first? Download
`djinn-linux-x86_64.tar.gz`, unpack it, and run `./djinn/install.sh`. Remove
DJINN with `./djinn/install.sh --uninstall`.

DJINN plays through ALSA, PulseAudio or PipeWire and needs the ALSA library
(`libasound2`), which almost every desktop has. Settings and the database live
in `~/.config/djinn/`.

## First party

1. Start DJINN. It opens your browser to create the admin account.
2. Add your music folder. DJINN reads the songs and their tags (it only reads).
3. Show the join QR code (tray icon > Show join QR code, or the TV page) and let
   people pick songs.

Everything else is under **Admin**: speakers, crossfade, rules for guests,
casting, AI, and updates. The [guide](docs/guide/README.md) explains each part.

## Build from source

You need Rust (stable) and Node.js. On Linux also `libasound2-dev` and
`pkg-config`; on Windows the Visual Studio Build Tools with the C++ workload.

```sh
cd web && npm ci && npm run build && cd ..
cargo build --release          # the app: target/release/djinn
cargo test --workspace
```

`djinn-spec.md` describes the design, and `CLAUDE.md` maps the code.

## License

DJINN is free for personal and other noncommercial use under the
[PolyForm Noncommercial License 1.0.0](LICENSE.md). It is source-available, not
open source: you may use, study, change and share it, but not sell it or use it
commercially.

Questions or ideas: DJINN@newterrastudios.com, or open an issue.

Copyright (c) 2026 New Terra Studios.
