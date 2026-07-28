#!/usr/bin/env bash
# Behavioural coverage for the Wayland aiming harness (#690).
#
# `aim.py` is what turns the app's geometry record into real pointer input, and its whole
# value is that a round it reports as delivered was delivered. That cannot be proved by a
# display-backed smoke alone - the interesting cases are the ones where delivery fails -
# so this drives the real script against a stub injector that plays the compositor and the
# app: it moves a pointer in a global space whose origin the harness is never told, only
# reports motion that actually reaches the window, and records presses.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
AIM="$ROOT/scripts/wayland-drag-harness/aim.py"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { echo "FAIL: $1" >&2; exit 1; }

mkdir -p "$WORK/bin"
cat >"$WORK/bin/ydotool" <<'STUB'
#!/usr/bin/env python3
"""Stub injector: moves a pointer in a global space, delivers to the window, records clicks."""
import os
import sys

log = os.environ["OKP_TEST_LOG"]
pos = os.environ["OKP_TEST_POS"]
clicks = os.environ["OKP_TEST_CLICKS"]
moves = os.environ["OKP_TEST_MOVES"]
origin_x = float(os.environ.get("OKP_TEST_ORIGIN_X", "0"))
origin_y = float(os.environ.get("OKP_TEST_ORIGIN_Y", "0"))
silent = os.environ.get("OKP_TEST_SILENT") == "1"

DERIVED = {"window", "pointer", "drag-target"}


def record():
    """Newest geometry record: part -> fields, in emission order."""
    planes = {}
    seq = None
    with open(log) as handle:
        for line in handle:
            if not line.startswith("interaction: geometry "):
                continue
            fields = dict(t.split("=", 1) for t in line.split() if "=" in t)
            part = fields.get("part")
            if not part or part == "pointer":
                continue
            if fields.get("seq") != seq:
                seq = fields.get("seq")
                planes = {}
            planes[part] = fields
    return planes, seq


args = sys.argv[1:]
if args and args[0] == "mousemove":
    x = float(args[args.index("-x") + 1])
    y = float(args[args.index("-y") + 1])
    with open(moves, "a") as handle:
        handle.write(f"{x} {y}\n")
    with open(pos, "w") as handle:
        handle.write(f"{x} {y}\n")
    if silent:
        sys.exit(0)
    planes, seq = record()
    window = planes.get("window")
    if not window:
        sys.exit(0)
    local_x, local_y = x - origin_x, y - origin_y
    width, height = float(window["w"]), float(window["h"])
    if not (0 <= local_x < width and 0 <= local_y < height):
        # Motion that misses the window reaches no client, so the app says nothing.
        sys.exit(0)
    over = "unknown"
    for part, fields in planes.items():
        if part in DERIVED or fields.get("interactive") != "1":
            continue
        px, py = float(fields["local-x"]), float(fields["local-y"])
        if px <= local_x < px + float(fields["w"]) and py <= local_y < py + float(fields["h"]):
            over = part
    with open(log, "a") as handle:
        handle.write(
            f"interaction: geometry part=pointer reason=pointer seq={seq} "
            f"local-x={local_x:.1f} local-y={local_y:.1f} over={over} scale=1.000\n"
        )
elif args and args[0] == "click":
    with open(pos) as handle:
        where = handle.read().strip()
    with open(clicks, "a") as handle:
        handle.write(where + "\n")
sys.exit(0)
STUB
chmod +x "$WORK/bin/ydotool"
export PATH="$WORK/bin:$PATH"

windowed_record() {
  cat <<'RECORD'
interaction: geometry part=window reason=map seq=4 origin=unknown x=unknown y=unknown w=1120.0 h=680.0 scale=1.000 fullscreen=0 maximized=0 compact=0 monitor=TEST-1 monitor-x=0.0 monitor-y=0.0 monitor-w=1920.0 monitor-h=1080.0 monitor-scale=1.000 desktop-x=0.0 desktop-y=0.0 desktop-w=1920.0 desktop-h=1080.0 monitors=1
interaction: geometry part=video reason=map seq=4 local-x=0.0 local-y=0.0 w=1120.0 h=680.0 x=unknown y=unknown center-x=unknown center-y=unknown device-x=0.0 device-y=0.0 device-w=1120.0 device-h=680.0 interactive=1
interaction: geometry part=titlebar reason=map seq=4 local-x=0.0 local-y=0.0 w=1120.0 h=42.0 x=unknown y=unknown center-x=unknown center-y=unknown device-x=0.0 device-y=0.0 device-w=1120.0 device-h=42.0 interactive=1
interaction: geometry part=osc reason=map seq=4 local-x=0.0 local-y=590.0 w=1120.0 h=90.0 x=unknown y=unknown center-x=unknown center-y=unknown device-x=0.0 device-y=590.0 device-w=1120.0 device-h=90.0 interactive=1
RECORD
}

drag_target_line() {
  cat <<'RECORD'
interaction: geometry part=drag-target reason=map seq=4 local-x=0.0 local-y=42.0 w=1120.0 h=548.0 x=unknown y=unknown center-x=unknown center-y=unknown device-x=0.0 device-y=42.0 device-w=1120.0 device-h=548.0 interactive=1
RECORD
}

new_case() {
  local name="$1"
  CASE="$WORK/$name"
  mkdir -p "$CASE"
  export OKP_TEST_LOG="$CASE/app.log"
  export OKP_TEST_POS="$CASE/pos"
  export OKP_TEST_CLICKS="$CASE/clicks"
  export OKP_TEST_MOVES="$CASE/moves"
  : >"$OKP_TEST_CLICKS"
  : >"$OKP_TEST_MOVES"
  echo "0 0" >"$OKP_TEST_POS"
  unset OKP_TEST_SILENT
}

clicks() { wc -l <"$OKP_TEST_CLICKS" | tr -d ' '; }
moves() { wc -l <"$OKP_TEST_MOVES" | tr -d ' '; }

# 1. A windowed toplevel publishes no global coordinates. The harness must discover the
#    origin from the app's own pointer samples and press the drag target's centre:
#    local (560, 316) + origin (640, 231) = (1200, 547).
new_case verified
export OKP_TEST_ORIGIN_X=640 OKP_TEST_ORIGIN_Y=231
{ windowed_record; drag_target_line; } >"$OKP_TEST_LOG"
python3 "$AIM" --log "$OKP_TEST_LOG" click >"$CASE/out" 2>&1 \
  || fail "aiming at a reachable drag target must succeed: $(cat "$CASE/out")"
[[ "$(clicks)" == "1" ]] || fail "expected exactly one press, got $(clicks)"
[[ "$(cat "$OKP_TEST_CLICKS")" == "1200.0 547.0" ]] \
  || fail "pressed $(cat "$OKP_TEST_CLICKS"), expected the drag-target centre at 1200.0 547.0"
grep -q "over=video error=(0.0,0.0)" "$CASE/out" \
  || fail "the press must be preceded by a verified landing on the video plane"

# 2. A record that no longer describes the window - here a drag target that has drifted
#    into the OSC band - must abort. Delivery succeeds; the plane is simply not the one
#    asked for, which is exactly the round that used to be reported as delivered.
new_case wrong-plane
export OKP_TEST_ORIGIN_X=640 OKP_TEST_ORIGIN_Y=231
{
  windowed_record
  echo "interaction: geometry part=drag-target reason=map seq=4 local-x=0.0 local-y=600.0 w=1120.0 h=60.0 x=unknown y=unknown center-x=unknown center-y=unknown device-x=0.0 device-y=600.0 device-w=1120.0 device-h=60.0 interactive=1"
} >"$OKP_TEST_LOG"
python3 "$AIM" --log "$OKP_TEST_LOG" click >"$CASE/out" 2>&1 \
  && fail "aiming at a target that resolves to chrome must fail"
[[ "$(clicks)" == "0" ]] || fail "a rejected target must not be pressed, got $(clicks) presses"
grep -q "landed on osc, not video" "$CASE/out" \
  || fail "the refusal must name the plane that actually owns the point: $(cat "$CASE/out")"

# 3. Injection that never reaches the window must not be reported as a delivered round.
new_case undelivered
export OKP_TEST_ORIGIN_X=640 OKP_TEST_ORIGIN_Y=231 OKP_TEST_SILENT=1
{ windowed_record; drag_target_line; } >"$OKP_TEST_LOG"
python3 "$AIM" --log "$OKP_TEST_LOG" click >"$CASE/out" 2>&1 \
  && fail "aiming must fail when no injected motion reaches the window"
[[ "$(clicks)" == "0" ]] || fail "an unverifiable target must not be pressed"

# 4. A fullscreen record carries the aim point, so no calibration sweep may happen: one
#    verifying move, then the press.
new_case fullscreen
export OKP_TEST_ORIGIN_X=0 OKP_TEST_ORIGIN_Y=0
cat >"$OKP_TEST_LOG" <<'RECORD'
interaction: geometry part=window reason=fullscreen seq=9 origin=fullscreen-monitor x=0.0 y=0.0 w=1120.0 h=680.0 scale=1.000 fullscreen=1 maximized=0 compact=0 monitor=TEST-1 monitor-x=0.0 monitor-y=0.0 monitor-w=1920.0 monitor-h=1080.0 monitor-scale=1.000 desktop-x=0.0 desktop-y=0.0 desktop-w=1920.0 desktop-h=1080.0 monitors=1
interaction: geometry part=video reason=fullscreen seq=9 local-x=0.0 local-y=0.0 w=1120.0 h=680.0 x=0.0 y=0.0 center-x=560.0 center-y=340.0 device-x=0.0 device-y=0.0 device-w=1120.0 device-h=680.0 interactive=1
interaction: geometry part=osc reason=fullscreen seq=9 local-x=0.0 local-y=590.0 w=1120.0 h=90.0 x=0.0 y=590.0 center-x=560.0 center-y=635.0 device-x=0.0 device-y=590.0 device-w=1120.0 device-h=90.0 interactive=1
interaction: geometry part=drag-target reason=fullscreen seq=9 local-x=0.0 local-y=0.0 w=1120.0 h=590.0 x=0.0 y=0.0 center-x=560.0 center-y=295.0 device-x=0.0 device-y=0.0 device-w=1120.0 device-h=590.0 interactive=1
RECORD
python3 "$AIM" --log "$OKP_TEST_LOG" click >"$CASE/out" 2>&1 \
  || fail "a fullscreen record must aim without calibration: $(cat "$CASE/out")"
[[ "$(moves)" == "1" ]] || fail "expected one verifying move and no sweep, got $(moves)"
[[ "$(cat "$OKP_TEST_CLICKS")" == "560.0 295.0" ]] \
  || fail "pressed $(cat "$OKP_TEST_CLICKS"), expected the published aim point 560.0 295.0"

# 5. A record caught mid-write - the plane lines have not all landed yet - must be re-read,
#    not treated as a window with no drag target.
new_case mid-write
export OKP_TEST_ORIGIN_X=640 OKP_TEST_ORIGIN_Y=231
windowed_record >"$OKP_TEST_LOG"
( sleep 0.15; drag_target_line >>"$OKP_TEST_LOG" ) &
appender=$!
python3 "$AIM" --log "$OKP_TEST_LOG" click >"$CASE/out" 2>&1
result=$?
wait "$appender" 2>/dev/null
[[ "$result" == "0" ]] || fail "a record still being written must be re-read: $(cat "$CASE/out")"
[[ "$(cat "$OKP_TEST_CLICKS")" == "1200.0 547.0" ]] \
  || fail "pressed $(cat "$OKP_TEST_CLICKS") after the record completed, expected 1200.0 547.0"

echo "ok: the aiming harness resolves an unknown origin, refuses unverified presses, aims fullscreen without a sweep, and tolerates a record mid-write"
