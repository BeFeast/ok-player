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

  --rounds N       launches per clip (default 3; contract requires >= 3)
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
[[ "$ROUNDS" =~ ^[0-9]+$ && "$ROUNDS" -ge 1 ]] || { echo "--rounds must be a positive integer" >&2; exit 64; }

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
mkdir -p "$FIT_ROOT"
FIT_ROOT="$(cd "$FIT_ROOT" && pwd)"
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

echo "== headless gnome-shell (3840x2160 + 1920x1080)"
# The operator layout needs fractional scaling; the keyfile backend keeps the
# flag out of the host's dconf (see the harness README).
env "${SESSION_ENV[@]}" gsettings set org.gnome.mutter experimental-features "['scale-monitor-framebuffer']"
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

#-------------------------------------------------------------------- rounds --
# One launch: wait for the app to log its fit decision, then take it down.
# A launch that dies before any fit decision is not a fit verdict - that is
# the startup-crash class (#627, out of scope here), seen rarely on repeated
# back-to-back launches - so the round relaunches a bounded number of times
# before it becomes a harness fault.
run_round() {
  local clip="$1" log="$2"
  local attempt seen
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

# Every `window fit request` line in one log must be single-monitor-contained.
check_round() {
  local log="$1" label="$2"
  python3 - "$log" "$FIT_ROOT/layout.txt" "$label" <<'PY'
import re, sys

log_path, layout_path, label = sys.argv[1:4]
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
echo "PASS: all $total fits single-monitor-contained on the scaled dual-monitor layout (workarea never the union, bounds_source=current-monitor, zero deferred fits); evidence in $FIT_ROOT/logs"
