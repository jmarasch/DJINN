#!/bin/sh
# Snapshot the current user installation before testing a local build.
set -eu

bin="$HOME/.local/bin/djinn"
config="${XDG_CONFIG_HOME:-$HOME/.config}/djinn"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
snapshot="$data/djinn-install-backups/$(date +%Y%m%d-%H%M%S)"
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

if [ ! -f "$bin" ] || [ ! -f "$config/djinn.db" ]; then
    echo "DJINN binary or database was not found; no backup was made." >&2
    exit 1
fi

mkdir -p "$snapshot/config"
cp -a "$bin" "$snapshot/djinn"
# The SQLite backup API includes committed WAL data while DJINN is running.
sqlite3 "$config/djinn.db" ".backup '$snapshot/config/djinn.db'"
rsync -a --exclude='djinn.db' --exclude='djinn.db-wal' \
    --exclude='djinn.db-shm' "$config/" "$snapshot/config/"
if [ -f "$data/applications/djinn.desktop" ]; then
    cp -a "$data/applications/djinn.desktop" "$snapshot/djinn.desktop"
fi
if [ -f "$data/icons/hicolor/512x512/apps/djinn.png" ]; then
    cp -a "$data/icons/hicolor/512x512/apps/djinn.png" "$snapshot/djinn.png"
fi
cp -a "$script_dir/restore-install-backup.sh" "$snapshot/restore.sh"
sqlite3 "$snapshot/config/djinn.db" 'PRAGMA integrity_check' | grep -qx ok
echo "$snapshot"
