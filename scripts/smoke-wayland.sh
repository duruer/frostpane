#!/usr/bin/env bash
# Wayland smoke (porting design 5.3), as CI runs it. Wraps the Wayland
# backend's own end-to-end check, crates/fp-wayland/smoke/smoke-wayland.sh
# (headless sway, floating foot windows, sway IPC source, layer-shell
# overlay and sprite surfaces), normal and in behind-windows mode. The
# probe and sprite examples it uses are built first.
#
# Usage: smoke-wayland.sh [-s SECONDS] [-b BINARY]
# Env:   SMOKE_SHOT_DIR=dir keeps every run's screenshots and logs there.
# Needs: sway swaymsg foot grim python3 dbus-run-session mesa-vulkan-drivers.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
target="${CARGO_TARGET_DIR:-$root/target}"
secs=25
bin=""
while getopts "s:b:" o; do
  case "$o" in
    s) secs="$OPTARG" ;;
    b) bin="$OPTARG" ;;
    *) echo "usage: $0 [-s seconds] [-b binary]" >&2; exit 2 ;;
  esac
done
bin="${bin:-$target/debug/frostpane}"
[ -x "$bin" ] || { echo "smoke-wayland: no frostpane binary at $bin (pass -b)" >&2; exit 2; }

(cd "$root" && cargo build -q -p fp-wayland --examples)
export PROBE="$target/debug/examples/wl_probe" SPRITES="$target/debug/examples/wl_sprites"

fail=0
run() { # name, extra env...
  local name="$1"; shift
  echo "== smoke-wayland: $name"
  local keep=()
  if [ -n "${SMOKE_SHOT_DIR:-}" ]; then keep=(SMOKE_OUT="$SMOKE_SHOT_DIR/wayland-$name"); fi
  if ! env "${keep[@]}" "$@" "$root/crates/fp-wayland/smoke/smoke-wayland.sh" "$bin" "$secs"; then
    echo "smoke-wayland: $name FAILED" >&2
    fail=1
  fi
}
run normal
run behind BEHIND=1
exit "$fail"
