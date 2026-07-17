#!/usr/bin/env bash
# Mapped X11 smoke for the long-lived companion-window contract. It exercises
# real WM move/resize gestures, single-instance focus, lifetime geometry,
# uninterrupted player input, owner cleanup, themes, and full-window captures.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-media-info-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import xprop rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }

if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath "$BINARY")"
fi

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
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_DEBUG_INTERACTIONS=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

window_geometry() {
  xwininfo -id "$1" | awk '
    /Absolute upper-left X:/ { x=$4 }
    /Absolute upper-left Y:/ { y=$4 }
    /Width:/ { w=$2 }
    /Height:/ { h=$2 }
    END { print x, y, w, h }
  '
}

wait_for_window() {
  local title="$1"
  local id=""
  for _ in {1..30}; do
    id="$(xdotool search --onlyvisible --name "^${title}$" 2>/dev/null | head -n1 || true)"
    [[ -n "$id" ]] && break
    sleep 0.25
  done
  [[ -n "$id" ]] || { echo "Window did not map: $title" >&2; exit 1; }
  printf '%s\n' "$id"
}

wait_for_player_window() {
  local id=""
  local title=""
  for _ in {1..30}; do
    while read -r candidate; do
      [[ -n "$candidate" ]] || continue
      title="$(xdotool getwindowname "$candidate" 2>/dev/null || true)"
      if [[ "$title" != "Media Information" && "$title" != "Settings" ]]; then
        id="$candidate"
        break
      fi
    done < <(xdotool search --onlyvisible --class 'okp-linux-gtk' 2>/dev/null || true)
    [[ -n "$id" ]] && break
    sleep 0.25
  done
  [[ -n "$id" ]] || { echo "Player window did not map" >&2; exit 1; }
  printf '%s\n' "$id"
}

assert_single_window() {
  local title="$1"
  local count
  count="$(xdotool search --onlyvisible --name "^${title}$" 2>/dev/null | wc -l)"
  [[ "$count" == "1" ]] || {
    echo "Expected one visible '$title' window, found $count" >&2
    exit 1
  }
}

wait_for_window_absence() {
  local title="$1"
  for _ in {1..20}; do
    if ! xdotool search --onlyvisible --name "^${title}$" >/dev/null 2>&1; then
      return
    fi
    sleep 0.25
  done
  echo "Window remained visible after close: $title" >&2
  exit 1
}

assert_normal_utility_properties() {
  local id="$1"
  local name="$2"
  xprop -id "$id" _NET_WM_STATE WM_TRANSIENT_FOR >"$OUT_DIR/$name-properties.txt" 2>&1 || true
  if rg -q '_NET_WM_STATE_MODAL|_NET_WM_STATE_ABOVE' "$OUT_DIR/$name-properties.txt"; then
    echo "$name unexpectedly has modal/above state" >&2
    cat "$OUT_DIR/$name-properties.txt" >&2
    exit 1
  fi
  if rg -q 'WM_TRANSIENT_FOR\(WINDOW\): window id #' "$OUT_DIR/$name-properties.txt"; then
    echo "$name unexpectedly remains transient-for the player" >&2
    cat "$OUT_DIR/$name-properties.txt" >&2
    exit 1
  fi
}

drag_window() {
  local id="$1"
  read -r before_x before_y width height < <(window_geometry "$id")
  xdotool windowactivate --sync "$id"
  xdotool mousemove --window "$id" 220 28 mousedown 1
  sleep 0.2
  xdotool mousemove_relative --sync -- 90 55
  xdotool mouseup 1
  sleep 1
  read -r after_x after_y _ _ < <(window_geometry "$id")
  if [[ "$before_x" == "$after_x" && "$before_y" == "$after_y" ]]; then
    echo "Window did not move through its app-owned drag region: $id" >&2
    exit 1
  fi
  printf '%s %s %s %s\n' "$after_x" "$after_y" "$width" "$height"
}

resize_window() {
  local id="$1"
  read -r _ _ before_width before_height < <(window_geometry "$id")
  xdotool windowactivate --sync "$id"
  xdotool mousemove --window "$id" $((before_width - 3)) $((before_height - 3)) \
    mousedown 1
  sleep 0.2
  xdotool mousemove_relative --sync -- 120 90
  xdotool mouseup 1
  sleep 1
  read -r _ _ after_width after_height < <(window_geometry "$id")
  if (( after_width <= before_width || after_height <= before_height )); then
    echo "Window did not resize from its south-east WM hit zone: $id" >&2
    exit 1
  fi
  printf '%s %s\n' "$after_width" "$after_height"
}

state_dir="$OUT_DIR/state"
mkdir -p "$state_dir/config/ok-player" "$state_dir/state"
printf '%s\n' '{"version":1,"appearance":{"theme":"light"},"updates":{"auto_check":false}}' \
  >"$state_dir/config/ok-player/settings.json"

env \
  XDG_CONFIG_HOME="$state_dir/config" \
  XDG_STATE_HOME="$state_dir/state" \
  OKP_SETTINGS_COLOR_SCHEME=light \
  OKP_OPEN_MEDIA_INFO_ON_STARTUP=1 \
  OKP_OPEN_SETTINGS_ON_STARTUP=1 \
  timeout 45s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

player_id="$(wait_for_player_window)"
media_id="$(wait_for_window 'Media Information')"
settings_id="$(wait_for_window 'Settings')"
sleep 2

assert_normal_utility_properties "$media_id" media-info
assert_normal_utility_properties "$settings_id" settings
assert_single_window 'Media Information'
assert_single_window 'Settings'

read -r media_x media_y media_width media_height < <(window_geometry "$media_id")
read -r settings_x settings_y settings_width settings_height < <(window_geometry "$settings_id")
[[ "$media_width" == "720" && "$media_height" == "571" ]] || {
  echo "Unexpected Media Information natural size: ${media_width}x${media_height}" >&2
  exit 1
}
[[ "$settings_width" == "760" && "$settings_height" == "560" ]] || {
  echo "Unexpected Settings natural size: ${settings_width}x${settings_height}" >&2
  exit 1
}
(( media_x >= 0 && media_y >= 0 && media_x + media_width <= 1280 && media_y + media_height <= 852 )) || {
  echo "Media Information did not clamp inside the active workarea" >&2
  exit 1
}
(( settings_x >= 0 && settings_y >= 0 && settings_x + settings_width <= 1280 && settings_y + settings_height <= 852 )) || {
  echo "Settings did not clamp inside the active workarea" >&2
  exit 1
}

import -window "$media_id" "$OUT_DIR/media-info-natural-light.png"
import -window "$settings_id" "$OUT_DIR/settings-natural-light.png"

drag_window "$media_id" >"$OUT_DIR/media-info-moved.txt"
read -r media_resized_width media_resized_height < <(resize_window "$media_id")
import -window "$media_id" "$OUT_DIR/media-info-resized-light.png"

drag_window "$settings_id" >"$OUT_DIR/settings-moved.txt"
read -r settings_resized_width settings_resized_height < <(resize_window "$settings_id")
import -window "$settings_id" "$OUT_DIR/settings-resized-light.png"

# Reopening an already-open surface raises the same instance instead of stacking.
xdotool windowactivate --sync "$player_id"
sleep 0.2
xdotool key --clearmodifiers ctrl+comma
xdotool windowactivate --sync "$player_id"
sleep 0.2
xdotool key --clearmodifiers i
sleep 1
assert_single_window 'Media Information'
assert_single_window 'Settings'
rg -q 'interaction: companion=media-info focus-existing' "$OUT_DIR/app.log"
rg -q 'interaction: companion=settings focus-existing' "$OUT_DIR/app.log"

# Parent playback, seek, volume, menu, move, and fullscreen remain live.
xdotool windowactivate --sync "$player_id"
xdotool key --clearmodifiers space
xdotool key --clearmodifiers Right
xdotool key --clearmodifiers Up
xdotool mousemove --window "$player_id" 560 330 click 3
sleep 1
xdotool key --clearmodifiers Escape
xdotool key --clearmodifiers f
sleep 1
read -r _ _ fullscreen_width fullscreen_height < <(window_geometry "$player_id")
[[ "$fullscreen_width" == "1280" && "$fullscreen_height" == "900" ]] || {
  echo "Player did not enter fullscreen while companions were open" >&2
  exit 1
}
xdotool key --clearmodifiers Escape
sleep 1
drag_window "$player_id" >"$OUT_DIR/player-moved.txt"

for action in play-pause seek-forward volume-up fullscreen; do
  rg -q "interaction: keyboard=$action" "$OUT_DIR/app.log" || {
    echo "Parent keyboard action did not execute with companions open: $action" >&2
    exit 1
  }
done
rg -q 'interaction: player-context-menu-open' "$OUT_DIR/app.log" || {
  echo "Player context menu did not open with companions present" >&2
  exit 1
}

# Closing each companion leaves the player alive and restores its lifetime size.
xdotool windowactivate --sync "$media_id"
xdotool key --clearmodifiers Escape
sleep 1
xwininfo -id "$player_id" >/dev/null
assert_single_window 'Settings'
xdotool windowactivate --sync "$player_id"
xdotool key --clearmodifiers i
media_id="$(wait_for_window 'Media Information')"
read -r _ _ reopened_media_width reopened_media_height < <(window_geometry "$media_id")
[[ "$reopened_media_width" == "$media_resized_width" && "$reopened_media_height" == "$media_resized_height" ]] || {
  echo "Media Information did not restore lifetime geometry: resized=${media_resized_width}x${media_resized_height} reopened=${reopened_media_width}x${reopened_media_height}" >&2
  exit 1
}

xdotool windowactivate --sync "$settings_id"
xdotool key --clearmodifiers alt+F4
wait_for_window_absence 'Settings'
xwininfo -id "$player_id" >/dev/null
assert_single_window 'Media Information'
xdotool windowactivate --sync "$player_id"
xdotool key --clearmodifiers ctrl+comma
settings_id="$(wait_for_window 'Settings')"
read -r _ _ reopened_settings_width reopened_settings_height < <(window_geometry "$settings_id")
[[ "$reopened_settings_width" == "$settings_resized_width" && "$reopened_settings_height" == "$settings_resized_height" ]] || {
  echo "Settings did not restore lifetime geometry" >&2
  exit 1
}

# Closing the owner cleans up every companion and exits the process.
xdotool windowactivate --sync "$player_id"
xdotool key --clearmodifiers alt+F4
for _ in {1..20}; do
  if ! kill -0 "$app_pid" 2>/dev/null; then
    break
  fi
  sleep 0.25
done
if kill -0 "$app_pid" 2>/dev/null; then
  echo "Player process remained alive after closing its owner window" >&2
  exit 1
fi
if xdotool search --onlyvisible --name '^Media Information$|^Settings$' >/dev/null 2>&1; then
  echo "Companion window remained visible after the player closed" >&2
  exit 1
fi
app_pid=""

capture_theme() {
  local name="$1"
  local scheme="$2"
  local gtk_theme="$3"
  local theme_dir="$OUT_DIR/$name-state"
  mkdir -p "$theme_dir/config/ok-player" "$theme_dir/state"
  printf '%s\n' '{"version":1,"updates":{"auto_check":false}}' \
    >"$theme_dir/config/ok-player/settings.json"

  env \
    XDG_CONFIG_HOME="$theme_dir/config" \
    XDG_STATE_HOME="$theme_dir/state" \
    OKP_SETTINGS_COLOR_SCHEME="$scheme" \
    GTK_THEME="$gtk_theme" \
    OKP_OPEN_MEDIA_INFO_ON_STARTUP=1 \
    OKP_OPEN_SETTINGS_ON_STARTUP=1 \
    timeout 20s "$BINARY" >"$OUT_DIR/$name-app.log" 2>&1 &
  app_pid=$!
  local themed_media themed_settings
  themed_media="$(wait_for_window 'Media Information')"
  themed_settings="$(wait_for_window 'Settings')"
  sleep 2
  import -window "$themed_media" "$OUT_DIR/media-info-natural-$name.png"
  import -window "$themed_settings" "$OUT_DIR/settings-natural-$name.png"
  local themed_player
  themed_player="$(wait_for_player_window)"
  xdotool windowactivate --sync "$themed_player"
  xdotool key --clearmodifiers alt+F4
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture_theme dark dark Adwaita:dark
capture_theme high-contrast dark HighContrast

printf '%s\n' \
  "media_natural=720x571" \
  "media_resized=${media_resized_width}x${media_resized_height}" \
  "settings_natural=760x560" \
  "settings_resized=${settings_resized_width}x${settings_resized_height}" \
  "single_instance=pass" \
  "parent_interaction=pass" \
  "owner_cleanup=pass" >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Companion-window smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Companion-window smoke passed. Captures: $OUT_DIR"
