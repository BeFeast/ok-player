#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-smoke}"

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
OKP_OPEN_SETTINGS_ON_STARTUP=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 6
xdotool search --name "Settings" >"$OUT_DIR/settings.ids"
settings_id="$(head -n1 "$OUT_DIR/settings.ids")"

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$settings_id" >"$OUT_DIR/settings.xwininfo"
import -window root "$OUT_DIR/root.png"
import -window "$settings_id" "$OUT_DIR/settings.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/settings.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"

if [[ "$width" != "744" || "$height" != "1030" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected Settings geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# Regression guard for the old GTK caption/headerbar: it rendered a centered
# "Settings" title in the top chrome. The Windows-reference shell keeps that
# region blank; the only Settings title lives at the top-left rail.
top_center_dark_pixels="$(
  magick "$OUT_DIR/settings.png" \
    -crop 180x45+282+5 \
    -colorspace gray \
    -threshold 32% \
    -format '%[fx:(1-mean)*w*h]' info:
)"
top_center_dark_pixels="${top_center_dark_pixels%.*}"
if (( top_center_dark_pixels > 120 )); then
  echo "Unexpected centered caption pixels in Settings chrome: ${top_center_dark_pixels}" >&2
  exit 1
fi

rail_top_pixel="$(magick "$OUT_DIR/settings.png" -format '%[pixel:p{20,16}]' info:)"
content_top_pixel="$(magick "$OUT_DIR/settings.png" -format '%[pixel:p{220,16}]' info:)"
if [[ "$rail_top_pixel" != "srgb(234,240,245)" || "$content_top_pixel" != "srgb(238,244,249)" ]]; then
  echo "Unexpected Settings top strip colors: rail=${rail_top_pixel}, content=${content_top_pixel}" >&2
  exit 1
fi

if [[ "${OKP_EXPECT_UPDATE_STATUS_UP_TO_DATE:-0}" == "1" ]]; then
  # Optional network-backed guard: the About Updates status should settle on
  # "Up to date" instead of the old dead-looking "Not checked yet". This uses
  # a stable dark-pixel envelope for the right-aligned status text so the
  # default smoke remains OCR-free and offline-friendly.
  update_status_dark_pixels="$(
    magick "$OUT_DIR/settings.png" \
      -crop 125x24+560+444 \
      -colorspace gray \
      -threshold 50% \
      -format '%[fx:(1-mean)*w*h]' info:
  )"
  update_status_dark_pixels="${update_status_dark_pixels%.*}"
  if (( update_status_dark_pixels < 90 || update_status_dark_pixels > 190 )); then
    echo "Unexpected update status text pixels: ${update_status_dark_pixels}" >&2
    exit 1
  fi
fi
SMOKE
then
  echo "Settings smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Settings smoke passed. Screenshot: $OUT_DIR/settings.png"
