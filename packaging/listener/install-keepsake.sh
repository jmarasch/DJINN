#!/bin/sh
# Puts the AI keepsake (published beside a DJINN release as
# ai-keepsake-*.tar.zst.part-* files) where DJINN looks for it, so Admin > AI
# installs the lyrics listener with no internet.
#
#   install-keepsake.sh FOLDER_WITH_THE_PARTS [DEST]
#
# Run it once per kind of file: the packages for this computer's Debian
# (wheels-bookworm-* or wheels-trixie-*) and the models (models-*). Parts of
# one archive are joined in order. DEST defaults to /opt/djinn/keep. Needs
# zstd and tar.
set -eu

from=${1:?usage: install-keepsake.sh FOLDER [DEST]}
dest=${2:-/opt/djinn/keep}
mkdir -p "$dest"
# Each archive is name.tar.zst.part-aa, -ab, ...
for first in "$from"/*.tar.zst.part-aa; do
    [ -e "$first" ] || { echo "No keepsake parts in $from" >&2; exit 1; }
    base=${first%.part-aa}
    echo "Unpacking $(basename "$base")"
    cat "$base".part-* | zstd -d -q | tar -x -C "$dest"
done
if [ -f "$dest/SHA256SUMS" ]; then
    echo "Checking"
    (cd "$dest" && sha256sum -c --quiet SHA256SUMS)
fi
echo "Keepsake ready in $dest"
