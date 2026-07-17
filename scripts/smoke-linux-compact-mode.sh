#!/usr/bin/env bash
# Deterministic X11/Xvfb coverage for entering, resizing, and restoring the
# compact mini-player without replacing the active mpv/render surface.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-compact-mode-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo xprop import magick rg awk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }
mkdir -p "$OUT_DIR"

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$FIXTURE" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_START_COMPACT=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_PLAYBACK_FRAME_PREVIEW="${OKP_COMPACT_PREVIEW_SUBSTRATE:-bright}"
export OKP_DEBUG_INTERACTIONS=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_SKIP_UPDATE_CHECK=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

timeout 45s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

window_id=""
compact_width=""
compact_height=""
for _ in $(seq 1 120); do
  window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | tail -n1 || true)"
  if [[ -n "$window_id" ]]; then
    compact_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
    compact_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
    [[ "$compact_width" == 480 && "$compact_height" == 270 ]] && break
  fi
  sleep 0.1
done
[[ -n "$window_id" ]] || { cat "$OUT_DIR/app.log" >&2; exit 1; }
[[ "$compact_width" == 480 && "$compact_height" == 270 ]] || {
  echo "compact geometry is ${compact_width}x${compact_height}, expected 480x270" >&2
  exit 1
}

xdotool windowmove "$window_id" 40 40
xdotool mousemove 0 0 mousemove --window "$window_id" 240 76
sleep 0.4
xwininfo -id "$window_id" >"$OUT_DIR/compact.xwininfo"
xprop -id "$window_id" _NET_WM_STATE >"$OUT_DIR/compact.xprop"
import -window root "$OUT_DIR/compact-root.png"
magick "$OUT_DIR/compact-root.png" -crop 480x270+40+40 +repage "$OUT_DIR/compact-hover.png"

rg -q '_NET_WM_STATE_ABOVE' "$OUT_DIR/compact.xprop" || {
  echo "compact window is not marked always-on-top" >&2
  exit 1
}

frame_mean="$(magick "$OUT_DIR/compact-hover.png" -crop 300x110+90+80 -colorspace gray -format '%[fx:mean]' info:)"
top_max="$(magick "$OUT_DIR/compact-hover.png" -crop 456x44+12+8 -colorspace gray -format '%[fx:maxima]' info:)"
bottom_max="$(magick "$OUT_DIR/compact-hover.png" -crop 456x42+12+218 -colorspace gray -format '%[fx:maxima]' info:)"
if [[ "$OKP_PLAYBACK_FRAME_PREVIEW" == bright ]]; then
  awk -v value="$frame_mean" 'BEGIN { exit !(value > 0.70) }' || {
    echo "bright compact video substrate was not visible: $frame_mean" >&2
    exit 1
  }
else
  awk -v value="$frame_mean" 'BEGIN { exit !(value < 0.15) }' || {
    echo "dark compact video substrate was not visible: $frame_mean" >&2
    exit 1
  }
fi
awk -v top="$top_max" -v bottom="$bottom_max" 'BEGIN { exit !(top > 0.55 && bottom > 0.55) }' || {
  echo "compact title/transport chrome was not visible: top=$top_max bottom=$bottom_max" >&2
  exit 1
}

xdotool mousemove --window "$window_id" 240 82 mousedown 1 sleep 0.15 \
  mousemove_relative --sync -- -18 -18 sleep 0.15 \
  mousemove_relative --sync -- -28 -28 sleep 0.15 mouseup 1
for _ in $(seq 1 30); do
  snap_x="$(xwininfo -id "$window_id" | awk '/Absolute upper-left X:/ {print $4; exit}')"
  snap_y="$(xwininfo -id "$window_id" | awk '/Absolute upper-left Y:/ {print $4; exit}')"
  [[ "$snap_x" == 16 && "$snap_y" == 16 ]] && break
  sleep 0.1
done
[[ "$snap_x" == 16 && "$snap_y" == 16 ]] || {
  echo "compact corner snap settled at ${snap_x},${snap_y}, expected 16,16" >&2
  exit 1
}

xdotool mousemove 0 0 mousemove --window "$window_id" 20 24
sleep 0.3
xdotool click 1
for _ in $(seq 1 60); do
  restored_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
  restored_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
  [[ "$restored_width" == 1120 && "$restored_height" == 680 ]] && break
  sleep 0.1
done
[[ "$restored_width" == 1120 && "$restored_height" == 680 ]] || {
  echo "restored geometry is ${restored_width}x${restored_height}, expected 1120x680" >&2
  exit 1
}

kill "$app_pid" 2>/dev/null || true
wait "$app_pid" 2>/dev/null || true
app_pid=""
export OKP_COMPACT_START_AT_FLOOR=1
timeout 30s "$BINARY" "$FIXTURE" >"$OUT_DIR/floor-app.log" 2>&1 &
app_pid=$!
window_id=""
for _ in $(seq 1 100); do
  window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | tail -n1 || true)"
  if [[ -n "$window_id" ]]; then
    floor_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
    floor_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
    [[ "$floor_width" == 284 && "$floor_height" == 160 ]] && break
  fi
  sleep 0.1
done
[[ "$floor_width" == 284 && "$floor_height" == 160 ]] || {
  echo "compact floor is ${floor_width}x${floor_height}, expected 284x160" >&2
  exit 1
}
xdotool windowmove "$window_id" 40 40
xdotool mousemove 0 0 mousemove --window "$window_id" 142 76
sleep 0.4
import -window root "$OUT_DIR/floor-root.png"
magick "$OUT_DIR/floor-root.png" -crop 284x160+40+40 +repage "$OUT_DIR/compact-floor.png"

floor_top="$(magick "$OUT_DIR/compact-floor.png" -crop 260x42+12+8 -colorspace gray -format '%[fx:minima]' info:)"
floor_bottom="$(magick "$OUT_DIR/compact-floor.png" -crop 260x40+12+110 -colorspace gray -format '%[fx:minima]' info:)"
floor_middle="$(magick "$OUT_DIR/compact-floor.png" -crop 70x30+18+62 -colorspace gray -format '%[fx:mean]' info:)"
if [[ "$OKP_PLAYBACK_FRAME_PREVIEW" == bright ]]; then
  awk -v top="$floor_top" -v bottom="$floor_bottom" -v middle="$floor_middle" \
    'BEGIN { exit !(top < 0.50 && bottom < 0.50 && middle > 0.70) }' || {
    echo "compact floor overlaps or loses chrome: top=$floor_top bottom=$floor_bottom middle=$floor_middle" >&2
    exit 1
  }
else
  awk -v top="$floor_top" -v bottom="$floor_bottom" -v middle="$floor_middle" \
    'BEGIN { exit !(top < 0.50 && bottom < 0.50 && middle < 0.15) }' || {
    echo "dark compact floor overlaps or loses chrome: top=$floor_top bottom=$floor_bottom middle=$floor_middle" >&2
    exit 1
  }
fi

xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
xdotool key --clearmodifiers f
for _ in $(seq 1 60); do
  fullscreen_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
  fullscreen_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
  [[ "$fullscreen_width" == 1280 && "$fullscreen_height" == 900 ]] && break
  sleep 0.1
done
[[ "$fullscreen_width" == 1280 && "$fullscreen_height" == 900 ]] || {
  echo "fullscreen after compact is ${fullscreen_width}x${fullscreen_height}, expected 1280x900" >&2
  exit 1
}

rg -q 'interaction: compact-mode-enter size=480x270 floor=284x160' "$OUT_DIR/app.log"
rg -q 'interaction: compact-mode-snap x=16 y=16' "$OUT_DIR/app.log"
rg -q 'interaction: compact-mode-restore' "$OUT_DIR/app.log"
rg -q 'interaction: compact-mode-enter size=284x160 floor=284x160' "$OUT_DIR/floor-app.log"
rg -q 'interaction: compact-mode-restore' "$OUT_DIR/floor-app.log"
if rg -q 'Theme parser error|CSS parser error|panicked at' "$OUT_DIR/app.log" "$OUT_DIR/floor-app.log"; then
  cat "$OUT_DIR/app.log" >&2
  exit 1
fi

printf '%s\n' \
  'entry=pass' \
  "substrate=$OKP_PLAYBACK_FRAME_PREVIEW" \
  'default_geometry=480x270' \
  'always_on_top=pass' \
  'corner_snap=16,16' \
  'title_transport_visible=pass' \
  'floor_geometry=284x160' \
  'floor_overlap=pass' \
  'compact_to_fullscreen=1280x900' \
  'restore=pass' \
  'restored_geometry=1120x680' >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Compact-mode smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Compact-mode smoke passed. Results: $OUT_DIR/results.txt"
