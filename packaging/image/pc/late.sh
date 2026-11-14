#!/bin/sh
# The installer's last step: DJINN into the new system, and the screen
# (kiosk) too when the boot menu's "with a screen" entry was chosen.
set -e
# Everything on the disc's djinn folder but the AI keepsake (if the disc has
# one), which goes straight to its place and stays.
mkdir -p /target/tmp/djinn-install
for f in /cdrom/djinn/*; do
    [ "$f" = /cdrom/djinn/keep ] || cp -r "$f" /target/tmp/djinn-install/
done
if [ -d /cdrom/djinn/keep ]; then
    mkdir -p /target/opt/djinn
    cp -r /cdrom/djinn/keep /target/opt/djinn/keep
fi
# Keeps the name given when the installer asked.
in-target env DJINN_KEEP_HOSTNAME=1 sh /tmp/djinn-install/install-files.sh / /tmp/djinn-install/djinn
if grep -qw 'djinn_screen=1' /proc/cmdline; then
    in-target sh /tmp/djinn-install/kiosk.sh
fi
rm -rf /target/tmp/djinn-install
