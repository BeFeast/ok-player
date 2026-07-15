#!/usr/bin/env bash
# Deterministic visual smoke for the canonical idle canvas, Continue Watching,
# in-place History states, and the two PRD side-panel empty states.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-empty-states-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export OKP_SKIP_UPDATE_CHECK=1
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CONFIG_HOME="$OUT_DIR/config"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
  fi
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

mkdir -p "$XDG_STATE_HOME/ok-player" "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":2,"updates":{"auto_check":false}}
JSON

fixture_dir="$OUT_DIR/history-fixtures"
mkdir -p "$fixture_dir/today" "$fixture_dir/yesterday" "$fixture_dir/week" "$fixture_dir/earlier"
touch \
  "$fixture_dir/today/Dune.mkv" \
  "$fixture_dir/today/Severance.mkv" \
  "$fixture_dir/today/Blade-Runner-2049.mkv" \
  "$fixture_dir/yesterday/interview.mov" \
  "$fixture_dir/week/Free-Solo.mp4" \
  "$fixture_dir/week/Wedding.mov" \
  "$fixture_dir/earlier/Past-Lives.mkv" \
  "$fixture_dir/earlier/Whiplash.mkv"
now="$(date +%s)"
cat >"$XDG_STATE_HOME/ok-player/history.json" <<JSON
{
  "version": 2,
  "files": {
    "$fixture_dir/today/Dune.mkv": {"position":6840,"duration":7920,"finished":false,"updated_at_unix":$((now-600)),"title":"Dune: Part Two"},
    "$fixture_dir/today/Severance.mkv": {"position":0,"duration":2520,"finished":true,"updated_at_unix":$((now-3600)),"title":"Severance — S02E07"},
    "$fixture_dir/today/Blade-Runner-2049.mkv": {"position":1800,"duration":5400,"finished":false,"updated_at_unix":$((now-7200)),"title":"Blade Runner 2049"},
    "$fixture_dir/yesterday/interview.mov": {"position":120,"duration":3240,"finished":false,"updated_at_unix":$((now-86400)),"title":"interview-raw-take3.mov"},
    "$fixture_dir/week/Free-Solo.mp4": {"position":880,"duration":6000,"finished":false,"updated_at_unix":$((now-3*86400)),"title":"Free Solo"},
    "$fixture_dir/week/Wedding.mov": {"position":180,"duration":5760,"finished":false,"updated_at_unix":$((now-5*86400)),"title":"wedding-ceremony-4k.mov"},
    "$fixture_dir/earlier/Past-Lives.mkv": {"position":0,"duration":6360,"finished":true,"updated_at_unix":$((now-10*86400)),"title":"Past Lives"},
    "$fixture_dir/earlier/Whiplash.mkv": {"position":0,"duration":6420,"finished":true,"updated_at_unix":$((now-40*86400)),"title":"Whiplash"}
  }
}
JSON

capture() {
  local shot="$1" width="$2" height="$3"
  shift 3
  rm -f "$OUT_DIR/app.log" "$OUT_DIR/window.ids"
  env OKP_SKIP_OPEN_INSTALLER=1 OKP_SKIP_DEB_SELF_INSTALL=1 "$@" \
    timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
  sleep 5
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids" || true
  local window_id
  window_id="$(head -n1 "$OUT_DIR/window.ids" || true)"
  if [[ -z "$window_id" ]]; then
    echo "$shot: main window did not appear" >&2
    cat "$OUT_DIR/app.log" >&2 || true
    exit 1
  fi
  if [[ "$shot" == history-* ]] && xdotool search --name '^History$' >/dev/null 2>&1; then
    echo "$shot: History opened a separate window instead of replacing the idle canvas" >&2
    exit 1
  fi
  if [[ "$width" != 1120 || "$height" != 680 ]]; then
    xdotool windowsize "$window_id" "$width" "$height"
    sleep 1
  fi
  import -window "$window_id" "$OUT_DIR/$shot.png"
  local actual_width actual_height
  actual_width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
  actual_height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
  if (( actual_width < width - 8 || actual_width > width + 8 || actual_height < height - 8 || actual_height > height + 8 )); then
    echo "$shot: unexpected geometry ${actual_width}x${actual_height}, expected ${width}x${height}" >&2
    exit 1
  fi
  local maxima
  maxima="$(magick "$OUT_DIR/$shot.png" -colorspace gray -format '%[fx:maxima]' info:)"
  if ! awk -v max="$maxima" 'BEGIN {exit !(max > 0.45)}'; then
    echo "$shot: surface looks blank: maxima=$maxima" >&2
    exit 1
  fi
  echo "$shot: geometry=${actual_width}x${actual_height} maxima=$maxima"
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture_panel() {
  local mode="$1" shot="$2"
  capture "$shot" 1120 680 OKP_IDLE_THEME=dark OKP_OPEN_SIDE_PANEL_ON_STARTUP="$mode"
  local panel_max
  panel_max="$(magick "$OUT_DIR/$shot.png" -crop 292x520+816+52 -colorspace gray -format '%[fx:maxima]' info:)"
  if ! awk -v max="$panel_max" 'BEGIN {exit !(max > 0.45)}'; then
    echo "$shot: side panel looks blank: maxima=$panel_max" >&2
    exit 1
  fi
}

capture_panel up-next-empty up-next-empty
capture_panel chapters-empty chapters-empty

# The short queue pins its current item in teal and the no-chapters state must
# render dark copy and its bookmark action over the light panel substrate.
up_next_red="$(magick "$OUT_DIR/up-next-empty.png" -crop 280x70+820+94 -format '%[fx:mean.r]' info:)"
up_next_green="$(magick "$OUT_DIR/up-next-empty.png" -crop 280x70+820+94 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$up_next_red" -v g="$up_next_green" 'BEGIN {exit !(g-r > 0.01)}'; then
  echo "Up Next short-queue accent missing: red=$up_next_red green=$up_next_green" >&2
  exit 1
fi
left_dark="$(magick "$OUT_DIR/chapters-empty.png" -crop 150x300+816+90 -colorspace gray -threshold 50% -format '%[fx:(1-mean)*w*h]' info:)"
left_dark="${left_dark%.*}"
if (( left_dark < 40 )); then
  echo "Chapters no-chapters message did not render (left dark pixels: $left_dark)" >&2
  exit 1
fi

for theme in light dark; do
  capture "first-run-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_WELCOME_STATE=empty
  capture "continue-watching-$theme" 1120 680 OKP_IDLE_THEME="$theme"
  capture "history-has-data-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1
  capture "history-private-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_PRIVATE_SESSION_ON_STARTUP=1
  capture "history-empty-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_HISTORY_STATE=empty
  capture "history-cleared-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_HISTORY_STATE=cleared
  capture "history-error-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_HISTORY_STATE=error
  capture "history-loading-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_HISTORY_STATE=loading
  capture "history-no-match-$theme" 1120 680 OKP_IDLE_THEME="$theme" OKP_OPEN_HISTORY_ON_STARTUP=1 OKP_HISTORY_STATE=no-match
done
capture continue-watching-narrow 480 540 OKP_IDLE_THEME=light
capture history-has-data-narrow 480 540 OKP_IDLE_THEME=light OKP_OPEN_HISTORY_ON_STARTUP=1

# First run: centered teal tile and one dashed Open/drop target on a full-window canvas.
for theme in light dark; do
  shot="$OUT_DIR/first-run-$theme.png"
  tile_red="$(magick "$shot" -crop 80x80+520+225 -format '%[fx:mean.r]' info:)"
  tile_green="$(magick "$shot" -crop 80x80+520+225 -format '%[fx:mean.g]' info:)"
  if ! awk -v r="$tile_red" -v g="$tile_green" 'BEGIN {exit !(g-r > 0.05)}'; then
    echo "first-run-$theme: teal brand tile missing: red=$tile_red green=$tile_green" >&2
    exit 1
  fi
  open_edges="$(magick "$shot" -crop 330x115+395+345 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
  if ! awk -v edge="$open_edges" 'BEGIN {exit !(edge > 0.01)}'; then
    echo "first-run-$theme: dashed Open/drop target missing: edge=$open_edges" >&2
    exit 1
  fi
done

# Continue Watching: the center-bottom band must remain calm. The old disabled OSC
# created a dense high-contrast bar here; the canonical canvas leaves only the footer.
for theme in light dark; do
  shot="$OUT_DIR/continue-watching-$theme.png"
  osc_stddev="$(magick "$shot" -crop 760x42+180+575 -colorspace gray -format '%[fx:standard_deviation]' info:)"
  if ! awk -v dev="$osc_stddev" 'BEGIN {exit !(dev < 0.08)}'; then
    echo "continue-watching-$theme: idle OSC or duplicate chrome detected: stddev=$osc_stddev" >&2
    exit 1
  fi
  shelf_edges="$(magick "$shot" -crop 650x190+230+185 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
  if ! awk -v edge="$shelf_edges" 'BEGIN {exit !(edge > 0.015)}'; then
    echo "continue-watching-$theme: recent shelf did not render: edge=$shelf_edges" >&2
    exit 1
  fi
done

# Narrow capture must preserve a readable title and at least one complete 194px card
# without entering the caption-control strip or clipping horizontally.
narrow="$OUT_DIR/continue-watching-narrow.png"
overlap_edges="$(magick "$narrow" -crop 320x24+0+34 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
card_edges="$(magick "$narrow" -crop 210x135+35+140 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$overlap_edges" 'BEGIN {exit !(edge < 0.02)}'; then
  echo "Narrow Continue Watching overlaps the titlebar: edge=$overlap_edges" >&2
  exit 1
fi
if ! awk -v edge="$card_edges" 'BEGIN {exit !(edge > 0.012)}'; then
  echo "Narrow Continue Watching lost its card hierarchy: edge=$card_edges" >&2
  exit 1
fi

history_narrow="$OUT_DIR/history-has-data-narrow.png"
history_header_edges="$(magick "$history_narrow" -crop 430x90+20+55 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
history_row_edges="$(magick "$history_narrow" -crop 430x210+20+150 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$history_header_edges" 'BEGIN {exit !(edge > 0.01)}'; then
  echo "Narrow History lost its header hierarchy: edge=$history_header_edges" >&2
  exit 1
fi
if ! awk -v edge="$history_row_edges" 'BEGIN {exit !(edge > 0.01)}'; then
  echo "Narrow History rows are blank or clipped: edge=$history_row_edges" >&2
  exit 1
fi

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

echo "Linux canonical idle/history smoke captured in $OUT_DIR"
