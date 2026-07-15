#!/usr/bin/env bash
# Canonical playback-chrome visual smoke for issue #250. Captures the loaded
# paused, playing-active, playing-idle, bright/dark legibility, and fullscreen
# idle states at the 1120x680 reference geometry.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-playback-chrome-smoke}"
DARK_FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ ! -f "$DARK_FIXTURE" ]]; then
  echo "Missing media fixture: $DARK_FIXTURE" >&2
  exit 127
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$DARK_FIXTURE" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
DARK_FIXTURE="$3"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export LIBGL_ALWAYS_SOFTWARE=1

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1

launch_player() {
  local source="$1"
  local frame_preview="$2"
  OKP_PLAYBACK_FRAME_PREVIEW="$frame_preview" \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 35s "$BINARY" "$source" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
  sleep 3
  window_id="$(xdotool search --name 'OK Player' | tail -n1)"
  if [[ -z "$window_id" ]]; then
    echo "main window did not appear" >&2
    cat "$OUT_DIR/app.log" >&2 || true
    exit 1
  fi
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  sleep 1
  xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
  local width height
  width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
  height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
  if [[ "$width" != "1120" || "$height" != "680" ]]; then
    echo "unexpected playback geometry: ${width}x${height}" >&2
    exit 1
  fi
}

stop_player() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 1
}

capture_window() {
  local target="$1"
  import -window "$window_id" "$target"
}

# Bright source: pause pins chrome, resume + motion shows active chrome, then
# idle clears both standard and fullscreen surfaces.
launch_player "$DARK_FIXTURE" bright
xdotool key --clearmodifiers space
sleep 1
capture_window "$OUT_DIR/bright-paused.png"

xdotool key --clearmodifiers space
xdotool mousemove --window "$window_id" 560 340
sleep 1
capture_window "$OUT_DIR/bright-playing-active.png"

sleep 4
capture_window "$OUT_DIR/bright-playing-idle.png"

xdotool key --clearmodifiers f
sleep 4
import -window root "$OUT_DIR/fullscreen-idle.png"
stop_player

# Dark source: pause pins the same chrome over a near-black frame.
launch_player "$DARK_FIXTURE" dark
xdotool key --clearmodifiers space
sleep 1
capture_window "$OUT_DIR/dark-paused.png"
stop_player

# Bright-frame active chrome must darken the OSC band while retaining bright
# glyphs; title text and caption controls must remain legible at the top.
bright_frame_mean="$(magick "$OUT_DIR/bright-paused.png" -crop 500x240+310+180 -colorspace gray -format '%[fx:mean]' info:)"
bright_osc_mean="$(magick "$OUT_DIR/bright-paused.png" -crop 900x54+110+604 -colorspace gray -format '%[fx:mean]' info:)"
bright_osc_max="$(magick "$OUT_DIR/bright-paused.png" -crop 1088x70+16+592 -colorspace gray -format '%[fx:maxima]' info:)"
bright_top_max="$(magick "$OUT_DIR/bright-paused.png" -crop 420x42+0+0 -colorspace gray -format '%[fx:maxima]' info:)"
if ! awk -v frame="$bright_frame_mean" -v osc="$bright_osc_mean" -v max="$bright_osc_max" -v top="$bright_top_max" \
  'BEGIN { exit !(frame > 0.80 && osc < 0.62 && max > 0.82 && top > 0.55) }'; then
  echo "bright-frame chrome failed: frame=${bright_frame_mean} osc=${bright_osc_mean} max=${bright_osc_max} top=${bright_top_max}" >&2
  exit 1
fi

# Active and paused captures both keep the OSC visible. After the canonical
# idle timeout the bright frame must return to an unobstructed bottom band.
active_bottom_mean="$(magick "$OUT_DIR/bright-playing-active.png" -crop 900x54+110+604 -colorspace gray -format '%[fx:mean]' info:)"
idle_bottom_mean="$(magick "$OUT_DIR/bright-playing-idle.png" -crop 900x54+110+604 -colorspace gray -format '%[fx:mean]' info:)"
if ! awk -v active="$active_bottom_mean" -v idle="$idle_bottom_mean" \
  'BEGIN { exit !(active < 0.62 && idle > 0.82 && idle - active > 0.20) }'; then
  echo "standard idle chrome failed: active=${active_bottom_mean} idle=${idle_bottom_mean}" >&2
  exit 1
fi

# Fullscreen idle must be the bright frame edge-to-edge with no titlebar or OSC.
fullscreen_top_mean="$(magick "$OUT_DIR/fullscreen-idle.png" -crop 1280x50+0+0 -colorspace gray -format '%[fx:mean]' info:)"
fullscreen_bottom_mean="$(magick "$OUT_DIR/fullscreen-idle.png" -crop 1280x100+0+800 -colorspace gray -format '%[fx:mean]' info:)"
if ! awk -v top="$fullscreen_top_mean" -v bottom="$fullscreen_bottom_mean" \
  'BEGIN { exit !(top > 0.82 && bottom > 0.82) }'; then
  echo "fullscreen idle chrome failed: top=${fullscreen_top_mean} bottom=${fullscreen_bottom_mean}" >&2
  exit 1
fi

# Near-black video still needs bright glyphs and a visible hairline/material edge.
dark_osc_max="$(magick "$OUT_DIR/dark-paused.png" -crop 1088x70+16+592 -colorspace gray -format '%[fx:maxima]' info:)"
dark_osc_mean="$(magick "$OUT_DIR/dark-paused.png" -crop 1088x70+16+592 -colorspace gray -format '%[fx:mean]' info:)"
if ! awk -v max="$dark_osc_max" -v mean="$dark_osc_mean" \
  'BEGIN { exit !(max > 0.72 && mean > 0.035) }'; then
  echo "dark-frame chrome failed: max=${dark_osc_max} mean=${dark_osc_mean}" >&2
  exit 1
fi

echo "bright chrome: frame=${bright_frame_mean} osc=${bright_osc_mean} idle=${idle_bottom_mean}"
echo "dark chrome: max=${dark_osc_max} mean=${dark_osc_mean}"
SMOKE
then
  echo "Playback chrome smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Playback chrome smoke passed. Captures: $OUT_DIR"
