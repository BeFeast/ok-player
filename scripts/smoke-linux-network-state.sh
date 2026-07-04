#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-network-state-smoke}"
# Which fixture state to preview: error | buffering | connecting.
STATE="${3:-error}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$STATE" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
STATE="$3"

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
OKP_OPEN_NETWORK_STATE_ON_STARTUP="$STATE" \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 5
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

# The status card is centered over the video plane. If it rendered, that central
# band carries bright title text (and, for the error state, the teal Retry
# button) over the dim scrim, so a dark maximum there means the card failed to
# draw.
card_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 460x220+330+230 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$card_max" 'BEGIN { exit !(max > 0.55) }'; then
  echo "Network status card looks blank: content maxima=${card_max}" >&2
  exit 1
fi

# The scrim + card panel lift the near-black video, so the band's mean brightness
# should sit clearly above a bare video plane.
card_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 460x220+330+230 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$card_mean" 'BEGIN { exit !(mean > 0.04) }'; then
  echo "Network status card scrim missing: band mean=${card_mean}" >&2
  exit 1
fi
SMOKE
then
  echo "Network state smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Network state smoke passed. Screenshot: $OUT_DIR/window.png"
