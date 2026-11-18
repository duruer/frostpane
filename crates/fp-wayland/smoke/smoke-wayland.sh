#!/usr/bin/env bash
# End-to-end smoke of the Wayland backend on a headless sway (porting
# design 5.3): floating foot windows moved with swaymsg (sway IPC source),
# frostpane --seconds N, grim screenshots, then checks that frames were
# presented and that snow shows up on the foot windows' top edges.
#
# Usage: smoke-wayland.sh [path/to/frostpane] [seconds]
# Needs: sway, swaymsg, foot, grim, python3, dbus-run-session, a Vulkan
# driver (lavapipe from mesa-vulkan-drivers is used when present).
# Env: BEHIND=1 (behind-windows mode), FULLSCREEN=1, KEEP=1 (keep the temp
# dir), SMOKE_OUT=dir (copy screenshots and logs there), PROBE/SPRITES.
# Exit status 0 = pass. Only processes this script started are stopped.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
target="${CARGO_TARGET_DIR:-$repo/target}"
bin="${1:-$target/debug/frostpane}"
secs="${2:-25}"
probe="${PROBE:-$target/debug/examples/wl_probe}"

for t in sway swaymsg foot grim python3 dbus-run-session; do
    command -v "$t" >/dev/null || { echo "missing: $t" >&2; exit 2; }
done
[ -x "$bin" ] || { echo "missing frostpane binary: $bin" >&2; exit 2; }

if [ -z "${FP_SMOKE_INNER:-}" ]; then
    export FP_SMOKE_INNER=1
    exec dbus-run-session -- "$0" "$bin" "$secs"
fi

tmp="$(mktemp -d)"
pids=()
cleanup() {
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
    wait 2>/dev/null || true
    if [ -n "${SMOKE_OUT:-}" ]; then
        mkdir -p "$SMOKE_OUT"
        cp "$tmp"/*.ppm "$tmp"/*.txt "$tmp"/*.log "$SMOKE_OUT"/ 2>/dev/null || true
    fi
    [ -n "${KEEP:-}" ] || rm -rf "$tmp"
}
trap cleanup EXIT

# Isolated session: no WSLg / host display, own runtime dir and data dir.
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK HYPRLAND_INSTANCE_SIGNATURE
export XDG_RUNTIME_DIR="$tmp/run"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export XDG_CURRENT_DESKTOP=sway XDG_SESSION_TYPE=wayland
export FROSTPANE_DATA_DIR="$tmp/data" FROSTPANE_LOG="${FROSTPANE_LOG:-info}"
mkdir -p "$tmp/bin" "$FROSTPANE_DATA_DIR"
# Snow, shown at once; BEHIND=1 tests behind-windows mode (bottom layer).
behind=false
[ -n "${BEHIND:-}" ] && behind=true
printf '{"weather":"snow","heavySnow":true,"seenWelcome":true,"behindWindows":%s,"matchWeather":false,"randomWeather":false,"matchSeason":false}\n' \
    "$behind" > "$FROSTPANE_DATA_DIR/settings.json"
for t in xdg-open gsettings feh; do
    printf '#!/bin/sh\necho "%s $*" >> "%s/shim.log"\n' "$t" "$tmp" > "$tmp/bin/$t"
    chmod +x "$tmp/bin/$t"
done
export PATH="$tmp/bin:$PATH"
lvp="$(find /usr/share/vulkan/icd.d -name "lvp_icd.*.json" 2>/dev/null | head -n1 || true)"
[ -n "$lvp" ] && export VK_ICD_FILENAMES="$lvp"

cat > "$tmp/sway.conf" <<'CONF'
output HEADLESS-1 resolution 1280x720 position 0 0 bg #203040 solid_color
default_border pixel 2
for_window [app_id="foot"] floating enable
# The app's own (hidden-but-mapped on Wayland) window would cover the foots.
for_window [app_id="^(?!foot$).*"] move scratchpad
bar {
    position bottom
    status_command while :; do echo smoke; sleep 60; done
}
CONF

WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
    sway -c "$tmp/sway.conf" >"$tmp/sway.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
    sock="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name "sway-ipc.*.sock" 2>/dev/null | head -n1 || true)"
    disp="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name "wayland-*" ! -name "*.lock" -printf "%f\n" 2>/dev/null | head -n1 || true)"
    [ -n "$sock" ] && [ -n "$disp" ] && break
    sleep 0.2
done
[ -n "${sock:-}" ] || { echo "sway did not start"; cat "$tmp/sway.log"; exit 1; }
export SWAYSOCK="$sock" WAYLAND_DISPLAY="$disp"
echo "sway up: $WAYLAND_DISPLAY"

# Two floating foot windows at known places.
swaymsg -q 'exec foot -T fp1 sleep 600'
swaymsg -q 'exec foot -T fp2 sleep 600'
for _ in $(seq 50); do
    n="$(swaymsg -t get_tree | grep -c '"app_id": "foot"' || true)"
    [ "$n" -ge 2 ] && break
    sleep 0.2
done
swaymsg -q '[title="^fp1$"] resize set 420 240, move position 120 220'
swaymsg -q '[title="^fp2$"] resize set 360 200, move position 700 320'
sleep 1

if [ -x "$probe" ]; then
    "$probe" sway | tee "$tmp/probe.txt"
    grep -q '^layer_shell=true' "$tmp/probe.txt" || { echo "FAIL: no layer shell on sway"; exit 1; }
    grep -q '^has_geometry=true' "$tmp/probe.txt" || { echo "FAIL: sway IPC source missing"; exit 1; }
    grep -q 'owner=foot' "$tmp/probe.txt" || { echo "FAIL: foot windows not listed"; exit 1; }
    # No window source: ground and panel only, reported in Limits.
    "$probe" none > "$tmp/probe-none.txt"
    grep -q 'no_window_geometry: true' "$tmp/probe-none.txt" || { echo "FAIL: ground-only Limits"; exit 1; }
    grep -q '^has_geometry=false' "$tmp/probe-none.txt" || { echo "FAIL: ground-only geometry"; exit 1; }
    echo "ground-only: ok"
fi

# Sprite surfaces: show, move, fade, hide one red disc (layer surface).
sprites="${SPRITES:-$target/debug/examples/wl_sprites}"
if [ -x "$sprites" ]; then
    coproc SP { "$sprites"; }
    for _ in 1 2 3 4; do
        read -r step <&"${SP[0]}"
        grim -t ppm "$tmp/sp-$step.ppm"
        echo >&"${SP[1]}"
    done
    wait "$SP_PID" || true
    python3 - "$tmp" <<'PY' || { echo "FAIL: sprite surfaces"; exit 1; }
import sys
def load(p):
    d = open(p, 'rb').read(); parts, i = [], 0
    while len(parts) < 4:
        while d[i:i+1].isspace(): i += 1
        j = i
        while not d[j:j+1].isspace(): j += 1
        parts.append(d[i:j]); i = j
    return int(parts[1]), d[i+1:]
def px(step, x, y):
    w, d = load(f"{sys.argv[1]}/sp-{step}.ppm"); o = (y * w + x) * 3
    return d[o], d[o+1], d[o+2]
red = lambda p: p[0] > 200 and p[1] < 60 and p[2] < 60
half = lambda p: 100 < p[0] < 190 and p[1] < 70
checks = {
    "shown at 300,300": red(px("shown", 332, 332)),
    "moved away": not red(px("moved", 332, 332)) and red(px("moved", 732, 232)),
    "faded to half": half(px("faded", 732, 232)),
    "hidden": not red(px("hidden", 732, 232)) and not half(px("hidden", 732, 232)),
}
for k, v in checks.items(): print(f"sprite {k}: {'ok' if v else 'FAIL'}")
sys.exit(0 if all(checks.values()) else 1)
PY
fi

grim -t ppm "$tmp/before.ppm"
"$bin" --quiet --seconds "$secs" >"$tmp/out.txt" 2>"$tmp/err.txt" &
app=$!
pids+=("$app")
if [ -n "${FULLSCREEN:-}" ]; then
    # A fullscreen window occludes the panels (bottom layer in behind mode):
    # the app must keep running and exit on time.
    sleep 3
    swaymsg -q '[title="^fp1$"] fullscreen enable'
    sleep 2.5
    # Thread states while occluded (diagnostics: a stuck UI thread shows here).
    for t in /proc/"$app"/task/*; do
        echo "thread $(cat "$t/comm" 2>/dev/null): $(cat "$t/wchan" 2>/dev/null) $(awk '{print $3}' "$t/stat" 2>/dev/null)"
    done > "$tmp/threads.txt"
    sleep 2.5
    swaymsg -q '[title="^fp1$"] fullscreen disable'
    sleep $((secs > 14 ? secs - 12 : 1))
else
    sleep $((secs > 6 ? secs - 4 : secs / 2))
fi
# Move one window during the run (drag synthesis + ledge rebuild).
swaymsg -q '[title="^fp2$"] move position 720 330'
sleep 1.5
grim -t ppm "$tmp/during.ppm"
for _ in $(seq $((secs + 20))); do
    kill -0 "$app" 2>/dev/null || break
    sleep 1
done
if kill -0 "$app" 2>/dev/null; then
    echo "FAIL: frostpane did not exit after --seconds $secs (hung)"
    exit 1
fi
wait "$app" || true
cat "$tmp/out.txt"
grep -E 'WARN|ERROR|panicked' "$tmp/err.txt" | head -n 20 || true

frames="$(sed -n 's/.* \([0-9][0-9]*\) frames presented.*/\1/p' "$tmp/out.txt" | tail -n1)"
panels="$(sed -n 's/.*, \([0-9][0-9]*\) overlay panel(s).*/\1/p' "$tmp/out.txt" | tail -n1)"
echo "panels=${panels:-?} frames=${frames:-?}"
[ "${panels:-0}" -ge 1 ] || { echo "FAIL: no overlay panel"; tail -n 40 "$tmp/err.txt"; exit 1; }
[ "${frames:-0}" -gt 0 ] || { echo "FAIL: no frames presented"; tail -n 40 "$tmp/err.txt"; exit 1; }

# Snow on the window tops: light pixels in the band right above each foot
# window that were not there before the run.
python3 - "$tmp/before.ppm" "$tmp/during.ppm" <<'PY'
import sys

def load(p):
    d = open(p, 'rb').read()
    parts, i = [], 0
    while len(parts) < 4:
        while d[i:i+1].isspace(): i += 1
        if d[i:i+1] == b'#':
            while d[i:i+1] != b'\n': i += 1
            continue
        j = i
        while not d[j:j+1].isspace(): j += 1
        parts.append(d[i:j]); i = j
    i += 1
    w, h = int(parts[1]), int(parts[2])
    return w, h, d[i:]

w, h, a = load(sys.argv[1])
_, _, b = load(sys.argv[2])
def light(px, x, y):
    o = (y * w + x) * 3
    r, g, bl = px[o], px[o+1], px[o+2]
    return r > 170 and g > 170 and bl > 170
def band(x0, x1, top):
    new = 0
    for y in range(max(0, top - 12), top + 2):
        for x in range(x0, x1):
            if light(b, x, y) and not light(a, x, y): new += 1
    return new
# fp1 at (120,220) 420 wide; fp2 moved to (720,330) 360 wide.
r1, r2 = band(120, 540, 220), band(720, 1080, 330)
changed = sum(1 for i in range(0, len(a), 3) if a[i:i+3] != b[i:i+3])
print(f"ledge pixels: fp1={r1} fp2={r2}; changed pixels overall={changed}")
sys.exit(0 if (r1 > 20 or r2 > 20) else 1)
PY
echo "PASS"
