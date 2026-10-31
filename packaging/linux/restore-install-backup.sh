#!/bin/sh
# Restore a snapshot made before installing a local DJINN build.
# Usage: sh restore.sh
set -eu

snapshot=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin="$HOME/.local/bin/djinn"
config="${XDG_CONFIG_HOME:-$HOME/.config}/djinn"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

if [ ! -f "$snapshot/djinn" ] || [ ! -f "$snapshot/config/djinn.db" ]; then
    echo "The DJINN backup is incomplete: $snapshot" >&2
    exit 1
fi

# Stop only the installed DJINN. Other development instances stay running.
was_running=false
managed=false
if command -v systemctl >/dev/null 2>&1 && \
    systemctl --user is-active --quiet djinn-local-test.service; then
    systemctl --user stop djinn-local-test.service
    was_running=true
    managed=true
fi
for pid in $(pgrep -x djinn || true); do
    exe=$(readlink "/proc/$pid/exe" 2>/dev/null || true)
    case "$exe" in
        "$bin"|"$bin (deleted)")
            kill "$pid"
            was_running=true
            ;;
    esac
done
if $was_running; then
    for attempt in 1 2 3 4 5 6 7 8 9 10; do
        if ! pgrep -f "^$bin( |$)" >/dev/null 2>&1; then break; fi
        sleep 1
    done
    if pgrep -f "^$bin( |$)" >/dev/null 2>&1; then
        echo "DJINN is still running. Quit it before restoring." >&2
        exit 1
    fi
fi

install -Dm755 "$snapshot/djinn" "$bin"
mkdir -p "$config"
rm -f "$config/djinn.db-wal" "$config/djinn.db-shm"
cp -a "$snapshot/config/." "$config/"
if [ -f "$snapshot/djinn.desktop" ]; then
    install -Dm644 "$snapshot/djinn.desktop" "$data/applications/djinn.desktop"
fi
if [ -f "$snapshot/djinn.png" ]; then
    install -Dm644 "$snapshot/djinn.png" "$data/icons/hicolor/512x512/apps/djinn.png"
fi
echo "Restored DJINN and its data from $snapshot"
if $was_running; then
    if $managed && command -v systemd-run >/dev/null 2>&1; then
        systemd-run --user --unit=djinn-local-test --collect \
            "$bin" --log-file "$config/djinn.log"
    elif command -v gtk-launch >/dev/null 2>&1; then
        gtk-launch djinn
    else
        echo "Start DJINN from the app menu to use the restored version."
    fi
fi
