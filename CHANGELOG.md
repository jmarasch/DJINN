# Changelog

## Unreleased

### Faster library

- The Library tab opens right away and stays quick however far you scroll, on the party app and on GO. Only the rows near the screen are on the page, and lists are worked out once until the library changes.
- The first start after updating moves sound analysis into its own table and compacts the database, once. This can take a minute or more on a big library and needs free disk space about the size of the database. Nothing needs analyzing again.

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
