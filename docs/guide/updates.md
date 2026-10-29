# Updates

DJINN updates itself. Admin > **About** shows the version you have and checks
for new ones.

- **Check for updates once a day:** on by default. Checking asks GitHub for
  the latest release and sends nothing but DJINN's version number.
- When a new version is out, About says so, with **What's new**. Tap
  **Install and restart**; DJINN downloads it, checks it, and restarts.
- **Install updates by themselves, only while nothing is playing:** lets
  DJINN update on its own, never in the middle of a song.
- **Check now** checks right away.

Updates are only installed if they're signed with DJINN's release key, so a
damaged or tampered download is refused.

If About says it can't install the update itself (for example when running
from a folder it can't write to), it links to the release so you can download
it by hand. On the [Releases page](https://github.com/jmarasch/DJINN/releases),
each release starts with a "Which file do I download?" guide; on Windows it's
`djinn-setup-windows-x86_64.exe`.

About also has links to the source code, to report a problem, and to every
release.
