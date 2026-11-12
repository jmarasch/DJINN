#!/bin/sh
# Builds a DJINN image for Raspberry Pi: Raspberry Pi OS Lite (checked
# against its published checksum) with DJINN added. Flash it with
# Raspberry Pi Imager ("Use custom"), which can still set Wi-Fi and a login.
#
#   build-pi.sh arm64|armhf DJINN_BINARY OUT.img.xz
#
# arm64 is for a Pi 3, 4 or 5 (Main or node); armhf runs on every Pi,
# including a Pi 1, Pi 2 and Zero W (node). Runs everything inside a
# throwaway privileged Docker container (for the loop mounts), so the host
# only needs Docker.
set -eu

arch=${1:?arm64 or armhf}
binary=$(realpath "${2:?djinn binary}")
out=$(realpath -m "${3:?output .img.xz}")
here=$(cd "$(dirname "$0")/.." && pwd)
case "$arch" in
    arm64) latest=https://downloads.raspberrypi.com/raspios_lite_arm64_latest ;;
    armhf) latest=https://downloads.raspberrypi.com/raspios_lite_armhf_latest ;;
    *) echo "arm64 or armhf" >&2; exit 1 ;;
esac
cache=${PI_IMAGE_CACHE:-$(dirname "$out")}
mkdir -p "$cache" "$(dirname "$out")"

docker run --rm --privileged \
    -v "$here:/image:ro" -v "$binary:/djinn:ro" -v "$cache:/cache" -v "$(dirname "$out"):/out" \
    -e LATEST="$latest" -e OUT="/out/$(basename "$out")" -e OWNER="$(id -u):$(id -g)" \
    debian:trixie-slim sh -euc '
apt-get update -qq >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends ca-certificates curl xz-utils fdisk e2fsprogs mount >/dev/null 2>&1

# Where "latest" points, and its published checksum.
url=$(curl -fsSLI -o /dev/null -w "%{url_effective}" "$LATEST")
name=$(basename "$url")
curl -fsSL --retry 5 "$url.sha256" -o /tmp/sum
check() { (cd /cache && sed "s|  .*|  $name|" /tmp/sum | sha256sum -c --quiet - >/dev/null 2>&1); }
if ! check; then
    echo "Downloading $name"
    # Resumes and retries: these are big, and connections drop.
    curl -fsSL --retry 10 --retry-all-errors -C - "$url" -o "/cache/$name.part"
    mv "/cache/$name.part" "/cache/$name"
fi
check || { rm -f "/cache/$name"; echo "$name does not match its checksum" >&2; exit 1; }
chown "$OWNER" "/cache/$name"

echo "Adding DJINN to $name"
xz -dc "/cache/$name" > /tmp/pi.img
# Room for DJINN; the Pi grows to the whole card on its first start.
truncate -s +512M /tmp/pi.img
start=$(sfdisk -d /tmp/pi.img | awk -F"[=,]" "/img2/ {gsub(/ /,\"\",\$2); print \$2}")
echo ", +" | sfdisk -q -N 2 /tmp/pi.img
root=$(losetup -f --show -o $((start * 512)) /tmp/pi.img)
e2fsck -fy "$root" >/dev/null 2>&1 || true
resize2fs "$root" >/dev/null 2>&1
mkdir -p /mnt/root
mount "$root" /mnt/root
trap "umount /mnt/root 2>/dev/null; losetup -d $root 2>/dev/null" EXIT
sh /image/common/install-files.sh /mnt/root /djinn

sync
umount /mnt/root
losetup -d "$root"
trap - EXIT
echo "Compressing"
xz -T0 -6 -c /tmp/pi.img > "$OUT"
chown "$OWNER" "$OUT"
ls -la "$OUT"
'
