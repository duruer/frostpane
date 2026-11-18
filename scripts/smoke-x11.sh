#!/usr/bin/env bash
# X11 smoke: Xvfb + a window manager (openbox, else any of the fallbacks),
# then scripts/smoke-run.sh. Optionally takes a screenshot (xwd -> png when
# available) into $SMOKE_SHOT_DIR.
#
# Usage: smoke-x11.sh [-s SECONDS] [-b BINARY]
# Env: SMOKE_REQUIRE_FRAMES=1 (see smoke-run.sh), SMOKE_WM=openbox,
#      SMOKE_SHOT_DIR=dir, SMOKE_COMPOSITOR=picom (start a compositor too).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
secs=8
args=()
while getopts "s:b:" o; do
  case "$o" in
    s) secs="$OPTARG"; args+=(-s "$OPTARG") ;;
    b) args+=(-b "$OPTARG") ;;
    *) echo "usage: $0 [-s seconds] [-b binary]" >&2; exit 2 ;;
  esac
done

command -v Xvfb >/dev/null || { echo "smoke-x11: Xvfb missing (apt install xvfb)" >&2; exit 2; }

pids=()
cleanup() { for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

disp=":$((90 + $$ % 50))"
Xvfb "$disp" -screen 0 1920x1080x24 -nolisten tcp >/dev/null 2>&1 &
pids+=($!)
export DISPLAY="$disp"
for _ in $(seq 1 50); do
  if command -v xdpyinfo >/dev/null; then xdpyinfo >/dev/null 2>&1 && break
  else [ -e "/tmp/.X11-unix/X${disp#:}" ] && break; fi
  sleep 0.1
done

# Pin the backend; a runner may have stray Wayland variables.
unset WAYLAND_DISPLAY SWAYSOCK HYPRLAND_INSTANCE_SIGNATURE
export FROSTPANE_BACKEND=x11
export XDG_SESSION_TYPE=x11

wm="${SMOKE_WM:-openbox}"
if command -v "$wm" >/dev/null; then
  "$wm" >/dev/null 2>&1 &
  pids+=($!)
else
  echo "smoke-x11: window manager '$wm' not found, running without"
fi
if [ -n "${SMOKE_COMPOSITOR:-}" ] && command -v "$SMOKE_COMPOSITOR" >/dev/null; then
  "$SMOKE_COMPOSITOR" >/dev/null 2>&1 &
  pids+=($!)
fi
sleep 1

if [ -n "${SMOKE_SHOT_DIR:-}" ]; then
  mkdir -p "$SMOKE_SHOT_DIR"
  if command -v import >/dev/null; then
    export SMOKE_POST="import -window root '$SMOKE_SHOT_DIR/x11.png'"
  elif command -v xwd >/dev/null; then
    export SMOKE_POST="xwd -root -silent -out '$SMOKE_SHOT_DIR/x11.xwd'"
  fi
fi

exec "$here/smoke-run.sh" "${args[@]}"
