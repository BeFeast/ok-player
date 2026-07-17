#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-main-window-smoke}"
IDLE_OSC_ASSERT="$ROOT/scripts/assert-linux-idle-osc-absent.sh"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffmpeg ffprobe rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [[ "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" == "1" && "${OKP_MAIN_WINDOW_IDLE_ONLY:-0}" == "1" ]]; then
  echo "OKP_MAIN_WINDOW_FIT_ONLY and OKP_MAIN_WINDOW_IDLE_ONLY are mutually exclusive" >&2
  exit 2
fi

if [[ "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" != "1" ]]; then
  if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    LIBGL_ALWAYS_SOFTWARE=1 \
    xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
    dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$IDLE_OSC_ASSERT" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
IDLE_OSC_ASSERT="$3"

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

local_contrast() {
  local image="$1" crop="$2" blur="${3:-4}"
  magick "$image" \
    -crop "$crop" +repage \
    -colorspace gray \
    \( +clone -blur "0x${blur}" \) \
    -compose difference -composite \
    -format '%[fx:mean]' info:
}

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 40s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in $(seq 1 100); do
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids" 2>/dev/null || true
  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    candidate_info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
    candidate_width="$(awk '/Width:/ { print $2; exit }' <<<"$candidate_info")"
    candidate_height="$(awk '/Height:/ { print $2; exit }' <<<"$candidate_info")"
    candidate_state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$candidate_info")"
    if [[ "$candidate_width" == "1120" && "$candidate_height" == "680" && "$candidate_state" == "IsViewable" ]]; then
      window_id="$candidate"
      break 2
    fi
  done <"$OUT_DIR/window.ids"
  sleep 0.1
done
if [[ -z "$window_id" ]]; then
  echo "Timed out waiting for the visible 1120x680 main window" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi
xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"

# Mapping precedes the first media-state projection on a cold portal startup.
# Wait for the actual Welcome identity rather than accepting a transient black
# frame as evidence that the idle surface rendered.
welcome_ready=0
for _ in $(seq 1 40); do
  if import -window "$window_id" "$OUT_DIR/window.png" 2>/dev/null; then
    ready_identity="$(local_contrast "$OUT_DIR/window.png" 300x130+410+230)"
    if awk -v residual="$ready_identity" 'BEGIN { exit !(residual > 0.015) }'; then
      welcome_ready=1
      break
    fi
  fi
  sleep 0.25
done
if [[ "$welcome_ready" != "1" ]]; then
  echo "Timed out waiting for the Welcome canvas to paint" >&2
  exit 1
fi
import -window root "$OUT_DIR/root.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/window.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"

if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected main window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# The first-run surface owns its own Open actions; the standard playback OSC
# must not be present before media is loaded. The detector measures local pill
# and glyph structure rather than absolute brightness, so both approved idle
# themes remain valid substrates.
"$IDLE_OSC_ASSERT" "$OUT_DIR/window.png" "Initial Welcome"

# Regression guard for the old native GTK caption/headerbar. The player owns
# its caption controls, so the top-center strip must remain structurally empty,
# not carry a centered system title. Local residuals are theme-independent.
top_center_residual="$(local_contrast "$OUT_DIR/window.png" 220x36+450+0)"
if ! awk -v residual="$top_center_residual" 'BEGIN { exit !(residual < 0.008) }'; then
  echo "Unexpected centered title geometry in main window: residual=${top_center_residual}" >&2
  exit 1
fi

# The custom caption keeps its brand mark and title at top-left in both themes.
top_left_residual="$(local_contrast "$OUT_DIR/window.png" 180x36+0+0)"
if ! awk -v residual="$top_left_residual" 'BEGIN { exit !(residual > 0.015) }'; then
  echo "Custom top-left caption identity is missing: residual=${top_left_residual}" >&2
  exit 1
fi

# The centered mark, title, and supporting copy must produce real structure,
# independent of whether their substrate is light or dark.
identity_residual="$(local_contrast "$OUT_DIR/window.png" 300x130+410+230)"
if ! awk -v residual="$identity_residual" 'BEGIN { exit !(residual > 0.015) }'; then
  echo "Empty Welcome identity is missing: residual=${identity_residual}" >&2
  exit 1
fi

tagline_residual="$(local_contrast "$OUT_DIR/window.png" 260x28+430+332)"
if ! awk -v residual="$tagline_residual" 'BEGIN { exit !(residual > 0.01) }'; then
  echo "Welcome supporting copy is missing: residual=${tagline_residual}" >&2
  exit 1
fi

# The in-canvas Open action keeps the teal label inside the dashed drop target.
primary_red="$(magick "$OUT_DIR/window.png" -crop 140x24+490+395 -format '%[fx:mean.r]' info:)"
primary_green="$(magick "$OUT_DIR/window.png" -crop 140x24+490+395 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$primary_red" -v g="$primary_green" 'BEGIN { exit !(g - r > 0.02) }'; then
  echo "Primary action accent missing from welcome surface: red=${primary_red} green=${primary_green}" >&2
  exit 1
fi

# The drop guidance and target border remain visible without assuming a text
# luminance direction.
drop_target_residual="$(local_contrast "$OUT_DIR/window.png" 320x100+400+365)"
drop_hint_residual="$(local_contrast "$OUT_DIR/window.png" 220x24+450+415)"
if ! awk -v target="$drop_target_residual" -v hint="$drop_hint_residual" \
  'BEGIN { exit !(target > 0.008 && hint > 0.01) }'; then
  echo "Welcome drop target or guidance is missing: target=${drop_target_residual} hint=${drop_hint_residual}" >&2
  exit 1
fi
SMOKE
  then
    echo "Main window smoke failed. Session log: $OUT_DIR/session.log" >&2
    cat "$OUT_DIR/session.log" >&2
    exit 1
  fi
fi

if [[ "${OKP_MAIN_WINDOW_IDLE_ONLY:-0}" == "1" ]]; then
  echo "Main window idle-surface smoke passed. Screenshot: $OUT_DIR/window.png"
  exit 0
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

wait_for_log() {
  local file="$1" pattern="$2"
  for _ in $(seq 1 50); do
    if rg -q "$pattern" "$file"; then
      return 0
    fi
    if [[ -n "$app_pid" ]] && ! kill -0 "$app_pid" 2>/dev/null; then
      return 1
    fi
    sleep 0.1
  done
  return 1
}

export DISPLAY="$PRIMARY_DISPLAY"
start_app "fit-small-app.log" "$FIXTURES/fit-small.mkv"
small_id="$(wait_for_window "$OUT_DIR/fit-small-window.ids")"
sleep 4
xdotool mousemove 1200 850
sleep 3
capture_geometry "$small_id" "fit-small-window"
small_width="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Width)"
small_height="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Height)"
if [[ "$small_width" != "320" || "$small_height" != "180" ]]; then
  echo "Small video did not use native size: ${small_width}x${small_height}" >&2
  exit 1
fi
for expected in \
  "mpv render context initialized before source load" \
  "window fit request: video=320x180" \
  "window fit settled: client=320x180"; do
  if ! wait_for_log "$OUT_DIR/fit-small-app.log" "$expected"; then
    echo "Initial-fit sequence did not log: $expected" >&2
    cat "$OUT_DIR/fit-small-app.log" >&2
    exit 1
  fi
done

# Playback and the original render context remain live through initial sizing
# and ordinary seek input. The 12-second fixture accepts +10 then -10 without
# ending the source, which catches the prior active-source teardown failure.
xdotool key --clearmodifiers Right Left
sleep 1
if ! kill -0 "$app_pid" 2>/dev/null; then
  echo "Player exited after initial sizing and seek input" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi
if rg -qi "error -15|protocol error 71" "$OUT_DIR/fit-small-app.log"; then
  echo "Initial sizing reproduced a forbidden playback/protocol error" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
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
sleep 4
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
if ! wait_for_log "$OUT_DIR/fit-maximized-app.log" \
  "window fit skipped: fullscreen=false maximized=true"; then
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
sleep 4
capture_geometry "$fullscreen_id" "fit-fullscreen-window"
fullscreen_width="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Width)"
fullscreen_height="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Height)"
if (( fullscreen_width < 1270 || fullscreen_height < 890 )); then
  echo "Media load resized a fullscreen window: ${fullscreen_width}x${fullscreen_height}" >&2
  exit 1
fi
if ! wait_for_log "$OUT_DIR/fit-fullscreen-app.log" \
  "window fit skipped: fullscreen=true maximized=false"; then
  echo "Fullscreen load did not exercise the window-fit guard" >&2
  exit 1
fi
stop_app

export DISPLAY="$SECONDARY_DISPLAY"
start_app "fit-4k-right-monitor-app.log" "$FIXTURES/fit-4k.mkv"
right_id="$(wait_for_window "$OUT_DIR/fit-4k-right-monitor-window.ids")"
sleep 4
capture_geometry "$right_id" "fit-4k-right-monitor-window"
fit_width="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Width)"
fit_height="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Height)"
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
stop_app
FIT_SMOKE
then
  echo "Window-fit smoke failed. Session log: $OUT_DIR/window-fit-session.log" >&2
  cat "$OUT_DIR/window-fit-session.log" >&2
  exit 1
fi

echo "Main window fit smoke passed. Screenshots: $OUT_DIR/fit-small-window.png, $OUT_DIR/fit-4k-right-monitor-window.png"
