#!/usr/bin/env bash
# Set the workspace version in Cargo.toml and in Cargo.lock.
# Usage: bump-version.sh X.Y.Z
set -euo pipefail

ver="${1:?usage: bump-version.sh X.Y.Z}"
[[ "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "bad version: $ver" >&2; exit 1; }

cd "$(dirname "$0")/../.."

# [workspace.package] version: the first top-level `version = "..."` line.
awk -v v="$ver" '
  /^\[workspace\.package\]/ { inpkg = 1; print; next }
  /^\[/ { inpkg = 0 }
  inpkg && !done && /^version[ ]*=/ { print "version = \"" v "\""; done = 1; next }
  { print }
  END { if (!done) exit 1 }
' Cargo.toml > Cargo.toml.new
mv Cargo.toml.new Cargo.toml

# Cargo.lock: rewrite the version of the workspace members only. They are
# path packages (no `source` line), but so are vendored crates under vendor/,
# which keep their own versions. The lock is read and written with LF endings.
# Some manifests are committed with CRLF line endings; drop the CR first.
members="$(cat crates/*/Cargo.toml app/*/Cargo.toml | tr -d '\r' | sed -n 's/^name = "\(.*\)"$/\1/p' | tr '\n' ' ')"
[ -n "$members" ] || { echo "no workspace members found" >&2; exit 1; }
awk -v v="$ver" -v members=" $members" '
  function flush(   i) { for (i = 0; i < n; i++) { if (inblk && !src && mine && buf[i] ~ /^version = /) print "version = \"" v "\""; else print buf[i] } n = 0 }
  /^\[\[package\]\]/ { flush(); src = 0; mine = 0; inblk = 1 }
  /^name = / { nm = $3; gsub(/["
]/, "", nm); mine = index(members, " " nm " ") > 0 }
  /^source = / { src = 1 }
  { buf[n++] = $0 }
  END { flush() }
' Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock

echo "bumped to $ver"
