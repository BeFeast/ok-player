#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-seek-preview-smoke}"

for tool in xvfb-run dbus-run-session ffmpeg xfwm4 xdotool xwininfo import magick rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# Exercise the production hover path with a real, deterministic video. The old
# fixture-only hook proved popover styling but never delivered pointer motion,
# launched ffmpeg, or refreshed a stationary card when the worker completed.
ffmpeg \
  -hide_banner \
  -loglevel error \
  -f lavfi \
  -i 'testsrc2=size=640x360:rate=30' \
  -t 12 \
  -c:v libx264 \
  -pix_fmt yuv420p \
  -g 30 \
  -an \
  -y \
  "$OUT_DIR/seek-preview.mp4"

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi
export LIBGL_ALWAYS_SOFTWARE=1

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export OKP_DEBUG_INTERACTIONS=1
export OKP_SKIP_UPDATE_CHECK=1
export XDG_CACHE_HOME="$OUT_DIR/cache"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
# Bind app_pid before installing the trap so cleanup stays safe under `set -u`
# if setup exits before the app launches.
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
OKP_PLAYBACK_FRAME_PREVIEW=dark \
timeout 16s "$BINARY" "$OUT_DIR/seek-preview.mp4" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 5
xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"

# The first motion reveals hidden chrome; the second enters the now-targetable
# seek scale. Leave the pointer stationary for longer than the 2.5 s OSC hide
# timeout so the capture proves timeline hover pins the chrome and that the
# completed worker pushes its frame into the already-open card.
xdotool mousemove --window "$window_id" 400 400
sleep 0.2
xdotool mousemove --window "$window_id" 400 632
sleep 4

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
# Capture both the desktop and the main window: the preview is now composed in
# the player overlay, while the root image remains useful for z-order diagnosis.
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

# The thumbnail occupies the colored upper band of the card. It is black when
# the worker result never reaches the stationary hover, and the whole band is
# black when the unpinned OSC auto-hides underneath the pointer.
thumbnail_mean="$(
  magick "$OUT_DIR/root.png" \
    -crop 132x69+334+494 \
    -colorspace sRGB \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$thumbnail_mean" 'BEGIN { exit !(mean > 0.08) }'; then
  echo "Seek preview thumbnail looks blank or the hover card disappeared: mean=${thumbnail_mean}" >&2
  exit 1
fi

thumbnail_path="$(rg --files "$XDG_CACHE_HOME/ok-player/chapter-thumbnails" -g '**/hover/*.jpg' | head -n1)"
if [[ -z "$thumbnail_path" ]]; then
  echo "Seek hover did not generate a cached thumbnail" >&2
  exit 1
fi
thumbnail_geometry="$(magick identify -format '%wx%h' "$thumbnail_path")"
if [[ "$thumbnail_geometry" != "144x80" && "$thumbnail_geometry" != "144x81" ]]; then
  echo "Unexpected seek thumbnail geometry: $thumbnail_geometry" >&2
  exit 1
fi
SMOKE
then
  echo "Seek preview smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Seek preview smoke passed. Screenshot: $OUT_DIR/root.png"
