#!/bin/sh
# Builds the DJINN install disc for PCs: Debian's official netinstall disc
# (checked against Debian's checksums) with DJINN and answers for the
# installer added. It boots straight into installing; Debian comes from
# the internet during the install, so the PC needs a network connection.
#
#   build-iso.sh DJINN_X86_64_BINARY OUT.iso [extra preseed lines file]
#
# With DJINN_KEEPSAKE=DIR (made by packaging/listener/make-keepsake.sh) the
# disc also carries the lyrics listener's packages and models, and the
# installed system finds them in /opt/djinn/keep, so Admin > AI installs the
# listener with no internet. That makes a large disc (up to about 12 GB with
# everything); the released disc leaves it out, and the keepsake is
# published beside it as separate files (see docs/releasing.md).
#
# Needs xorriso, cpio, gzip, curl. The third argument is for tests that
# answer the remaining questions too.
set -eu

binary=${1:?djinn binary}
out=${2:?output .iso}
extra=${3:-}
mkdir -p "$(dirname "$out")"
here=$(cd "$(dirname "$0")" && pwd)
mirror=${DEBIAN_CD:-https://cdimage.debian.org/debian-cd/current/amd64/iso-cd}

work=$(mktemp -d)
trap 'chmod -R u+w "$work" 2>/dev/null; rm -rf "$work"' EXIT

echo "Finding Debian's current netinstall disc"
curl -fsSL "$mirror/SHA512SUMS" -o "$work/SHA512SUMS"
iso=$(awk '/netinst\.iso$/ && !/edu/ && !/mac/ {print $2; exit}' "$work/SHA512SUMS")
[ -n "$iso" ] || { echo "no netinstall disc listed" >&2; exit 1; }
cache=${DEBIAN_CD_CACHE:-$work}
if [ ! -f "$cache/$iso" ] || ! (cd "$cache" && grep " $iso\$" "$work/SHA512SUMS" | sha512sum -c --quiet - 2>/dev/null); then
    echo "Downloading $iso"
    curl -fL --progress-bar "$mirror/$iso" -o "$cache/$iso"
fi
(cd "$cache" && grep " $iso\$" "$work/SHA512SUMS" | sha512sum -c --quiet -) || { echo "$iso doesn't match Debian's checksum" >&2; exit 1; }

echo "Adding DJINN"
mkdir "$work/iso"
xorriso -osirrox on -indev "$cache/$iso" -extract / "$work/iso" 2>/dev/null
chmod -R u+w "$work/iso"
mkdir -p "$work/iso/djinn"
cp "$binary" "$work/iso/djinn/djinn"
cp "$here/../common/install-files.sh" "$here/../common/config.toml" "$here/../common/djinn.service" \
    "$here/../common/kiosk.sh" "$here/late.sh" "$work/iso/djinn/"
# The user guide and language files (packaging/content.sh), beside the
# program; late.sh hands them to install-files.sh with it.
content=${DJINN_CONTENT:-$here/../../../target/content}
for part in guide lang; do
    if [ -f "$content/$part/VERSION" ]; then cp -r "$content/$part" "$work/iso/djinn/$part"; fi
done
if [ -n "${DJINN_KEEPSAKE:-}" ]; then
    echo "Adding the AI keepsake from $DJINN_KEEPSAKE"
    cp -r "$DJINN_KEEPSAKE" "$work/iso/djinn/keep"
fi
chmod 755 "$work/iso/djinn/djinn" "$work/iso/djinn/install-files.sh" "$work/iso/djinn/kiosk.sh" "$work/iso/djinn/late.sh"

# The answers go inside the installer's own startup files, where it looks
# first.
mkdir "$work/initrd"
cp "$here/preseed.cfg" "$work/initrd/preseed.cfg"
[ -n "$extra" ] && cat "$extra" >> "$work/initrd/preseed.cfg"
(cd "$work/initrd" && echo preseed.cfg | cpio -H newc -o --quiet | gzip) >> "$work/iso/install.amd/initrd.gz"

# Straight to "Install DJINN" after a few seconds, on BIOS and UEFI PCs;
# the second entry also sets up a screen (kiosk) showing the TV page.
cat > "$work/iso/isolinux/isolinux.cfg" <<'CFG'
ui vesamenu.c32
default djinn
prompt 0
timeout 100
menu title DJINN
label djinn
  menu label Install DJINN (headless; erases the disk you choose)
  kernel /install.amd/vmlinuz
  append vga=788 initrd=/install.amd/initrd.gz auto=true priority=high --- quiet
label screen
  menu label Install DJINN with a screen (shows the TV page)
  kernel /install.amd/vmlinuz
  append vga=788 initrd=/install.amd/initrd.gz auto=true priority=high djinn_screen=1 --- quiet
CFG
cat > "$work/iso/boot/grub/grub.cfg" <<'CFG'
set timeout=10
set default=0
insmod all_video
menuentry "Install DJINN (headless; erases the disk you choose)" {
    linux /install.amd/vmlinuz auto=true priority=high --- quiet
    initrd /install.amd/initrd.gz
}
menuentry "Install DJINN with a screen (shows the TV page)" {
    linux /install.amd/vmlinuz auto=true priority=high djinn_screen=1 --- quiet
    initrd /install.amd/initrd.gz
}
CFG
(cd "$work/iso" && find . -type f ! -name md5sum.txt ! -path './isolinux/*' ! -path './djinn/keep/*' -exec md5sum {} + > md5sum.txt)
# Files over 4 GiB (the biggest model) need ISO level 3.
level=""
[ -n "${DJINN_KEEPSAKE:-}" ] && level="-iso-level 3"

echo "Writing $out"
# The original disc's boot setup (BIOS, UEFI, and USB-stick hybrid), as
# xorriso describes it, applied to the changed files.
xorriso -indev "$cache/$iso" -report_el_torito as_mkisofs 2>/dev/null > "$work/boot-options"
eval "xorriso -as mkisofs $(tr '\n' ' ' < "$work/boot-options") $level -o \"$out\" \"$work/iso\"" 2>&1 | grep -iE 'failure|error' && exit 1
ls -la "$out"
