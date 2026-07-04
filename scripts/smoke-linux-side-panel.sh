#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-side-panel-smoke}"

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
OKP_OPEN_SIDE_PANEL_ON_STARTUP=1 \
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

# The Chapters/Up Next panel is anchored to the right (halign End, 344px wide,
# 24px inset). If the fixture side panel rendered, that band carries bright text,
# badges and thumbnail placeholders over the near-black video, so a dark maximum
# there means the panel failed to draw.
panel_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 300x440+772+64 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$panel_max" 'BEGIN { exit !(max > 0.55) }'; then
  echo "Side panel looks blank: content maxima=${panel_max}" >&2
  exit 1
fi

# The panel carries the OK Player teal accent (selected Chapters tab, the current
# chapter highlight and its PLAYING badge), so across the band the green channel
# should read stronger than the red one — a stock grey panel would not.
panel_red="$(magick "$OUT_DIR/window.png" -crop 300x440+772+64 -format '%[fx:mean.r]' info:)"
panel_green="$(magick "$OUT_DIR/window.png" -crop 300x440+772+64 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$panel_red" -v g="$panel_green" 'BEGIN { exit !(g - r > 0.01) }'; then
  echo "Side panel accent missing: red=${panel_red} green=${panel_green}" >&2
  exit 1
fi
SMOKE
then
  echo "Side panel smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Side panel smoke passed. Screenshot: $OUT_DIR/window.png"
