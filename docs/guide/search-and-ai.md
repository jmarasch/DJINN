# Search and AI

## Searching

The **Library** tab searches as you type: artists, albums and songs by name.
You can also browse artists, albums, songs and genres, sort by name or year,
and jump to a letter (or year) with the strip beside long lists.

## Searching by description

You can also describe what you want: "80s hair metal", "chill house",
"upbeat like KREAM". DJINN shows the songs that fit under **Matches**, with
what it understood (like "1980s · hair metal"). Admins can tap **Save as
playlist** to keep them.

Descriptions work from genres, years and how songs sound, so they get better
when DJINN knows more about your music: tags in your files, genres from
MusicBrainz (see [Your library](your-library.md)), and the AI tools below.

## The AI tools (optional)

Admin > **AI** has two optional AI models. Both run on this computer; nothing
is sent anywhere, and your music folders aren't changed. Each is a one-time
download, and the page shows this computer's processor and memory so you can
judge whether it can keep up. Without enough memory they can slow the whole
computer down, music included.

### AI listening

An AI model (LAION's CLAP for music) listens to each song once, so
descriptions match how songs actually sound, even songs with no genre tags.
New songs are listened to as they're added.

1. Tap **Download the model**.
2. Switch on **Listen to songs with AI**. The page shows how many songs it has
   listened to.

Options:

- **Listening speed:** Gentle (one song at a time, lightest on the computer),
  Normal, or Fast (uses half the processor).
- **Give songs with no genres the ones the AI hears:** fills in genres for
  untagged songs, which helps genre pages, radio and smart play.
- **Stop for now** pauses it; it carries on when DJINN restarts or new music
  arrives. **Remove the model** deletes the download.

### AI reader

A small language model (Qwen2.5 1.5B Instruct) reads searches DJINN's own
phrase reader can't place, like "something for a first dance at a wedding, no
rap", and turns them into genres, years and moods. It works best with AI
listening on.

1. Tap **Download the model**.
2. Switch on **Read searches with AI**.
3. Choose who it **reads searches for**: everyone (guests too), signed-in
   users, or admins only.

When a search has words DJINN didn't understand, it asks the AI reader once
you stop typing. Matches it found are marked **read by AI**. A new search takes
about 2 to 10 seconds on the processor; searches it has read before are
instant.

### NVIDIA graphics cards

If this computer has an NVIDIA graphics card, the AI reader can use it: each
new search takes about 0.3 seconds. Tap **Download the GPU pack** (it comes
from GitHub with NVIDIA's CUDA libraries, so nothing else needs installing),
then switch on **Use the graphics card**. If the card ever fails, DJINN goes
back to the processor by itself.

The GPU pack needs an RTX 30-series or newer card. Older cards use the
processor.
