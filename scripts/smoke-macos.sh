#!/usr/bin/env bash
# macOS smoke: run frostpane --seconds N through the generic runner. While the
# backend is Null this only asserts a clean exit; set SMOKE_REQUIRE_FRAMES=1
# once the real backend lands.
# Usage: smoke-macos.sh [-s SECONDS] [-b BINARY]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
[ "$(uname -s)" = "Darwin" ] || { echo "smoke-macos: not macOS" >&2; exit 2; }
exec "$here/smoke-run.sh" "$@"
