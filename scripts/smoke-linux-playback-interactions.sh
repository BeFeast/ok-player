#!/usr/bin/env bash
# Real-mpv interaction smoke for the canonical player canvas. It proves the
# delayed single-click commit, double-click cancellation/fullscreen, clickable
# total/remaining label, always-on-top action, and gesture isolation from the
# seek bar and side-panel control.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-playback-interactions-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick xprop; do
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
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_DEBUG_INTERACTIONS=1
export OKP_PLAYBACK_FRAME_PREVIEW=bright
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

timeout 40s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
sleep 6
window_id="$(xdotool search --name 'OK Player' | head -n1)"
[[ -n "$window_id" ]] || { cat "$OUT_DIR/app.log" >&2; exit 1; }
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 1

# A single canvas click commits after the desktop double-click interval. Paused
# playback pins the OSC, so it remains visible well past the 2.5s idle timeout.
xdotool mousemove --window "$window_id" 560 330 click 1
sleep 4
import -window "$window_id" "$OUT_DIR/single-click-paused.png"
paused_bottom_max="$(magick "$OUT_DIR/single-click-paused.png" -crop 1088x80+16+582 -colorspace gray -format '%[fx:maxima]' info:)"
awk -v value="$paused_bottom_max" 'BEGIN { exit !(value > 0.50) }' || {
  echo "single click did not pause/pin chrome: bottom maxima=$paused_bottom_max" >&2
  exit 1
}

# The trailing label is a real click target and toggles remaining -> total.
xdotool mousemove --window "$window_id" 610 638 click 1
sleep 1
grep -q 'interaction: time-label=Total' "$OUT_DIR/app.log" || {
  echo "time label click did not toggle to total" >&2
  exit 1
}

# Seek and panel actions live above the GLArea. They must not schedule a video
# click while remaining functional targets of their own.
commits_before="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
xdotool mousemove --window "$window_id" 400 638 click 1
xdotool mousemove --window "$window_id" 914 638 click 1
sleep 1
xdotool mousemove --window "$window_id" 914 638 click 1
sleep 1
commits_after="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
[[ "$commits_before" == "$commits_after" ]] || {
  echo "seek/panel interaction leaked into video click handling" >&2
  exit 1
}

# Resume with one click, then wait for playing-idle chrome to clear.
xdotool mousemove --window "$window_id" 560 330 click 1
sleep 4
import -window "$window_id" "$OUT_DIR/single-click-playing.png"
playing_bottom_mean="$(magick "$OUT_DIR/single-click-playing.png" -crop 900x54+110+604 -colorspace gray -format '%[fx:mean]' info:)"
awk -v value="$playing_bottom_mean" 'BEGIN { exit !(value > 0.80) }' || {
  echo "second single click did not resume/auto-hide: bottom mean=$playing_bottom_mean" >&2
  exit 1
}

# Double-click must cancel its pending single-click and enter fullscreen without
# a pause flash. A playing fullscreen surface still clears its OSC after idle.
commits_before_double="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
xdotool mousemove --window "$window_id" 560 330 click --repeat 2 --delay 100 1
sleep 1
xwininfo -id "$window_id" >"$OUT_DIR/fullscreen.xwininfo"
fs_width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/fullscreen.xwininfo")"
fs_height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/fullscreen.xwininfo")"
[[ "$fs_width" == 1280 && "$fs_height" == 900 ]] || {
  echo "double click did not enter fullscreen: ${fs_width}x${fs_height}" >&2
  exit 1
}
sleep 4
commits_after_double="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
[[ "$commits_before_double" == "$commits_after_double" ]] || {
  echo "double click committed the pending single click" >&2
  exit 1
}
grep -q 'interaction: video-double-click-fullscreen' "$OUT_DIR/app.log" || {
  echo "double-click intent was not observed" >&2
  exit 1
}
xdotool key --clearmodifiers Escape
sleep 1

# The canonical pin is a real EWMH above-state action on X11.
xdotool mousemove --window "$window_id" 959 21 click 1
sleep 1
xprop -id "$window_id" _NET_WM_STATE >"$OUT_DIR/window-state.txt"
grep -q '_NET_WM_STATE_ABOVE' "$OUT_DIR/window-state.txt" || {
  echo "always-on-top action did not set _NET_WM_STATE_ABOVE" >&2
  cat "$OUT_DIR/window-state.txt" >&2
  exit 1
}

scheduled="$(grep -c 'video-single-click-scheduled' "$OUT_DIR/app.log" || true)"
committed="$(grep -c 'video-single-click-committed' "$OUT_DIR/app.log" || true)"
printf '%s\n' \
  "single_click_scheduled=$scheduled" \
  "single_click_committed=$committed" \
  "double_click_fullscreen=pass" \
  "time_label_toggle=pass" \
  "seek_panel_isolation=pass" \
  "always_on_top=pass" >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Playback interaction smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Playback interaction smoke passed. Results: $OUT_DIR/results.txt"
