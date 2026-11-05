#!/usr/bin/env bash
# Number this checkout as a build, for nightlies: sets DJINN's version in
# Cargo.toml and the workspace crates' entries in Cargo.lock, so
# `cargo build --locked` still works. Nothing is committed.
#   bash .github/scripts/set-version.sh 2027.1.5-nightly
set -euo pipefail

version=$1
if [[ ! "$version" =~ ^[0-9]{4}\.[0-9]{1,2}\.[0-9]{1,2}(-nightly)?$ ]]; then
  echo "Not a DJINN version: $version" >&2
  exit 1
fi
# The first version line is [workspace.package]'s, which every crate uses.
sed -i "0,/^version = \".*\"$/s//version = \"$version\"/" Cargo.toml
# Workspace crates are the djinn-* packages; each version line follows its name.
sed -i "/^name = \"djinn-[a-z-]*\"$/{n;s/^version = \".*\"$/version = \"$version\"/}" Cargo.lock
echo "Version $version"
