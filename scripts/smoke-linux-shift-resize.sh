#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-shift-resize-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo awk; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

mkdir -p "$OUT_DIR"

env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1600x1000x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
cleanup() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

OKP_SKIP_UPDATE_CHECK=1 OKP_SKIP_OPEN_INSTALLER=1 OKP_SKIP_DEB_SELF_INSTALL=1 \
  OKP_DEBUG_WINDOW_RESIZE=1 timeout 30s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in $(seq 1 100); do
  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
    width="$(awk '/Width:/ { print $2; exit }' <<<"$info")"
    height="$(awk '/Height:/ { print $2; exit }' <<<"$info")"
    state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$info")"
    if [[ "$width" == "1120" && "$height" == "680" && "$state" == "IsViewable" ]]; then
      window_id="$candidate"
      break 2
    fi
  done < <(xdotool search --name 'OK Player' 2>/dev/null || true)
  sleep 0.1
done
[[ -n "$window_id" ]] || { echo "Main window did not appear" >&2; exit 1; }
sleep 1

geometry() {
  xwininfo -id "$window_id" | awk '/Width:/ { w=$2 } /Height:/ { h=$2 } END { print w, h }'
}

read -r start_w start_h < <(geometry)
xdotool windowactivate --sync "$window_id"
xdotool mousemove --sync --window "$window_id" "$((start_w - 3))" "$((start_h - 3))"
sleep 0.2
xdotool keydown Shift_L
sleep 0.1
xdotool mousedown 1
sleep 0.2

previous_w=$start_w
previous_h=$start_h
: >"$OUT_DIR/locked-samples.txt"
for _ in 1 2 3 4; do
  xdotool mousemove_relative --sync -- -40 -24
  sleep 0.15
  read -r width height < <(geometry)
  printf '%s %s\n' "$width" "$height" >>"$OUT_DIR/locked-samples.txt"
  (( width < previous_w && height < previous_h )) || {
    echo "Locked resize was not monotonic: ${previous_w}x${previous_h} -> ${width}x${height}" >&2
    exit 1
  }
  awk -v w="$width" -v h="$height" -v sw="$start_w" -v sh="$start_h" \
    'BEGIN { expected=sw/sh; actual=w/h; d=actual-expected; if (d<0) d=-d; exit !(d <= 0.01) }' || {
      echo "Aspect drifted during locked resize: ${width}x${height}" >&2
      exit 1
    }
  previous_w=$width
  previous_h=$height
done

xdotool keyup Shift_L
xdotool mousemove_relative --sync -- 0 40
sleep 0.2
read -r released_w released_h < <(geometry)
xdotool mouseup 1
printf 'start=%sx%s\nlocked=%sx%s\nreleased=%sx%s\n' \
  "$start_w" "$start_h" "$previous_w" "$previous_h" "$released_w" "$released_h" \
  >"$OUT_DIR/geometry.txt"

(( released_w == previous_w && released_h > previous_h )) || {
  echo "Shift release did not continue freeform from the reached size: locked=${previous_w}x${previous_h} released=${released_w}x${released_h}" >&2
  exit 1
}
SMOKE

echo "Shift-resize X11 smoke passed: $OUT_DIR"
