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
- While DJINN works through new music, the top of the Library tab shows
  each step: finding music, reading songs, sorting albums and artists, and
  then sound analysis and AI listening when they're on. Songs can be played as
  soon as they've been read; the later steps carry on in the background.

## Artist names that are never split

Songs credited to "A & B" or "A feat. B" count for each artist. Band names
that contain &, commas or "x" (like "Earth, Wind & Fire") go under Admin >
Library > **Artist names that are never split**, one per line, so they stay
one artist. Saving re-reads the library.

## Extras

A library check reads your music folders and the songs' tags, and that's all
DJINN needs to play them. Everything else is an extra on Admin > **Library**,
each off until you switch it on, so a modest computer isn't kept busy with
work you don't want.

### Sound analysis

Listens to every song once, on the DJINN computer, for "more like this",
similar artists, radio by sound, smart play, and even volume for songs without
ReplayGain tags. It's the heaviest job DJINN does: on a modest computer about
a song a second, using up to half the processor, so a big library takes hours
the first time. After that, only new songs.

### When heavy work runs

Sound analysis and AI listening can run **any time**, **only while no music is
playing**, or **only between two hours** you pick (this computer's time; from
10 PM until 6 AM runs overnight). They pause between songs, so they stop
within seconds when music starts or the hours end, and carry on where they
left off. The progress at the top of the Library tab says when they're
waiting and why.

### Artist photos and bios

From Deezer, Wikimedia Commons (with credit) and Wikipedia: sends artist
names to them after a library check that finds new artists. Pictures in your
own folders always win.

**More pictures** adds a slideshow to artist pages, and to the TV page while
their songs play: up to eight photos from the artist's Wikimedia Commons
category, each with its photographer and license. It needs MusicBrainz too
(below), which is how DJINN finds the right artist; switching it on rechecks
your artists once. Each artist's pictures are fetched the first time they're
shown, so the first visit takes a few seconds. Not every artist has pictures
on Commons; those pages look as before.

### MusicBrainz

Album covers from the Cover Art Archive, album and artist genres, and exact
artist matches (which also finds better photos and bios). MusicBrainz needs no
account, but asks apps to say who to contact if something goes wrong: fill in
an email address or website, then switch it on.

With either lookup on, **Look up new artists** does it now and **Recheck all**
redoes every artist. Everything found is kept in DJINN's own folder (**Open
image folder** shows it).

### Identify songs by sound

Finds the album, year and genres for songs whose tags are missing them, by
how they sound (AcoustID, then MusicBrainz). It only runs when you start it.

1. Fill in the MusicBrainz contact, and an AcoustID key: free for
   non-commercial use, register an application at
   [acoustid.org/new-application](https://acoustid.org/new-application) and
   paste its key.
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
