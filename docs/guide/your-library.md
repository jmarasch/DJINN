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

## Album years

A reissue's files often carry the reissue's date. DJINN uses the original
release date when the tags have one, and an album shows the year most of its
songs give. To use the file's own date instead, turn off **Use the original
release year** in Admin > Library; that re-reads the library.

## Albums with the same name

Two releases can share a name and artist, like Weezer's Blue and Green albums.
When two folders hold different songs at the same place on an album (two
different "track 1"s), DJINN shows them as separate albums, each with its
folder's name. Discs of one album stay together, whether in folders like `CD1`
or with their own disc numbers, and so does one song in the wrong folder. Likes
and playlists stay with the biggest part. **Keep albums with the same name
apart** in Admin > Library turns this off.

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

The Library's **Genres** tab lists every genre with its number of songs, or
shows them as a cloud, where bigger means more songs and lines join related
genres.

The cloud opens on your 100 biggest genres. Tap one to center on it, with what's
near it and what's near those; tap it again to open its page, or tap another to
move there. That way every genre can be reached, however small. **Home** goes
back to the top from anywhere, and your back button goes back a step. Drag to
move around, and pinch, scroll or use the buttons to zoom. You can choose how
many genres the top shows, and how many lines to draw.

Each genre's page shows related genres, a row of its artists, a radio button,
and all its songs, loading more as you scroll. They come in a shuffled order
that favors what's been played, by anyone, on the jukebox, GO or a phone app.
The same order stays for a while, so you can come back to it. **Admin > Library**
sets how many load at a time and how many hours until it's shuffled again.


## Importing lyrics from a zip

Admin > Library > Lyrics can load lyrics from a zip of files. DJINN keeps them in its database and never writes to your music folders.

**The zip.** Lay the files out like your music folders, the same way Export .lrc files does: under the name of the library folder, at the song's place, named like the song. For example, `Music/Artist/Album/01 Song.lrc` for `/srv/music/Artist/Album/01 Song.flac`. Use `.lrc` or `.txt`, as UTF-8 text.

- A `.lrc` file may start with the header lines `[ar:Artist]`, `[ti:Title]`, `[al:Album]` and `[length:mm:ss]`. DJINN matches the song by artist and title first, using album and length when several songs fit. When those lines are missing, and for every `.txt`, it matches by the path.
- Timed lines (`[mm:ss.xx]words`) make timed lyrics; anything else is plain lyrics. A file holding only `[Instrumental]` is ignored.
- A zip may hold up to 250 MB and 150,000 files, and each file up to 200 KB. For a bigger library, send several zips: the import is safe to repeat and any order works.

**What it does.** It only fills songs that have no lyrics. Lyrics from a `.lrc` beside the song, from the song's tags or from LRCLIB always win. By default it skips songs LRCLIB hasn't been asked about yet (while the LRCLIB lookup is on), so LRCLIB gets the first go; a tick box imports them anyway. Songs that already hold lyrics from an earlier import keep them unless you tick "Replace lyrics from an earlier import".

**Safety.** Files are read only as text. A file that looks like code or markup (SQL, script or HTML tags, shell commands, binary data) is thrown out and listed as a warning, and program or data files in the zip (`.exe`, `.sql`, `.zip` and so on) are never read. Admin Home reminds you until you clear the report. The rest of the zip still imports.

Timed lyrics are always preferred: if a song has both timed and plain imported lyrics, the timed ones are kept, and a later zip with timed lyrics takes the place of plain ones without asking.

When it finishes, the panel shows what was saved and skipped, and the full report can be downloaded. **Remove all imported lyrics** in Admin > System takes back everything that came from zips and leaves other lyrics alone.

## Reporting bad lyrics

On the Playing tab and in DJINN GO, **Report these lyrics** under a song's lyrics lets anyone say they are another song's, out of time, garbled, or missing parts. Reports show in Admin > Library > Problems as **Lyrics listeners reported**, with a button to look at the lyrics. **Remove and block** takes the lyrics away and remembers which source they came from (a `.lrc` file, the song's tags, LRCLIB or an import): that source's same text is not used for the song again. If the source gives different text later, it is used. **Done** clears the reports and keeps the lyrics. **Allow them again** in Admin > Library > Lyrics lifts every block.

## Problems and duplicates

Admin > Library > **Problems** lists what a library check couldn't add or found broken (missing folders, unreadable files, songs that won't play, listeners' reports) and gaps in the tags. One check is about copies:

- **Albums filed twice** are albums by one artist that hold almost the same songs (at least 85% of the larger one's song titles, and nearly the same number of songs): a copy in another folder, or the same album under a slightly different name such as "(Remastered)". A deluxe edition with extra songs isn't listed.
- **A song on several albums is fine.** A track on both its album and a greatest hits is not a problem. The **Possible duplicates** card lists copies of a song only when they are in the same album (or have no album); tick **Also list copies in different albums** to see the rest.

Other checks on the Problems page (each can be hidden once you know about it):

- **The same file twice in one album**: same size, length and title.
- **Impossible numbers**: a track or disc number of 0 or less, or a length of 0.
- **Very short or very long songs**: under 10 seconds or over 3 hours.
- **Low-quality songs**: under 96 kbps.
- **Titles with leftovers**: a track number and separator in front ("01 - "), "(Official Video)", a web address, spaces around, all capitals.
- **File names that don't match the title**: the file's name and folders don't contain the title, ignoring spaces, dashes, colons, quotes and other punctuation.
- **Artists spelled two ways**: "The Beatles" and "Beatles", "AC/DC" and "ACDC".
- **Albums split across artist names**: an album's folder filed under two artists.
- **Albums with different covers inside**.
- **Albums with missing or repeated track numbers**.
- **Songs with no lyrics**, and **songs with no loudness data** (only once other songs have it).
- **Songs that went missing lately**: gone from the library in the last 30 days and not back under another path (a song you moved isn't listed).

Not checked yet: covers that are tiny, huge or not square (DJINN doesn't measure pictures yet), and silent or very quiet files.

DJINN never deletes anything: remove the copy you don't want from your music folders, then check for changes.

### MP3 lengths

Many MP3s have no length header, so a tag reader can only guess their length, and it guesses wrong (often several times too long) for variable bit rate files that open with a quiet stretch. DJINN looks at such files: if the bit rate is the same wherever it samples, the guess stands, otherwise it counts every frame, which is exact. The figure is kept in DJINN's database; your files are never changed. The first library check after an update re-reads every file once to correct existing songs.

## Library editor

Admin > **Library editor** is where library problems will be fixed: tags, names, covers and lengths, in DJINN's database or, when you choose, in the files themselves. It sits behind the lock in Admin > Protected areas (see Accounts and roles), so opening it asks for your password, and a code too at the strictest level.

The first piece is a **compare**. It reads every song's tags from its file and sets them next to what DJINN's database holds, for title, artist, album, album artist, track and disc number, year and genre. Each difference says why:

- **Corrected in DJINN**: you changed it with Edit tags or Edit album; the file still has the old value.
- **Filled in by identification**: the file had nothing there and an applied identification filled it in.
- **Other**: the file changed since DJINN read it (a library check re-reads changed files), or the two readings differ.

The compare only reads; nothing in your music folders or the database changes. About half a minute for 40,000 songs on a local disk.
