#!/bin/sh
# Lays out the files that ship beside the program: the user guide
# (docs/guide) as guide/ and the language files (web/src/i18n) as lang/,
# each stamped with this version in a VERSION file, in target/content/.
# With --zip, also zips them as target/content/djinn-content.zip, the
# release file that in-app updates download. DJINN ignores a folder stamped
# with another version (crates/djinn-server/src/content.rs).
#   sh packaging/content.sh [--zip]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ -n "$version" ] || { echo "No version in Cargo.toml" >&2; exit 1; }

out=target/content
rm -rf "$out"
mkdir -p "$out/guide/images" "$out/lang"
cp docs/guide/*.md "$out/guide/"
if [ -d docs/guide/images ]; then
  find docs/guide/images -maxdepth 1 -type f \( -name '*.webp' -o -name '*.png' -o -name '*.jpg' -o -name '*.md' \) \
    -exec cp {} "$out/guide/images/" \;
fi
cp web/src/i18n/*.json "$out/lang/"
printf '%s\n' "$version" > "$out/guide/VERSION"
printf '%s\n' "$version" > "$out/lang/VERSION"

if [ "${1:-}" = "--zip" ]; then
  (cd "$out" && rm -f djinn-content.zip && zip -qrX djinn-content.zip guide lang)
  echo "Content for $version in $out ($(du -sh "$out/djinn-content.zip" | cut -f1) zipped)"
else
  echo "Content for $version in $out"
fi
