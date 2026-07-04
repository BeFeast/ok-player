#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-seek-preview-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
# The seek-preview hook pops the timeline hover tooltip with a representative
# timecode and chapter and no thumbnail: the deliberate timecode-only fallback the
# tooltip shows for a stream, a not-yet-generated frame, or an unavailable source.
OKP_OPEN_SEEK_PREVIEW_ON_STARTUP=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 5
xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
# The tooltip is a popover surface anchored above the seek bar, so it is captured
# from the root window rather than the main window pixmap.
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

# The tooltip pops just above the seek bar and below the welcome card, over the
# near-black video. That band is empty (black) without the tooltip, so a bright
# maximum there means the timecode/chapter text drew.
preview_max="$(
  magick "$OUT_DIR/root.png" \
    -crop 200x28+262+574 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$preview_max" 'BEGIN { exit !(max > 0.55) }'; then
  echo "Seek preview tooltip looks blank: content maxima=${preview_max}" >&2
  exit 1
fi

# The fallback is bright text on a dark card, not a solid fill: the band's mean must
# stay low so a stray opaque rectangle can never masquerade as the tooltip.
preview_mean="$(
  magick "$OUT_DIR/root.png" \
    -crop 200x28+262+574 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$preview_mean" 'BEGIN { exit !(mean < 0.45) }'; then
  echo "Seek preview tooltip is not a text-on-dark tooltip: mean=${preview_mean}" >&2
  exit 1
fi
SMOKE
then
  echo "Seek preview smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Seek preview smoke passed. Screenshot: $OUT_DIR/root.png"
