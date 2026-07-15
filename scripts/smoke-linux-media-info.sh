#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-media-info-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath "$BINARY")"
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
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
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1

capture() {
  local name="$1"
  local width="$2"
  local height="$3"
  shift 3

  local state_dir="$OUT_DIR/$name"
  mkdir -p "$state_dir/config/ok-player" "$state_dir/state"
  printf '%s\n' '{"version":1,"updates":{"auto_check":false}}' \
    >"$state_dir/config/ok-player/settings.json"

  env \
    XDG_CONFIG_HOME="$state_dir/config" \
    XDG_STATE_HOME="$state_dir/state" \
    OKP_OPEN_MEDIA_INFO_ON_STARTUP=1 \
    OKP_SKIP_UPDATE_CHECK=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    "$@" \
    timeout 15s "$BINARY" >"$state_dir/app.log" 2>&1 &
  app_pid=$!

  visible_windows=()
  for _ in {1..12}; do
    mapfile -t visible_windows < <(xdotool search --onlyvisible --name '^OK Player$' || true)
    [[ "${#visible_windows[@]}" == "1" ]] && break
    sleep 0.5
  done
  if [[ "${#visible_windows[@]}" != "1" ]]; then
    echo "$name: expected one visible player window, found ${#visible_windows[@]}" >&2
    xwininfo -root -tree >&2 || true
    exit 1
  fi
  local window_id="${visible_windows[0]}"
  sleep 3

  if [[ "$width" != "1120" || "$height" != "680" ]]; then
    xdotool windowsize "$window_id" "$width" "$height"
    sleep 1
  fi

  xwininfo -id "$window_id" >"$state_dir/window.xwininfo"
  xwininfo -root -tree >"$state_dir/tree.txt"
  import -window "$window_id" "$OUT_DIR/$name.png"

  if [[ "$name" == "streams-dark" ]]; then
    xdotool windowactivate --sync "$window_id"
    xdotool key Right
    sleep 0.4
    import -window "$window_id" "$OUT_DIR/keyboard-stats.png"
    xdotool key Escape
    sleep 0.4
    import -window "$window_id" "$OUT_DIR/escape-closed.png"
  elif [[ "$name" == "missing-fields" ]]; then
    xdotool mousemove --window "$window_id" 100 300 click 1
    sleep 0.4
    import -window "$window_id" "$OUT_DIR/backdrop-closed.png"
  fi

  local actual_width actual_height border map_state
  actual_width="$(awk '/Width:/ { print $2; exit }' "$state_dir/window.xwininfo")"
  actual_height="$(awk '/Height:/ { print $2; exit }' "$state_dir/window.xwininfo")"
  border="$(awk '/Border width:/ { print $3; exit }' "$state_dir/window.xwininfo")"
  map_state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$state_dir/window.xwininfo")"
  if [[ "$actual_width" != "$width" || "$actual_height" != "$height" || "$border" != "0" || "$map_state" != "IsViewable" ]]; then
    echo "$name: unexpected player geometry ${actual_width}x${actual_height}, border=${border}, state=${map_state}" >&2
    exit 1
  fi

  if xdotool search --onlyvisible --name 'Media Information' >/dev/null 2>&1; then
    echo "$name: legacy Media Information window is still visible" >&2
    exit 1
  fi
  if rg -q '680x820' "$state_dir/tree.txt"; then
    echo "$name: legacy 680x820 transient geometry is still present" >&2
    exit 1
  fi

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture streams-dark 1120 680 OKP_IDLE_THEME=dark OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=dark
capture stats 1120 680 OKP_IDLE_THEME=dark OKP_MEDIA_INFO_TAB=stats OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=dark
capture long-metadata 1120 680 OKP_IDLE_THEME=light OKP_MEDIA_INFO_PREVIEW=long OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=bright
capture missing-fields 1120 680 OKP_IDLE_THEME=dark OKP_MEDIA_INFO_PREVIEW=missing OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=dark
capture scroll-bottom 1120 680 OKP_IDLE_THEME=dark OKP_MEDIA_INFO_SCROLL_BOTTOM=1 OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=dark
capture narrow 480 540 OKP_IDLE_THEME=light OKP_MEDIA_INFO_PREVIEW=long OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=bright
capture bright-video 1120 680 OKP_IDLE_THEME=light OKP_MEDIA_INFO_PREVIEW_SUBSTRATE=bright

read -r streams_width streams_height < <(
  magick "$OUT_DIR/streams-dark.png" \
    -crop 1120x620+0+40 \
    -colorspace gray -threshold 70% -trim \
    -format '%w %h\n' info:
)
if [[ "$streams_width" != "720" || "$streams_height" != "571" ]]; then
  echo "Unexpected Streams modal bounds: ${streams_width}x${streams_height}, expected 720x571" >&2
  exit 1
fi

read -r stats_width stats_height < <(
  magick "$OUT_DIR/stats.png" \
    -crop 1120x620+0+40 \
    -colorspace gray -threshold 70% -trim \
    -format '%w %h\n' info:
)
if [[ "$stats_width" != "720" ]] || (( stats_height > 571 )); then
  echo "Unexpected Stats modal bounds: ${stats_width}x${stats_height}" >&2
  exit 1
fi

read -r narrow_width narrow_height < <(
  magick "$OUT_DIR/narrow.png" \
    -crop 480x457+0+41 \
    -colorspace gray -threshold 70% -trim \
    -format '%w %h\n' info:
)
if [[ "$narrow_width" != "441" || "$narrow_height" != "453" ]]; then
  echo "Unexpected narrow modal bounds: ${narrow_width}x${narrow_height}, expected 441x453" >&2
  exit 1
fi

stats_delta="$(
  magick compare -metric RMSE "$OUT_DIR/streams-dark.png" "$OUT_DIR/stats.png" null: 2>&1 \
    | awk '{print $1}' || true
)"
scroll_delta="$(
  magick compare -metric RMSE "$OUT_DIR/streams-dark.png" "$OUT_DIR/scroll-bottom.png" null: 2>&1 \
    | awk '{print $1}' || true
)"
keyboard_delta="$(
  magick compare -metric RMSE "$OUT_DIR/streams-dark.png" "$OUT_DIR/keyboard-stats.png" null: 2>&1 \
    | awk '{print $1}' || true
)"
if ! awk -v delta="$stats_delta" 'BEGIN { exit !(delta > 0.02) }'; then
  echo "Streams and Stats captures did not diverge: RMSE=${stats_delta}" >&2
  exit 1
fi
if ! awk -v delta="$scroll_delta" 'BEGIN { exit !(delta > 0.01) }'; then
  echo "Scroll-bottom capture did not move the content: RMSE=${scroll_delta}" >&2
  exit 1
fi
if ! awk -v delta="$keyboard_delta" 'BEGIN { exit !(delta > 0.02) }'; then
  echo "Right-arrow keyboard navigation did not switch tabs: RMSE=${keyboard_delta}" >&2
  exit 1
fi

for closed_capture in escape-closed backdrop-closed; do
  closed_mean="$(
    magick "$OUT_DIR/$closed_capture.png" \
      -crop 300x220+410+230 \
      -colorspace gray \
      -format '%[fx:mean]' info:
  )"
  if ! awk -v mean="$closed_mean" 'BEGIN { exit !(mean < 0.20) }'; then
    echo "$closed_capture did not dismiss the modal: center mean=${closed_mean}" >&2
    exit 1
  fi
done

bright_scrim_mean="$(
  magick "$OUT_DIR/bright-video.png" \
    -crop 150x300+20+180 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
dark_scrim_mean="$(
  magick "$OUT_DIR/streams-dark.png" \
    -crop 150x300+20+180 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$bright_scrim_mean" 'BEGIN { exit !(mean > 0.30 && mean < 0.65) }'; then
  echo "Bright substrate scrim is outside the expected dimmed range: mean=${bright_scrim_mean}" >&2
  exit 1
fi
if ! awk -v mean="$dark_scrim_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Dark substrate scrim is unexpectedly bright: mean=${dark_scrim_mean}" >&2
  exit 1
fi

header_dark_pixels="$(
  magick "$OUT_DIR/streams-dark.png" \
    -crop 500x48+260+67 \
    -colorspace gray -threshold 55% \
    -format '%[fx:(1-mean)*w*h]' info:
)"
header_dark_pixels="${header_dark_pixels%.*}"
if (( header_dark_pixels < 250 )); then
  echo "Media Information header did not render: dark pixels=${header_dark_pixels}" >&2
  exit 1
fi

printf 'streams=%sx%s stats=%sx%s narrow=%sx%s\n' \
  "$streams_width" "$streams_height" "$stats_width" "$stats_height" "$narrow_width" "$narrow_height"
printf 'stats-rmse=%s scroll-rmse=%s bright-scrim=%s dark-scrim=%s\n' \
  "$stats_delta" "$scroll_delta" "$bright_scrim_mean" "$dark_scrim_mean"
SMOKE
then
  echo "Media Information smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Media Information smoke passed. Captures: $OUT_DIR"
