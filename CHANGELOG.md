# Changelog

## Unreleased

### Search

- A song, album or artist whose name begins with exactly what you typed now comes first. Short names like **S&M** could be buried by thousands of other matches and never show up.

### DJINN GO for Android

- **Browse by genre**, and jump through artists, albums and songs with an **A to Z (or year) strip** beside the list.
- On Now Playing, **tap the artist or album** to open its page.
- **Search uses the server's AI** when it's on and allowed for your account, for searches like "music for cooking dinner".
- The first tab is called **Home** again.
- The import box says whether YouTube links are on for your server.

### Playlists

- **One set of playlists.** Your playlists are now the same in the jukebox app, on GO and in the Android app. They're private until you share them with everyone or with particular people. Anyone signed in can request from their playlists, and a new role permission lets chosen people start one as the auto mix. Guests don't have playlists. Existing party playlists are shared with everyone, so admins keep them. A song listed twice in a GO playlist now keeps only its first place.
- **Import playlists** from Spotify, YouTube Music, Pandora, Apple Music and other services, using a file they export or a list of songs you paste. YouTube and YouTube Music playlist links work too, once an admin adds a YouTube API key: DJINN sends only the key and which playlist. Only the list comes in, never the music. See [the guide](docs/guide/playlists-and-radio.md#importing-playlists).
- Playlists now **flag songs that can't play** and skip them: imported songs you don't have, files that are missing, and files that are damaged. **Check files** on a playlist looks for damage ahead of time. Songs fill back in by themselves once a library check finds them.

## 2026.11.02 (November 2, 2026)

### Radio and the queue

- Radio now favors the artist it started from, both at the party and on GO, while still mixing in other artists that sound alike. GO's radio also stays around that artist as it runs on, instead of drifting away from it.
- On GO, drag songs in Up next to change the order. Tapping a song or using its menu can now **play it next** or **add it to the end of the queue**.

### DJINN GO for Android

- Added the official Android app (Android 7 and newer): streaming, downloads and smart cache for offline listening, songs saved as alarm, ringtone and notification sounds, visuals, a widget, voice requests, Android Auto and Cast. See [the guide](docs/guide/djinn-go.md#on-your-phone).
- FLAC and WAV songs kept on a phone can be converted to MP3, 128 kbit/s VBR by default, when converting is on. Compressed songs stay as they are.

### Faster library

- The Library tab opens right away and stays quick however far you scroll, on the party app and on GO. Only the rows near the screen are on the page, and lists are worked out once until the library changes.
- The first start after updating moves sound analysis into its own table and compacts the database, once. This can take a minute or more on a big library and needs free disk space about the size of the database. Nothing needs analyzing again.

### Encryption and finding servers

- Connections are now encrypted, on by default with nothing to set up: DJINN makes its own certificate. The Android app uses it, remembers it, and warns if it ever changes. Admin > GO shows a security code to compare, and scanning the QR code checks it for you. Browsers can use `https://` but warn about the certificate, so plain addresses keep working for them, cast devices and Subsonic apps. Turn it off in Admin > GO > Safety.
- Servers can be named in Admin > GO and announce themselves on the network (this can be turned off). The app lists what it finds, so nobody has to type an address.

### Keeping songs across servers

- The app remembers several servers, each with its own sign-in. **Settings > Other servers** moves between them without signing out.
- Kept songs belong to one account on one server, and moving to another server or someone else signing in leaves them alone. **Library > On phone > All servers** plays them whichever server you're on.
- Admin > GO > **Songs kept on devices** chooses what happens at sign-out (delete, or keep locked) and after too long away from the server (lock, delete or nothing, 30 days by default). Too long away now locks songs rather than deleting them.

### Library

- Artists and albums can be sorted by **Recently added**, as songs could.
- Scrolling back up brings the tabs and sort options back, with a button to the top. The search box no longer disappears behind the top menu.
- The Android app can sort and filter its lists.

### Android app

- Your server now carries the app. Open GO on an Android phone, or scan the QR code in Admin > GO, to install it with no store. It offers its own updates when your server has a newer version. **If you installed a test build, uninstall it first**, because this one is signed differently.
- **Home** is now **Playlist**, and a **Playing** button pulls up the player.
- Now Playing shows the cover by default, and a button switches to visuals on devices that can run them. The cover and visuals go full screen, with controls that appear on a tap and fade.
- Visuals no longer shake: Tunnel, Stars and Nebula were following every tiny change in the bass. Settings can now choose colors, RGB rotation, the number and spacing of bars, line thickness and smoothing.
- Android Auto only shows apps from outside the Play Store once **Unknown sources** is on. The guide has the steps.

## 2026.11.01 (November 1, 2026)

### DJINN GO away from home

- Added **Allow connections from outside the home network** in Admin > GO > Safety. It's off by default and works together with the role's **Use GO away from home** permission. **After updating, anyone using GO from outside your home network needs this switched on.** Tailscale addresses still count as home.
- Added a guide to public HTTPS with a Caddy reverse proxy, and direct HTTP forwarding for those who choose it. The Admin page shows the address to forward to.
- Added an optional UPnP switch that forwards HTTPS (port 443) on the router after checking the public address, DNS, and certificate. The one-hour lease renews while the switch is on. GO's HTTP port is never forwarded this way.

### System

- Added **Server ports** in Admin > System to change the party and GO ports. Saved ports apply after a restart and override `config.toml`.
- Logs now go to one dated file per day, kept for seven days by default (1 to 365).
- Added a **Record in the log** choice that takes effect immediately, and downloads of earlier days under **Daily logs**.

### Fixes

- Playback has its own database connections, so heavy library browsing can't hold up the music.
- Cancelled library requests no longer affect the next request.
- Songs with no album stay in the artist list.

## 2026.10.31 (October 31, 2026)

### Interface

- Moved the jukebox navigation to the top and added a large Back button beside Home.
- Added a Back button to DJINN GO and made Now Playing a full page.
- Added artwork for empty playback and listening screens, and changed artist photos into a faded page background.
- Added a switch between the regular Playing page and the TV display.
- Gave Playing the TV layout's large artwork, song details, Up Next list, and join code while keeping its playback controls.
- Extended the subtle Playing artwork and artist photo backgrounds across their full pages.
- Moved TV display controls clear of the Up Next heading.
- Hid Tune In when the jukebox browser is on the server computer.
- Added a 12-band equalizer in Admin > Audio, with vertical sliders, editable presets, an output level control, and a flat reset. Soft peak limiting keeps boosts from lowering the whole mix.
- Placed the Albums Show and Sort by filters side by side when space allows.
- Kept artist song lists above the photo fade and added a subtle full-page album art background to album pages.
- Added username changes in Your account and Admin > Users, and a second password field when admins add accounts.

### Releases

- Adopted date-based release numbers (`YYYY.MM.DD`). Cargo keeps the same date without zero padding; published tags and the About page show the padded date.

### Artist images

- Added artist picture source choices, browse links for licensed libraries, and admin import and management controls.
- Added admin controls for per-artist picture count and storage, total image cache size, image dimensions, and minimum resolution.
- Added image resizing on download and cache usage reporting.

### Library

- Added an admin switch for showing secondary credited artists in the Artists library. Search, song links, and direct artist pages still include them.
- Grouped untagged albums with three or more distinct lead artists from the same folder as Various Artists; explicit soundtrack labels and genres take precedence.
- Added Various Artists and Soundtracks filters to the Albums library.
- Added a restore script for backups made before testing a local Linux build.

## 0.4.5 (October 2026)

See [ROADMAP.md](ROADMAP.md) for the released feature history.
