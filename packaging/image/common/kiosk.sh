#!/bin/sh
# Adds a screen to a DJINN box: cage (a kiosk display that runs one program
# full screen, no desktop) with Chromium in kiosk mode, started at boot on
# the first console. It opens http://localhost:8066/screen, which is the TV
# page on Main and, on a node, the node's page until it joins Main, then
# Main's TV page for the node's room. Run as root inside the installed
# system; the packages need the network unless they're already installed
# (the PC disc installs them in its software step). The console login moves
# to Ctrl+Alt+F2.
set -eu

export DEBIAN_FRONTEND=noninteractive
apt-get install -y --no-install-recommends \
    cage chromium curl fonts-noto-core fonts-noto-color-emoji libgl1-mesa-dri dbus-user-session

# Its own account, allowed to use the screen, GPU and keyboard.
if ! id djinn-screen >/dev/null 2>&1; then
    useradd --system --create-home --home-dir /var/lib/djinn-screen --shell /usr/sbin/nologin djinn-screen
fi
for group in video render input; do
    getent group "$group" >/dev/null && usermod -aG "$group" djinn-screen
done

cat > /etc/systemd/system/djinn-screen.service <<'UNIT'
[Unit]
Description=DJINN screen (kiosk)
After=djinn.service systemd-user-sessions.service plymouth-quit-wait.service
Wants=djinn.service
Conflicts=getty@tty1.service
After=getty@tty1.service

[Service]
User=djinn-screen
PAMName=login
TTYPath=/dev/tty1
TTYReset=yes
TTYVHangup=yes
TTYVTDisallocate=yes
StandardInput=tty-fail
StandardOutput=journal
StandardError=journal
UtmpIdentifier=tty1
UtmpMode=user
Environment=XDG_SESSION_TYPE=wayland
# Wait (up to a minute) for DJINN to answer, so the browser doesn't open on
# an error page.
ExecStartPre=/bin/sh -c 'for i in $(seq 60); do curl -fs -o /dev/null http://localhost:8066/screen && exit 0; sleep 1; done; exit 0'
ExecStart=/usr/bin/cage -s -- /usr/bin/chromium --kiosk --ozone-platform=wayland \
    --no-first-run --noerrdialogs --disable-infobars --disable-session-crashed-bubble \
    --disable-features=Translate --password-store=basic --check-for-update-interval=31536000 \
    --user-data-dir=/var/lib/djinn-screen/chromium http://localhost:8066/screen
Restart=always
RestartSec=3

[Install]
WantedBy=graphical.target
UNIT

systemctl enable djinn-screen.service
systemctl set-default graphical.target
echo "DJINN screen set up"
