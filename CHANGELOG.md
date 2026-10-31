# Changelog

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
