# Updates

DJINN updates itself. **Admin > System > Updates** checks for new versions;
the top of **Admin > System** shows the version you have.

- **Check for updates once a day:** on by default. Checking asks GitHub for
  the latest release and sends nothing but DJINN's version number.
- When a new version is out, Updates says so, with **What's new**. Tap
  **Install and Restart**; DJINN downloads it, checks it, and restarts. The
  music stops for a few seconds.
- **Install updates by themselves, only while nothing is playing:** lets
  DJINN update on its own, never in the middle of a song. Off by default.
- **Check Now** checks right away.
- **Admin > Overview** says when an update is ready to install.

Updates are only installed if they're signed with DJINN's release key, so a
damaged or tampered download is refused.

## Stable or nightly

**Update channel:**

- **Stable** (the default): releases come out less often (usually weekly)
  and have had more testing.
- **Nightly:** the newest changes, built each night there are some. They're
  tested less, so something may break.

Switching from nightly back to stable doesn't go backwards: a nightly build
stays as it is until a newer stable release comes out, since an older DJINN
can't always open a newer one's database.

## Nodes

[Nodes](nodes.md) update from their main DJINN, never from the internet:
update the main DJINN and its nodes follow within minutes.

## Installing by hand

If Updates says it can't install the update itself (for example when running
from a folder it can't write to), it links to the release so you can download
it by hand. On the [Releases page](https://github.com/jmarasch/DJINN/releases),
each release starts with a "Which file do I download?" guide; on Windows it's
`djinn-setup-windows-x86_64.exe`.

The credits at the bottom of **Admin > System** link to the source code, to
report a problem, and to every release.
