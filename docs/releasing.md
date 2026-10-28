# Releasing DJINN

DJINN checks GitHub (`jmarasch/DJINN`) for the latest release and can
install it from Admin > About. It only installs a download listed in the
release's `SHA256SUMS`, and only if `SHA256SUMS.sig` is signed with
DJINN's release key. The public half of that key is `PUBLIC_KEY` in
`crates/djinn-server/src/update.rs`.

While the repository is private, GitHub hides releases from the app, so
the About tab says "No releases published yet". Releases still build.

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

1. Bump `version` in the root `Cargo.toml` (for example `0.2.0`), run
   `cargo build` so `Cargo.lock` follows, and commit.
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. The Release workflow builds Linux and Windows binaries
   (`djinn-linux-x86_64`, `djinn-windows-x86_64.exe`) and the downloads
   for first installs (below), writes and signs `SHA256SUMS`, and
   publishes the release with generated notes. Edit the notes on GitHub
   if you like; the About tab shows them.

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
- **Linux:** `djinn-linux-x86_64.tar.gz` holds the binary, icon, menu
  entry and `install.sh`. Unpack and run `./djinn/install.sh` (add
  `--autostart` to start at login, `--uninstall` to remove).

## The GPU pack

The same workflow builds the optional GPU pack: `djinn-gpu` (the AI
reader built with CUDA, `cargo build -p djinn-ai --features cuda --bin
djinn-gpu`) and NVIDIA's CUDA runtime libraries (cudart, cuBLAS,
cuBLASLt, cuRAND, which NVIDIA allows apps to ship), each compressed as
`gpu-<os>-<arch>.<file>.zst` and listed in the signed `SHA256SUMS`.
DJINN downloads the pack for its own version from Admin > AI when it sees
an NVIDIA driver, runs the reader through the helper, and falls back to
the processor if the helper fails. It's built for Turing (GTX 16xx, RTX
20xx) and newer. The GPU job hasn't run yet; check its output on the
first release.

## How an install works

The app downloads the binary for its system next to itself, checks size
and SHA-256, and swaps it in (Linux: replaces the file; Windows: renames
the running `djinn.exe` to `djinn.old.exe`, deleted at the next start).
Then it shuts down cleanly and starts the new version with the same
arguments. Copies running from a `target/` build folder don't update
themselves.
