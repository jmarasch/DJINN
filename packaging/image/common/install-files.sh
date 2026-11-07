#!/bin/sh
# Puts DJINN into a Debian-family system at ROOT (a mounted image, or / when
# run inside the installer's target): the program in /opt/djinn, a system
# account `djinn` in the audio group, /etc/djinn/config.toml, data in
# /var/lib/djinn, music in /srv/music, the systemd service (enabled), and
# the hostname `djinn` (so it's http://djinn.local:8066 on the network).
#
# It only writes files, so a Pi's file system can be prepared from a PC
# without running anything built for the Pi.
#
#   install-files.sh ROOT DJINN_BINARY
set -eu

root=${1:?root folder}
binary=${2:?djinn binary}
here=$(cd "$(dirname "$0")" && pwd)

install -d -m 755 "$root/opt/djinn" "$root/etc/djinn" "$root/srv/music" "$root/etc/systemd/system/multi-user.target.wants"
install -m 755 "$binary" "$root/opt/djinn/djinn"
install -m 644 "$here/config.toml" "$root/etc/djinn/config.toml"
install -m 644 "$here/djinn.service" "$root/etc/systemd/system/djinn.service"
ln -sf /etc/systemd/system/djinn.service "$root/etc/systemd/system/multi-user.target.wants/djinn.service"

# The account, written straight into the files (no useradd inside the image).
uid=990
if ! grep -q '^djinn:' "$root/etc/passwd"; then
    while grep -q ":x:$uid:" "$root/etc/passwd" || grep -q ":x:$uid:" "$root/etc/group"; do uid=$((uid - 1)); done
    echo "djinn:x:$uid:$uid:DJINN:/var/lib/djinn:/usr/sbin/nologin" >> "$root/etc/passwd"
    echo "djinn:x:$uid:" >> "$root/etc/group"
    echo "djinn:!*:20000::::::" >> "$root/etc/shadow"
    [ -f "$root/etc/gshadow" ] && echo "djinn:!::" >> "$root/etc/gshadow"
fi
uid=$(grep '^djinn:' "$root/etc/passwd" | cut -d: -f3)
# In the audio group, for the speakers.
if grep -q '^audio:' "$root/etc/group"; then
    sed -i -E '/^audio:/{/[:,]djinn(,|$)/!{s/:$/:djinn/;t;s/$/,djinn/}}' "$root/etc/group"
fi
# DJINN updates itself, so it owns its program folder; and its data.
install -d -m 750 "$root/var/lib/djinn"
chown -R "$uid:$uid" "$root/opt/djinn" "$root/var/lib/djinn" "$root/srv/music"

echo djinn > "$root/etc/hostname"
if grep -q '^127.0.1.1' "$root/etc/hosts" 2>/dev/null; then
    sed -i 's/^127\.0\.1\.1.*/127.0.1.1\tdjinn/' "$root/etc/hosts"
else
    printf '127.0.1.1\tdjinn\n' >> "$root/etc/hosts"
fi

# The address on a plugged-in screen until DJINN writes its own.
install -d -m 755 "$root/etc/issue.d"
printf 'DJINN: open http://djinn.local:8066 on a phone or computer on this network\n\n' > "$root/etc/issue.d/djinn.issue"
# DJINN (not root) rewrites the banner at each start, with the address it
# found; this makes the file its own.
install -d -m 755 "$root/etc/tmpfiles.d"
cat > "$root/etc/tmpfiles.d/djinn.conf" <<'TMP'
d /run/issue.d 0755 root root -
f /run/issue.d/djinn.issue 0644 djinn djinn -
TMP
