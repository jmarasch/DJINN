# Getting started

DJINN runs on one computer, the one connected to your speakers. Everyone else
uses their phone's browser; there's no app to install.

## Install

Downloads are on the
[Releases page](https://github.com/jmarasch/DJINN/releases/latest). Both
installs are for your user only and need no admin rights.

**Windows 10 and 11:** download and run `djinn-setup-windows-x86_64.exe`.
That's the installer. (The other `.exe`, `djinn-windows-x86_64.exe`, is the
program on its own, which DJINN downloads when it updates itself. You don't
need it.)

- The installer isn't code signed, so Windows may say "Windows protected
  your PC". Click **More info**, then **Run anyway**. If the installer does
  nothing at all, Smart App Control is blocking it; see
  [Troubleshooting](troubleshooting.md).
- On first start Windows asks whether DJINN may use the network. Allow
  **private networks**, or phones won't be able to reach it.

**Linux** (64-bit PC, Ubuntu 22.04 or newer, Debian 12 or newer, Fedora, Mint,
Arch or similar):

```sh
curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh | sh
```

Add `-s -- --autostart` after `sh` to start DJINN when you log in. To look
before you run it, download `djinn-linux-x86_64.tar.gz`, unpack it, and run
`./djinn/install.sh`.

## First start

1. **Create the admin account.** DJINN opens your browser to "Welcome to
   DJINN". Pick a username and a password (at least 6 characters). This is the
   host account; you can add more later.
2. **Add your music.** Pick the folders where your music lives. DJINN reads
   the songs and their tags; it never changes your files. You can add more
   folders later under Admin > Library.
3. **Let people in.** Show the join QR code: tray icon > **Show join QR
   code**, or the **Join** button at the bottom of the app. Guests scan it with
   their phone camera.

DJINN lives in the system tray (the icons by the clock). Its menu has **Open
DJINN**, **Show join QR code**, **Open TV page**, and **Shut down DJINN**.

## Around the app

The bar at the bottom has:

- **Playing:** what's on now, what's next, and (for admins) play, pause, skip
  and volume.
- **Library:** search and browse artists, albums, songs and genres.
- **Queue:** the songs people have requested.
- **Join:** the QR code and address for guests.
- **Playlists** and **Admin:** for admins only.

The Admin tabs come in two rows. The first is about running the party (Rules,
Smart play, Users, Log), the second about the system (Library, Audio, AI,
About).

## Where DJINN keeps its things

- **Windows:** the program and its settings, database, log, artwork and AI
  models are in `%LOCALAPPDATA%\Programs\DJINN`. Uninstall from Settings >
  Apps; your settings stay unless you choose to remove them.
- **Linux:** the program is in `~/.local/bin`, and settings and the database
  in `~/.config/djinn/`. Uninstall with `./djinn/install.sh --uninstall`.

Your music folders are only ever read.
