#!/bin/sh
# The installer's last step: DJINN into the new system, and the screen
# (kiosk) too when the boot menu's "with a screen" entry was chosen.
set -e
cp -r /cdrom/djinn /target/tmp/djinn-install
in-target sh /tmp/djinn-install/install-files.sh / /tmp/djinn-install/djinn
if grep -qw 'djinn_screen=1' /proc/cmdline; then
    in-target sh /tmp/djinn-install/kiosk.sh
fi
rm -rf /target/tmp/djinn-install
