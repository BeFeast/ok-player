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
camera_dir="$fixture_dir/yesterday/Camera Originals/2026-07-16 Production Day/Primary Camera A"
camera_file="A001_C014_0716QZ_001-super-long-camera-original-filename-with-scene-and-take.mov"
mkdir -p "$fixture_dir/today" "$camera_dir" "$fixture_dir/week" "$fixture_dir/earlier"
touch \
  "$fixture_dir/today/Dune.mkv" \
  "$fixture_dir/today/Severance.mkv" \
  "$fixture_dir/today/Blade-Runner-2049.mkv" \
  "$camera_dir/$camera_file" \
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
    "$camera_dir/$camera_file": {"position":120,"duration":3240,"finished":false,"updated_at_unix":$((now-86400)),"title":"$camera_file"},
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
  if [[ "$shot" == history-via-recents-arrow-* ]]; then
    import -window "$window_id" "$OUT_DIR/$shot-before.png"
    xdotool mousemove --window "$window_id" 868 220 click 1
    sleep 1
    if xdotool search --name '^History$' >/dev/null 2>&1; then
      echo "$shot: recents arrow opened a separate History window" >&2
      exit 1
    fi
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
capture history-has-data-high-contrast 1120 680 GTK_THEME=HighContrast OKP_IDLE_THEME=dark OKP_OPEN_HISTORY_ON_STARTUP=1
capture history-via-recents-arrow-light 1120 680 OKP_IDLE_THEME=light
capture continue-watching-narrow 480 540 OKP_IDLE_THEME=light
capture continue-watching-narrow-actions 480 760 OKP_IDLE_THEME=light
capture history-has-data-narrow 480 540 OKP_IDLE_THEME=light OKP_OPEN_HISTORY_ON_STARTUP=1
capture history-long-path-narrow 480 760 OKP_IDLE_THEME=dark OKP_OPEN_HISTORY_ON_STARTUP=1

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
trim_offset() {
  local image="$1" crop="$2" mode="$3"
  local geometry offsets
  if [[ "$mode" == direct ]]; then
    geometry="$(magick "$image" -crop "$crop" +repage -colorspace gray -threshold 50% -trim -format '%g' info:)"
  else
    geometry="$(magick "$image" -crop "$crop" +repage -colorspace gray -edge 1 -threshold 12% -trim -format '%g' info:)"
  fi
  offsets="${geometry#*+}"
  printf '%s %s\n' "${offsets%%+*}" "${offsets##*+}"
}

contrast_bounds() {
  local image="$1" crop="$2" axis="$3" baseline_index="$4" delta="$5"
  magick "$image" -crop "$crop" +repage -colorspace gray -depth 8 txt:- |
    awk -v axis="$axis" -v baseline_index="$baseline_index" -v delta="$delta" '
      NR > 1 {
        position = $1
        sub(/:/, "", position)
        split(position, xy, ",")
        coordinate = axis == "x" ? xy[1] : xy[2]
        value = $0
        sub(/^.*gray\(/, "", value)
        sub(/\).*$/, "", value)
        samples[coordinate] = value
      }
      END {
        baseline = samples[baseline_index]
        for (coordinate in samples) {
          difference = samples[coordinate] - baseline
          if (difference > delta || difference < -delta) {
            if (minimum == "" || coordinate + 0 < minimum) minimum = coordinate + 0
            if (maximum == "" || coordinate + 0 > maximum) maximum = coordinate + 0
          }
        }
        if (minimum != "" && maximum != "") print minimum, maximum
      }
    '
}

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

  read -r heading_x heading_y < <(trim_offset "$shot" 500x70+180+60 direct)
  read -r card_x card_y < <(trim_offset "$shot" 720x130+190+130 edge)
  read -r action_x action_y < <(trim_offset "$shot" 720x170+190+300 edge)
  read -r action_min_x action_max_x < <(contrast_bounds "$shot" 160x1+200+340 x 0 20)
  read -r drop_min_x drop_max_x < <(contrast_bounds "$shot" 545x1+360+350 x 100 15)
  read -r drop_min_y drop_max_y < <(contrast_bounds "$shot" 1x110+500+310 y 40 15)
  if [[ -z "${action_min_x:-}" || -z "${drop_min_x:-}" || -z "${drop_min_y:-}" ]]; then
    echo "continue-watching-$theme: action-row bounds could not be measured" >&2
    exit 1
  fi
  read -r _ footer_y < <(trim_offset "$shot" 1000x40+60+630 edge)
  heading_x=$((180 + heading_x))
  heading_y=$((60 + heading_y))
  card_x=$((190 + card_x))
  card_y=$((130 + card_y))
  action_x=$((190 + action_x))
  action_y=$((300 + action_y))
  action_column_x=$((200 + action_min_x))
  action_column_width=$((action_max_x - action_min_x + 1))
  drop_target_x=$((360 + drop_min_x + 1))
  drop_target_width=$((drop_max_x - drop_min_x - 1))
  action_row_height=$((drop_max_y - drop_min_y - 1))
  footer_y=$((630 + footer_y))
  if (( heading_y < 72 || heading_y > 76 )); then
    echo "continue-watching-$theme: heading top is y=$heading_y, expected 74 +/- 2" >&2
    exit 1
  fi
  if (( heading_x < 218 || heading_x > 224 || card_x < 218 || card_x > 222 )); then
    echo "continue-watching-$theme: wrapper shifted: heading-x=$heading_x card-x=$card_x" >&2
    exit 1
  fi
  if (( card_y < 144 || card_y > 148 )); then
    echo "continue-watching-$theme: cards top is y=$card_y, expected about 146" >&2
    exit 1
  fi
  if (( action_y < 318 || action_y > 322 )); then
    echo "continue-watching-$theme: action row top is y=$action_y, expected about 320" >&2
    exit 1
  fi
  if (( action_column_x < 219 || action_column_x > 221 || action_column_width < 130 || action_column_width > 134 )); then
    echo "continue-watching-$theme: action column is x=$action_column_x width=$action_column_width, expected x=220 width about 132" >&2
    exit 1
  fi
  if (( drop_target_x < 365 || drop_target_x > 367 || drop_target_width < 532 || drop_target_width > 536 )); then
    echo "continue-watching-$theme: drop target is x=$drop_target_x width=$drop_target_width, expected x=366 width about 534" >&2
    exit 1
  fi
  if (( action_row_height < 82 || action_row_height > 86 )); then
    echo "continue-watching-$theme: action row height is $action_row_height, expected about 84" >&2
    exit 1
  fi
  if (( footer_y < 636 || footer_y > 641 )); then
    echo "continue-watching-$theme: footer moved: first edge y=$footer_y" >&2
    exit 1
  fi
  arrow_edges="$(magick "$shot" -crop 48x148+844+146 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
  if ! awk -v edge="$arrow_edges" 'BEGIN {exit !(edge > 0.006)}'; then
    echo "continue-watching-$theme: trailing History arrow missing: edge=$arrow_edges" >&2
    exit 1
  fi
  echo "continue-watching-$theme: heading=(${heading_x},${heading_y}) cards=(${card_x},${card_y}) actions=(${action_column_x},${action_y}) action-width=${action_column_width} drop=(${drop_target_x},${action_y}) drop-width=${drop_target_width} row-height=${action_row_height} footer-y=${footer_y} arrow-edge=${arrow_edges}"
done

arrow_history="$OUT_DIR/history-via-recents-arrow-light.png"
arrow_history_before="$OUT_DIR/history-via-recents-arrow-light-before.png"
arrow_history_rmse="$({ magick compare -metric RMSE "$arrow_history_before" "$arrow_history" null: || true; } 2>&1 | sed -n 's/.*(\([^)]*\)).*/\1/p')"
if ! awk -v rmse="$arrow_history_rmse" 'BEGIN {exit !(rmse > 0.05)}'; then
  echo "Recents arrow did not switch the in-canvas surface: normalized-rmse=$arrow_history_rmse" >&2
  exit 1
fi

# Narrow capture must preserve a readable title and at least one complete 194px card
# without entering the caption-control strip or clipping horizontally.
narrow="$OUT_DIR/continue-watching-narrow.png"
overlap_edges="$(magick "$narrow" -crop 320x24+0+34 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
card_edges="$(magick "$narrow" -crop 210x135+35+140 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
right_edges="$(magick "$narrow" -crop 8x390+472+70 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$overlap_edges" 'BEGIN {exit !(edge < 0.02)}'; then
  echo "Narrow Continue Watching overlaps the titlebar: edge=$overlap_edges" >&2
  exit 1
fi
if ! awk -v edge="$card_edges" 'BEGIN {exit !(edge > 0.012)}'; then
  echo "Narrow Continue Watching lost its card hierarchy: edge=$card_edges" >&2
  exit 1
fi
if ! awk -v edge="$right_edges" 'BEGIN {exit !(edge < 0.005)}'; then
  echo "Narrow Continue Watching clips content at the right edge: edge=$right_edges" >&2
  exit 1
fi

# A taller capture at the same narrow width must show both complete action children above
# the fixed footer; the initial 480x540 hierarchy capture alone cannot prove this reflow.
narrow_actions="$OUT_DIR/continue-watching-narrow-actions.png"
action_edges="$(magick "$narrow_actions" -crop 150x92+24+470 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
drop_edges="$(magick "$narrow_actions" -crop 424x94+24+570 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
drop_bottom_edges="$(magick "$narrow_actions" -crop 424x6+24+662 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
footer_edges="$(magick "$narrow_actions" -crop 440x42+20+718 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
tall_right_edges="$(magick "$narrow_actions" -crop 8x610+472+70 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$action_edges" 'BEGIN {exit !(edge > 0.01)}'; then
  echo "Tall narrow Continue Watching lost the action column: edge=$action_edges" >&2
  exit 1
fi
if ! awk -v edge="$drop_edges" 'BEGIN {exit !(edge > 0.005)}'; then
  echo "Tall narrow Continue Watching lost the drop target: edge=$drop_edges" >&2
  exit 1
fi
if ! awk -v edge="$drop_bottom_edges" 'BEGIN {exit !(edge > 0.02)}'; then
  echo "Tall narrow drop target remains hidden behind the footer: edge=$drop_bottom_edges" >&2
  exit 1
fi
if ! awk -v edge="$footer_edges" 'BEGIN {exit !(edge > 0.006)}'; then
  echo "Tall narrow Continue Watching lost its fixed footer: edge=$footer_edges" >&2
  exit 1
fi
if ! awk -v edge="$tall_right_edges" 'BEGIN {exit !(edge < 0.005)}'; then
  echo "Tall narrow Continue Watching clips content at the right edge: edge=$tall_right_edges" >&2
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

# History must use the canonical 792px desktop wrapper instead of retaining its compact
# natural width. The divider spans the 740px inner content area at x=190..929.
read -r history_divider_min history_divider_max < <(
  contrast_bounds "$OUT_DIR/history-has-data-light.png" 1000x1+60+148 x 0 4
)
history_divider_min=$((60 + history_divider_min))
history_divider_max=$((60 + history_divider_max))
if (( history_divider_min < 188 || history_divider_min > 192 || history_divider_max < 927 || history_divider_max > 931 )); then
  echo "Desktop History wrapper drifted: divider=${history_divider_min}..${history_divider_max}, expected 190..929" >&2
  exit 1
fi

# The first and third progress rows are separated by two deliberate 65px row pitches.
# Their progress fill must remain inside the 64px thumbnail rather than taking GTK's
# default progress-bar minimum width and colliding with the title column.
mapfile -t history_progress_runs < <(
  magick "$OUT_DIR/history-has-data-light.png" -crop 1x240+203+180 +repage -depth 8 txt:- |
    awk '
      NR > 1 {
        coordinate = $1
        sub(/:.*/, "", coordinate)
        split(coordinate, xy, ",")
        value = $0
        sub(/^.*srgb\(/, "", value)
        sub(/\).*$/, "", value)
        split(value, rgb, ",")
        hit = rgb[2] - rgb[1] > 35 && rgb[2] - rgb[3] > 0
        if (hit && !inside) {
          print xy[2]
          inside = 1
        } else if (!hit) {
          inside = 0
        }
      }
    '
)
if (( ${#history_progress_runs[@]} < 2 )); then
  echo "History progress rows could not be measured" >&2
  exit 1
fi
history_row_pitch=$((history_progress_runs[1] - history_progress_runs[0]))
if (( history_row_pitch < 128 || history_row_pitch > 132 )); then
  echo "History row rhythm drifted: two-row pitch=$history_row_pitch, expected about 130" >&2
  exit 1
fi
read -r _ _ history_progress_width < <(
  magick "$OUT_DIR/history-has-data-light.png" -crop 300x1+180+235 +repage -depth 8 txt:- |
    awk '
      NR > 1 {
        coordinate = $1
        sub(/:.*/, "", coordinate)
        split(coordinate, xy, ",")
        value = $0
        sub(/^.*srgb\(/, "", value)
        sub(/\).*$/, "", value)
        split(value, rgb, ",")
        if (rgb[2] - rgb[1] > 35 && rgb[2] - rgb[3] > 0) {
          if (minimum == "") minimum = xy[1]
          maximum = xy[1]
        }
      }
      END {
        if (minimum != "") print minimum, maximum, maximum - minimum + 1
      }
    '
)
if (( history_progress_width < 45 || history_progress_width > 64 )); then
  echo "History thumbnail progress escaped its 64px frame: width=$history_progress_width" >&2
  exit 1
fi

# Four complete rows, including the long camera filename/path, remain visible in the
# standard viewport. At 480px the title/path still ellipsize before the reserved metadata
# block, with neither column drawing into the right-edge clip probe.
history_long_row_edges="$(magick "$OUT_DIR/history-has-data-light.png" -crop 740x52+190+430 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
history_narrow_metadata_edges="$(magick "$history_narrow" -crop 90x260+365+190 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
history_narrow_right_edges="$(magick "$history_narrow" -crop 8x400+472+70 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$history_long_row_edges" 'BEGIN {exit !(edge > 0.012)}'; then
  echo "Desktop History lost the fourth long-path row: edge=$history_long_row_edges" >&2
  exit 1
fi
if ! awk -v edge="$history_narrow_metadata_edges" 'BEGIN {exit !(edge > 0.01)}'; then
  echo "Narrow History lost its right metadata block: edge=$history_narrow_metadata_edges" >&2
  exit 1
fi
if ! awk -v edge="$history_narrow_right_edges" 'BEGIN {exit !(edge < 0.005)}'; then
  echo "Narrow History clips content at the right edge: edge=$history_narrow_right_edges" >&2
  exit 1
fi

history_hc="$OUT_DIR/history-has-data-high-contrast.png"
history_hc_mean="$(magick "$history_hc" -colorspace gray -format '%[fx:mean]' info:)"
history_hc_row_edges="$(magick "$history_hc" -crop 740x250+190+180 -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v mean="$history_hc_mean" 'BEGIN {exit !(mean < 0.12)}'; then
  echo "High-contrast History did not use the solid dark substrate: mean=$history_hc_mean" >&2
  exit 1
fi
if ! awk -v edge="$history_hc_row_edges" 'BEGIN {exit !(edge > 0.02)}'; then
  echo "High-contrast History lost row boundaries and text: edge=$history_hc_row_edges" >&2
  exit 1
fi

echo "history-redline: divider=${history_divider_min}..${history_divider_max} two-row-pitch=$history_row_pitch progress-width=$history_progress_width long-row-edge=$history_long_row_edges narrow-metadata-edge=$history_narrow_metadata_edges high-contrast-edge=$history_hc_row_edges"

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

echo "Linux canonical idle/history smoke captured in $OUT_DIR"
