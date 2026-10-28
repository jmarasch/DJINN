#!/bin/sh
# Installs DJINN for the current user: the binary in ~/.local/bin, an app
# menu entry (no terminal, tray icon only), and the icon. Works from the
# unpacked release download (djinn-linux-x86_64.tar.gz) or from a source
# checkout after a release build. No root needed, so DJINN can update
# itself in place.
#
#   ./install.sh                            install or update
#   ./install.sh --autostart                also start DJINN at login
#   ./install.sh --uninstall                remove all of the above
#
# Logs go to ~/.config/djinn/djinn.log. Config and database stay in
# ~/.config/djinn/ and are never touched by this script.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
if [ -f "$here/djinn" ]; then
    # The release download: everything sits next to this script.
    built="$here/djinn"
    png="$here/djinn.png"
    template="$here/djinn.desktop.in"
else
    root=$(cd "$here/../.." && pwd)
    built="$root/target/release/djinn"
    png="$root/web/public/icon-512.png"
    template="$here/djinn.desktop.in"
fi
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

if [ ! -f "$built" ]; then
    echo "Build first: (cd web && npm ci && npm run build) && cargo build --release" >&2
    exit 1
fi

install -Dm755 "$built" "$bin"
install -Dm644 "$png" "$icon"
mkdir -p "$(dirname "$desktop")"
sed -e "s|@BIN@|$bin|" -e "s|@LOG@|$conf/djinn/djinn.log|" \
    "$template" > "$desktop"
echo "Installed $bin and added DJINN to the app menu."

if [ "${1:-}" = "--autostart" ]; then
    install -Dm644 "$desktop" "$autostart"
    echo "DJINN will start when you log in."
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data/applications" >/dev/null 2>&1 || true
fi
