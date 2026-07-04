#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-media-info-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x1100x24 -nolisten tcp' \
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
OKP_OPEN_MEDIA_INFO_ON_STARTUP=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 6
xdotool search --name "Media Information" >"$OUT_DIR/media-info.ids"
info_id="$(head -n1 "$OUT_DIR/media-info.ids")"

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$info_id" >"$OUT_DIR/media-info.xwininfo"
import -window root "$OUT_DIR/root.png"
import -window "$info_id" "$OUT_DIR/media-info.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/media-info.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/media-info.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/media-info.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/media-info.xwininfo")"

if [[ "$width" != "680" || "$height" != "820" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected Media Info geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# Regression guard for the old GTK caption/headerbar: it rendered a centered
# window title in the top chrome. The captionless shell keeps that region blank
# (the only title lives at the top-left of the content), so the top-center strip
# must stay light background, not dark caption text.
top_center_dark_pixels="$(
  magick "$OUT_DIR/media-info.png" \
    -crop 180x40+250+6 \
    -colorspace gray \
    -threshold 42% \
    -format '%[fx:(1-mean)*w*h]' info:
)"
top_center_dark_pixels="${top_center_dark_pixels%.*}"
if (( top_center_dark_pixels > 120 )); then
  echo "Unexpected centered caption pixels in Media Info chrome: ${top_center_dark_pixels}" >&2
  exit 1
fi

# The captionless window background must be the calm light surface, and the
# content must actually render (title text produces dark pixels on the left).
top_left_pixel="$(magick "$OUT_DIR/media-info.png" -format '%[pixel:p{20,16}]' info:)"
if [[ "$top_left_pixel" != "srgb(238,244,249)" ]]; then
  echo "Unexpected Media Info top-left pixel: ${top_left_pixel}" >&2
  exit 1
fi

title_dark_pixels="$(
  magick "$OUT_DIR/media-info.png" \
    -crop 360x40+30+64 \
    -colorspace gray \
    -threshold 50% \
    -format '%[fx:(1-mean)*w*h]' info:
)"
title_dark_pixels="${title_dark_pixels%.*}"
if (( title_dark_pixels < 200 )); then
  echo "Media Info title text did not render (dark pixels: ${title_dark_pixels})" >&2
  exit 1
fi
SMOKE
then
  echo "Media Info smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Media Info smoke passed. Screenshot: $OUT_DIR/media-info.png"
