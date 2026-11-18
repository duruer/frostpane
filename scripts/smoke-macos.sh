#!/usr/bin/env bash
# macOS smoke: run frostpane --seconds N through the generic runner. It
# asserts a clean exit; set SMOKE_REQUIRE_FRAMES=1 to demand presented frames
# (hosted runners' GPU and WindowServer access are not verified yet).
# Usage: smoke-macos.sh [-s SECONDS] [-b BINARY]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
[ "$(uname -s)" = "Darwin" ] || { echo "smoke-macos: not macOS" >&2; exit 2; }
exec "$here/smoke-run.sh" "$@"
