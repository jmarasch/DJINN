# Roadmap

What DJINN can do today, and what's planned next. Plans can change; ideas
and requests are welcome at DJINN@newterrastudios.com or as an issue.

## Released

### 0.2.5 (October 2026)

- **Party styles.** Keep a separate memory for each kind of party
  ("Halloween", "Family BBQ") and pick the one that fits tonight. The
  auto mix learns from requests and skips per style, now for artists and
  genres too, not just songs. Admin > Smart play turns it on or off, sets
  how much it counts, can use it for shuffled playlists, shows what each
  style learned, and clears a style's history.
- **A user guide** to every feature, in [docs/guide](docs/guide/README.md).
- Each release now starts with "Which file do I download?".

### 0.2.0 (October 2026)

- **Browse by genre.** A Genres tab in the library lists every genre with
  its number of songs, as tags or as a cloud: bigger genres have more songs,
  and lines join genres that share songs or sound alike. Drag, pinch or
  scroll to move around and zoom the cloud.
- **Related genres** on each genre's page, the same ones its lines go to on
  the cloud.
- **Jump through long lists.** Artists, albums and songs have an A to Z strip
  on the side; sorted by year, it runs from your oldest year to your newest.
- **Sort by year,** oldest or newest first, for albums and songs.
- Library folders on any drive on Windows, not just C:.
- A clear button in the search box on every browser.
- DJINN quits promptly, even with background work running, so restarts and
  updates aren't held up.

### 0.1.0 (October 2026), the first release

**The party**

- Guests scan a QR code and queue songs from their phones, with no app or
  account needed. Hosts and regular users can have accounts.
- Asking for a song that's already queued backs it; songs more people want
  play first.
- When the queue runs dry, DJINN keeps playing from the library or a
  playlist (shuffled without repeats, or in order).
- Optional rules: requests per guest, a cap on the whole queue, and skip
  votes (voting to skip one song means sitting out the vote on the next).
- A now-playing page for a TV.
- Playlists.

**Sound**

- Gapless playback, so albums play without breaks.
- Crossfades between songs, also when skipping.
- ReplayGain, so songs play at the same loudness (from tags, or measured).
- Several speakers at once (sound card, HDMI, Bluetooth), kept in sync, with
  a delay per speaker to line them up.

**Casting**

- Play the party on Chromecast and Google TV, DLNA TVs, receivers and
  speakers, and Sonos, several at once, with or without local speakers.
- Show the now-playing page on a TV that plays the music too.

**Smart mixes and AI**

- DJINN listens to your songs to find ones that sound alike: "more like
  this", similar artists, and radio from a song, artist or genre.
- The radio and auto mix learn from the party: requested songs come up more,
  skipped ones less.
- Search by description, like "80s rock" or "chill house".
- Genres shown next to each song, with a page per genre.
- Optional local AI, downloaded on request and run on your own computer:
  tags songs that have no genre by how they sound, and understands free-form
  searches like "chill songs for a dinner". Faster with an NVIDIA graphics
  card.

**Your library**

- Reads MP3, FLAC, AAC/M4A, Ogg Vorbis and WAV, with tags and cover art.
- Watches your music folders and picks up new songs.
- Never changes your music files.
- Optional online lookups: artist photos and bios, album covers and genres
  from MusicBrainz, and identifying songs with missing tags by how they
  sound (AcoustID). All kept in DJINN's own folder.

**Running it**

- Windows 10 and 11, and Linux, with installers that need no admin rights.
- Lives in the system tray and starts with your computer if you like.
- Updates itself from signed releases.

## Planned

### 0.3.0

- **Play here.** A button that plays the party on the phone, tablet or
  laptop you're using: an extra speaker for another room, or the party on
  your headphones. Like casting, it runs a few seconds behind the main
  speakers.
- **Edit song tags.** Correct wrong or missing titles, artists, albums,
  years and genres from the app, for one song, a whole album, or an
  artist (a name fixed everywhere they appear).
- **Back up and restore.** Save everything DJINN knows (settings, users,
  playlists, history, speakers and casts, artwork, analysis) to one file,
  and restore it, or move it to another computer with your music in a
  different place. Your music files aren't part of the backup.
- **Picks up where it left off.** After a restart or an update, the song
  that was playing carries on from where it was.
- **System tools.** An Admin > System tab with DJINN's status (including
  the addresses phones should open), updates, backups, a log viewer,
  restart and quit, and tools to clear the queue, history, analysis, tag
  corrections or settings, or reset DJINN completely (your music is never
  touched).

### 0.4.0

- **Personal streaming.** Listen to your library on your own phone, apart
  from the party.

### 0.5.0

- **Visualizers.** Visuals moving to the music, from a large library of
  MilkDrop-style presets, drawn by the DJINN computer and sent to TVs as
  video together with the sound, so picture and music stay in step on
  every screen. Pick the size (480p, 720p, 1080p), or show them in a
  window on the DJINN computer itself.

### Later

- **Karaoke mode.**

### Not planned for now

- AirPlay.
- Streaming services; DJINN plays your own music.
