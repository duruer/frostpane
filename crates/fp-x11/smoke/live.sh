#!/usr/bin/env bash
# Runs the live X11 integration test (`tests/live.rs`) inside a private
# Xvfb + openbox + picom session. Usage, from the workspace root:
#   crates/fp-x11/smoke/live.sh
# Extra arguments go to `cargo test`. Kills only the PIDs it started.
set -u
DPY=${SMOKE_DISPLAY:-:95}
for t in Xvfb openbox picom xwininfo; do
    command -v "$t" >/dev/null || { echo "live: missing $t"; exit 2; }
done
PIDS=()
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    wait 2>/dev/null
}
trap cleanup EXIT
unset WAYLAND_DISPLAY
export DISPLAY=$DPY
Xvfb "$DPY" -screen 0 1920x1080x24 -nolisten tcp -nolisten unix >/dev/null 2>&1 &
PIDS+=($!)
for _ in $(seq 50); do xwininfo -root >/dev/null 2>&1 && break; sleep 0.1; done
openbox >/dev/null 2>&1 &
PIDS+=($!)
picom --backend xrender >/dev/null 2>&1 &
PIDS+=($!)
sleep 1.5
FP_X11_LIVE=1 cargo test -p fp-x11 --test live "$@" -- --nocapture --test-threads=1
