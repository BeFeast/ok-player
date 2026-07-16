#!/usr/bin/env bash
# Operator QA for captionless window movement on a packaged GNOME/Wayland build.
# This intentionally requires real pointer input: headless rendering and input
# synthesis cannot prove compositor-native movement, focus, or control behavior.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-window-drag-wayland-qa}"
FIXTURE="${3:-$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv}"

if [[ "${XDG_SESSION_TYPE:-}" != "wayland" || -z "${WAYLAND_DISPLAY:-}" ]]; then
  echo "Run this QA from a live Wayland desktop session." >&2
  exit 2
fi
if [[ "${XDG_CURRENT_DESKTOP:-}" != *GNOME* ]]; then
  echo "Run this QA from the required live GNOME desktop session." >&2
  exit 2
fi
if ! command -v "$BINARY" >/dev/null 2>&1 && [[ ! -x "$BINARY" ]]; then
  echo "Packaged player binary is not executable: $BINARY" >&2
  exit 127
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing playback fixture: $FIXTURE" >&2
  exit 127
fi
if ! command -v gnome-screenshot >/dev/null 2>&1; then
  echo "Missing required GNOME screenshot tool: gnome-screenshot" >&2
  exit 127
fi

if [[ -d "$OUT_DIR" && -n "$(find "$OUT_DIR" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "Choose an empty evidence directory: $OUT_DIR" >&2
  exit 2
fi
mkdir -p "$OUT_DIR"

app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

confirm() {
  local prompt="$1" answer
  read -r -p "$prompt [y/N] " answer
  [[ "$answer" == y || "$answer" == Y ]]
}

capture_player() {
  local name="$1"
  echo "Select the complete player window for the $name capture."
  gnome-screenshot -a -f "$OUT_DIR/$name.png"
}

start_player() {
  local log="$1"
  shift
  env OKP_DEBUG_INTERACTIONS=1 OKP_SKIP_UPDATE_CHECK=1 \
    OKP_SKIP_OPEN_INSTALLER=1 OKP_SKIP_DEB_SELF_INSTALL=1 \
    "$BINARY" "$@" >"$OUT_DIR/$log" 2>&1 &
  app_pid=$!
  sleep 4
}

stop_player() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

confirm "Is '$BINARY' the installed or otherwise packaged build being accepted, not a Cargo development binary?" || exit 1

echo "Idle-window phase: use a normal mouse or touchpad; do not use input synthesis."
start_player idle-app.log
capture_player idle-before
confirm "Does a left-drag from blank idle canvas move the restored window only after a small threshold?" || exit 1
confirm "Does a left-drag from empty top chrome move the restored window?" || exit 1
confirm "Do the Open target, recent cards, History list, footer buttons, and resize handles retain their normal behavior without moving the window?" || exit 1
capture_player idle-after
stop_player

echo "Playback phase: verify both movement and control isolation with real media."
start_player playback-app.log "$FIXTURE"
capture_player playback-before
confirm "Does a simple video click toggle play/pause once without moving the window?" || exit 1
confirm "Does a video-canvas left-drag move the restored window without toggling play/pause?" || exit 1
confirm "Does a left-drag from empty top chrome move the restored window?" || exit 1
confirm "Do seek and volume drags adjust their controls without moving the window?" || exit 1
confirm "Do player buttons, menus, popovers, and the scrollable side panel retain normal click, drag, and scroll behavior?" || exit 1
confirm "While maximized, is video/title dragging ignored while controls remain usable?" || exit 1
confirm "While fullscreen, is video dragging ignored while click, double-click, and controls remain usable?" || exit 1
capture_player playback-after
stop_player

native_move_starts="$({ grep -h -c 'interaction: native-window-move-started' \
  "$OUT_DIR/idle-app.log" "$OUT_DIR/playback-app.log" || true; } | awk '{ total += $1 } END { print total + 0 }')"
if (( native_move_starts < 3 )); then
  echo "The app did not record all three required native move starts: count=$native_move_starts" >&2
  exit 1
fi

cat >"$OUT_DIR/results.json" <<JSON
{
  "schema_version": 1,
  "session": "GNOME/Wayland",
  "input": "live operator pointer input",
  "packaged_binary": true,
  "idle_canvas_drag": "pass",
  "empty_top_chrome_drag": "pass",
  "video_canvas_drag": "pass",
  "simple_video_click": "pass",
  "seek_volume_isolation": "pass",
  "button_menu_popover_list_isolation": "pass",
  "resize_handle_isolation": "pass",
  "maximized_drag_block": "pass",
  "fullscreen_drag_block": "pass",
  "native_move_starts": $native_move_starts,
  "headless_limit": "not applicable; a live operator performed compositor checks"
}
JSON

echo "Live GNOME/Wayland window-drag QA passed. Evidence: $OUT_DIR/results.json"
