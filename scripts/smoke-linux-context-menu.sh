#!/usr/bin/env bash
# X11/Xvfb smoke for the player-wide context-menu routing. It verifies that
# non-interactive player surfaces open one canonical menu while controls and
# existing popovers retain their own secondary-click handling.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-context-menu-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }

rm -rf "$OUT_DIR"
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
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CONFIG_HOME="$OUT_DIR/config"
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_DEBUG_INTERACTIONS=1
export OKP_PLAYBACK_FRAME_PREVIEW=bright
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_SKIP_UPDATE_CHECK=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

context_count() {
  grep -c 'interaction: player-context-menu-open' "$OUT_DIR/app.log" 2>/dev/null || true
}

open_context() {
  local window_id="$1" x="$2" y="$3" capture="$4"
  local before after attempt
  before="$(context_count)"
  after="$before"
  for attempt in 1 2 3; do
    xdotool mousemove --window "$window_id" "$x" "$y" click 3
    sleep 1
    after="$(context_count)"
    [[ "$after" -gt "$before" ]] && break
  done
  [[ "$after" -eq $((before + 1)) ]] || {
    echo "context click at ${x},${y} opened $((after - before)) menus" >&2
    exit 1
  }
  import -window root "$OUT_DIR/$capture"
  xdotool key --clearmodifiers Escape
  sleep 1
}

activate_context_action() {
  local window_id="$1" tab_count="$2" action="$3" capture="$4"
  local before_context before_action after_context after_action
  before_context="$(context_count)"
  before_action="$(grep -c "interaction: video-geometry=${action}" "$OUT_DIR/app.log" || true)"
  xdotool mousemove --window "$window_id" 560 330 click 3
  sleep 1
  for ((tab = 0; tab < tab_count; tab++)); do
    xdotool key --clearmodifiers Tab
  done
  sleep 1
  import -window root "$OUT_DIR/$capture"
  xdotool key --clearmodifiers Return
  sleep 1
  after_context="$(context_count)"
  after_action="$(grep -c "interaction: video-geometry=${action}" "$OUT_DIR/app.log" || true)"
  [[ "$after_context" -eq $((before_context + 1)) ]] || {
    echo "geometry keyboard probe opened $((after_context - before_context)) menus" >&2
    exit 1
  }
  [[ "$after_action" -eq $((before_action + 1)) ]] || {
    echo "geometry keyboard probe did not activate ${action}" >&2
    exit 1
  }
}

capture_context_focus() {
  local window_id="$1" tab_count="$2" capture="$3"
  local before after
  before="$(context_count)"
  xdotool mousemove --window "$window_id" 560 330 click 3
  sleep 1
  for ((tab = 0; tab < tab_count; tab++)); do
    xdotool key --clearmodifiers Tab
  done
  sleep 1
  import -window root "$OUT_DIR/$capture"
  xdotool key --clearmodifiers Escape
  sleep 1
  after="$(context_count)"
  [[ "$after" -eq $((before + 1)) ]] || {
    echo "geometry selected-state probe opened $((after - before)) menus" >&2
    exit 1
  }
}

timeout 50s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
sleep 7
window_id="$(xdotool search --onlyvisible --name 'OK Player' | head -n1)"
[[ -n "$window_id" ]] || { cat "$OUT_DIR/app.log" >&2; exit 1; }
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true

drag_result=pass
if [[ "${OKP_CONTEXT_SMOKE_SKIP_DRAG:-0}" != 1 ]]; then
  # The new secondary-click controller must not interfere with the existing
  # primary-button title drag. Move the window through the real WM drag path,
  # then restore the fixed origin used by the coordinate probes below.
  xwininfo -id "$window_id" >"$OUT_DIR/before-drag.xwininfo"
  before_drag_x="$(awk '/Absolute upper-left X:/ {print $4; exit}' "$OUT_DIR/before-drag.xwininfo")"
  before_drag_y="$(awk '/Absolute upper-left Y:/ {print $4; exit}' "$OUT_DIR/before-drag.xwininfo")"
  xdotool mousemove --window "$window_id" 300 20 mousedown 1 sleep 0.25 \
    mousemove_relative --sync 80 60 sleep 0.25 mouseup 1
  sleep 1
  xwininfo -id "$window_id" >"$OUT_DIR/after-drag.xwininfo"
  after_drag_x="$(awk '/Absolute upper-left X:/ {print $4; exit}' "$OUT_DIR/after-drag.xwininfo")"
  after_drag_y="$(awk '/Absolute upper-left Y:/ {print $4; exit}' "$OUT_DIR/after-drag.xwininfo")"
  [[ "$before_drag_x" != "$after_drag_x" || "$before_drag_y" != "$after_drag_y" ]] || {
    echo "primary-button title drag did not move the window" >&2
    exit 1
  }
  xdotool windowmove "$window_id" 0 0
  sleep 1

  # #329 whole-surface move: a left-drag that clears the threshold over the video
  # canvas (not the title bar) must also begin a compositor move, without
  # committing the pending play/pause single click.
  video_commits_before="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
  xwininfo -id "$window_id" >"$OUT_DIR/before-video-drag.xwininfo"
  before_video_x="$(awk '/Absolute upper-left X:/ {print $4; exit}' "$OUT_DIR/before-video-drag.xwininfo")"
  before_video_y="$(awk '/Absolute upper-left Y:/ {print $4; exit}' "$OUT_DIR/before-video-drag.xwininfo")"
  xdotool mousemove --window "$window_id" 560 330 mousedown 1 sleep 0.25 \
    mousemove_relative --sync 90 70 sleep 0.25 mouseup 1
  sleep 1
  xwininfo -id "$window_id" >"$OUT_DIR/after-video-drag.xwininfo"
  after_video_x="$(awk '/Absolute upper-left X:/ {print $4; exit}' "$OUT_DIR/after-video-drag.xwininfo")"
  after_video_y="$(awk '/Absolute upper-left Y:/ {print $4; exit}' "$OUT_DIR/after-video-drag.xwininfo")"
  [[ "$before_video_x" != "$after_video_x" || "$before_video_y" != "$after_video_y" ]] || {
    echo "primary-button video-surface drag did not move the window" >&2
    exit 1
  }
  grep -q 'interaction: player-window-move' "$OUT_DIR/app.log" || {
    echo "video-surface drag did not report a window move" >&2
    exit 1
  }
  video_commits_after="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
  [[ "$video_commits_before" == "$video_commits_after" ]] || {
    echo "video-surface drag leaked a play/pause single click" >&2
    exit 1
  }
  xdotool windowmove "$window_id" 0 0
  sleep 1
else
  drag_result=skipped
fi

# Video/canvas, title/background, and an empty OSC gap all route to the same
# Advanced commands popover. Each click must produce exactly one open event.
open_context "$window_id" 560 330 video-context.png
open_context "$window_id" 300 20 title-context.png
open_context "$window_id" 650 638 chrome-gap-context.png

# The primary-button double-click policy remains independent: it cancels the
# delayed single-click commit and enters fullscreen exactly as before.
commits_before_double="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
xdotool mousemove --window "$window_id" 560 330 click --repeat 2 --delay 100 1
sleep 1
xwininfo -id "$window_id" >"$OUT_DIR/fullscreen.xwininfo"
fullscreen_width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/fullscreen.xwininfo")"
fullscreen_height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/fullscreen.xwininfo")"
[[ "$fullscreen_width" == 1280 && "$fullscreen_height" == 900 ]] || {
  echo "double-click did not enter fullscreen: ${fullscreen_width}x${fullscreen_height}" >&2
  exit 1
}
commits_after_double="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
[[ "$commits_before_double" == "$commits_after_double" ]] || {
  echo "double-click committed a pending single click" >&2
  exit 1
}
xdotool key --clearmodifiers Escape
sleep 1

# Moving the player against the lower-right workarea edge forces GTK to flip
# and clamp the click-anchored popover rather than placing it off-screen.
xdotool windowmove "$window_id" 160 220
sleep 1
open_context "$window_id" 1090 520 workarea-edge-context.png
xdotool windowmove "$window_id" 0 0
sleep 1

# A real OSC button remains an interaction owner. Its secondary click is
# suppressed instead of leaking through to the player menu.
before_control="$(context_count)"
for point in '48 638' '400 638' '610 638' '675 638'; do
  read -r control_x control_y <<<"$point"
  xdotool mousemove --window "$window_id" "$control_x" "$control_y" click 3
done
sleep 1
after_control="$(context_count)"
[[ "$before_control" == "$after_control" ]] || {
  echo "right-click on an OSC control leaked into the player menu" >&2
  exit 1
}
grep -q 'interaction: player-context-menu-suppressed' "$OUT_DIR/app.log" || {
  echo "control suppression was not observed" >&2
  exit 1
}

# The existing More popover is a separate native interaction surface. A
# secondary click inside it must not create the player context menu.
xdotool mousemove --window "$window_id" 1070 638 click 1
sleep 1
import -window root "$OUT_DIR/more-popover.png"
before_popover="$(context_count)"
xdotool mousemove --window "$window_id" 1010 540 click 3
sleep 1
after_popover="$(context_count)"
[[ "$before_popover" == "$after_popover" ]] || {
  echo "right-click inside the More popover opened the player menu" >&2
  exit 1
}
xdotool key --clearmodifiers Escape
sleep 1

# Tab focus remains inside the canonical menu and Escape closes it, preserving
# native keyboard traversal without adding a second menu implementation.
before_keyboard="$(context_count)"
xdotool mousemove --window "$window_id" 560 330 click 3
sleep 1
xdotool key --clearmodifiers Tab
sleep 1
import -window root "$OUT_DIR/keyboard-focus-context.png"
xdotool key --clearmodifiers Escape
sleep 1
after_keyboard="$(context_count)"
[[ "$after_keyboard" -eq $((before_keyboard + 1)) ]] || {
  echo "keyboard context-menu probe did not open exactly one menu" >&2
  exit 1
}

# The tucked-away Video group is keyboard reachable inside the canonical
# context menu. Exercise the real libmpv command path and capture the focused
# zoom/pan/deinterlace rows after GTK scrolls them into view. These commands do
# not exist in the primary More popover captured above.
activate_context_action "$window_id" 17 ZoomIn geometry-zoom-in-focus.png
activate_context_action "$window_id" 19 PanLeft geometry-pan-left-focus.png
activate_context_action "$window_id" 26 ToggleDeinterlace geometry-deinterlace-focus.png
capture_context_focus "$window_id" 26 geometry-deinterlace-selected.png

# Repeat with no media: the welcome card's surrounding empty canvas is still a
# player surface, while its actual buttons remain protected by the same filter.
kill "$app_pid" 2>/dev/null || true
wait "$app_pid" 2>/dev/null || true
app_pid=""
mv "$OUT_DIR/app.log" "$OUT_DIR/playback-app.log"
timeout 30s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
sleep 6
empty_window_id="$(xdotool search --onlyvisible --name 'OK Player' | head -n1)"
[[ -n "$empty_window_id" ]] || { cat "$OUT_DIR/app.log" >&2; exit 1; }
xdotool windowactivate "$empty_window_id" >/dev/null 2>&1 || true
open_context "$empty_window_id" 100 300 empty-canvas-context.png

playback_opens="$(grep -c 'interaction: player-context-menu-open' "$OUT_DIR/playback-app.log" || true)"
empty_opens="$(context_count)"
[[ "$playback_opens" -eq 9 && "$empty_opens" -eq 1 ]] || {
  echo "unexpected context-menu counts: playback=$playback_opens empty=$empty_opens" >&2
  exit 1
}

printf '%s\n' \
  'video_surface=pass' \
  'title_background=pass' \
  'chrome_gap=pass' \
  'workarea_edge_anchor=pass' \
  'control_isolation=pass' \
  'popover_isolation=pass' \
  'keyboard_traversal=pass' \
  'geometry_zoom=pass' \
  'geometry_pan=pass' \
  'geometry_deinterlace=pass' \
  'empty_canvas=pass' \
  "left_drag=$drag_result" \
  'double_click_fullscreen=pass' \
  "playback_menu_open_count=$playback_opens" \
  "empty_menu_open_count=$empty_opens" >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Context-menu smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Context-menu smoke passed. Results: $OUT_DIR/results.txt"
