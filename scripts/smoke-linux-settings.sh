#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-smoke}"
PAGE="${3:-about}"
COLOR_SCHEME="${4:-light}"
UPDATE_PREVIEW="${5:-}"
SCALE_PERCENT="${6:-100}"

case "$PAGE" in
  about|appearance|playback|subtitles|video|audio|shortcuts|integration|updates|advanced) ;;
  *) echo "Unsupported Settings page: $PAGE" >&2; exit 2 ;;
esac
case "$UPDATE_PREVIEW" in
  ""|up-to-date|checking|available|skipped|install-error|error) ;;
  *) echo "Unsupported Settings update preview: $UPDATE_PREVIEW" >&2; exit 2 ;;
esac
case "$COLOR_SCHEME" in
  light|dark|high-contrast) ;;
  *) echo "Unsupported Settings color scheme: $COLOR_SCHEME" >&2; exit 2 ;;
esac
case "$SCALE_PERCENT" in
  100) DPI_SCALE=1 ;;
  125) DPI_SCALE=1.25 ;;
  150) DPI_SCALE=1.5 ;;
  200) DPI_SCALE=2 ;;
  *) echo "Unsupported Settings scale: $SCALE_PERCENT" >&2; exit 2 ;;
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
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$PAGE" "$COLOR_SCHEME" "$UPDATE_PREVIEW" "$DPI_SCALE" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
PAGE="$3"
COLOR_SCHEME="$4"
UPDATE_PREVIEW="$5"
DPI_SCALE="$6"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export GDK_DPI_SCALE="$DPI_SCALE"
export XDG_CONFIG_HOME="$OUT_DIR/xdg-config"
export XDG_STATE_HOME="$OUT_DIR/xdg-state"
export XDG_CACHE_HOME="$OUT_DIR/xdg-cache"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
if [[ "$PAGE" == "integration" ]]; then
  mkdir -p "$XDG_STATE_HOME/ok-player"
  now_unix="$(date +%s)"
  printf '{\n  "version": 2,\n  "files": {\n    "/media/old.mkv": { "position": 120.0, "duration": 600.0, "finished": false, "updated_at_unix": 1 },\n    "/media/recent.mkv": { "position": 120.0, "duration": 600.0, "finished": false, "updated_at_unix": %s }\n  }\n}\n' \
    "$now_unix" >"$XDG_STATE_HOME/ok-player/history.json"
fi
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

settings_id=""
for _ in $(seq 1 60); do
  if xdotool search --onlyvisible --name '^Settings$' >"$OUT_DIR/settings.ids" 2>/dev/null \
    && [[ -s "$OUT_DIR/settings.ids" ]]; then
    while IFS= read -r candidate; do
      candidate_info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
      candidate_width="$(awk '/Width:/ { print $2; exit }' <<<"$candidate_info")"
      candidate_state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$candidate_info")"
      if [[ "$candidate_width" -gt 1 && "$candidate_state" == "IsViewable" ]]; then
        settings_id="$candidate"
        break 2
      fi
    done <"$OUT_DIR/settings.ids"
  fi
  sleep 0.25
done
if [[ -z "$settings_id" ]]; then
  echo "Settings window did not appear" >&2
  exit 1
fi
printf '%s\n' "$settings_id" >"$OUT_DIR/settings.ids"
xdotool windowactivate --sync "$settings_id"
sleep 1

capture_window() {
  local window_id="$1" output="$2" info x y width height root_capture
  info="$(xwininfo -id "$window_id")"
  x="$(awk '/Absolute upper-left X:/ { print $4; exit }' <<<"$info")"
  y="$(awk '/Absolute upper-left Y:/ { print $4; exit }' <<<"$info")"
  width="$(awk '/Width:/ { print $2; exit }' <<<"$info")"
  height="$(awk '/Height:/ { print $2; exit }' <<<"$info")"
  root_capture="${output%.png}-root.png"
  import -window root "$root_capture"
  magick "$root_capture" -crop "${width}x${height}+${x}+${y}" +repage "$output"
  rm -f "$root_capture"
}

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$settings_id" >"$OUT_DIR/settings.xwininfo"
import -window root "$OUT_DIR/root.png"
capture_window "$settings_id" "$OUT_DIR/settings.png"

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

  # Hardware decode must use the same compact Settings switch contract as
  # Playback and Updates. The shared 39x22 content request plus its 3px inset
  # renders a 45x28 horizontal track; measure the actual accent-painted bounds
  # so a stock GtkSwitch or fractional-scale vertical allocation cannot return.
  if [[ "$COLOR_SCHEME" == "dark" ]]; then
    switch_accent='#28b3aa'
  else
    switch_accent='#10938a'
  fi
  switch_geometry="$(
    magick "$OUT_DIR/settings.png" \
      -crop 64x64+636+120 \
      -fuzz 6% \
      -fill white +opaque "$switch_accent" \
      -fill black -opaque "$switch_accent" \
      -trim \
      -format '%wx%h' info:
  )"
  if [[ "$switch_geometry" != "45x28" ]]; then
    echo "Hardware decode switch violates the shared Settings geometry: ${switch_geometry}" >&2
    exit 1
  fi

  # Exercise the off state as well: the shared button must remain interactive,
  # and persist the same hwdec setting that the former GtkSwitch owned.
  xdotool mousemove --window "$settings_id" 665 151 click 1
  sleep 0.25
  settings_json="$XDG_CONFIG_HOME/ok-player/settings.json"
  if ! grep -q '"hwdec": "no"' "$settings_json"; then
    echo "Hardware decode off state was not persisted" >&2
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

if [[ "$PAGE" == "integration" ]]; then
  # The Integration page is intentionally long. Scroll the independent content pane until the
  # Privacy card is fully visible, then capture the actual controls rather than treating the
  # section header at the fold as evidence.
  xdotool mousemove --window "$settings_id" 620 470
  xdotool click --repeat 7 --delay 45 5
  sleep 0.5
  capture_window "$settings_id" "$OUT_DIR/settings-privacy.png"

  privacy_variance="$(
    magick "$OUT_DIR/settings-privacy.png" \
      -crop 500x250+216+170 \
      -colorspace gray \
      -format '%[fx:standard_deviation]' info:
  )"
  if ! awk -v variance="$privacy_variance" 'BEGIN { exit !(variance > 0.04) }'; then
    echo "Privacy controls are unexpectedly flat: variance=${privacy_variance}" >&2
    exit 1
  fi

  # Private session is transient: toggle it on and prove the native control updates in place.
  xdotool mousemove --window "$settings_id" 640 291 click 1
  sleep 0.25
  capture_window "$settings_id" "$OUT_DIR/settings-private-on.png"
  private_difference="$(
    magick "$OUT_DIR/settings-privacy.png" "$OUT_DIR/settings-private-on.png" \
      -compose difference -composite \
      -crop 116x42+582+270 \
      -colorspace gray \
      -format '%[fx:mean]' info:
  )"
  if ! awk -v difference="$private_difference" 'BEGIN { exit !(difference > 0.002) }'; then
    echo "Private session control did not update: difference=${private_difference}" >&2
    exit 1
  fi

  # Move Forever to 7 days through the native dropdown. The setting must persist and prune
  # the seeded stale entry immediately while preserving the recent one.
  xdotool mousemove --window "$settings_id" 620 339 click 1
  xdotool key Down Return
  sleep 0.4
  settings_json="$XDG_CONFIG_HOME/ok-player/settings.json"
  history_json="$XDG_STATE_HOME/ok-player/history.json"
  if ! grep -q '"history_retention_days": 7' "$settings_json"; then
    echo "History retention selection was not persisted as 7 days" >&2
    exit 1
  fi
  if grep -q 'old.mkv' "$history_json" || ! grep -q 'recent.mkv' "$history_json"; then
    echo "History retention did not prune only the stale seeded entry" >&2
    exit 1
  fi

  # The destructive action must open a confirmation with Cancel as the safe default.
  xdotool mousemove --window "$settings_id" 615 388 click 1
  clear_dialog_id=""
  for _ in $(seq 1 20); do
    xdotool search --onlyvisible --name 'Clear watch history' >"$OUT_DIR/clear-dialog.ids" 2>/dev/null || true
    clear_dialog_id="$(head -n1 "$OUT_DIR/clear-dialog.ids" 2>/dev/null || true)"
    [[ -n "$clear_dialog_id" ]] && break
    sleep 0.1
  done
  if [[ -z "$clear_dialog_id" ]]; then
    echo "Clear watch history confirmation did not appear" >&2
    exit 1
  fi
  xwininfo -id "$clear_dialog_id" >"$OUT_DIR/clear-dialog.xwininfo"
  xdotool key Tab Return
  sleep 0.3
  if grep -q 'recent.mkv' "$history_json"; then
    echo "Confirmed Clear watch history action did not empty history.json" >&2
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
  capture_window "$settings_id" "$OUT_DIR/settings-advanced.png"
  xdotool mousemove --window "$settings_id" 90 376 click 1
  sleep 1
  capture_window "$settings_id" "$OUT_DIR/settings-mouse.png"

  # Keyboard + Settings search: focus the field, type a major Updates control,
  # then activate the result with Enter.
  xdotool mousemove --window "$settings_id" 90 414 click 1
  sleep 1
  xdotool mousemove --window "$settings_id" 90 70 click 1
  xdotool type --delay 35 'automatic checks'
  sleep 1
  capture_window "$settings_id" "$OUT_DIR/settings-search-result.png"
  xdotool key Return
  sleep 1
  capture_window "$settings_id" "$OUT_DIR/settings-search.png"

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
  xdotool windowsize --sync "$settings_id" 760 480
  sleep 1
  xwininfo -id "$settings_id" >"$OUT_DIR/settings-minimum.xwininfo"
  capture_window "$settings_id" "$OUT_DIR/settings-minimum.png"
  min_width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/settings-minimum.xwininfo")"
  min_height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/settings-minimum.xwininfo")"
  if [[ "$min_width" != "760" || "$min_height" != "480" ]]; then
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

echo "Settings smoke passed (${PAGE}, ${COLOR_SCHEME}, scale=${SCALE_PERCENT}%, update=${UPDATE_PREVIEW:-live}). Screenshot: $OUT_DIR/settings.png"
