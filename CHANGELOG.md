# Changelog

## Unreleased

### DJINN GO

- **Your year.** See how long you listened, your top songs and artists, your busiest month and your first song of the year, under Library > Your year in the browser, or from Home in the Android app. Pick earlier years too.
- The Android app has **Not in the library?** under search results, like the browser, when the admin allows it.

### Admin

- **A new Admin page.** Sections are grouped in a side menu (Party, People, Library, System), which sits at the top on phones. **Find a setting** searches every setting by name or a word like "skip" or "backup", opens its section and highlights it. A dot marks sections with settings changed from the default, and a list shows them all.
- **Admin Home** shows how DJINN is doing at a glance: what's playing and on which speakers, whether extra speakers and casts are in time, library problems, the nightly check, the computer's processor, memory and disk, when you last saved a backup, and whether an update is waiting. Each line links to where to fix it.
- Library problems, possible duplicates and the nightly check have their own **Problems** section, with a count in the menu.
- **Stats.** Admin > Stats shows the songs played and asked for most, top artists, who asked most, songs skipped most, and the busiest hours and days, for the last party, today, 7 or 30 days, this year or all time, for everyone or one person, at the party, on DJINN GO or both. Download the lists as a spreadsheet, or clear just that stretch of history (it says how much first).
- **Party recap.** "Last party" covers everything since the first song after a break of six hours or more.
- **Search history.** See what people searched for, and especially what found nothing: a shopping list for the library. Typing one search counts once. Turn it off in Stats.
- **Tonight on the TV page.** Songs played and requests so far, the most requested song, and who asked for the most songs. Turn it off in Admin > Rules.
- **Wishlist.** "Not in the library?" under search results lets people ask for a song. Admins see every ask with how many people want it, and a library check that adds the song marks it Got. Only admins see the button until you let everyone use it.

### Library

- **Library problems.** Admin > Library has a **Problems** list: files a library check couldn't add (formats DJINN doesn't play, folders it isn't allowed to open, names it can't read, music folders that aren't there), songs that won't play or whose tags couldn't be read, and gaps such as songs with no artist or albums with no cover. Each kind says what to do about it. Hide kinds you already know about, or download the whole list as a spreadsheet. The first library check after updating reads every file once.
- **Possible duplicates.** Admin > Library lists songs you have more than one copy of: the same title and artist at about the same length (notes like "Remastered" are ignored, "Live" is not), songs identified as the same recording, and, with sound analysis on, copies that sound the same even when named differently. The best copy (lossless, then the higher bit rate) is marked to keep. DJINN never deletes files; mark a group **Not duplicates** if it's wrong. With sound analysis on, each song is analyzed once more after updating to take its fingerprint.
- **The auto mix won't play another copy of a song that just played.** A copy counts as played, the same as the song itself. Turn this off in Admin > Rules.
- **Report a problem with a song.** Everyone, guests included, can flag a song from its menu (in the party app, DJINN GO and the Android app): wrong title or artist, wrong cover, sounds bad, plays a different song, or something else, with an optional note. Reports show under Problems until you mark them **Done**.
- **Nightly check.** Admin > Library can check the library every night at an hour you pick (or when DJINN starts, if it was off then) and sums up what changed: songs added and gone, problems, songs with copies, and listener reports. Useful for network folders that don't say when they change.
- A file that crashed the tag reader used to take the files read alongside it out of the library. Now only that file is affected, and it's listed under its name.

### Updates

- **Choose Stable or Nightly** in Admin > System > Updates. Stable gets a tested release about once a week. Nightly gets the newest work every morning there's something new, and may have rough edges.

### DJINN GO for Android

- **Favorites:** Artists, Albums and Songs in the Library have a **Favorites** filter.
- **Hearts on album and artist pages.**
- **The app asks when your server has a new version,** once per version. You can turn this off in Settings.

## 2026.11.05 (November 5, 2026)

### Fixes

- **The library no longer freezes while scrolling in grid view.** An album cover that wasn't square made its tile taller, and the page could lock up trying to lay out rows of different heights. This was easiest to hit scrolling albums in DJINN GO on a phone. Covers in grids are now always square, and the lists can't get stuck like that again even if rows differ.

## 2026.11.04 (November 4, 2026)

### Security

This release follows a security review. A few things work differently:

- **Away from home, signing in to DJINN GO over plain HTTP is refused**, since passwords would cross the internet readable. The app and `https://` addresses work as before. If you need it anyway, for a Subsonic app that can't accept DJINN's certificate, Admin > GO has **Also allow unencrypted connections from outside**.
- **App passwords are only shown when they're made**, and making one asks for your password. Changing your password, or an admin turning the account off, removes it, so apps need a new one after that.
- **Setting up two-step sign-in, or moving it to a new phone, asks for your password.**
- **The TV page in a browser is silent unless you turn on** Admin > Rules > **Sound on the TV page**, because anyone who can open the page can listen. Casting to a TV still has sound. The casting section of Admin can give the live stream a **new address**.
- Wrong passwords from someone else no longer lock you out of your account: tries count per account and address, with higher limits per address and per account.
- A phone's sign-in only works on GO's port, and song links end when their session does.
- Bursts of sign-ins can't use up the computer's memory, and each port limits how many connections it keeps.
- On Linux, the database and logs can only be read by your own account. A backup left half downloaded no longer stays on disk, and backups only bring art cache files with them.
- The party port now sends the same security headers GO does, and pages from elsewhere can't press its buttons.
- Over DJINN's own HTTPS the sign-in cookie is marked secure.
- DJINN GO for Android: other apps on the phone get play and pause buttons, not the library.
- The Linux installer checks the release's signature when OpenSSL 3 is available.

### Library

- **Albums show their original release year**, not a reissue's, when the tags have one. **Use the original release year** in Admin > Library turns this off. The next library check re-reads every file once.
- **Correcting a year on an album's songs now changes the album's year too.**
- **Albums with the same name stay apart** when their folders hold different releases, such as two different "track 1"s, and each shows its folder's name. Discs of one album stay together, and so does one stray song. **Keep albums with the same name apart** in Admin > Library turns this off.
- **The genre cloud zooms in.** It opens on the 100 biggest genres, with more of the lines between them. Tap one to center on it with what's near it and what's near those, which reaches every genre; tap the center to open its page. Home, fit and zoom buttons sit in the corners, and you can choose how many genres and lines to show.
- **Genre pages show the whole genre.** Artists are a sideways row with pictures, and songs keep loading as you scroll, instead of stopping at 24 artists and 100 songs. Everything comes in a shuffled order that favors what's been played, so a list doesn't stop after a few albums. **Admin > Library** sets how many load at a time and how many hours before a genre is shuffled again.
- **Plays on GO and phone apps now count toward what's popular**, along with the jukebox. A song skipped early doesn't count.
- **Sideways rows show there's more:** the cut-off side fades, and with a mouse there are arrows.

### DJINN GO

- **Genres in GO:** GO on the web gets a Genres page, and GO on both the web and Android has the zooming cloud as well as the list.
- **Clear the queue at once.** Up next has **Clear upcoming**, which lets the song finish, and **Clear all**, which stops it too. Both can be undone.
- **When a sign-in ends, the music stops**, including one ended from elsewhere, such as an admin removing a device. That's noticed within about 15 seconds. On Android, casts stop too.
- **Android: choose what the music does when another app needs to talk**, such as an assistant screening a call or directions: **Lower the music**, **Keep playing** or **Pause**, in Settings. A call you answer always pauses.

## 2026.11.03 (November 3, 2026)

### Search

- A song, album or artist whose name begins with exactly what you typed now comes first. Short names like **S&M** could be buried by thousands of other matches and never show up.

### Library

- Years that can't be real, like the 9999 some files carry as a placeholder, now count as unknown, and a song's year can be next year at the latest. One bad year could stretch the year lists by thousands of years. Your files aren't changed.

### DJINN GO for Android

- **Browse by genre**, and jump through artists, albums, songs and genres with an **A to Z (or year) strip** beside the list.
- On Now Playing, **tap the artist or album** to open its page.
- **Search uses the server's AI** when it's on and allowed for your account, for searches like "music for cooking dinner".
- The first tab is called **Home** again.
- The import box says whether YouTube links are on for your server.

### Fixes

- On Windows, audio should come back after an HDMI output switches off and on, such as a TV and receiver sharing a remote, without restarting DJINN.
- The log no longer fills with harmless notes from reading song files, which could be nearly all of a day's lines. A song that can't play still shows up as a failed play.

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
