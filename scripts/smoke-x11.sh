#!/usr/bin/env bash
# X11 smoke (porting design 5.3), as CI runs it. Wraps the X11 backend's own
# end-to-end check, crates/fp-x11/smoke/smoke.sh (private Xvfb + openbox +
# picom + an xterm; asserts panels, frames and a snow ledge on the xterm),
# in three sessions: normal, behind-windows (overlay stacked under the
# xterm) and without a compositor (no panel, clean exit). Then runs the
# live backend test, crates/fp-x11/smoke/live.sh.
#
# Usage: smoke-x11.sh [-s SECONDS] [-b BINARY]
# Env:   SMOKE_SHOT_DIR=dir keeps every run's screenshots and logs there.
#        SMOKE_SKIP_LIVE=1 skips the live test (it runs cargo).
# Needs: Xvfb openbox picom xterm xdotool x11-utils imagemagick dbus
#        mesa-vulkan-drivers (see smoke.sh).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
target="${CARGO_TARGET_DIR:-$root/target}"
secs=20
bin=""
while getopts "s:b:" o; do
  case "$o" in
    s) secs="$OPTARG" ;;
    b) bin="$OPTARG" ;;
    *) echo "usage: $0 [-s seconds] [-b binary]" >&2; exit 2 ;;
  esac
done
bin="${bin:-$target/debug/frostpane}"
[ -x "$bin" ] || { echo "smoke-x11: no frostpane binary at $bin (pass -b)" >&2; exit 2; }

out="${SMOKE_SHOT_DIR:-$(mktemp -d)}"
mkdir -p "$out"
# The screenshot is taken a few seconds before the run ends.
shot_at=$((secs > 8 ? secs - 6 : secs / 2))
fail=0
run() { # name, extra env...
  local name="$1"; shift
  echo "== smoke-x11: $name"
  if ! env SMOKE_OUT="$out/x11-$name" SHOT_AT="$shot_at" "$@" "$root/crates/fp-x11/smoke/smoke.sh" "$bin" "$secs"; then
    echo "smoke-x11: $name FAILED" >&2
    fail=1
  fi
}
run normal
run behind SMOKE_BEHIND=1
run no-compositor SMOKE_NO_COMPOSITOR=1

if [ "${SMOKE_SKIP_LIVE:-0}" != 1 ]; then
  echo "== smoke-x11: live backend test"
  (cd "$root" && crates/fp-x11/smoke/live.sh) || { echo "smoke-x11: live test FAILED" >&2; fail=1; }
fi
echo "smoke-x11: artifacts in $out"
exit "$fail"
