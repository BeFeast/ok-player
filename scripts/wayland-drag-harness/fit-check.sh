#!/usr/bin/env bash
# Headless dual-monitor fit acceptance for #546: the player window must fit
# inside ONE monitor's workarea on the scaled dual-head GNOME Wayland layout,
# never the union of both heads.
#
# Reuses the harness environment: gnome-shell --headless with two virtual
# monitors (3840x2160 + 1920x1080), setscales.py applying the operator layout
# (scale 2.0 / ~1.67), a private XDG_RUNTIME_DIR and a private session bus so
# nothing touches the host's real session. No pointer input is needed - the
# assertions read the `window fit request:` records the app emits under
# OKP_DEBUG_WINDOW_FIT=1 from a live Wayland session.
#
# Per round the source-built player is launched with a clip; round 1 covers
# initial map + fit-to-media on load, and each later round re-opens against
# the same persisted HOME/XDG state (the re-open/second-launch shape). Every
# `window fit request` line in every round must satisfy:
#   (a) monitor_geometry is exactly one logical monitor of the applied layout
#       (a union betrays itself as wider than either head);
#   (b) workarea lies inside that single monitor's geometry;
#   (c) the placed window lies entirely inside that workarea;
#   (d) bounds_source is current-monitor;
#   (e) `window fit deferred: monitor workarea unavailable` never appears.
# The `window fit request/configure` records are only what the app ASKED for -
# they are emitted before set_default_size and never measured back, so per
# round the harness also reads the compositor-APPLIED frame rect and the
# per-monitor workareas straight out of Mutter (org.gnome.Shell.Eval on the
# private session, unsafe-mode enabled by a throwaway extension in the private
# HOME) and asserts:
#   (f) the applied window frame rect is contained in a single logical
#       monitor's workarea - so a compositor that ignores or re-constrains the
#       request cannot turn a spanning window into a PASS.
set -euo pipefail

HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd -- "$HERE/../.." && pwd)"

# /var/tmp, not /tmp: rounds keep full app logs and generated clips around as
# evidence, and on tmpfs hosts that is RAM.
FIT_ROOT="${OKP_FIT_ROOT:-/var/tmp/okp-fit-check}"
ROUNDS=3
CLIP_16X9=""
CLIP_4K=""
BINARY=""
WL_NAME="okp-fit-wl"

usage() {
  cat >&2 <<'EOF'
usage: fit-check.sh [--rounds N] [--clip-16x9 PATH] [--clip-4k PATH]
                    [--binary PATH] [--root DIR]

  --rounds N       launches per clip (default 3; the contract requires >= 3,
                   smaller values are rejected)
  --clip-16x9 P    16:9 fixture (default: ffmpeg lavfi 1920x1080, generated)
  --clip-4k P      4K fixture   (default: ffmpeg lavfi 3840x2160, generated)
  --binary P       player binary (default: cargo build --release -p okp-linux-gtk)
  --root DIR       run root for the private session + evidence
                   (default /var/tmp/okp-fit-check, env OKP_FIT_ROOT)

Exit 0 = every fit single-monitor-contained. 1 = a fit assertion failed
(the #546 defect). 2 = harness fault (environment, not the app).
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --rounds) ROUNDS="${2:?}"; shift 2 ;;
    --clip-16x9) CLIP_16X9="${2:?}"; shift 2 ;;
    --clip-4k) CLIP_4K="${2:?}"; shift 2 ;;
    --binary) BINARY="${2:?}"; shift 2 ;;
    --root) FIT_ROOT="${2:?}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 64 ;;
  esac
done
# The acceptance contract requires at least three launches per clip; a
# shortened run must not be mistakable for contract-compliant evidence.
[[ "$ROUNDS" =~ ^[0-9]+$ && "$ROUNDS" -ge 3 ]] || { echo "--rounds must be an integer >= 3 (the acceptance contract requires at least three launches per clip)" >&2; exit 64; }

for tool in gnome-shell dbus-daemon python3 ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing required tool: $tool" >&2; exit 127; }
done
python3 -c 'import gi' 2>/dev/null || { echo "python3-gi is required (setscales.py, layout query)" >&2; exit 127; }

#------------------------------------------------------------------- binary --
if [[ -z "$BINARY" ]]; then
  command -v cargo >/dev/null 2>&1 || { echo "no --binary given and cargo is missing" >&2; exit 127; }
  echo "== building okp-linux-gtk (release)"
  cargo build --release -p okp-linux-gtk --manifest-path "$REPO/rust/Cargo.toml"
  BINARY="$REPO/rust/target/release/okp-linux-gtk"
fi
[[ -x "$BINARY" ]] || { echo "missing or non-executable player binary: $BINARY" >&2; exit 64; }

#--------------------------------------------------------------- private env --
# Fresh session state per run, but generated clips are reusable.
#
# The cleanup below is recursive, so refuse to point it at anything that is
# not a dedicated harness-owned directory: system/shared roots are rejected
# outright, and any other pre-existing non-empty directory must carry the
# marker file this script drops on first use. A typo like `--root /var/tmp/..`
# therefore cannot delete unrelated data.
FIT_ROOT_MARKER=".okp-fit-check-root"
mkdir -p "$FIT_ROOT"
FIT_ROOT="$(cd "$FIT_ROOT" && pwd)"
case "$FIT_ROOT" in
  /|/bin|/boot|/dev|/etc|/home|/lib|/lib64|/media|/mnt|/opt|/proc|/root|/run|/srv|/sys|/tmp|/usr|/var|/var/tmp|/workspace|"$HOME")
    echo "refusing unsafe --root $FIT_ROOT: use a dedicated subdirectory (e.g. /var/tmp/okp-fit-check)" >&2
    exit 64 ;;
esac
if [[ ! -e "$FIT_ROOT/$FIT_ROOT_MARKER" ]]; then
  if [[ -n "$(ls -A "$FIT_ROOT")" ]]; then
    { echo "refusing to clean $FIT_ROOT: it has content but no $FIT_ROOT_MARKER marker,"
      echo "so it does not look harness-owned. Point --root at a fresh or dedicated"
      echo "directory (or, if this really is a fit-check root, touch $FIT_ROOT/$FIT_ROOT_MARKER)."
    } >&2
    exit 64
  fi
  touch "$FIT_ROOT/$FIT_ROOT_MARKER"
fi
rm -rf "$FIT_ROOT/home" "$FIT_ROOT/xdg" "$FIT_ROOT/logs" "$FIT_ROOT/bus" "$FIT_ROOT/layout.txt"
mkdir -p "$FIT_ROOT/home/.config" "$FIT_ROOT/logs"
mkdir -p -m 700 "$FIT_ROOT/xdg"

SESSION_ENV=(
  HOME="$FIT_ROOT/home"
  XDG_CONFIG_HOME="$FIT_ROOT/home/.config"
  XDG_STATE_HOME="$FIT_ROOT/home/.local/state"
  XDG_CACHE_HOME="$FIT_ROOT/home/.cache"
  XDG_DATA_HOME="$FIT_ROOT/home/.local/share"
  XDG_RUNTIME_DIR="$FIT_ROOT/xdg"
  GSETTINGS_BACKEND=keyfile
  DBUS_SESSION_BUS_ADDRESS="unix:path=$FIT_ROOT/bus"
)

DBUS_PID=""
SHELL_PID=""
APP_PID=""
cleanup() {
  local status=$?
  [[ -n "$APP_PID" ]] && kill "$APP_PID" 2>/dev/null || true
  [[ -n "$SHELL_PID" ]] && kill "$SHELL_PID" 2>/dev/null || true
  [[ -n "$DBUS_PID" ]] && kill "$DBUS_PID" 2>/dev/null || true
  return $status
}
trap cleanup EXIT

echo "== private session bus"
dbus-daemon --session --address="unix:path=$FIT_ROOT/bus" --fork --print-pid >"$FIT_ROOT/logs/dbus.pid" 2>/dev/null
DBUS_PID="$(cat "$FIT_ROOT/logs/dbus.pid")"

# The applied-geometry assertion reads frame rects via org.gnome.Shell.Eval,
# which only answers in unsafe mode; recent shells dropped the --unsafe-mode
# flag, so a throwaway extension in the PRIVATE home flips it at startup. It
# never touches the host's real session or extensions.
UNSAFE_UUID="okp-fit-unsafe@fit-check"
EXT_DIR="$FIT_ROOT/home/.local/share/gnome-shell/extensions/$UNSAFE_UUID"
SHELL_MAJOR="$(gnome-shell --version | grep -oE '[0-9]+' | head -1)"
mkdir -p "$EXT_DIR"
cat >"$EXT_DIR/metadata.json" <<EOF
{
  "uuid": "$UNSAFE_UUID",
  "name": "okp fit-check unsafe mode",
  "description": "Enable unsafe mode so fit-check.sh can read compositor-applied window geometry via Shell.Eval.",
  "shell-version": ["$SHELL_MAJOR"],
  "url": ""
}
EOF
cat >"$EXT_DIR/extension.js" <<'EOF'
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

export default class OkpFitUnsafeMode extends Extension {
    enable() {
        global.context.unsafe_mode = true;
    }

    disable() {
        global.context.unsafe_mode = false;
    }
}
EOF

echo "== headless gnome-shell (3840x2160 + 1920x1080)"
# The operator layout needs fractional scaling; the keyfile backend keeps the
# flag out of the host's dconf (see the harness README).
env "${SESSION_ENV[@]}" gsettings set org.gnome.mutter experimental-features "['scale-monitor-framebuffer']"
env "${SESSION_ENV[@]}" gsettings set org.gnome.shell disable-extension-version-validation true
env "${SESSION_ENV[@]}" gsettings set org.gnome.shell enabled-extensions "['$UNSAFE_UUID']"
env "${SESSION_ENV[@]}" gnome-shell --headless --wayland-display "$WL_NAME" \
  --virtual-monitor 3840x2160 --virtual-monitor 1920x1080 \
  >"$FIT_ROOT/logs/shell.log" 2>&1 &
SHELL_PID=$!
for _ in $(seq 1 100); do [[ -S "$FIT_ROOT/xdg/$WL_NAME" ]] && break; sleep 0.2; done
[[ -S "$FIT_ROOT/xdg/$WL_NAME" ]] || { echo "gnome-shell never created $WL_NAME (see $FIT_ROOT/logs/shell.log)" >&2; exit 2; }

echo "== operator scale layout (setscales.py)"
applied=0
for _ in $(seq 1 20); do
  if env "${SESSION_ENV[@]}" OKP_BUS="unix:path=$FIT_ROOT/bus" \
      python3 "$HERE/setscales.py" >"$FIT_ROOT/logs/setscales.log" 2>&1; then
    applied=1; break
  fi
  sleep 0.5
done
[[ "$applied" == 1 ]] || { echo "setscales.py never applied the layout (see $FIT_ROOT/logs/setscales.log)" >&2; exit 2; }

# Read the applied logical layout back out of DisplayConfig - the assertion
# needs the real per-head rectangles, not this script's expectations.
env "${SESSION_ENV[@]}" OKP_BUS="unix:path=$FIT_ROOT/bus" python3 - >"$FIT_ROOT/layout.txt" <<'PY'
import os
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio

bus = Gio.DBusConnection.new_for_address_sync(
    os.environ["OKP_BUS"],
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
    | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
    None, None)
state = bus.call_sync(
    "org.gnome.Mutter.DisplayConfig", "/org/gnome/Mutter/DisplayConfig",
    "org.gnome.Mutter.DisplayConfig", "GetCurrentState",
    None, None, 0, -1, None).unpack()
_, monitors, logical_monitors, _ = state
current = {}
for spec, modes, _props in monitors:
    for mode_id, w, h, _rate, _pscale, _sscales, mprops in modes:
        if mprops.get("is-current"):
            current[spec[0]] = (w, h)
for x, y, scale, _transform, _primary, assigned, _props in logical_monitors:
    w, h = current[assigned[0][0]]
    print(f"{round(w / scale)}x{round(h / scale)}+{x},{y}")
PY
echo "logical monitors:"
sed 's/^/  /' "$FIT_ROOT/layout.txt"
[[ "$(wc -l <"$FIT_ROOT/layout.txt")" -ge 2 ]] || { echo "expected two logical monitors after setscales" >&2; exit 2; }

#------------------------------------------------------------------ fixtures --
gen_clip() {
  local out="$1" size="$2"
  [[ -f "$out" ]] && return 0
  echo "== generating $size fixture: $out"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "testsrc2=size=$size:rate=30" -f lavfi -i sine -t 30 \
    -c:v libx264 -preset ultrafast -crf 30 -c:a aac "$out"
}
[[ -n "$CLIP_16X9" ]] || { CLIP_16X9="$FIT_ROOT/clip-16x9.mkv"; gen_clip "$CLIP_16X9" 1920x1080; }
[[ -n "$CLIP_4K" ]] || { CLIP_4K="$FIT_ROOT/clip-4k.mkv"; gen_clip "$CLIP_4K" 3840x2160; }
[[ -f "$CLIP_16X9" ]] || { echo "missing 16:9 clip: $CLIP_16X9" >&2; exit 64; }
[[ -f "$CLIP_4K" ]] || { echo "missing 4K clip: $CLIP_4K" >&2; exit 64; }

#-------------------------------------------------------------- applied geo --
# Snapshot what the compositor ACTUALLY applied: mapped frame rects plus the
# per-monitor geometries/workareas, straight out of Mutter via Shell.Eval.
# Polls until an app window exists and its frame rect is stable across two
# consecutive reads, so a mid-resize/mid-placement transient is never the
# evidence. Exit 0 = stable snapshot written; 3 = timed out waiting for a
# stable app window; anything else = Eval/harness fault.
capture_compositor_state() {
  local out="$1"
  env "${SESSION_ENV[@]}" python3 - "$out" <<'PY'
import json, os, sys, time
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

JS = """
JSON.stringify((() => {
    const ws = global.workspace_manager.get_active_workspace();
    const monitors = [];
    for (let i = 0; i < global.display.get_n_monitors(); i++) {
        const g = global.display.get_monitor_geometry(i);
        const w = ws.get_work_area_for_monitor(i);
        monitors.push({geometry: [g.width, g.height, g.x, g.y],
                       workarea: [w.width, w.height, w.x, w.y]});
    }
    const windows = global.get_window_actors().map(a => {
        const mw = a.meta_window;
        const r = mw.get_frame_rect();
        return {wm_class: mw.get_wm_class(), title: mw.get_title(),
                rect: [r.width, r.height, r.x, r.y]};
    });
    return {monitors, windows};
})())
"""

bus = Gio.DBusConnection.new_for_address_sync(
    os.environ["DBUS_SESSION_BUS_ADDRESS"],
    Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT
    | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
    None, None)
def snapshot():
    ok, result = bus.call_sync(
        "org.gnome.Shell", "/org/gnome/Shell", "org.gnome.Shell", "Eval",
        GLib.Variant("(s)", (JS,)), None, 0, -1, None).unpack()
    if not ok:
        print(f"Shell.Eval failed (unsafe mode not active?): {result}", file=sys.stderr)
        sys.exit(1)
    # Eval JSON-encodes its return value, and the JS stringifies too on some
    # shell versions - unwrap until the object comes out.
    state = json.loads(result)
    if isinstance(state, str):
        state = json.loads(state)
    return state

deadline = time.monotonic() + 12
prev_rects = None
state = None
while time.monotonic() < deadline:
    state = snapshot()
    rects = sorted(tuple(w["rect"]) for w in state["windows"]
                   if "okplayer" in (w["wm_class"] or "").lower())
    if rects and rects == prev_rects:
        with open(sys.argv[1], "w") as f:
            json.dump(state, f)
        sys.exit(0)
    prev_rects = rects
    time.sleep(0.3)
if state is not None:
    with open(sys.argv[1], "w") as f:
        json.dump(state, f)
sys.exit(3)
PY
}

#-------------------------------------------------------------------- rounds --
# One launch: wait for the app to log its fit decision, then take it down.
# A launch that dies before any fit decision is not a fit verdict - that is
# the startup-crash class (#627, out of scope here), seen rarely on repeated
# back-to-back launches - so the round relaunches a bounded number of times
# before it becomes a harness fault.
run_round() {
  local clip="$1" log="$2"
  local attempt seen rc
  for attempt in 1 2 3; do
    env "${SESSION_ENV[@]}" \
      WAYLAND_DISPLAY="$WL_NAME" GDK_BACKEND=wayland OKP_DEBUG_WINDOW_FIT=1 \
      "$BINARY" "$clip" >"$log" 2>&1 &
    APP_PID=$!
    seen=0
    for _ in $(seq 1 240); do
      if grep -Eq 'window fit (request|deferred)' "$log" 2>/dev/null; then seen=1; break; fi
      kill -0 "$APP_PID" 2>/dev/null || break
      sleep 0.25
    done
    # Let the paired `window fit configure` land before tearing the app down.
    sleep 1
    if [[ "$seen" == 1 ]]; then
      # The fit records are only the request side; while the window is still
      # mapped, snapshot the geometry the compositor actually applied.
      rm -f "$log.compositor.json"
      rc=0; capture_compositor_state "$log.compositor.json" || rc=$?
      if [[ "$rc" != 0 ]]; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
        APP_PID=""
        echo "HARNESS FAULT: could not read the compositor-applied window geometry (rc=$rc; Shell.Eval via $UNSAFE_UUID)" >&2
        exit 2
      fi
    fi
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
    APP_PID=""
    [[ "$seen" == 1 ]] && return 0
    if ! kill -0 "$SHELL_PID" 2>/dev/null; then
      echo "HARNESS FAULT: gnome-shell died mid-run (see $FIT_ROOT/logs/shell.log)" >&2
      exit 2
    fi
    cp "$log" "$log.attempt$attempt" 2>/dev/null || true
    echo "  launch attempt $attempt exited before a fit decision; relaunching (kept $log.attempt$attempt)"
    sleep 2
  done
  echo "HARNESS FAULT: the player never logged a fit decision in 3 launches (see $log)" >&2
  exit 2
}

# Every `window fit request` line in one log must be single-monitor-contained,
# and the compositor-applied frame rect (captured while the window was mapped)
# must land inside a single monitor's workarea too - the request alone proves
# nothing about what GTK/Mutter actually did with it.
check_round() {
  local log="$1" label="$2"
  python3 - "$log" "$FIT_ROOT/layout.txt" "$label" "$log.compositor.json" <<'PY'
import json, re, sys

log_path, layout_path, label, comp_path = sys.argv[1:5]
heads = []
for line in open(layout_path):
    m = re.match(r"(\d+)x(\d+)\+(-?\d+),(-?\d+)", line.strip())
    if m:
        heads.append(tuple(int(v) for v in m.groups()))  # (w, h, x, y)

def fail(msg):
    print(f"FAIL [{label}]: {msg}")
    sys.exit(1)

log = open(log_path, errors="replace").read()
if "window fit deferred: monitor workarea unavailable" in log:
    fail("the fit was deferred - monitor workarea unavailable")

pattern = re.compile(
    r"window fit request: video=(\d+)x(\d+) scale=([0-9.]+) monitor=(\S+) "
    r"monitor_geometry=(\d+)x(\d+)\+(-?\d+),(-?\d+) "
    r"workarea=(\d+)x(\d+)\+(-?\d+),(-?\d+) "
    r"window=(\d+)x(\d+)\+(-?\d+),(-?\d+) bounds_source=(\S+)")
fits = pattern.findall(log)
if not fits:
    fail("no `window fit request` record in the log")

def contains(outer, inner):
    ow, oh, ox, oy = outer
    iw, ih, ix, iy = inner
    return ix >= ox and iy >= oy and ix + iw <= ox + ow and iy + ih <= oy + oh

for f in fits:
    vw, vh = int(f[0]), int(f[1])
    monitor = f[3]
    geo = (int(f[4]), int(f[5]), int(f[6]), int(f[7]))
    work = (int(f[8]), int(f[9]), int(f[10]), int(f[11]))
    win = (int(f[12]), int(f[13]), int(f[14]), int(f[15]))
    source = f[16]
    if geo not in heads:
        fail(f"monitor_geometry {geo[0]}x{geo[1]}+{geo[2]},{geo[3]} is not a single "
             f"logical monitor of the applied layout {heads} - a union/spanning fit")
    if not contains(geo, work):
        fail(f"workarea {work} leaks outside monitor {monitor} geometry {geo} - "
             "not a single monitor's workarea")
    if not contains(work, win):
        fail(f"window {win[0]}x{win[1]}+{win[2]},{win[3]} is not contained in "
             f"workarea {work[0]}x{work[1]}+{work[2]},{work[3]} on {monitor}")
    if source != "current-monitor":
        fail(f"bounds_source={source}, expected current-monitor")
    print(f"  {label}: video={vw}x{vh} window={win[0]}x{win[1]}+{win[2]},{win[3]} "
          f"inside workarea={work[0]}x{work[1]}+{work[2]},{work[3]} on {monitor} "
          f"(bounds_source={source})")

# The requested rectangle passing is necessary but not sufficient: assert the
# geometry the compositor APPLIED (Mutter frame rect, in the same logical
# coordinate space as Mutter's own monitor geometries/workareas).
comp = json.load(open(comp_path))
app_windows = [w for w in comp["windows"]
               if "okplayer" in (w["wm_class"] or "").lower()]
if not app_windows:
    fail("no compositor-side app window in the applied-geometry snapshot")
for w in app_windows:
    rect = tuple(w["rect"])  # (w, h, x, y), same shape as heads
    homes = [m for m in comp["monitors"] if contains(tuple(m["geometry"]), rect)]
    if not homes:
        fail(f"compositor-APPLIED window {rect[0]}x{rect[1]}+{rect[2]},{rect[3]} is not "
             f"contained in any single monitor geometry "
             f"{[tuple(m['geometry']) for m in comp['monitors']]} - the applied window "
             "spans monitors or overflows (#546), whatever the request said")
    if not any(contains(tuple(m["workarea"]), rect) for m in homes):
        fail(f"compositor-APPLIED window {rect[0]}x{rect[1]}+{rect[2]},{rect[3]} escapes "
             f"its monitor's workarea {[tuple(m['workarea']) for m in homes]}")
    print(f"  {label}: compositor applied window={rect[0]}x{rect[1]}+{rect[2]},{rect[3]} "
          "contained in a single monitor workarea")
PY
}

failures=0
total=0
for kind in 16x9 4k; do
  clip="$CLIP_16X9"; [[ "$kind" == 4k ]] && clip="$CLIP_4K"
  for r in $(seq 1 "$ROUNDS"); do
    shape="re-open"; [[ "$r" == 1 ]] && shape="initial map + fit-to-media"
    log="$FIT_ROOT/logs/fit-$kind-$r.log"
    echo "== round $r/$ROUNDS ($kind, $shape)"
    run_round "$clip" "$log"
    total=$((total + 1))
    check_round "$log" "$kind round $r" || failures=$((failures + 1))
  done
done

echo
if (( failures > 0 )); then
  echo "FAIL: $failures of $total rounds produced a spanning/uncontained fit (#546 defect); evidence in $FIT_ROOT/logs"
  exit 1
fi
echo "PASS: all $total fits single-monitor-contained on the scaled dual-monitor layout (requested AND compositor-applied geometry, workarea never the union, bounds_source=current-monitor, zero deferred fits); evidence in $FIT_ROOT/logs"
