# Rules

Admin > **Rules** holds the settings for guests and the queue. Change what you
need, then tap **Save**.

## Guest mode

**Guest mode: people join with just a name, no account.** Off, everyone needs
an account (Admin > Users). See [Guests and requests](guests-and-requests.md).

## TV page

**TV page:** a screen showing what's playing, what's next and the join code,
for a TV or tablet. Open `/tv` on that screen (for example
`http://192.168.1.20:8066/tv`); no sign-in needed. See
[Casting, Play here and the TV page](casting-and-tv.md).

## Play here

**Play here:** a **Tune In** button on the Playing tab lets anyone at the
party play it on their own phone or computer, a few seconds behind the
speakers. Off by default. See
[Play here](casting-and-tv.md#play-here).

## Online lookups

- **Look up artist photos and bios:** sends artist names to Deezer and
  Wikipedia.
- **MusicBrainz contact (email or website):** MusicBrainz needs no account,
  but asks apps to say who to contact if something goes wrong. With a contact
  filled in, DJINN adds album covers, genres and exact artist matches from
  MusicBrainz. Leave it empty to skip MusicBrainz.
- **AcoustID key:** for identifying songs by their sound. It's free for
  non-commercial use: register an application at
  [acoustid.org/new-application](https://acoustid.org/new-application) and
  paste its key.

More in [Your library](your-library.md).

## Artist names that are never split

Songs credited to "A & B" or "A feat. B" count for each artist. Band names
that contain &, commas or "x" (like "Earth, Wind & Fire") go here, one per
line, so they stay one artist. Saving re-reads the library.

## The queue

- **Most songs in the queue at once:** 0 for no limit.
- **Songs each person can have queued:** 3 by default. Backing a song someone
  else queued counts too.

Admins and anyone marked **No limit** under Admin > Users skip both limits.

## Skip votes

**Votes needed to skip a song:** 0 turns skip voting off. With it on, guests
get a **Vote to skip** button on the Playing tab. Whoever requested a song can
always skip it alone, and admins skip right away.

## Repeats

**Songs before a shuffled playlist song can repeat:** 25 by default. Songs
played through requests count too.
