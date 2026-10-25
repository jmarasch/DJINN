#!/bin/sh
# Installs DJINN for the current user: the binary in ~/.local/bin, an app
# menu entry (no terminal, tray icon only), and the icon.
#
#   packaging/linux/install.sh              install or update
#   packaging/linux/install.sh --autostart  also start DJINN at login
#   packaging/linux/install.sh --uninstall  remove all of the above
#
# Logs go to ~/.config/djinn/djinn.log. Config and database stay in
# ~/.config/djinn/ and are never touched by this script.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
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

built="$root/target/release/djinn"
if [ ! -x "$built" ]; then
    echo "Build first: (cd web && npm ci && npm run build) && cargo build --release" >&2
    exit 1
fi

install -Dm755 "$built" "$bin"
install -Dm644 "$root/web/public/icon-512.png" "$icon"
mkdir -p "$(dirname "$desktop")"
sed -e "s|@BIN@|$bin|" -e "s|@LOG@|$conf/djinn/djinn.log|" \
    "$root/packaging/linux/djinn.desktop.in" > "$desktop"
echo "Installed $bin and added DJINN to the app menu."

if [ "${1:-}" = "--autostart" ]; then
    install -Dm644 "$desktop" "$autostart"
    echo "DJINN will start when you log in."
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$data/applications" >/dev/null 2>&1 || true
fi
