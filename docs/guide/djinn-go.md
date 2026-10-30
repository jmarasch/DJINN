# DJINN GO

DJINN GO is your music library for yourself, apart from the party. Each
person signs in on their own phone or computer, picks songs, and they play
there. The DJINN computer only sends the song files; each device does the
playing, so many people can listen at once.

GO has its own address (port 8067), separate from the party's. It only
serves listening: no Admin pages, no queue, no party controls. That makes
it safer to reach from away from home.

## Turning it on

Admin > **GO**, switch **DJINN GO is on**. The tab then shows the
addresses to open on your home network, such as `http://192.168.1.20:8067`,
and your Tailscale address if this computer runs Tailscale.

Who can use it is set per role under Admin > **Users** (see
[Accounts and roles](accounts-and-roles.md)):

- **Use DJINN GO:** on for Listener when you start.
- **Use GO away from home:** off for Listener. Without it, GO only lets that
  role in from your home network (and Tailscale).
- **GO streams at once:** how many devices can play at the same time.
- **Download songs:** off unless you turn it on.
- **Must use two-step sign-in for GO:** see below.

Admins can always use GO.

## Listening

- **Home** carries on where you left off (on this device or another), and
  shows what you played lately, your playlists, and what's new.
- **Library** is the whole library, like the party app's: browse by
  artist, album, song or genre (as a list or a grid, with the A to Z strip
  and sorting), or type to search. Tapping a song in the big lists plays it,
  then songs like it; in search results, an album or a playlist, the songs
  after it follow.
- **The player** along the bottom opens to Now Playing: seek, shuffle,
  repeat (all, or one song), and **Up next**, where you can jump to a song
  or take it out. The next song loads before the current one ends, so
  albums play on with hardly a gap. Lock-screen and headphone buttons work.
- **A song's menu** (the dots): play next, add to the queue, start radio
  from it, like it, add it to a playlist, go to its album or artist, send it
  to the party, or download it if your role allows.
- **Radio** plays songs that sound like the one you start from, and keeps
  going. It leaves out songs you heard in the last few hours and puts ones
  you skipped lately at the end. Without sound analysis, it plays the same
  artist and genre.
- **Your music:** your liked songs, albums and artists (the heart), your
  playlists, and your history. Share a playlist with other GO listeners if
  your role allows it.
- **Send to the party** requests the song at the party, like the party app
  does, when you're at home. Your own listening never changes what the
  party learns.
- **Even volume:** when ReplayGain is on for the party (Admin > Audio), GO
  applies the same levels on your device.

## On your phone

**Getting there:** in GO on a computer, **Account > Open on your phone**
shows a QR code for GO's address; scan it with the phone's camera.

**Install it as an app:** open GO in your phone's browser, then use
**Add to Home Screen** (Safari: the Share button; Chrome: the menu). It
opens full screen with its own icon, and keeps playing with the screen off.

**Or use a Subsonic app.** GO speaks the Subsonic API, so many music apps
work with it, with offline downloads (when your role allows downloads),
CarPlay and Android Auto:

- Android: Symfonium, DSub, Ultrasonic, Tempo
- iPhone: play:Sub, Amperfy, Substreamer
- Computers: Feishin, Sonixd

In GO, **Account > Phone apps > Make an app password**. In the app, add a
Subsonic (or OpenSubsonic) server with GO's address, your username, and
that app password. The app password only works in these apps, never to sign
in to DJINN, and it skips two-step sign-in, so keep it to your own devices;
make a new one (or remove it) any time and apps using the old one stop
working. Likes, playlists, history and "carry on" are shared between the
apps and GO's page. Apps get the original files, whatever quality they ask
for.

## Submitting music

Roles with **Submit music** see **Submit music** under Your music in GO. Pick
song files (MP3, FLAC, M4A with AAC, Ogg Vorbis or WAV: the formats DJINN plays) and they upload one by one.

- DJINN checks each one is audio it can play, reads its tags, and refuses
  a file that was sent before.
- A song that looks like one the library already has (same title, artist
  and length) is marked as a likely duplicate.
- With **Submitted music goes straight in**, songs join the library at once,
  except likely duplicates, which wait for you. Otherwise each one waits.
- **Reviewing:** Admin > GO > **Submitted music** lists what's waiting, with
  its tags and a player. **Add to library** or **Turn down** (with a note
  for the sender; the file is deleted). Senders see what happened to each
  song under Submit music, and can take back one still waiting.
- **Where they go:** approved songs move into one folder just for uploads
  (`Uploaded music` next to DJINN's database, or a folder you pick), sorted
  into Artist/Album folders. It becomes a library folder the first time.
  DJINN never writes to your other music folders.
- **Size:** files up to 200 MB by default; change it under Submitted music.

## Formats

Devices play the files as they are. MP3, AAC (M4A), FLAC and WAV play in
every current browser. Ogg files may not play on older iPhones, and ALAC
only on Apple devices. GO tells you when your device can't play a song.

## Converting

Off unless you turn it on, under Admin > GO > **Converting**. Then DJINN
makes MP3 copies:

- for a device that can't play a song's format (GO asks for one by itself);
- for people who turn on **Save data** under Account in GO, to use less
  mobile data;
- for phone apps that ask for a lower quality (or for MP3).

Downloads are always the original file. Pick the quality (192 kbit/s is
good), how many songs convert at once (1 keeps the party smooth on most
computers), and how much space the copies may use; the oldest go first.
Each song is converted once and kept, so only the first play waits a few
seconds. Roles choose who can use it with **Have songs converted**.

## Two-step sign-in

With two-step sign-in, signing in asks for a 6-digit code from an
authenticator app on your phone after the password, so a stolen password
isn't enough.

- **Turn it on for yourself:** in the party app, **Your account** (on the
  Playing tab or at the bottom of Admin), or in GO under **Account**. Scan
  the QR code with an authenticator app (Google Authenticator, Microsoft
  Authenticator, 1Password, Bitwarden and others), type the code, and save
  the recovery codes somewhere safe. Each recovery code works once if you
  lose your phone.
- **Require it:** per role with **Must use two-step sign-in for GO**, and
  for admins under Admin > GO > Safety. People who need it set it up the
  first time they open GO.
- **Lost phone and codes:** an admin can **Turn off two-step** for that
  account under Admin > Users.

Once it's on for an account, every sign-in asks for a code, on the party
address too.

## Safety

- **Lockouts:** five wrong passwords for an account lock it for 15
  minutes; twenty from one address lock that address.
- **Sign-in log:** Admin > GO lists every sign-in and failed try, with the
  address and device, for 90 days.
- **Devices:** everyone can see where they're signed in and sign devices
  out under **Your account**. Sessions end after 30 days unused.
- **Song links** are private and expire after a few hours.
- **Passwords** need at least 8 characters and can't be common ones.

## Using GO away from home

Your home router keeps the internet out. There are three ways to let
yourself and your listeners in, safest first.

### Tailscale (recommended)

[Tailscale](https://tailscale.com) is a free app that links your devices
into a private network, wherever they are. Nothing is opened to the
internet, and it works even when your internet provider blocks incoming
connections.

1. Install Tailscale on the DJINN computer and sign in.
2. Install it on your phone (and each listener's) and sign in to the same
   Tailscale network. To add other people, invite them from the Tailscale
   admin console, or share the DJINN computer with them.
3. Admin > GO shows the Tailscale address, like `http://100.101.102.103:8067`.
   Open it on the phone.

Tailscale addresses count as home, so roles without **Use GO away from
home** work over Tailscale too. Turn that permission on only for people
coming in some other way.

### Cloudflare Tunnel

A [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/)
gives GO a public `https://` address on your own domain, with no router
changes, and listeners don't install anything. You need a free Cloudflare
account and a domain on Cloudflare.

1. Install `cloudflared` on the DJINN computer and create a tunnel, with a
   public hostname (like `music.example.com`) pointing to
   `http://localhost:8067`.
2. In Admin > GO > Safety, set **Trusted proxies** to `127.0.0.1` and save.
   DJINN then sees each visitor's real address, so lockouts and the
   home-network check work. Without it, every visitor through the tunnel
   counts as away from home.
3. Turn on **Use GO away from home** for the roles that should get in, and
   consider requiring two-step sign-in for them.

### Port forwarding

You can forward port 8067 on your router to the DJINN computer, but GO is
then plain HTTP on the open internet, and passwords travel unencrypted. Use
Tailscale or Cloudflare Tunnel instead, or put a reverse proxy with HTTPS
(like Caddy) in front of GO and add its address to **Trusted proxies**. A
button that sets this up safely is planned.

## Settings file

GO listens on `0.0.0.0:8067`. To change it, add this to `config.toml`:

```toml
[go]
listen = "0.0.0.0:8067"
```
