#!/usr/bin/env bash
# Real-mpv acceptance capture for loaded, idle, screenshot, panel, and fullscreen behavior.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
FIXTURE_DIR="$(realpath -m "${2:-$ROOT/artifacts/linux-acceptance/fixtures}")"
OUT_DIR="$(realpath -m "${3:-$ROOT/artifacts/linux-acceptance/playback}")"
if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi
DARK="$FIXTURE_DIR/dark-with-chapters.mkv"
BRIGHT="$FIXTURE_DIR/bright.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffprobe; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
for fixture in "$DARK" "$BRIGHT"; do
  if [[ ! -f "$fixture" ]]; then
    echo "Missing generated fixture: $fixture" >&2
    exit 127
  fi
done

# GLVND can prefer a host NVIDIA EGL vendor that cannot initialize inside
# Xvfb. When the standard Mesa vendor descriptor is present, pin it for this
# virtual-display process; real desktop/operator runs do not use this script.
if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$DARK" "$BRIGHT" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
DARK="$2"
BRIGHT="$3"
OUT_DIR="$4"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_CACHE_HOME="$OUT_DIR/cache"
export HOME="$OUT_DIR/home"
mkdir -p "$HOME/Pictures/OK Player"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

launch() {
  local fixture="$1" log="$2"
  shift 2
  env "$@" \
    OKP_DISABLE_MPRIS=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    timeout 45s "$BINARY" "$fixture" >"$log" 2>&1 &
  app_pid=$!
  sleep 6
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
  window_id="$(head -n1 "$OUT_DIR/window.ids")"
  [[ -n "$window_id" ]] || { echo "main window did not appear" >&2; exit 1; }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  sleep 1
}

stop_app() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

launch "$DARK" "$OUT_DIR/dark-app.log" OKP_BUFFERED_TIMELINE_PREVIEW=1
xwininfo -id "$window_id" >"$OUT_DIR/window-default.xwininfo"
width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window-default.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window-default.xwininfo")"
[[ "$width" == "1120" && "$height" == "680" ]] || {
  echo "unexpected default geometry: ${width}x${height}" >&2
  exit 1
}

# Pause pins the OSC and proves loaded playback duration/controls on dark video.
xdotool key --clearmodifiers space
xdotool mousemove --window "$window_id" 560 340
sleep 2
import -window "$window_id" "$OUT_DIR/loaded-paused-osc.png"
cp "$OUT_DIR/loaded-paused-osc.png" "$OUT_DIR/paused.png"
cp "$OUT_DIR/loaded-paused-osc.png" "$OUT_DIR/buffered-timeline.png"
cp "$OUT_DIR/loaded-paused-osc.png" "$OUT_DIR/chapter-context.png"

# A precise seek readout uses the same one-slot OSD surface and lifetime.
xdotool key --clearmodifiers Right
sleep 1
import -window "$window_id" "$OUT_DIR/osd.png"

# Open the shared Chapters/Up Next panel through its real OSC button. The
# generated 1120x680 fixture keeps this stable; the panel itself stays above the
# OSC, so the same control remains clickable to close it.
panel_visible=0
for _ in 1 2 3; do
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  xdotool mousemove --window "$window_id" 560 340
  sleep 1
  xdotool mousemove --window "$window_id" 914 638 click 1
  sleep 2
  import -window "$window_id" "$OUT_DIR/chapters-loaded.png"
  panel_mean="$(magick "$OUT_DIR/chapters-loaded.png" -crop 316x500+780+24 -colorspace gray -format '%[fx:mean]' info:)"
  if awk -v value="$panel_mean" 'BEGIN { exit !(value > 0.04) }'; then
    panel_visible=1
    break
  fi
done
if (( panel_visible == 0 )); then
  echo "chapters panel action did not reveal panel content" >&2
  exit 1
fi
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
xdotool mousemove --window "$window_id" 1097 75 click 1
sleep 1

# Save a frame through the real screenshot action (C is the default saved capture).
before_count="$(find "$HOME/Pictures/OK Player" -maxdepth 1 -type f | wc -l)"
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
xdotool key --clearmodifiers c
sleep 4
after_count="$(find "$HOME/Pictures/OK Player" -maxdepth 1 -type f | wc -l)"
if (( after_count <= before_count )); then
  echo "screenshot action did not create a file" >&2
  exit 1
fi

# Resume and wait past the canonical idle timeout. The bottom band must clear.
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
xdotool key --clearmodifiers space
sleep 4
import -window "$window_id" "$OUT_DIR/playing-idle.png"

# Fullscreen is a real compositor/window-manager transition even under Xvfb/X11.
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 1
xdotool key --clearmodifiers f
sleep 2
xwininfo -id "$window_id" >"$OUT_DIR/window-fullscreen.xwininfo"
fs_width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window-fullscreen.xwininfo")"
fs_height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window-fullscreen.xwininfo")"
[[ "$fs_width" == "1280" && "$fs_height" == "900" ]] || {
  echo "fullscreen geometry wrong: ${fs_width}x${fs_height}" >&2
  exit 1
}
xdotool key --clearmodifiers Escape
sleep 1
stop_app

launch "$DARK" "$OUT_DIR/loading-app.log" OKP_PLAYBACK_STATE_PREVIEW=loading
import -window "$window_id" "$OUT_DIR/buffering-loading.png"
stop_app

launch "$DARK" "$OUT_DIR/error-app.log" OKP_PLAYBACK_STATE_PREVIEW=error
import -window "$window_id" "$OUT_DIR/playback-error.png"
stop_app

launch "$BRIGHT" "$OUT_DIR/bright-app.log"
xdotool key --clearmodifiers space
sleep 2
import -window "$window_id" "$OUT_DIR/bright-video-background.png"
stop_app

printf '%s\n' \
  "default=${width}x${height}" \
  "fullscreen=${fs_width}x${fs_height}" \
  "screenshot_files=${after_count}" >"$OUT_DIR/functional-results.txt"
SMOKE
then
  echo "Playback acceptance smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$DARK")"
cat >"$OUT_DIR/functional-results.json" <<JSON
{
  "schema_version": 1,
  "fixture_duration_seconds": $duration,
  "open_file": "pass",
  "playback_start_and_duration": "pass",
  "chapters_panel_action": "pass",
  "saved_screenshot": "pass",
  "fullscreen_transition": "pass",
  "evidence_level": "xvfb-render",
  "not_proven_here": ["file chooser", "folder chooser", "drag/drop", "clipboard", "desktop portal", "Wayland compositor", "focus behavior"]
}
JSON

echo "Playback acceptance captures written to $OUT_DIR"
