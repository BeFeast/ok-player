#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-main-window-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffmpeg ffprobe rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [[ "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" != "1" ]]; then
  if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    LIBGL_ALWAYS_SOFTWARE=1 \
    xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
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

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 4
xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
import -window root "$OUT_DIR/root.png"
import -window "$window_id" "$OUT_DIR/window.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/window.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"

if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected main window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# The first-run surface owns its own Open actions; the standard playback OSC
# must not be present before media is loaded.
empty_bottom_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 1088x70+16+592 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$empty_bottom_max" 'BEGIN { exit !(max < 0.08) }'; then
  echo "Standard OSC leaked into the empty state: bottom maxima=${empty_bottom_max}" >&2
  exit 1
fi

# Regression guard for the old native GTK caption/headerbar. The player owns
# its caption controls, so the top-center strip should remain dark video/chrome,
# not a light system titlebar with centered text.
top_center_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 220x36+450+0 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$top_center_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Unexpected bright top-center caption strip in main window: mean=${top_center_mean}" >&2
  exit 1
fi

# Guard the captionless dark shell as a region rather than pinning one
# decorative pixel. Small renderer/color-management differences may shift an
# individual RGB value while the perceived surface remains identical.
top_left_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 180x36+0+0 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$top_left_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Unexpected bright top-left chrome region: mean=${top_left_mean}" >&2
  exit 1
fi

# The empty player shell must render the OK Player welcome surface, not a blank
# or overlapped viewport. The centered identity band (app mark + wordmark) is
# the brightest thing in the middle of the window, so a near-black maximum there
# means the welcome surface failed to draw.
identity_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 260x150+430+165 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
identity_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 280x150+420+160 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v max="$identity_max" -v mean="$identity_mean" \
  'BEGIN { exit !(max > 0.6 && max - mean > 0.4) }'; then
  echo "Empty welcome identity lacks contrast: maxima=${identity_max} mean=${identity_mean}" >&2
  exit 1
fi

# Supporting copy must produce a meaningful bright-text area, not merely exist
# as near-black pixels over the video plane.
tagline_bright_pixels="$(
  magick "$OUT_DIR/window.png" \
    -crop 280x34+420+275 \
    -colorspace gray \
    -threshold 42% \
    -format '%[fx:mean*w*h]' info:
)"
tagline_bright_pixels="${tagline_bright_pixels%.*}"
if (( tagline_bright_pixels < 180 )); then
  echo "Welcome supporting copy is missing or too dim: bright pixels=${tagline_bright_pixels}" >&2
  exit 1
fi

# The primary "Open media" action carries the OK Player accent, so its band must
# be clearly green-dominant (teal) rather than a stock grey GTK button.
primary_red="$(magick "$OUT_DIR/window.png" -crop 130x60+372+296 -format '%[fx:mean.r]' info:)"
primary_green="$(magick "$OUT_DIR/window.png" -crop 130x60+372+296 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$primary_red" -v g="$primary_green" 'BEGIN { exit !(g - r > 0.12) }'; then
  echo "Primary action accent missing from welcome surface: red=${primary_red} green=${primary_green}" >&2
  exit 1
fi

# Secondary actions carry readable light labels and borders beside the CTA.
secondary_bright_pixels="$(
  magick "$OUT_DIR/window.png" \
    -crop 260x70+495+320 \
    -colorspace gray \
    -threshold 55% \
    -format '%[fx:mean*w*h]' info:
)"
secondary_bright_pixels="${secondary_bright_pixels%.*}"
if (( secondary_bright_pixels < 300 )); then
  echo "Welcome secondary actions are missing or unreadable: bright pixels=${secondary_bright_pixels}" >&2
  exit 1
fi
SMOKE
  then
    echo "Main window smoke failed. Session log: $OUT_DIR/session.log" >&2
    cat "$OUT_DIR/session.log" >&2
    exit 1
  fi
fi

"$ROOT/scripts/generate-linux-acceptance-media.sh" "$OUT_DIR/fixtures" \
  >"$OUT_DIR/fixtures.log" 2>&1

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -screen 1 1024x768x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/window-fit-session.log" 2>&1 <<'FIT_SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
FIXTURES="$OUT_DIR/fixtures"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/fit-config"
export XDG_STATE_HOME="$OUT_DIR/fit-state"

PRIMARY_DISPLAY="$DISPLAY"
SECONDARY_DISPLAY="${DISPLAY%%.*}.1"

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

DISPLAY="$PRIMARY_DISPLAY" xfwm4 --sm-client-disable >"$OUT_DIR/window-fit-xfwm4-primary.log" 2>&1 &
wm_pid=$!
DISPLAY="$SECONDARY_DISPLAY" xfwm4 --sm-client-disable >"$OUT_DIR/window-fit-xfwm4-secondary.log" 2>&1 &
wm_secondary_pid=$!
app_pid=""

cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
  fi
  kill "$wm_pid" 2>/dev/null || true
  kill "$wm_secondary_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1

start_app() {
  local log="$1"
  shift
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  OKP_DEBUG_WINDOW_FIT=1 \
  "$BINARY" "$@" >"$OUT_DIR/$log" 2>&1 &
  app_pid=$!
}

wait_for_window() {
  local ids="$1"
  for _ in $(seq 1 80); do
    if xdotool search --name "OK Player" >"$ids" 2>/dev/null; then
      head -n1 "$ids"
      return 0
    fi
    sleep 0.1
  done
  echo "Timed out waiting for OK Player window" >&2
  return 1
}

wait_for_fit_log() {
  local log="$1" pattern="$2"
  for _ in $(seq 1 120); do
    if rg -q "$pattern" "$OUT_DIR/$log" 2>/dev/null; then
      return 0
    fi
    sleep 0.1
  done
  echo "Timed out waiting for window-fit state '$pattern' in $log" >&2
  return 1
}

stop_app() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  for _ in $(seq 1 40); do
    if ! xdotool search --name "OK Player" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.1
  done
  echo "OK Player window did not close" >&2
  return 1
}

capture_geometry() {
  local window_id="$1" stem="$2"
  xwininfo -id "$window_id" >"$OUT_DIR/$stem.xwininfo"
  import -window "$window_id" "$OUT_DIR/$stem.png"
}

geometry_value() {
  local file="$1" label="$2"
  awk -v label="$label" '$1 == label ":" { print $2; exit }' "$file"
}

window_position_value() {
  local file="$1" axis="$2"
  awk -v axis="$axis" '$1 == "Absolute" && $2 == "upper-left" && $3 == axis ":" { print $4; exit }' "$file"
}

export DISPLAY="$PRIMARY_DISPLAY"
start_app "fit-small-app.log" "$FIXTURES/fit-small.mkv"
small_id="$(wait_for_window "$OUT_DIR/fit-small-window.ids")"
wait_for_fit_log "fit-small-app.log" "window fit positioned"
sleep 1
xdotool mousemove 1200 850
sleep 3
capture_geometry "$small_id" "fit-small-window"
small_width="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Width)"
small_height="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Height)"
small_x="$(window_position_value "$OUT_DIR/fit-small-window.xwininfo" X)"
small_y="$(window_position_value "$OUT_DIR/fit-small-window.xwininfo" Y)"
if [[ "$small_width" != "320" || "$small_height" != "180" ]]; then
  echo "Small video did not use native size: ${small_width}x${small_height}" >&2
  exit 1
fi
if [[ "$small_x" != "480" || "$small_y" != "360" ]]; then
  echo "Small video was not centered in the 1280x900 workarea: +${small_x},${small_y}" >&2
  exit 1
fi

xdotool windowsize "$small_id" 700 500
sleep 2
capture_geometry "$small_id" "fit-small-manual-resize"
manual_width="$(geometry_value "$OUT_DIR/fit-small-manual-resize.xwininfo" Width)"
manual_height="$(geometry_value "$OUT_DIR/fit-small-manual-resize.xwininfo" Height)"
if [[ "$manual_width" != "700" || "$manual_height" != "500" ]]; then
  echo "Window fought manual resize after load: ${manual_width}x${manual_height}" >&2
  exit 1
fi
stop_app

OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
OKP_DEBUG_WINDOW_FIT=1 \
OKP_START_MAXIMIZED=1 \
  "$BINARY" "$FIXTURES/fit-small.mkv" >"$OUT_DIR/fit-maximized-app.log" 2>&1 &
app_pid=$!
max_id="$(wait_for_window "$OUT_DIR/fit-maximized-window.ids")"
wait_for_fit_log "fit-maximized-app.log" "window fit skipped: fullscreen=false maximized=true"
sleep 1
capture_geometry "$max_id" "fit-maximized-before-load"
before_max_width="$(geometry_value "$OUT_DIR/fit-maximized-before-load.xwininfo" Width)"
before_max_height="$(geometry_value "$OUT_DIR/fit-maximized-before-load.xwininfo" Height)"
if (( before_max_width < 1200 || before_max_height < 840 )); then
  echo "Could not maximize the player before the load guard check: ${before_max_width}x${before_max_height}" >&2
  exit 1
fi
capture_geometry "$max_id" "fit-maximized-window"
max_width="$(geometry_value "$OUT_DIR/fit-maximized-window.xwininfo" Width)"
max_height="$(geometry_value "$OUT_DIR/fit-maximized-window.xwininfo" Height)"
if (( max_width < 1200 || max_height < 840 )); then
  echo "Media load resized a maximized window: ${max_width}x${max_height}" >&2
  exit 1
fi
if ! rg -q "window fit skipped: fullscreen=false maximized=true" "$OUT_DIR/fit-maximized-app.log"; then
  echo "Maximized load did not exercise the window-fit guard" >&2
  exit 1
fi
stop_app

OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
OKP_DEBUG_WINDOW_FIT=1 \
OKP_START_FULLSCREEN=1 \
  "$BINARY" "$FIXTURES/fit-small.mkv" >"$OUT_DIR/fit-fullscreen-app.log" 2>&1 &
app_pid=$!
fullscreen_id="$(wait_for_window "$OUT_DIR/fit-fullscreen-window.ids")"
wait_for_fit_log "fit-fullscreen-app.log" "window fit skipped: fullscreen=true maximized=false"
sleep 1
capture_geometry "$fullscreen_id" "fit-fullscreen-window"
fullscreen_width="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Width)"
fullscreen_height="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Height)"
if (( fullscreen_width < 1270 || fullscreen_height < 890 )); then
  echo "Media load resized a fullscreen window: ${fullscreen_width}x${fullscreen_height}" >&2
  exit 1
fi
if ! rg -q "window fit skipped: fullscreen=true maximized=false" "$OUT_DIR/fit-fullscreen-app.log"; then
  echo "Fullscreen load did not exercise the window-fit guard" >&2
  exit 1
fi
stop_app

export DISPLAY="$SECONDARY_DISPLAY"
start_app "fit-4k-right-monitor-app.log" "$FIXTURES/fit-4k.mkv"
right_id="$(wait_for_window "$OUT_DIR/fit-4k-right-monitor-window.ids")"
wait_for_fit_log "fit-4k-right-monitor-app.log" "window fit positioned"
sleep 1
capture_geometry "$right_id" "fit-4k-right-monitor-window"
fit_width="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Width)"
fit_height="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Height)"
fit_x="$(window_position_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" X)"
fit_y="$(window_position_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Y)"
if (( fit_width < 958 || fit_width > 964 || fit_height < 538 || fit_height > 543 )); then
  echo "4K video did not fit the active 1024x768 monitor: ${fit_width}x${fit_height}" >&2
  exit 1
fi
if ! awk -v w="$fit_width" -v h="$fit_height" \
  'BEGIN { aspect=w/h; exit !(aspect > 1.775 && aspect < 1.780) }'; then
  echo "4K fitted window lost 16:9 aspect: ${fit_width}x${fit_height}" >&2
  exit 1
fi
if ! rg -q "workarea=1024x768" "$OUT_DIR/fit-4k-right-monitor-app.log"; then
  echo "4K load did not use the monitor containing the player window" >&2
  exit 1
fi
if (( fit_x < 28 || fit_x > 34 || fit_y < 110 || fit_y > 116 \
      || fit_x + fit_width > 1024 || fit_y + fit_height > 768 )); then
  echo "4K fitted window was not centered/clamped inside the monitor: ${fit_width}x${fit_height}+${fit_x},${fit_y}" >&2
  exit 1
fi
stop_app
FIT_SMOKE
then
  echo "Window-fit smoke failed. Session log: $OUT_DIR/window-fit-session.log" >&2
  cat "$OUT_DIR/window-fit-session.log" >&2
  exit 1
fi

# Keep the HiDPI case on its own X server. Multi-screen Xvfb window-manager
# ownership is backend-dependent and can make a scale-2 app land on the small
# secondary screen, which tests monitor selection rather than scaling.
if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 3840x2160x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/window-fit-scale-2-session.log" 2>&1 <<'SCALE_SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
FIXTURE="$OUT_DIR/fixtures/fit-4k.mkv"

export GDK_BACKEND=x11
export GDK_SCALE=2
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/fit-scale-2-config"
export XDG_STATE_HOME="$OUT_DIR/fit-scale-2-state"

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/window-fit-xfwm4-scale-2.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
  fi
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
OKP_DEBUG_WINDOW_FIT=1 \
  "$BINARY" "$FIXTURE" >"$OUT_DIR/fit-4k-scale-2-app.log" 2>&1 &
app_pid=$!

for _ in $(seq 1 120); do
  if xdotool search --name "OK Player" >"$OUT_DIR/fit-4k-scale-2-window.ids" 2>/dev/null \
      && rg -q "window fit positioned" "$OUT_DIR/fit-4k-scale-2-app.log" 2>/dev/null; then
    break
  fi
  sleep 0.1
done
if ! rg -q "window fit positioned" "$OUT_DIR/fit-4k-scale-2-app.log" 2>/dev/null; then
  echo "Timed out waiting for the scale-2 window fit" >&2
  exit 1
fi
scaled_id="$(head -n1 "$OUT_DIR/fit-4k-scale-2-window.ids")"
sleep 1
xwininfo -id "$scaled_id" >"$OUT_DIR/fit-4k-scale-2-window.xwininfo"
import -window "$scaled_id" "$OUT_DIR/fit-4k-scale-2-window.png"

scaled_width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/fit-4k-scale-2-window.xwininfo")"
scaled_height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/fit-4k-scale-2-window.xwininfo")"
scaled_x="$(awk '$1 == "Absolute" && $2 == "upper-left" && $3 == "X:" { print $4; exit }' "$OUT_DIR/fit-4k-scale-2-window.xwininfo")"
scaled_y="$(awk '$1 == "Absolute" && $2 == "upper-left" && $3 == "Y:" { print $4; exit }' "$OUT_DIR/fit-4k-scale-2-window.xwininfo")"

if (( scaled_width < 1727 || scaled_width > 1731 || scaled_height < 971 || scaled_height > 975 )); then
  echo "Scaled 4K video did not fit the logical 1920x1080 workarea: ${scaled_width}x${scaled_height}" >&2
  exit 1
fi
if (( scaled_x < 188 || scaled_x > 194 || scaled_y < 104 || scaled_y > 110 \
      || scaled_x + scaled_width > 1920 || scaled_y + scaled_height > 1080 )); then
  echo "Scaled 4K fitted window was not centered/clamped: ${scaled_width}x${scaled_height}+${scaled_x},${scaled_y}" >&2
  exit 1
fi
if ! rg -q "scale=2.00 workarea=1920x1080" "$OUT_DIR/fit-4k-scale-2-app.log"; then
  echo "Scale-2 load did not convert the monitor workarea to logical coordinates" >&2
  exit 1
fi
SCALE_SMOKE
then
  echo "Scale-2 window-fit smoke failed. Session log: $OUT_DIR/window-fit-scale-2-session.log" >&2
  cat "$OUT_DIR/window-fit-scale-2-session.log" >&2
  exit 1
fi

echo "Main window fit smoke passed. Screenshots: $OUT_DIR/fit-small-window.png, $OUT_DIR/fit-4k-right-monitor-window.png, $OUT_DIR/fit-4k-scale-2-window.png"
