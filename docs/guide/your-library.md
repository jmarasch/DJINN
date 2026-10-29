# Your library

DJINN reads MP3, FLAC, AAC/M4A, Ogg Vorbis and WAV files, with their tags and
cover art. It never changes your music files: everything it adds (artwork,
measurements, identified details) lives in its own folder.

## Music folders

Admin > **Library** lists your music folders.

- **Add a folder** with the folder picker; on Windows it can be on any drive.
  DJINN reads the new folder right away.
- **Remove a folder** (×): its songs disappear from DJINN. The files stay.
- DJINN watches your folders and picks up new music by itself. To look for
  changes now, use **Check for changes**.

## Artist photos, bios and album details

All of these are optional and off until you turn them on under Admin >
[Rules](rules.md#online-lookups):

- **Artist photos and bios:** from Deezer, Wikimedia Commons (with credit)
  and Wikipedia.
- **MusicBrainz:** with a contact filled in, album covers from the Cover Art
  Archive, genres, and exact artist matches.

DJINN looks up new artists and albums after a library check that found
changes. On the Library tab, **Look up new artists** does it now, and
**Recheck all** redoes every artist. Everything found is kept in DJINN's own
folder (**Open image folder** shows it).

## Identify songs

Admin > AI > **Identify songs** finds the album, year and genres for songs
whose tags are missing them, by how they sound (AcoustID, then MusicBrainz).

1. Add an AcoustID key and a MusicBrainz contact under Admin > Rules.
2. Tap **Identify songs**. DJINN listens for matches in the background.
3. Review what it found. Each match shows what it would fill in and how sure
   it is. Tap **Use** to keep it or **Skip** to leave the song as it is.

A match must agree with the title and artist already in the file. What you
keep is stored in DJINN's database; your files aren't changed.

## Correct tags

Wrong or missing titles, artists, albums, years and genres can be fixed in
the app (admins only). DJINN keeps the corrections in its own database and
uses them in place of the file's tags, even after the file is read again;
the files themselves aren't changed.

- **One song:** open its menu (⋮) and tap **Edit tags**.
- **A whole album:** on the album's page, tap **Edit album** to change the
  album name, album artist, year or genre for all its songs at once.
- **An artist:** on the artist's page, tap **Edit artist** to fix their
  name on every song and album they're on (their photo and bio stay), or
  set the genres of the songs they lead.
- Genres are separated by commas. Corrected genres take the place of the
  ones looked up online for those songs.
- An artist like "Björk feat. Thom Yorke" is split into its artists, the
  same way as tags from the file.
- A corrected field says **Corrected · use the file's**; tap it to go back
  to what the file says. Emptying a field does the same, and **Use the
  file's tags** undoes every correction.

## Genres

The Library's **Genres** tab lists every genre with its number of songs, as
tags or as a cloud: bigger genres have more songs, and lines join genres that
share songs or sound alike. Drag, pinch or scroll to move around the cloud.
Each genre's page shows related genres, its artists and songs, and a radio
button.
