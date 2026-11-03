# Playlists and radio

Everyone signed in has their own playlists, which are the same in the jukebox
app, on DJINN GO and in the Android app. Guests don't have playlists.

Requests always play first. When the queue runs dry, DJINN keeps the music
going from the **auto mix**: a playlist, wild play, or radio. Admins choose
which, and a role with **Choose what the party plays** can start a playlist too. The Playing tab shows what the auto mix is playing from, and the
Playlists tab has a **Stop** button to turn it off.

## Wild play

Playlists tab > **Wild play**: random songs from your whole library, with no
repeats until it has played them all. [Smart play](smart-play.md) can steer it
toward what the party likes.

## Playlists

Make one per mood or kind of party ("Chill", "Dance", "Dinner"). Tap a song in
a playlist to request it, within the usual request limits.

### Who can see a playlist

A playlist is private to whoever made it, even from admins, until they share
it. Open it and choose under **Sharing**: everyone on this server, or particular
people. Sharing needs the **Share playlists** permission on the account's role.
People you share with can see and play it, but only you can change it.

1. On the Playlists tab, type a name and tap **Create**.
2. Add music from the Library: open a song, album or artist and tap the
   playlist button. A song's ⋯ menu also has **Add to playlist**.
3. Open the playlist and tap **Play**.

Adding a whole album or artist keeps it up to date: music by them that you add
to your library later joins the playlist by itself.

In a playlist you can:

- Choose **Shuffle** or **In order**. Shuffle plays every song once, in a
  random order, before starting over. In order picks up where it left off.
- Tap a song in the Songs list to play the playlist from there.
- Reorder or remove what's in it, rename it (tap its name), or delete it.

A song never repeats too soon: Admin > Rules > "Songs before a shuffled
playlist song can repeat" (25 by default) sets how many other songs play
first. Songs that played through requests count too.

**From a search:** search by description (like "80s rock") and tap **Save as
playlist** to keep the matches. See [Search and AI](search-and-ai.md).

## Importing playlists

Bring a playlist over from Spotify, YouTube Music, Pandora or another service.
This brings in the list, not the music: DJINN never downloads songs.

In the jukebox app or on GO, go to your playlists and tap **Import a playlist**,
then choose files or paste songs, one per line (`Artist - Title`). The Android
app has it under Library > Lists. A file holding several playlists makes
several playlists here.

A share link alone doesn't work, because it doesn't tell DJINN what's in the
playlist. To get a file:

| From | How |
| --- | --- |
| Spotify, YouTube Music, Pandora, Tidal, Deezer, Amazon and others | [Tune My Music](https://www.tunemymusic.com/) or [Soundiiz](https://soundiiz.com/): choose the service as the source and export to a file. |
| Spotify | [Exportify](https://exportify.net/), or `Playlist1.json` from your account data. |
| YouTube Music | Google Takeout. Its playlists list only video ids, so DJINN asks YouTube for each title. This is the one import that goes online. |
| Apple Music | File > Library > Export Playlist (either format). |
| Other music players | M3U, M3U8, PLS, XSPF or JSPF files. |

Matching is careful, because a flagged gap is better than the wrong song at a
party. "Song (Remastered 2011)" finds "Song", but "Song (Live)" doesn't, and
the artist has to match too.

## Songs that can't play

A playlist flags songs it can't play, and skips them when it plays. That covers:

- **Not in your library:** an imported song you don't have. It keeps its place
  in the list, and fills in by itself when a library check finds it.
- **File is missing:** the file was moved or deleted. If the same song turns
  up somewhere else in your library, the playlist switches to it.
- **File won't play:** the file is damaged. DJINN notices when a song fails
  while playing. To look for damage before that, an admin can open the playlist
  and tap **Check files**, which reads every song all the way through.

Each library check tries damaged files again, and replacing a file clears its
flag. Songs on a drive that's unplugged aren't flagged as missing while it's
away. A shuffled playlist or wild play won't pick a flagged song, and a playlist
playing in order carries on from the next song that works.

## Radio

Radio plays songs that sound like where it started, more often by the artist
it started from, and drifts with the
party: toward songs that play through, away from ones that get skipped.

Start it from:

- **A song:** its ⋯ menu > **Start radio from this song**.
- **An artist:** the **Radio** button on their page.
- **A genre:** the radio button on the genre's page.

Radio by sound needs [sound analysis](your-library.md#sound-analysis)
(Admin > Library). Without it, radio plays the same artists and genres.

## More like this

A song's ⋯ menu also has **More like this**, a list of songs that sound alike,
and shows the song's genres. Artist pages list similar artists. Both need
[sound analysis](your-library.md#sound-analysis); similar artists also use
genres from MusicBrainz.
