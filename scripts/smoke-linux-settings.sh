#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-smoke}"
PAGE="${3:-about}"
COLOR_SCHEME="${4:-light}"

case "$PAGE" in
  about|appearance|playback|subtitles|video|audio|shortcuts|integration|advanced) ;;
  *) echo "Unsupported Settings page: $PAGE" >&2; exit 2 ;;
esac
case "$COLOR_SCHEME" in
  light|dark|high-contrast) ;;
  *) echo "Unsupported Settings color scheme: $COLOR_SCHEME" >&2; exit 2 ;;
esac

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$PAGE" "$COLOR_SCHEME" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
PAGE="$3"
COLOR_SCHEME="$4"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
if [[ "$COLOR_SCHEME" == "high-contrast" ]]; then
  export GTK_THEME=HighContrast
  APP_COLOR_SCHEME=light
else
  APP_COLOR_SCHEME="$COLOR_SCHEME"
fi

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_OPEN_SETTINGS_ON_STARTUP=1 \
OKP_OPEN_SETTINGS_PAGE_ON_STARTUP="$PAGE" \
OKP_SETTINGS_COLOR_SCHEME="$APP_COLOR_SCHEME" \
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

if [[ "$width" != "760" || "$height" -lt 300 || "$height" -gt 852 || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected Settings geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# Regression guard for a GTK native caption/headerbar: the app-owned title is
# left-aligned, leaving the center of the 42px strip visually quiet.
top_center_variance="$(
  magick "$OUT_DIR/settings.png" \
    -crop 180x36+290+3 \
    -colorspace gray \
    -format '%[fx:standard_deviation]' info:
)"
if ! awk -v variance="$top_center_variance" 'BEGIN { exit !(variance < 0.025) }'; then
  echo "Unexpected centered caption pixels in Settings chrome: variance=${top_center_variance}" >&2
  exit 1
fi

rail_mean="$(magick "$OUT_DIR/settings.png" -crop 120x80+20+60 -colorspace gray -format '%[fx:mean]' info:)"
content_mean="$(magick "$OUT_DIR/settings.png" -crop 160x80+360+60 -colorspace gray -format '%[fx:mean]' info:)"
if [[ "$COLOR_SCHEME" == "light" ]]; then
  surface_ok="$(awk -v rail="$rail_mean" -v content="$content_mean" 'BEGIN { print (rail > 0.75 && content > 0.75) ? 1 : 0 }')"
elif [[ "$COLOR_SCHEME" == "dark" ]]; then
  surface_ok="$(awk -v rail="$rail_mean" -v content="$content_mean" 'BEGIN { print (rail < 0.30 && content < 0.30) ? 1 : 0 }')"
else
  surface_ok="$(awk -v content="$content_mean" 'BEGIN { print (content < 0.10) ? 1 : 0 }')"
fi
if [[ "$surface_ok" != "1" ]]; then
  echo "Unexpected Settings ${COLOR_SCHEME} surfaces: rail=${rail_mean}, content=${content_mean}" >&2
  exit 1
fi

if [[ "$PAGE" == "about" ]]; then
  # The canonical identity sits in the first content row. A blank crop catches
  # both launcher-image regressions and failed custom-drawing realization.
  about_variance="$(magick "$OUT_DIR/settings.png" -crop 118x94+216+70 -colorspace gray -format '%[fx:standard_deviation]' info:)"
  if ! awk -v variance="$about_variance" 'BEGIN { exit !(variance > 0.06) }'; then
    echo "About illustration crop is unexpectedly flat: variance=${about_variance}" >&2
    exit 1
  fi
fi
SMOKE
then
  echo "Settings smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Settings smoke passed (${PAGE}, ${COLOR_SCHEME}). Screenshot: $OUT_DIR/settings.png"
