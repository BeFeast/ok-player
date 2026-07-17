#!/usr/bin/env bash
# Mapped-window regression smoke for issue #327. It renders the real OSC under
# Xvfb, proves the audio-track note owns non-background pixels without relying
# on the host icon theme, distinguishes it from Volume, and exercises the same
# note after adaptive collapse moves Audio into the overflow popover.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-audio-track-osc-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

if [[ "$BINARY" == */* ]]; then
  BINARY="$(readlink -f "$BINARY")"
fi

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick rg awk; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
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
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_DEBUG_INTERACTIONS=1
export OKP_DEBUG_OSC_LAYOUT=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_SKIP_UPDATE_CHECK=1

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":1,"updates":{"auto_check":false}}
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

layout_line() {
  local log="$1" id="$2"
  rg "osc-layout: id=${id} " "$log" | tail -n1
}

layout_value() {
  local line="$1" key="$2"
  awk -v key="$key" '{
    for (i = 1; i <= NF; i++) {
      split($i, field, "=")
      if (field[1] == key) { print field[2]; exit }
    }
  }' <<<"$line"
}

widget_bounds() {
  local log="$1" label="$2"
  rg "osc-widget-bounds: label=${label} " "$log" | tail -n1
}

control_icon_bounds() {
  local log="$1" id="$2" window_height="$3" icon_size="$4"
  local line bar_height control_x control_y control_width control_height
  line="$(layout_line "$log" "$id")"
  bar_height="$(layout_value "$line" bar_height)"
  control_x="$(layout_value "$line" x)"
  control_y="$(layout_value "$line" y)"
  control_width="$(layout_value "$line" width)"
  control_height="$(layout_value "$line" height)"
  printf 'x=%s y=%s width=%s height=%s\n' \
    "$((16 + control_x + (control_width - icon_size) / 2))" \
    "$((window_height - 18 - bar_height + control_y + (control_height - icon_size) / 2))" \
    "$icon_size" "$icon_size"
}

capture_icon() {
  local shot="$1" bounds="$2" target="$3"
  local x y width height
  x="$(layout_value "$bounds" x)"
  y="$(layout_value "$bounds" y)"
  width="$(layout_value "$bounds" width)"
  height="$(layout_value "$bounds" height)"
  [[ -n "$x" && -n "$y" && "$width" -gt 0 && "$height" -gt 0 ]] || {
    echo "invalid icon bounds: $bounds" >&2
    exit 1
  }
  magick "$shot" -crop "${width}x${height}+${x}+${y}" +repage "$target"
}

assert_glyph_pixels() {
  local icon="$1" label="$2"
  local bright_fraction
  bright_fraction="$(magick "$icon" -colorspace gray -threshold 70% -format '%[fx:mean]' info:)"
  awk -v value="$bright_fraction" 'BEGIN { exit !(value > 0.035) }' || {
    echo "$label rendered no visible glyph pixels: bright_fraction=$bright_fraction" >&2
    exit 1
  }
  printf '%s=%s\n' "$label" "$bright_fraction" >>"$OUT_DIR/results.txt"
}

assert_dark_glyph_pixels() {
  local icon="$1" label="$2"
  local dark_fraction
  dark_fraction="$(magick "$icon" -colorspace gray -threshold 35% -negate -format '%[fx:mean]' info:)"
  awk -v value="$dark_fraction" 'BEGIN { exit !(value > 0.02 && value < 0.65) }' || {
    echo "$label rendered no dark glyph pixels: dark_fraction=$dark_fraction" >&2
    exit 1
  }
  printf '%s=%s\n' "$label" "$dark_fraction" >>"$OUT_DIR/results.txt"
}

launch() {
  local mode="$1" theme="${2:-}"
  local log="$OUT_DIR/app-${mode}.log"
  if [[ -n "$theme" ]]; then
    GTK_THEME="$theme" OKP_PLAYBACK_FRAME_PREVIEW=dark timeout 45s "$BINARY" "$FIXTURE" >"$log" 2>&1 &
  elif [[ "$mode" == "narrow" ]]; then
    timeout 45s "$BINARY" "$FIXTURE" >"$log" 2>&1 &
  else
    OKP_PLAYBACK_FRAME_PREVIEW="$mode" timeout 45s "$BINARY" "$FIXTURE" >"$log" 2>&1 &
  fi
  app_pid=$!
  sleep 6
  window_id="$(xdotool search --name 'OK Player' | tail -n1)"
  [[ -n "$window_id" ]] || { cat "$log" >&2; exit 1; }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  xdotool key --clearmodifiers space
  sleep 1
  xwininfo -id "$window_id" >"$OUT_DIR/window-${mode}.xwininfo"
  import -window "$window_id" "$OUT_DIR/${mode}-paused.png"
}

stop() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 1
}

# Normal bright frame: exact mapped icon bounds, visible pixels, semantic
# difference from Volume, hover state, and functional popover opening.
launch bright
bright_log="$OUT_DIR/app-bright.log"
height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/window-bright.xwininfo")"
audio_bounds="$(control_icon_bounds "$bright_log" Audio "$height" 19)"
capture_icon "$OUT_DIR/bright-paused.png" "$audio_bounds" "$OUT_DIR/audio-bright.png"
assert_glyph_pixels "$OUT_DIR/audio-bright.png" audio_bright_fraction

volume_line="$(layout_line "$bright_log" Volume)"
bar_height="$(layout_value "$volume_line" bar_height)"
volume_x="$((16 + $(layout_value "$volume_line" x)))"
volume_y="$((height - 18 - bar_height + $(layout_value "$volume_line" y)))"
volume_width="$(layout_value "$volume_line" width)"
volume_height="$(layout_value "$volume_line" height)"
magick "$OUT_DIR/bright-paused.png" \
  -crop "${volume_width}x${volume_height}+${volume_x}+${volume_y}" +repage \
  "$OUT_DIR/volume-bright.png"
magick "$OUT_DIR/audio-bright.png" -colorspace gray -threshold 70% -resize 64x64! "$OUT_DIR/audio-mask.png"
magick "$OUT_DIR/volume-bright.png" -colorspace gray -threshold 70% -resize 64x64! "$OUT_DIR/volume-mask.png"
identity_rmse="$(magick compare -metric RMSE "$OUT_DIR/audio-mask.png" "$OUT_DIR/volume-mask.png" null: 2>&1 || true)"
identity_normalized="$(sed -n 's/.*(\([^()]*\)).*/\1/p' <<<"$identity_rmse")"
awk -v value="$identity_normalized" 'BEGIN { exit !(value > 0.08) }' || {
  echo "audio and volume glyph identities are not visually distinct: RMSE=$identity_rmse" >&2
  exit 1
}
printf 'audio_volume_identity_rmse=%s\n' "$identity_normalized" >>"$OUT_DIR/results.txt"

audio_x="$(layout_value "$audio_bounds" x)"
audio_y="$(layout_value "$audio_bounds" y)"
audio_width="$(layout_value "$audio_bounds" width)"
audio_height="$(layout_value "$audio_bounds" height)"
xdotool mousemove --window "$window_id" "$((audio_x + audio_width / 2))" "$((audio_y + audio_height / 2))"
sleep 1
import -window "$window_id" "$OUT_DIR/audio-hover.png"
capture_icon "$OUT_DIR/audio-hover.png" "$audio_bounds" "$OUT_DIR/audio-hover-icon.png"
assert_glyph_pixels "$OUT_DIR/audio-hover-icon.png" audio_hover_fraction
xdotool click 1
sleep 1
rg -q 'interaction: audio-popover=shown' "$bright_log" || {
  echo "audio-track button did not open its chooser" >&2
  exit 1
}
import -window "$window_id" "$OUT_DIR/audio-popover.png"
capture_icon "$OUT_DIR/audio-popover.png" "$audio_bounds" "$OUT_DIR/audio-selected-icon.png"
assert_glyph_pixels "$OUT_DIR/audio-selected-icon.png" audio_selected_fraction
xdotool key --clearmodifiers Escape
stop

# Narrow-but-visible: prove the glyph remains allocated before the adaptive
# policy folds it. Then shrink to the floor, open overflow, and prove the same
# bundled note renders in the mapped overflow row.
launch narrow
narrow_log="$OUT_DIR/app-narrow.log"
for narrow_width in 900 960 1000; do
  xdotool windowsize "$window_id" "$narrow_width" 600
  sleep 1
  audio_state="$(layout_line "$narrow_log" Audio)"
  if [[ "$audio_state" == *"visible=true"* ]]; then
    break
  fi
done
[[ "$audio_state" == *"visible=true"* ]] || { echo "audio action never mapped at a narrow width" >&2; exit 1; }
actual_narrow_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
[[ "$actual_narrow_width" -lt 1120 ]] || { echo "narrow resize did not apply" >&2; exit 1; }
import -window "$window_id" "$OUT_DIR/narrow-audio.png"
narrow_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
narrow_audio_bounds="$(control_icon_bounds "$narrow_log" Audio "$narrow_height" 19)"
capture_icon "$OUT_DIR/narrow-audio.png" "$narrow_audio_bounds" "$OUT_DIR/audio-narrow-icon.png"
assert_glyph_pixels "$OUT_DIR/audio-narrow-icon.png" audio_narrow_fraction

xdotool windowsize "$window_id" 480 540
sleep 1
audio_state="$(layout_line "$narrow_log" Audio)"
[[ "$audio_state" == *"visible=false"* ]] || { echo "adaptive floor did not collapse Audio" >&2; exit 1; }
overflow_line="$(layout_line "$narrow_log" Overflow)"
[[ "$overflow_line" == *"visible=true"* ]] || { echo "overflow action is not mapped" >&2; exit 1; }
floor_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
floor_bar_height="$(layout_value "$overflow_line" bar_height)"
overflow_x="$((16 + $(layout_value "$overflow_line" x) + $(layout_value "$overflow_line" width) / 2))"
overflow_y="$((floor_height - 18 - floor_bar_height + $(layout_value "$overflow_line" y) + $(layout_value "$overflow_line" height) / 2))"
xdotool mousemove --window "$window_id" "$overflow_x" "$overflow_y" click 1
sleep 1
overflow_audio_bounds="$(widget_bounds "$narrow_log" audio-overflow-icon)"
# GtkPopover is a separate native surface under X11, so capture the root rather
# than the application window; the debug bounds are in that root coordinate
# space and therefore cover the actually mapped popup glyph.
import -window root "$OUT_DIR/adaptive-overflow.png"
capture_icon "$OUT_DIR/adaptive-overflow.png" "$overflow_audio_bounds" "$OUT_DIR/audio-overflow-icon.png"
assert_dark_glyph_pixels "$OUT_DIR/audio-overflow-icon.png" audio_overflow_dark_fraction
stop

# Dark video and HighContrast theme retain the exact same mapped content.
launch dark
dark_height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/window-dark.xwininfo")"
dark_bounds="$(control_icon_bounds "$OUT_DIR/app-dark.log" Audio "$dark_height" 19)"
capture_icon "$OUT_DIR/dark-paused.png" "$dark_bounds" "$OUT_DIR/audio-dark.png"
assert_glyph_pixels "$OUT_DIR/audio-dark.png" audio_dark_fraction
stop

launch high-contrast HighContrast
contrast_height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/window-high-contrast.xwininfo")"
contrast_bounds="$(control_icon_bounds "$OUT_DIR/app-high-contrast.log" Audio "$contrast_height" 19)"
capture_icon "$OUT_DIR/high-contrast-paused.png" "$contrast_bounds" "$OUT_DIR/audio-high-contrast.png"
assert_glyph_pixels "$OUT_DIR/audio-high-contrast.png" audio_high_contrast_fraction
stop

printf '%s\n' \
  'audio_popover=pass' \
  'normal_width=pass' \
  "narrow_width=${actual_narrow_width}" \
  'adaptive_overflow=pass' \
  'dark_video=pass' \
  'high_contrast=pass' >>"$OUT_DIR/results.txt"
SMOKE
then
  echo "Audio-track OSC smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Audio-track OSC smoke passed. Results: $OUT_DIR/results.txt"
