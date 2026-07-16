#!/usr/bin/env bash
# Visual smoke guard for the PRD P1-D3 / §2.1 fullscreen chrome-persistence rule
# (issue #235): in fullscreen at rest, zero pixels of persistent UI remain over
# the video. The titlebar (`okp-window-chrome`) is fully hidden and the OSC is
# the only chrome — it auto-hides after the canonical idle timeout while playing
# and pins while paused. This script loads real media, enters fullscreen, waits
# past the idle timeout while playing and asserts the chrome band clears, then
# pauses and asserts the chrome returns. Pixel-based, like the sibling
# smoke-linux-*.sh guards.
#
# Needs real media (mpv decode advances playback so the OSC auto-hide timer
# arms), which is why it is tracked separately from the preview-fixture smokes.
# The tiny synthetic clip in tests/OkPlayer.IntegrationTests/fixtures is a dark
# 1280x720 H.264 stream, so a near-black maximum in the chrome bands means the
# chrome cleared; a bright maximum means the OSC/titlebar drew.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-fullscreen-chrome-smoke}"
FIXTURE="${3:-$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv}"
SUBSTRATE="${4:-dark}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi
if [[ "$SUBSTRATE" != dark && "$SUBSTRATE" != bright ]]; then
  echo "Fullscreen substrate must be 'dark' or 'bright': $SUBSTRATE" >&2
  exit 2
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$FIXTURE" "$SUBSTRATE" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
FIXTURE="$3"
SUBSTRATE="$4"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export OKP_SKIP_UPDATE_CHECK=1
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

# Let the window manager settle before any GTK window is created (the sibling
# smokes do the same).
sleep 1

# Load the fixture clip via the command line so mpv starts decoding immediately
# on GLArea realize. mpv defaults to playing (pause=no), so the OSC auto-hide
# timer arms as soon as the state poll observes playback.
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 30s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

# Wait for the window to appear and the media to load and start playing. The
# state poll runs every 200 ms, so 6 s is plenty for the pump to observe
# time-pos / pause and enable auto-hide.
sleep 6

xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"
if [[ -z "$window_id" ]]; then
  echo "main window did not appear" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi

# Drive the app with real (XTest) key events: synthetic XSendEvent events are
# filtered by GTK4, so focus the window first and let xdotool inject keysyms
# the EventControllerKey path actually receives.
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 1

# Enter fullscreen (default F binding). The keypress also pings
# chrome.show_for_activity(), which reveals the OSC and schedules a hide — the
# exact idle timer whose expiry this smoke waits out.
xdotool key --clearmodifiers f

# Let xfwm4 apply the fullscreen transition and confirm the window now covers
# the whole screen (1280x900), i.e. the titlebar really went fullscreen.
sleep 1
xwininfo -id "$window_id" >"$OUT_DIR/window-fullscreen.xwininfo"
width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window-fullscreen.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window-fullscreen.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window-fullscreen.xwininfo")"
if [[ "$width" != "1280" || "$height" != "900" || "$state" != "IsViewable" ]]; then
  echo "fullscreen geometry wrong: ${width}x${height}, state=${state}" >&2
  exit 1
fi

# Wait well past the canonical idle timeout (~2.5 s) with no pointer/keyboard
# activity so the OSC auto-hide fires. The titlebar is hidden by the fullscreen
# notify handler, so the top band should be pure letterbox/video and the bottom
# band should be clear too.
sleep 4
import -window root "$OUT_DIR/fullscreen-playing.png"

# Top band: the custom titlebar (`okp-window-chrome`) carries bright caption
# glyphs when visible. In fullscreen it is set invisible, so a dark maximum
# here means the titlebar cleared (a regression that keeps it visible would
# leave bright button glyphs in this strip).
top_max="$(
  magick "$OUT_DIR/fullscreen-playing.png" \
    -crop 1280x50+0+0 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
top_mean="$(magick "$OUT_DIR/fullscreen-playing.png" -crop 1280x50+0+0 -colorspace gray -format '%[fx:mean]' info:)"

# Bottom band: the OSC revealer sits at the bottom (valign End, ~18 px margin).
# When auto-hidden while playing, this strip is letterbox/video only, so a dark
# maximum means the chrome band cleared.
bottom_max="$(
  magick "$OUT_DIR/fullscreen-playing.png" \
    -crop 1280x90+0+810 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
bottom_mean="$(magick "$OUT_DIR/fullscreen-playing.png" -crop 1280x90+0+810 -colorspace gray -format '%[fx:mean]' info:)"
frame_mean="$(magick "$OUT_DIR/fullscreen-playing.png" -crop 700x360+290+180 -colorspace gray -format '%[fx:mean]' info:)"
if ! awk -v top="$top_max" -v bottom="$bottom_max" 'BEGIN { exit !(top < 0.12 && bottom < 0.12) }'; then
  echo "fullscreen chrome did not clear over the letterbox bands: top=${top_max} bottom=${bottom_max}" >&2
  exit 1
fi
if [[ "$SUBSTRATE" == bright ]] && ! awk -v frame="$frame_mean" 'BEGIN { exit !(frame > 0.75) }'; then
  echo "bright fullscreen video is not visible: frame=${frame_mean}" >&2
  exit 1
fi

# Pause (default Space binding). The keypress reveals the OSC, then the next
# state-poll tick observes pause and calls set_auto_hide_enabled(false), which
# pins the chrome persistently — so the OSC must return and stay.
xdotool key --clearmodifiers space
sleep 2
import -window root "$OUT_DIR/fullscreen-paused.png"

# The OSC carries bright white icon glyphs over its translucent bar, so a bright
# maximum in the bottom band means the chrome returned when playback paused.
bottom_paused_max="$(
  magick "$OUT_DIR/fullscreen-paused.png" \
    -crop 1280x90+0+810 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$bottom_paused_max" 'BEGIN { exit !(max > 0.5) }'; then
  echo "fullscreen OSC did not return on pause: bottom maxima=${bottom_paused_max}" >&2
  exit 1
fi

# The titlebar must stay hidden across the pause toggle (fullscreen is the only
# chrome-changing state here), so the top band stays clear.
top_paused_max="$(
  magick "$OUT_DIR/fullscreen-paused.png" \
    -crop 1280x50+0+0 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
top_paused_mean="$(magick "$OUT_DIR/fullscreen-paused.png" -crop 1280x50+0+0 -colorspace gray -format '%[fx:mean]' info:)"
bottom_paused_mean="$(magick "$OUT_DIR/fullscreen-paused.png" -crop 1280x90+0+810 -colorspace gray -format '%[fx:mean]' info:)"
if ! awk -v max="$top_paused_max" 'BEGIN { exit !(max < 0.12) }'; then
  echo "fullscreen titlebar reappeared on pause: top maxima=${top_paused_max}" >&2
  exit 1
fi

echo "fullscreen-playing: substrate=${SUBSTRATE} frame-mean=${frame_mean} top-max=${top_max} top-mean=${top_mean} bottom-max=${bottom_max} bottom-mean=${bottom_mean}"
echo "fullscreen-paused:  top-max=${top_paused_max} top-mean=${top_paused_mean} bottom-max=${bottom_paused_max} bottom-mean=${bottom_paused_mean}"
SMOKE
then
  echo "Fullscreen chrome smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Fullscreen chrome smoke passed. Screenshots: $OUT_DIR/fullscreen-playing.png $OUT_DIR/fullscreen-paused.png"
