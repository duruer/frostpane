#!/usr/bin/env bash
# End-to-end X11 smoke of the X11 backend (porting design 5.3).
#
# Starts a private headless X server (Xvfb) with openbox (EWMH WM), picom
# (compositor) and an xterm as the foreign window, runs frostpane with
# `--quiet --seconds`, takes a screenshot and checks:
#   1. the exit line reports >= 1 overlay panel and > 0 frames presented;
#   2. a band just above the xterm's frame (the ledge) holds clearly more
#      snow pixels than the same band in open air and than before the run;
#   3. with SMOKE_BEHIND=1: an overlay panel is stacked below the xterm frame.
#
# Usage: crates/fp-x11/smoke/smoke.sh [path/to/frostpane] [seconds]
# Env:   SMOKE_BEHIND=1, SMOKE_OUT=dir, SMOKE_DISPLAY=:97, SHOT_AT=14,
#        SMOKE_NO_COMPOSITOR=1 (expects no panel, still exits cleanly).
# Needs: Xvfb openbox picom xterm xdotool x11-utils imagemagick dbus
#        mesa-vulkan-drivers (lavapipe). Kills only the PIDs it started.
set -u

BIN=${1:-${CARGO_TARGET_DIR:-target}/debug/frostpane}
SECS=${2:-20}
SHOT_AT=${SHOT_AT:-14}
DPY=${SMOKE_DISPLAY:-:97}
OUT=${SMOKE_OUT:-$(mktemp -d)}
mkdir -p "$OUT"

for t in Xvfb openbox picom xterm xdotool xwininfo import convert dbus-run-session; do
    command -v "$t" >/dev/null || { echo "smoke: missing $t"; exit 2; }
done
[ -x "$BIN" ] || { echo "smoke: no binary at $BIN"; exit 2; }

PIDS=()
cleanup() {
    for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null; done
    wait 2>/dev/null
}
trap cleanup EXIT

# Hygiene: X11 only, private data dir, shims for desktop helpers.
unset WAYLAND_DISPLAY
export DISPLAY=$DPY
export XDG_SESSION_TYPE=x11
export FROSTPANE_BACKEND=x11
export FROSTPANE_DATA_DIR=$OUT/data
export FROSTPANE_LOG=${FROSTPANE_LOG:-info}
export XDG_RUNTIME_DIR=$OUT/run
mkdir -p "$XDG_RUNTIME_DIR" "$FROSTPANE_DATA_DIR" "$OUT/shim"
chmod 700 "$XDG_RUNTIME_DIR"
# Snow shown, welcome seen; SMOKE_BEHIND=1 runs behind-windows mode.
BEHIND=false
[ "${SMOKE_BEHIND:-0}" = 1 ] && BEHIND=true
printf '{"weather":"snow","seenWelcome":true,"heavySnow":true,"behindWindows":%s}\n' "$BEHIND" \
    > "$FROSTPANE_DATA_DIR/settings.json"
for s in xdg-open gsettings feh; do
    printf '#!/bin/sh\necho "%s $*" >> "%s/shim.log"\n' "$s" "$OUT" > "$OUT/shim/$s"
    chmod +x "$OUT/shim/$s"
done
export PATH=$OUT/shim:$PATH
LVP=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1)
[ -n "$LVP" ] && export VK_ICD_FILENAMES=$LVP

# Abstract socket only: /tmp/.X11-unix may be read-only (WSLg).
Xvfb "$DPY" -screen 0 1920x1080x24 -nolisten tcp -nolisten unix >"$OUT/xvfb.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 50); do xwininfo -root >/dev/null 2>&1 && break; sleep 0.1; done
openbox >"$OUT/openbox.log" 2>&1 &
PIDS+=($!)
if [ "${SMOKE_NO_COMPOSITOR:-0}" != 1 ]; then
    picom --backend xrender --no-fading-openclose >"$OUT/picom.log" 2>&1 &
    PIDS+=($!)
fi
xterm -xrm '*allowTitleOps: false' -geometry 100x30+500+400 -title smoke-xterm >/dev/null 2>&1 &
PIDS+=($!)
sleep 2

XT=$(xdotool search --name smoke-xterm | head -1)
[ -n "$XT" ] || { echo "smoke: xterm did not appear"; exit 1; }
geo() { xwininfo -frame -id "$XT" | awk -v k="$1" '$0 ~ k {print $NF}'; }
FX=$(geo "Absolute upper-left X"); FY=$(geo "Absolute upper-left Y"); FW=$(geo "Width")
echo "smoke: xterm frame at $FX,$FY width $FW"
# Band just above the frame, inset from the corners.
BAND_H=10
BX=$((FX + 20)); BW=$((FW - 40)); BY=$((FY - BAND_H))
bright() { # file [band top]
    convert "$1" -crop "${BW}x${BAND_H}+${BX}+${2:-$BY}" +repage -colorspace gray -threshold 60% \
        -format '%[fx:round(mean*w*h)]' info:
}
import -window root "$OUT/before.png"
BEFORE=$(bright "$OUT/before.png")

dbus-run-session -- "$BIN" --quiet --seconds "$SECS" >"$OUT/app.out" 2>"$OUT/app.err" &
APP=$!
PIDS+=($APP)
sleep "$SHOT_AT"
import -window root "$OUT/shot.png"
# Root children while the app runs (xwininfo lists them top to bottom).
xwininfo -root -tree > "$OUT/tree.txt"
wait "$APP"
CODE=$?

AFTER=$(bright "$OUT/shot.png")
AIR=$(bright "$OUT/shot.png" $((BY - 150)))
LINE=$(grep 'frostpane: exit' "$OUT/app.out")
echo "smoke: $LINE (exit $CODE)"
echo "smoke: bright pixels above the xterm: before=$BEFORE ledge=$AFTER open-air=$AIR"
echo "smoke: artifacts in $OUT"
PANELS=$(echo "$LINE" | sed -n 's/.* \([0-9]*\) overlay panel.*/\1/p')
FRAMES=$(echo "$LINE" | sed -n 's/.* \([0-9]*\) frames presented.*/\1/p')
ok=0
[ "$CODE" -eq 0 ] || { echo "smoke: FAIL exit code $CODE"; ok=1; }
if [ "${SMOKE_NO_COMPOSITOR:-0}" = 1 ]; then
    [ "${PANELS:-1}" -eq 0 ] || { echo "smoke: FAIL panel without a compositor"; ok=1; }
else
    [ "${PANELS:-0}" -ge 1 ] || { echo "smoke: FAIL no overlay panel"; ok=1; }
    [ "${FRAMES:-0}" -gt 0 ] || { echo "smoke: FAIL no frames"; ok=1; }
    [ "$AFTER" -gt "$((BEFORE + 2 * AIR + 50))" ] || { echo "smoke: FAIL no snow on the xterm ledge"; ok=1; }
fi
if [ "$BEHIND" = true ]; then
    # Top-level children have 5 spaces of indent; count from the top. The
    # lowest panel is the main one (the snow-only "above" panel stays on top).
    ORDER=$(awk '/^     0x/ {top++} /^     0x.*Frostpane overlay/ {p=top} /smoke-xterm/ && !x {x=top} END {print p+0, x+0}' "$OUT/tree.txt")
    echo "smoke: behind: lowest overlay / xterm frame position from the top: $ORDER"
    read -r PO XO <<< "$ORDER"
    [ "$PO" -gt 0 ] && [ "$XO" -gt 0 ] && [ "$PO" -gt "$XO" ] || { echo "smoke: FAIL overlay not below the xterm"; ok=1; }
fi
[ $ok -eq 0 ] && echo "smoke: PASS"
exit $ok
