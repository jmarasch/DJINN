# Testing DJINN on Windows

Context for an AI agent session (Claude Code or Codex) on the Windows test
machine. Read this and `CLAUDE.md` before doing anything else.

## Your job on this machine

**Test, troubleshoot, and report. Never write or change code here.**
All development happens on the Linux machine; fixes come back through
`git pull`.

Allowed:

- Install tools, clone and `git pull`, build, run tests, build the
  installer, install and uninstall DJINN, run the engine and cast examples.
- Read any file, the logs, the registry, firewall rules, device lists.
- Run PowerShell commands to diagnose a problem.
- File GitHub issues (see Reporting).

Not allowed:

- Editing, creating, or deleting files in the repository (source, config
  examples, docs, lockfiles, this file). `git status` must stay clean.
- `git commit`, `git push`, branches, stashes, pull requests.
- Commands that rewrite files: `cargo fmt` (use `cargo fmt --all --check`),
  `npm install` (use `npm ci`), `cargo update`, `cargo fix`,
  `cargo clippy --fix`.
- "Quick fixes" or workarounds that hide a bug. If something is broken,
  report it; a fix can be suggested in the issue but is never applied here.

If a test needs a config file, put it outside the repository (for
example `%USERPROFILE%\djinn-test\config.toml`) and pass `--config`.

## Standing rules (same as on Linux)

- Music folders are read-only. DJINN never writes to them, and neither
  do you.
- Don't cast to or interrupt any speaker, TV, or Sonos without the user
  saying so for that device. Discovery (`--example discover`) is fine.
- The GitHub repo `jmarasch/DJINN` stays private. Never change its
  visibility.
- Never send the user's email address to any service.
- AcoustID needs the user's own key; don't register one or work around it.
- The release signing key is not on this machine and must never be.

## What DJINN is and where it stands

A party jukebox: one `djinn.exe` that plays music on this computer's own
speakers while guests pick songs from their phones in a browser. It sits
in the system tray with no window. Version 0.1.0; Phases 1 to 6 are done
(playback, library, outputs, casting, AI search and listening, radio,
smart playlists). This is the first time it runs on Windows. CI builds
and tests it on Windows, but nobody has clicked through it there yet.

Windows-only code that has never run on a real machine (test these
hardest):

| Area | Where | What to check |
| --- | --- | --- |
| Hidden console | `djinn-server/src/main.rs` (`windows_subsystem`) | Release build shows no console window, logs still written |
| Tray | `djinn-platform/src/tray.rs`, `djinn-server/src/desktop.rs` | Icon appears; Open, join code, TV page, Quit all work |
| Audio out | `djinn-engine` (cpal WASAPI) | All devices listed, playback clean, gapless, multiple outputs in sync |
| Memory and cores | `djinn-platform/src/system.rs` (`GlobalMemoryStatusEx`) | Admin > AI shows the right RAM |
| LAN devices | `djinn-platform/src/neighbors.rs` | Device names/MACs in admin lists |
| Self-update | `djinn-server/src/update.rs` | Rename to `djinn.old.exe`, restart (can't fully test until a release exists) |
| GPU pack | `djinn-server/src/gpu.rs` | NVIDIA detection via `System32\nvcuda.dll`, helper with no window |
| Installer | `packaging/windows/djinn.iss` | Everything in section D below |
| Exe icon and info | `djinn-server/build.rs` (winresource) | Icon in Explorer, taskbar, Task Manager name "DJINN" |

## Where things are on Windows

DJINN keeps everything next to `djinn.exe`:

- Installed: `%LOCALAPPDATA%\Programs\DJINN\` holds `djinn.exe`,
  `config.toml` (if any), `djinn.db`, `djinn.log`, `art\`, `models\`
  (AI models and the GPU pack).
- Built from source: the same files next to `target\release\djinn.exe`.
  Copies running from `target\` don't update themselves.
- Web UI: `http://localhost:8066` (also on the LAN IP). The first start
  opens the browser at `/setup?token=...` to create the admin.
- Start at sign-in: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
  value `DJINN`.
- Command-line options: `djinn.exe --help` (`--config`, `--no-tray`,
  `--log-file`). More log detail: set `RUST_LOG=debug` before starting.

## Setup

1. Tools, from PowerShell:
   ```
   winget install Rustlang.Rustup OpenJS.NodeJS.LTS Git.Git GitHub.cli JRSoftware.InnoSetup
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   rustup default stable-msvc
   gh auth login
   ```
   The Build Tools need that `--override`, or they install without the C++
   compiler. If a Build Tools install was interrupted, winget may call it
   installed while nothing works; run the same command with `--force`.
   Open a new PowerShell window afterwards so the new tools are on PATH.
2. `gh repo clone jmarasch/DJINN` and `cd DJINN`. Note the commit you
   test: `git log --oneline -1`.
3. Build, from PowerShell:
   ```
   cd web; npm ci; npm run build; cd ..
   cargo build --release
   $iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
   & $iscc packaging\windows\djinn.iss
   ```
   winget installs Inno Setup for the current user (the first path); an
   installer from the Inno Setup site goes to Program Files (the second).
   The setup program lands in `target\installer\djinn-setup-windows-x86_64.exe`.
4. Also run the checks CI runs and report any failure with full output:
   `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo test --workspace`, and `cd web; npm run check`.

Report every build problem, even ones you get past (a missing tool the
docs don't mention is a docs bug).

## Test plan

Work through it in order and keep notes as you go. Tick off what passed
in your final summary; file an issue for each thing that failed.

**A. Fresh install**
- Run the setup program. A downloaded one shows SmartScreen "Windows
  protected your PC" (unsigned): More info, Run anyway. A locally built
  one doesn't (no download mark), so SmartScreen waits for a release.
- License page, task options (desktop shortcut off, start at sign-in on),
  install with no admin prompt.
- "Start DJINN now": tray icon appears, browser opens the setup page,
  Windows Firewall asks about network access (allow private networks).
- Start menu entry exists, with the icon. Settings > Apps lists DJINN.

**B. First run and library**
- Create the admin, add a music folder, let the library check finish.
- Artist photos and covers (if online lookups are on), search, genre pages.

**C. Playback and outputs** (keep the volume reasonable; ask the user
before using speakers in a shared room)
- Admin > Audio lists the Windows output devices. Play, pause, skip,
  volume, seek.
- Gapless album playback, crossfade on and off, ReplayGain.
- Two outputs at once (for example speakers plus Bluetooth or HDMI):
  in sync, delay setting works, unplugging one doesn't stop the other.
- Optional engine checks: `cargo run --release -p djinn-engine --example devices`,
  `--example outputs`, `--example play -- <file>`.

**D. Guests**
- A phone on the same Wi-Fi opens the join URL or QR code. Requests,
  skip votes, queue updates live.
- If the phone can't connect: check the firewall rule and whether the
  network is set to Public (see Troubleshooting) before reporting. Windows
  blocks incoming connections on Public networks; switching the Wi-Fi to
  Private (Settings > Network & internet > Wi-Fi > the network) is the
  user's call. Note how DJINN behaves either way: a guest who can't reach
  it is the most likely real-world Windows problem.

**E. Features**
- Playlists, radio from a song, artist and genre, "More like this".
- Admin > AI: this computer's RAM and cores look right; download the
  listening model, listening runs in the background and can be stopped;
  download the reader, try a free-form search ("chill songs for a
  dinner"). Note load and response times.
- On an NVIDIA machine: the GPU section should appear. Downloading the
  pack is expected to fail with "GPU packs come with published releases"
  (no release exists yet). That's not a bug.
- Casting: discovery only, unless the user picks a device to test.
- Admin > About: version 0.1.0 and the commit you built. "Check for
  updates" is expected to fail or find nothing, since the repo is private
  and has no releases. Report the exact message so it can be made
  friendlier.

**F. Tray and restarts**
- Tray menu: Open, join code, TV page, Quit. No console windows flash,
  including when the AI helper starts.
- Sign out and back in (or reboot): DJINN starts by itself.
- Quit and start from the Start menu: settings and library survive.

**G. Upgrade**
- With DJINN running, run the same setup program again. It should close
  DJINN, install, and keep all data. Note any "files in use" prompts.

**H. Uninstall**
- Settings > Apps > DJINN > Uninstall. It should stop DJINN, then ask
  whether to remove settings and data (default No).
- Answer No: the folder keeps `djinn.db` and friends; reinstalling picks
  them up again.
- Uninstall again and answer Yes: `%LOCALAPPDATA%\Programs\DJINN` is
  gone. The Start menu entry, the Run key, and the desktop shortcut are
  gone either way. The music folder is untouched.

## Troubleshooting (PowerShell)

```
$dj = "$env:LOCALAPPDATA\Programs\DJINN"
Get-Content "$dj\djinn.log" -Tail 100              # add -Wait to follow
Get-Process djinn, djinn-gpu -ErrorAction SilentlyContinue
Get-NetTCPConnection -LocalPort 8066 -ErrorAction SilentlyContinue
Get-NetFirewallApplicationFilter | Where-Object Program -like '*djinn*' | Get-NetFirewallRule | Select-Object DisplayName, Enabled, Profile, Action
Get-NetConnectionProfile                           # Public networks block guests by default
Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run
Get-ComputerInfo -Property OsName, OsVersion, CsTotalPhysicalMemory
nvidia-smi                                         # NVIDIA machines only
```

To watch logs live in a console instead of the file, run a debug build
from the checkout: `cargo run -p djinn-server -- --config $env:USERPROFILE\djinn-test\config.toml`
(the debug build has a console, and reads `web\dist` from disk).

## Reporting

File one GitHub issue per problem on `jmarasch/DJINN` with the `windows`
label (plus `bug` or `documentation`). Ask the user before filing the
first one in a session. Use the template below for the body.

```
gh issue create -R jmarasch/DJINN --label windows --label bug --title "<short summary>" --body-file <file>
```

Write the body file outside the repository (for example
`%TEMP%\djinn-issue.md`). Before filing, search for an existing issue
(`gh issue list -R jmarasch/DJINN --label windows`) and add a comment
there instead if it's the same problem. If `gh` isn't working, append the
reports to `%USERPROFILE%\Documents\djinn-windows-report.md` and tell the
user to bring that file to the Linux machine.

Issue body template:

```
**Commit:** <git log --oneline -1>   **Build:** source / installer
**Windows:** <edition, version, build>   **Hardware:** <CPU, RAM, GPU, audio devices involved>

**Steps**
1.
2.

**Expected**

**Actual**

**Log** (relevant lines from djinn.log or console, trimmed)

**Notes / suspected area**
<file or module from CLAUDE.md's code map, what you ruled out, a suggested
fix in words or a short snippet; never applied on this machine>
```

Scrub logs and screenshots before posting: no passwords, tokens, setup
URLs (`?token=`), or the user's email.

At the end of a session, give the user a summary: the commit tested,
what passed, the issues filed (numbers and titles), and anything you
couldn't test and why.
