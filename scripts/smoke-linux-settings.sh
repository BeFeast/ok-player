#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-smoke}"
PAGE="${3:-about}"
COLOR_SCHEME="${4:-light}"
UPDATE_PREVIEW="${5:-}"

case "$PAGE" in
  about|appearance|playback|subtitles|video|audio|shortcuts|integration|updates|advanced) ;;
  *) echo "Unsupported Settings page: $PAGE" >&2; exit 2 ;;
esac
case "$UPDATE_PREVIEW" in
  ""|up-to-date|checking|available|skipped|error|install-error) ;;
  *) echo "Unsupported Settings update preview: $UPDATE_PREVIEW" >&2; exit 2 ;;
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
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$PAGE" "$COLOR_SCHEME" "$UPDATE_PREVIEW" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
PAGE="$3"
COLOR_SCHEME="$4"
UPDATE_PREVIEW="$5"

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
OKP_SETTINGS_UPDATE_PREVIEW="$UPDATE_PREVIEW" \
OKP_SKIP_UPDATE_CHECK=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 30s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

for _ in $(seq 1 30); do
  if xdotool search --name "Settings" >"$OUT_DIR/settings.ids" 2>/dev/null \
    && [[ -s "$OUT_DIR/settings.ids" ]]; then
    break
  fi
  sleep 0.5
done
if [[ ! -s "$OUT_DIR/settings.ids" ]]; then
  echo "Settings window did not appear" >&2
  exit 1
fi
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

if [[ "$PAGE" == "playback" ]]; then
  if ! grep -q 'playback capability: gapless=deferred' "$OUT_DIR/app.log"; then
    echo "Playback Settings did not report the deferred gapless capability" >&2
    exit 1
  fi
fi

if [[ "$PAGE" == "video" ]]; then
  if ! grep -q 'video capability: hdr=engine-managed controls=unavailable' "$OUT_DIR/app.log"; then
    echo "Video Settings did not report the reserved engine-managed HDR state" >&2
    exit 1
  fi
fi

if [[ "$PAGE" == "subtitles" ]]; then
  # The Presentation card occupies the top of the 500px-wide content column. Its three segmented
  # rows must render without forcing the canonical 760px window wider (geometry check above) or
  # collapsing into a flat/blank card.
  presentation_variance="$(
    magick "$OUT_DIR/settings.png" \
      -crop 500x235+216+70 \
      -colorspace gray \
      -format '%[fx:standard_deviation]' info:
  )"
  if ! awk -v variance="$presentation_variance" 'BEGIN { exit !(variance > 0.05) }'; then
    echo "Subtitle Presentation controls are unexpectedly flat: variance=${presentation_variance}" >&2
    exit 1
  fi
fi

if [[ "$PAGE" == "updates" ]]; then
  # Initial-page routing must open the dedicated page with a non-flat card at
  # the canonical minimum width.
  updates_variance="$(
    magick "$OUT_DIR/settings.png" \
      -crop 500x300+216+70 \
      -colorspace gray \
      -format '%[fx:standard_deviation]' info:
  )"
  if ! awk -v variance="$updates_variance" 'BEGIN { exit !(variance > 0.04) }'; then
    echo "Updates page is unexpectedly flat: variance=${updates_variance}" >&2
    exit 1
  fi

  # Mouse navigation: Updates sits immediately before Advanced.
  xdotool mousemove --window "$settings_id" 90 414 click 1
  sleep 1
  import -window "$settings_id" "$OUT_DIR/settings-advanced.png"
  xdotool mousemove --window "$settings_id" 90 376 click 1
  sleep 1
  import -window "$settings_id" "$OUT_DIR/settings-mouse.png"

  # Keyboard + Settings search: focus the field, type a major Updates control,
  # then activate the result with Enter.
  xdotool mousemove --window "$settings_id" 90 414 click 1
  sleep 1
  xdotool mousemove --window "$settings_id" 90 70 click 1
  xdotool type --delay 35 'automatic checks'
  sleep 1
  import -window "$settings_id" "$OUT_DIR/settings-search-result.png"
  xdotool key Return
  sleep 1
  import -window "$settings_id" "$OUT_DIR/settings-search.png"

  mouse_difference="$(
    magick "$OUT_DIR/settings-advanced.png" "$OUT_DIR/settings-mouse.png" \
      -compose difference -composite \
      -crop 500x300+216+70 \
      -colorspace gray \
      -format '%[fx:mean]' info:
  )"
  search_difference="$(
    magick "$OUT_DIR/settings-advanced.png" "$OUT_DIR/settings-search.png" \
      -compose difference -composite \
      -crop 500x300+216+70 \
      -colorspace gray \
      -format '%[fx:mean]' info:
  )"
  if ! awk -v mouse="$mouse_difference" -v search="$search_difference" \
    'BEGIN { exit !(mouse > 0.02 && search > 0.02) }'; then
    echo "Updates navigation did not change the content pane: mouse=${mouse_difference}, search=${search_difference}" >&2
    exit 1
  fi

  # At the minimum supported 760px width, height can contract and both the rail
  # and content remain independently scrollable.
  xdotool windowsize --sync "$settings_id" 760 360
  sleep 1
  xwininfo -id "$settings_id" >"$OUT_DIR/settings-minimum.xwininfo"
  import -window "$settings_id" "$OUT_DIR/settings-minimum.png"
  min_width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/settings-minimum.xwininfo")"
  min_height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/settings-minimum.xwininfo")"
  if [[ "$min_width" != "760" || "$min_height" != "360" ]]; then
    echo "Unexpected minimum Settings geometry: ${min_width}x${min_height}" >&2
    exit 1
  fi
fi
SMOKE
then
  echo "Settings smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Settings smoke passed (${PAGE}, ${COLOR_SCHEME}, update=${UPDATE_PREVIEW:-live}). Screenshot: $OUT_DIR/settings.png"
