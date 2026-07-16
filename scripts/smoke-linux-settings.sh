#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="$(realpath -m "${2:-$ROOT/artifacts/manual-ui/linux-settings-smoke}")"
PAGE="${3:-about}"
COLOR_SCHEME="${4:-light}"
EXPECTED_HEIGHT="${5:-}"
WORKAREA_HEIGHT="${6:-}"
if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi

case "$PAGE" in
  about|appearance|playback|subtitles|video|audio|shortcuts|integration|advanced) ;;
  *) echo "Unsupported Settings page: $PAGE" >&2; exit 2 ;;
esac
case "$COLOR_SCHEME" in
  light|dark|high-contrast) ;;
  *) echo "Unsupported Settings color scheme: $COLOR_SCHEME" >&2; exit 2 ;;
esac
if [[ -n "$EXPECTED_HEIGHT" && ! "$EXPECTED_HEIGHT" =~ ^[0-9]+$ ]]; then
  echo "Expected Settings height must be a positive integer" >&2
  exit 2
fi
if [[ -n "$WORKAREA_HEIGHT" && ! "$WORKAREA_HEIGHT" =~ ^[0-9]+$ ]]; then
  echo "Settings workarea height must be a positive integer" >&2
  exit 2
fi

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$PAGE" "$COLOR_SCHEME" "$EXPECTED_HEIGHT" "$WORKAREA_HEIGHT" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
PAGE="$3"
COLOR_SCHEME="$4"
EXPECTED_HEIGHT="$5"
WORKAREA_HEIGHT="$6"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
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
OKP_SETTINGS_WORKAREA_HEIGHT="$WORKAREA_HEIGHT" \
OKP_DISABLE_MPRIS=1 \
OKP_SKIP_UPDATE_CHECK=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 20s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

settings_id=""
for _ in $(seq 1 120); do
  settings_id="$(xdotool search --name 'Settings' 2>/dev/null | head -n1 || true)"
  [[ -n "$settings_id" ]] && break
  sleep 0.1
done
if [[ -z "$settings_id" ]]; then
  echo "Settings window did not appear" >&2
  xwininfo -root -tree >"$OUT_DIR/tree.txt" 2>&1 || true
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi
printf '%s\n' "$settings_id" >"$OUT_DIR/settings.ids"
sleep 1

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$settings_id" >"$OUT_DIR/settings.xwininfo"
import -window root "$OUT_DIR/root.png"
import -window "$settings_id" "$OUT_DIR/settings.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/settings.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/settings.xwininfo")"

if [[ "$width" != "760" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected Settings geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi
if (( height < 282 || height > 836 )); then
  echo "Settings height is outside the bounded natural range: ${height}" >&2
  exit 1
fi
if [[ -n "$EXPECTED_HEIGHT" && "$height" != "$EXPECTED_HEIGHT" ]]; then
  echo "Unexpected Settings height: ${height}, expected ${EXPECTED_HEIGHT}" >&2
  exit 1
fi
if [[ -n "$WORKAREA_HEIGHT" && "$height" -gt $((WORKAREA_HEIGHT - 64)) ]]; then
  echo "Settings exceeds constrained workarea: height=${height}, workarea=${WORKAREA_HEIGHT}" >&2
  exit 1
fi
printf 'page=%s\nwidth=%s\nheight=%s\nworkarea-height=%s\n' \
  "$PAGE" "$width" "$height" "${WORKAREA_HEIGHT:-900}" >"$OUT_DIR/geometry.txt"

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

if [[ -n "$WORKAREA_HEIGHT" ]]; then
  titlebar_signature="$(magick "$OUT_DIR/settings.png" -crop "${width}x42+0+0" -format '%#' info:)"
  content_signature="$(magick "$OUT_DIR/settings.png" -crop "$((width - 192))x$((height - 42))+192+42" -format '%#' info:)"

  xdotool mousemove --window "$settings_id" $((width - 80)) $((height - 30))
  xdotool click --repeat 16 --delay 20 5
  sleep 0.5
  import -window "$settings_id" "$OUT_DIR/settings-content-bottom.png"

  scrolled_titlebar_signature="$(magick "$OUT_DIR/settings-content-bottom.png" -crop "${width}x42+0+0" -format '%#' info:)"
  scrolled_content_signature="$(magick "$OUT_DIR/settings-content-bottom.png" -crop "$((width - 192))x$((height - 42))+192+42" -format '%#' info:)"
  if [[ "$titlebar_signature" != "$scrolled_titlebar_signature" ]]; then
    echo "Settings titlebar changed while content scrolled" >&2
    exit 1
  fi
  if [[ "$content_signature" == "$scrolled_content_signature" ]]; then
    echo "Settings overflowing content did not scroll" >&2
    exit 1
  fi

  xdotool mousemove --window "$settings_id" 96 $((height - 30))
  sleep 0.2
  import -window "$settings_id" "$OUT_DIR/settings-rail-top.png"
  rail_top_signature="$(magick "$OUT_DIR/settings-rail-top.png" -crop "192x$((height - 42))+0+42" -format '%#' info:)"
  xdotool click --repeat 16 --delay 20 5
  sleep 0.5
  import -window "$settings_id" "$OUT_DIR/settings-rail-bottom.png"
  rail_bottom_signature="$(magick "$OUT_DIR/settings-rail-bottom.png" -crop "192x$((height - 42))+0+42" -format '%#' info:)"
  if [[ "$rail_top_signature" == "$rail_bottom_signature" ]]; then
    echo "Settings overflowing navigation rail did not scroll" >&2
    exit 1
  fi
elif [[ "$PAGE" == "about" && "$COLOR_SCHEME" == "light" ]]; then
  xdotool mousemove --window "$settings_id" 92 112 click 1
  switched_height=""
  for _ in $(seq 1 80); do
    switched_height="$(xwininfo -id "$settings_id" | awk '/Height:/ { print $2; exit }')"
    [[ "$switched_height" == "465" ]] && break
    sleep 0.05
  done
  if [[ "$switched_height" != "465" ]]; then
    echo "Settings did not resize to the Appearance page natural height: ${switched_height}" >&2
    exit 1
  fi
  import -window "$settings_id" "$OUT_DIR/settings-appearance.png"

  xdotool mousemove --window "$settings_id" 92 436 click 1
  restored_height=""
  for _ in $(seq 1 80); do
    restored_height="$(xwininfo -id "$settings_id" | awk '/Height:/ { print $2; exit }')"
    [[ "$restored_height" == "751" ]] && break
    sleep 0.05
  done
  if [[ "$restored_height" != "751" ]]; then
    echo "Settings did not restore the About page natural height: ${restored_height}" >&2
    exit 1
  fi
  printf 'about-height=%s\nappearance-height=%s\nrestored-about-height=%s\n' \
    "$height" "$switched_height" "$restored_height" >"$OUT_DIR/page-switch-geometry.txt"
fi
SMOKE
then
  echo "Settings smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Settings smoke passed (${PAGE}, ${COLOR_SCHEME}). Screenshot: $OUT_DIR/settings.png"
