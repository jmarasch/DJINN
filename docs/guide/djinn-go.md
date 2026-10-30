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

In 0.4.0, GO plays a song, then the next one in the list. Your own queue,
likes, playlists, history and radio are on the way.

## Formats

Devices play the files as they are. MP3, AAC (M4A), FLAC and WAV play in
every current browser. Ogg files may not play on older iPhones, and ALAC
only on Apple devices. GO tells you when your device can't play a song.

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
