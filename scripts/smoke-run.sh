#!/usr/bin/env bash
# Generic smoke run: start frostpane for N seconds, assert a clean exit and
# inspect the report line it prints on exit:
#   frostpane: exit after <ticks> ticks, <P> overlay panel(s), <F> frames presented
#
# Usage: smoke-run.sh [-s SECONDS] [-b BINARY] [-l LOGFILE]
# Environment:
#   SMOKE_REQUIRE_FRAMES=1  fail unless frames > 0 (a real backend must draw)
#   SMOKE_MIN_PANELS=N      fail unless panels >= N (default 0)
#   A session served by the Null overlay logs "no overlay backend" and presents
#   nothing; that is accepted unless SMOKE_REQUIRE_FRAMES=1.
#   SMOKE_POST=cmd          optional command run ~half way through the run
#                           (e.g. a screenshot); its failure fails the smoke.
# Extra env (FROSTPANE_BACKEND, DISPLAY, ...) passes straight through.
set -euo pipefail

secs=8
bin=""
log=""
while getopts "s:b:l:" o; do
  case "$o" in
    s) secs="$OPTARG" ;;
    b) bin="$OPTARG" ;;
    l) log="$OPTARG" ;;
    *) echo "usage: $0 [-s seconds] [-b binary] [-l logfile]" >&2; exit 2 ;;
  esac
done

root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -z "$bin" ]; then
  for c in "$root/target/release/frostpane" "$root/target/debug/frostpane" \
           "${CARGO_TARGET_DIR:-}/release/frostpane" "${CARGO_TARGET_DIR:-}/debug/frostpane"; do
    if [ -x "$c" ]; then bin="$c"; break; fi
  done
fi
[ -x "$bin" ] || { echo "smoke: no frostpane binary (pass -b)" >&2; exit 2; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
log="${log:-$work/run.log}"
export FROSTPANE_DATA_DIR="$work/data"
export FROSTPANE_LOG="${FROSTPANE_LOG:-info}"
mkdir -p "$FROSTPANE_DATA_DIR"

echo "smoke: $bin --seconds $secs"
rc=0
post_pid=""
if [ -n "${SMOKE_POST:-}" ]; then
  ( sleep $((secs / 2)); bash -c "$SMOKE_POST" ) &
  post_pid=$!
fi
# Hard limit well above --seconds so a hang fails instead of stalling CI.
timeout $((secs + 45)) "$bin" --seconds "$secs" >"$log" 2>&1 || rc=$?
if [ -n "$post_pid" ]; then
  wait "$post_pid" || { echo "smoke: SMOKE_POST failed" >&2; sed 's/^/  | /' "$log" | tail -40; exit 1; }
fi
sed 's/^/  | /' "$log" | tail -40

if [ "$rc" -ne 0 ]; then
  echo "smoke: FAIL exit code $rc (124 = timeout)" >&2
  exit 1
fi
if grep -Eq "panicked at|stack overflow|SIGSEGV|Segmentation fault" "$log"; then
  echo "smoke: FAIL crash marker in log" >&2
  exit 1
fi

line="$(grep -E "^frostpane: exit after" "$log" | tail -1 || true)"
if [ -z "$line" ]; then
  echo "smoke: FAIL no exit report line" >&2
  exit 1
fi
panels="$(sed -n 's/.*, \([0-9][0-9]*\) overlay panel.*/\1/p' <<<"$line")"
frames="$(sed -n 's/.*, \([0-9][0-9]*\) frames presented.*/\1/p' <<<"$line")"
panels="${panels:-0}"
frames="${frames:-0}"
echo "smoke: panels=$panels frames=$frames"

if grep -q "no overlay backend" "$log"; then
  echo "smoke: backend is a Null placeholder"
  if [ "${SMOKE_REQUIRE_FRAMES:-0}" = "1" ]; then
    echo "smoke: FAIL frames required but backend is Null" >&2
    exit 1
  fi
  exit 0
fi
if [ "${SMOKE_REQUIRE_FRAMES:-0}" = "1" ] && [ "$frames" -le 0 ]; then
  echo "smoke: FAIL no frames presented" >&2
  exit 1
fi
if [ "$panels" -lt "${SMOKE_MIN_PANELS:-0}" ]; then
  echo "smoke: FAIL panels $panels < ${SMOKE_MIN_PANELS:-0}" >&2
  exit 1
fi
echo "smoke: OK"
