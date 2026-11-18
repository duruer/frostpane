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

# Cargo.lock: path (workspace) packages have no `source` line. Rewrite their
# version only. The lock is read and written with LF endings.
awk -v v="$ver" '
  function flush(   i) { for (i = 0; i < n; i++) { if (inblk && !src && buf[i] ~ /^version = /) print "version = \"" v "\""; else print buf[i] } n = 0 }
  /^\[\[package\]\]/ { flush(); src = 0; inblk = 1 }
  /^source = / { src = 1 }
  { buf[n++] = $0 }
  END { flush() }
' Cargo.lock > Cargo.lock.new
mv Cargo.lock.new Cargo.lock

echo "bumped to $ver"
