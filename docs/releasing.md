# Releasing DJINN

DJINN checks GitHub (`jmarasch/DJINN`) for the latest release and can
install it from Admin > About. It only installs a download listed in the
release's `SHA256SUMS`, and only if `SHA256SUMS.sig` is signed with
DJINN's release key. The public half of that key is `PUBLIC_KEY` in
`crates/djinn-server/src/update.rs`.

Before a first release, the About tab says "No releases published yet".

## Stable and nightly

There are two branches and two kinds of release:

- **`dev`** is where work lands. Feature branches open their pull requests
  against `dev`.
- **`main`** is stable. It only moves when `dev` is ready to become a
  stable release: open a pull request from `dev` to `main`, merge it, and
  tag it (below).
- **Nightly builds.** Each night at 08:13 UTC (about 3 AM Eastern in winter,
  4 AM in summer) the Release workflow looks at `dev`. If it moved since
  the last nightly or stable release, it builds everything a stable release
  has, numbered by the date (`2027.1.5-nightly` in Cargo, tag
  `v2027.01.05-nightly`), signs it with the same key, and publishes it as a
  GitHub **pre-release**. If `dev` hasn't moved, it stops after a few
  seconds and builds nothing. The newest 7 nightlies are kept and older
  ones are deleted. Nothing is committed: the version is set in the
  workflow's checkout by `.github/scripts/set-version.sh`.

Admin > System > Updates has an **Update channel**. On Stable (the
default), DJINN asks GitHub for its "latest" release, which never includes
a pre-release, so stable installs never see nightlies. On Nightly it takes the
newest of the recent stable releases and nightlies. A nightly sorts after
the day before's release and before the stable release of its own date. A
copy that is itself a nightly starts on the Nightly channel. Switching a
nightly copy back to Stable keeps it as it is until a newer stable release
comes out.

Android follows the same order. `versionCode` is the date times 100, plus 50
for a stable release (2026.11.05 is 2026110550, 2026.11.05-nightly is
2026110500); `go_app.rs` and `android/app/build.gradle.kts` must agree.
Phones get the app their server carries, so a nightly server offers its
phones the nightly app. A phone can't go back to an older number without
uninstalling, so it stays on that app until its server carries a newer one.

The schedule always uses `main`'s copy of `release.yml`, so changes to the
workflow only take effect once they're on `main`. To publish a nightly
straight away, even if `dev` hasn't moved, run the Release workflow from
the Actions tab with **Publish a nightly** ticked. Run it from the same day
again and it replaces that day's nightly.

## The release key

The secret key is `~/.config/djinn-release/signing.key` on the
maintainer's computer (64 hex digits). It is not in the repository.

- Back it up somewhere safe (a password manager). If it's lost, existing
  installs can't verify new releases and must be updated by hand once to
  a build with a new public key.
- Add it to the GitHub repository as the Actions secret
  `DJINN_SIGNING_KEY` (Settings > Secrets and variables > Actions).

A new key pair, if ever needed:
`cargo run -p djinn-platform --example sign -- keygen <file>`, then put the
printed public key in `update.rs`.

## Making a release

Release numbers show the publication date as `YYYY.MM.DD`. Cargo requires
unpadded numeric parts, so a release shown as `2027.01.05` uses `2027.1.5`
in `Cargo.toml`. The Git tag and About page use the padded form. The updater
compares both forms numerically, including older `0.4.5` releases. Make at
most one public release per date; batch any same-day fixes before tagging.

1. On release day, set `version` in the root `Cargo.toml` to that date without
   padding (for example `2027.1.5`), run `cargo build` so `Cargo.lock` follows,
   and commit. Keep the changelog heading on the same date.
2. Get the tag with `bash .github/scripts/release-tag.sh`, then tag and push
   that value (for example `v2027.01.05`).
3. The Release workflow (`.github/workflows/release.yml`) builds Linux and Windows binaries
   (`djinn-linux-x86_64`, `djinn-windows-x86_64.exe`) and the downloads
   for first installs (below), writes and signs `SHA256SUMS`, and
   publishes the release with notes: the "Which file do I download?"
   guide from `packaging/release-notes.md`, then the changes GitHub
   generates. Edit the notes on GitHub if you like; Admin > About shows
   what comes after the guide's closing comment.

## Installers

Both install for the current user only, with no admin or root rights,
so DJINN can update itself in place afterwards. Settings, the database
and AI models are kept on uninstall (Windows asks if you want them gone).

- **Windows:** `djinn-setup-windows-x86_64.exe`, an Inno Setup program
  (`packaging/windows/djinn.iss`). It installs to
  `%LOCALAPPDATA%\Programs\DJINN`, adds a Start menu entry, optionally a
  desktop shortcut, starts DJINN at sign-in unless unticked, and shows up
  in Settings > Apps for uninstalling. Build it locally with
  `iscc packaging\windows\djinn.iss` after a release build (Inno Setup 6:
  `winget install JRSoftware.InnoSetup`, which puts `ISCC.exe` in
  `%LOCALAPPDATA%\Programs\Inno Setup 6\`); it lands in `target\installer\`.
  The setup program isn't code signed, so SmartScreen says "Windows
  protected your PC" the first time: More info, then Run anyway. On first
  start Windows also asks whether DJINN may use the network; allow
  private networks so phones can reach it.
- **Linux:** `install.sh` is a release asset, so
  `curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh | sh`
  downloads the latest `djinn-linux-x86_64.tar.gz`, checks it against
  `SHA256SUMS`, and runs the `install.sh` inside it. The tarball holds the
  binary, icon, menu entry and `install.sh`; unpacked, run
  `./djinn/install.sh` (add `--autostart` to start at login, `--uninstall`
  to remove). It's built in an Ubuntu 22.04 container, so it needs glibc
  2.35 or newer.

To try the whole workflow without publishing, run it from the Actions tab
(Run workflow): it builds every download and the GPU pack, and skips
signing and the release.

## The GPU pack

The same workflow builds the optional GPU pack: `djinn-gpu` (the AI
reader built with CUDA, `cargo build -p djinn-ai --features cuda --bin
djinn-gpu`) and NVIDIA's CUDA runtime libraries (cudart, cuBLAS,
cuBLASLt, cuRAND, which NVIDIA allows apps to ship), each compressed as
`gpu-<os>-<arch>.<file>.zst` and listed in the signed `SHA256SUMS`.
DJINN downloads the pack for its own version from Admin > AI when it sees
an NVIDIA driver, runs the reader through the helper, and falls back to
the processor if the helper fails. It's built for Ampere (RTX 30xx) and
newer; older cards, Turing (GTX 16xx, RTX 20xx) included, use the
processor. The GPU job builds on ubuntu-22.04 and windows-2022 because
CUDA 12.4 doesn't support the newer runners' Ubuntu or Visual Studio.

Building the pack is the slow part of a release (installing CUDA, then
nvcc compiling candle's kernels), so the job reuses the last release's
pack when nothing it's built from changed. `.github/scripts/gpu-inputs.py`
lists what it's built from, for the release being made and for the last
one: the files in `crates/djinn-ai`, every package and feature Cargo uses
for `-p djinn-ai --features cuda` (test-only dependencies, and other
crates' dependencies and features, don't count), the root `Cargo.toml`'s
edition and build profiles, and the GPU job's steps (not its comments or
the reuse step). DJINN's version number isn't in the list, so a version
bump alone, or a dependency only the rest of DJINN uses, doesn't rebuild
it. The job log says what changed when it does build. To see the list:
`python3 .github/scripts/gpu-inputs.py .`

## How an install works

The app downloads the binary for its system next to itself, checks size
and SHA-256, and swaps it in (Linux: replaces the file; Windows: renames
the running `djinn.exe` to `djinn.old.exe`, deleted at the next start).
Then it shuts down cleanly and starts the new version with the same
arguments. Copies running from a `target/` build folder don't update
themselves.

## The Android app's key

The release workflow signs `djinn-go-android.apk` when these Actions secrets
exist; without them, the release goes out without an APK and the server doesn't
offer one. With them, the APK is built first, published with the release, and
carried inside the server, which is how most people get the app and its updates.
It's numbered by the release date, so each release is an upgrade to Android. Make
the key once:

```sh
keytool -genkeypair -v -keystore djinn-go.jks -alias djinn-go -keyalg RSA -keysize 4096 -validity 36500
base64 -w0 djinn-go.jks
```

Then add `DJINN_ANDROID_KEYSTORE_BASE64` (the output of the second command),
`DJINN_ANDROID_KEYSTORE_PASSWORD`, `DJINN_ANDROID_KEY_ALIAS` (`djinn-go`) and
`DJINN_ANDROID_KEY_PASSWORD`.

Back the key up with the release key. **If it's lost, phones can't update the
app to a build signed with a new one**; everyone would have to uninstall first,
which deletes what they've kept on the phone. Never commit it.
