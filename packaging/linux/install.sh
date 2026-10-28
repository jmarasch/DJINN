#!/bin/sh
# Installs DJINN for the current user: the binary in ~/.local/bin, an app
# menu entry (no terminal, tray icon only), and the icon. No root needed,
# so DJINN can update itself in place.
#
# Straight from the latest release:
#   curl -fsSL https://github.com/jmarasch/DJINN/releases/latest/download/install.sh | sh
#   (add `-s -- --autostart` after `sh` to also start DJINN at login)
#
# From the unpacked download (djinn-linux-x86_64.tar.gz), or from a source
# checkout after a release build:
#   ./install.sh                            install or update
#   ./install.sh --autostart                also start DJINN at login
#   ./install.sh --uninstall                remove all of the above
#
# Logs go to ~/.config/djinn/djinn.log. Config and database stay in
# ~/.config/djinn/ and are never touched by this script.
set -eu

repo=https://github.com/jmarasch/DJINN
# Where releases are downloaded from (tests point it at a local copy).
releases="${DJINN_RELEASES_URL:-$repo/releases/latest/download}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
conf="${XDG_CONFIG_HOME:-$HOME/.config}"
bin="$HOME/.local/bin/djinn"
desktop="$data/applications/djinn.desktop"
icon="$data/icons/hicolor/512x512/apps/djinn.png"
autostart="$conf/autostart/djinn.desktop"

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin" "$desktop" "$icon" "$autostart"
    echo "DJINN removed. Your config and database in $conf/djinn were kept."
    exit 0
fi

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        echo "Downloading DJINN needs curl or wget." >&2
        exit 1
    fi
}

here=$(cd "$(dirname "$0")" && pwd)
if [ -f "$here/djinn" ] && [ -f "$here/djinn.desktop.in" ]; then
    # The release download: everything sits next to this script.
    built="$here/djinn"
    png="$here/djinn.png"
    template="$here/djinn.desktop.in"
elif [ -f "$here/djinn.desktop.in" ]; then
    # A source checkout (packaging/linux).
    root=$(cd "$here/../.." && pwd)
    built="$root/target/release/djinn"
    png="$root/web/public/icon-512.png"
    template="$here/djinn.desktop.in"
    if [ ! -f "$built" ]; then
        echo "Build first: (cd web && npm ci && npm run build) && cargo build --release" >&2
        exit 1
    fi
else
    # On its own (piped from curl): fetch the latest release and run the
    # install.sh inside it.
    if [ "$(uname -m)" != x86_64 ]; then
        echo "Release downloads are for 64-bit PCs (x86_64) so far. On $(uname -m)," >&2
        echo "build DJINN from source: $repo" >&2
        exit 1
    fi
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT INT TERM
    echo "Downloading DJINN..."
    download "$releases/djinn-linux-x86_64.tar.gz" "$tmp/djinn-linux-x86_64.tar.gz"
    download "$releases/SHA256SUMS" "$tmp/SHA256SUMS"
    if ! (cd "$tmp" && grep ' djinn-linux-x86_64.tar.gz$' SHA256SUMS | sha256sum -c --quiet - >/dev/null 2>&1); then
        echo "The download didn't match the release's checksum; try again." >&2
        exit 1
    fi
    tar xzf "$tmp/djinn-linux-x86_64.tar.gz" -C "$tmp"
    sh "$tmp/djinn/install.sh" "$@"
    exit
fi

# DJINN plays through ALSA (and PulseAudio or PipeWire on top of it).
if command -v ldconfig >/dev/null 2>&1 && ! ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2'; then
    echo "Note: DJINN needs the ALSA library, which wasn't found. Install it with" >&2
    echo "  sudo apt install libasound2t64   (Ubuntu 24.04+; libasound2 on older Debian/Ubuntu)" >&2
    echo "  sudo dnf install alsa-lib        (Fedora)" >&2
    echo "  sudo pacman -S alsa-lib          (Arch)" >&2
fi

running=false
if command -v pgrep >/dev/null 2>&1 && pgrep -x djinn >/dev/null 2>&1; then
    running=true
fi

install -Dm755 "$built" "$bin"
install -Dm644 "$png" "$icon"
mkdir -p "$(dirname "$desktop")"
sed -e "s|@BIN@|$bin|" -e "s|@LOG@|$conf/djinn/djinn.log|" \
    "$template" > "$desktop"
echo "Installed $("$bin" --version 2>/dev/null || echo DJINN) to $bin and added DJINN to the app menu."

if [ "${1:-}" = "--autostart" ]; then
    install -Dm644 "$desktop" "$autostart"
    echo "DJINN will start when you log in."
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data/applications" >/dev/null 2>&1 || true
fi

if $running; then
    echo "DJINN is running: quit it from its tray icon and start it again to use this version."
else
    echo "Start DJINN from your app menu; it opens in your browser."
fi
