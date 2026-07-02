#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-main-window-smoke}"

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
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 4
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

# Regression guard for the old native GTK caption/headerbar. The player owns
# its caption controls, so the top-center strip should remain dark video/chrome,
# not a light system titlebar with centered text.
top_center_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 220x36+450+0 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$top_center_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Unexpected bright top-center caption strip in main window: mean=${top_center_mean}" >&2
  exit 1
fi

top_left_pixel="$(magick "$OUT_DIR/window.png" -format '%[pixel:p{20,16}]' info:)"
if [[ "$top_left_pixel" != "srgb(5,5,7)" ]]; then
  echo "Unexpected main window top-left pixel: ${top_left_pixel}" >&2
  exit 1
fi
SMOKE
then
  echo "Main window smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Main window smoke passed. Screenshot: $OUT_DIR/window.png"
