#!/usr/bin/env bash
# Print the public tag for a Cargo version, or read this checkout's version.
# A nightly keeps its suffix: 2027.1.5-nightly is v2027.01.05-nightly.
set -euo pipefail

version=${1:-$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)}
tag="v$version"
if [[ "$version" =~ ^([0-9]{4})\.([0-9]{1,2})\.([0-9]{1,2})(-nightly)?$ ]]; then
  year=${BASH_REMATCH[1]}
  month=$((10#${BASH_REMATCH[2]}))
  day=$((10#${BASH_REMATCH[3]}))
  suffix=${BASH_REMATCH[4]}
  if (( month < 1 || month > 12 || day < 1 || day > 31 )); then
    echo "Invalid calendar version $version" >&2
    exit 1
  fi
  printf -v tag 'v%04d.%02d.%02d%s' "$year" "$month" "$day" "$suffix"
fi
printf '%s\n' "$tag"
