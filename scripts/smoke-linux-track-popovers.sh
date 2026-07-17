#!/usr/bin/env bash
# Deterministic visual smoke for the quick pickers plus the shared searchable
# command surfaces from issues #264 and #377.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-track-popovers-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi

if [[ ! -x "$BINARY" ]] && ! command -v "$BINARY" >/dev/null 2>&1; then
  echo "Player binary is not executable: $BINARY" >&2
  exit 127
fi

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi
export LIBGL_ALWAYS_SOFTWARE=1

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$FIXTURE" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

popup_window() {
  local tree="$1"
  local best_area=0 best=""
  while read -r id geometry; do
    if [[ "$geometry" =~ ^([0-9]+)x([0-9]+)\+(-?[0-9]+)\+(-?[0-9]+)$ ]]; then
      local width="${BASH_REMATCH[1]}" height="${BASH_REMATCH[2]}"
      local x="${BASH_REMATCH[3]}" y="${BASH_REMATCH[4]}"
      local area=$((width * height))
      if (( width < 600 && height > 80 && area > best_area )); then
        best_area=$area
        best="$id $width $height $x $y"
      fi
    fi
  done < <(awk '/^     0x.*"okp-linux-gtk"/ {print $1, $5}' "$tree")

  [[ -n "$best" ]] || return 1
  printf '%s\n' "$best"
}

capture() {
  local shot="$1" substrate="$2" popover="$3" anchor_x="$4" requested_width="$5"
  local preview_state="${6:-}"
  local click_button="${7:-1}"
  local anchor_y="${8:-630}"
  local verify_anchor="${9:-yes}"
  local query="${10:-}"
  local target_width="${11:-1120}"
  local target_height="${12:-680}"
  local narrow_fixture="${13:-no}"
  local state_dir="$OUT_DIR/state-$shot"
  local config_dir="$OUT_DIR/config-$shot"
  mkdir -p "$state_dir" "$config_dir/ok-player"
  printf '%s\n' '{"version":2,"updates":{"auto_check":false}}' \
    >"$config_dir/ok-player/settings.json"

  local env_args=(
    "XDG_STATE_HOME=$state_dir"
    "XDG_CONFIG_HOME=$config_dir"
    "OKP_GTK_THEME_PREVIEW=$([[ "$substrate" == dark ]] && echo dark || echo light)"
  )
  if [[ "$target_width" == 1120 && "$target_height" == 680 ]]; then
    env_args+=("OKP_PLAYBACK_FRAME_PREVIEW=$substrate")
  else
    env_args+=("OKP_NARROW_COMMAND_PREVIEW=1")
    env_args+=("OKP_PLAYBACK_FRAME_PREVIEW=$substrate")
  fi
  if [[ -n "$preview_state" ]]; then
    env_args+=("OKP_PLAYER_POPOVER_PREVIEW_STATE=$preview_state")
  fi
  if [[ -n "$query" ]]; then
    env_args+=("OKP_COMMAND_SEARCH_QUERY=$query")
  fi

  local launch_env=(env)
  local app_args=()
  if [[ "$target_width" == 1120 && "$target_height" == 680 || "$narrow_fixture" == yes ]]; then
    app_args+=("$FIXTURE")
  fi
  "${launch_env[@]}" "${env_args[@]}" timeout 20s "$BINARY" "${app_args[@]}" \
    >"$OUT_DIR/$shot-app.log" 2>&1 &
  app_pid=$!

  local window_id=""
  for _ in $(seq 1 200); do
    while read -r candidate; do
      [[ -n "$candidate" ]] || continue
      candidate_width="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Width:/ {print $2; exit}')"
      if [[ -n "$candidate_width" ]] && (( candidate_width > 400 )); then
        window_id="$candidate"
        break
      fi
    done < <(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null || true)
    [[ -n "$window_id" ]] && break
    sleep 0.1
  done
  if [[ -z "$window_id" ]]; then
    echo "$shot: main window did not appear" >&2
    cat "$OUT_DIR/$shot-app.log" >&2 || true
    exit 1
  fi
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  if [[ "$target_width" != 1120 || "$target_height" != 680 ]]; then
    sleep 4
    xdotool windowsize "$window_id" "$target_width" "$target_height"
    sleep 1
  fi
  xdotool key --clearmodifiers space
  xdotool mousemove --window "$window_id" 560 340
  sleep 1

  xwininfo -id "$window_id" >"$OUT_DIR/$shot-window.xwininfo"
  local window_width window_height window_x
  window_width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/$shot-window.xwininfo")"
  window_height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/$shot-window.xwininfo")"
  window_x="$(awk -F': ' '/Absolute upper-left X:/ {print $2; exit}' "$OUT_DIR/$shot-window.xwininfo" | xargs)"
  if [[ "$window_width" != "$target_width" || "$window_height" != "$target_height" ]]; then
    echo "$shot: unexpected player geometry ${window_width}x${window_height}; expected ${target_width}x${target_height}" >&2
    exit 1
  fi

  xdotool mousemove --window "$window_id" "$anchor_x" "$anchor_y" click "$click_button"
  sleep 1
  import -window root "$OUT_DIR/$shot-window.png"
  xwininfo -root -tree >"$OUT_DIR/$shot-tree.txt"

  local popup_id outer_width outer_height outer_x outer_y
  read -r popup_id outer_width outer_height outer_x outer_y \
    <<<"$(popup_window "$OUT_DIR/$shot-tree.txt")"

  # GTK allocates 48 transparent shadow pixels horizontally, 36 above, and 60
  # below. The visible surface is the requested width plus its two borders.
  local visible_width=$((outer_width - 96))
  local visible_height=$((outer_height - 96))
  local expected_visible_width=$((requested_width + 2))
  if (( visible_width != expected_visible_width )); then
    echo "$shot: $popover width is $visible_width, expected $expected_visible_width" >&2
    exit 1
  fi

  local popup_center=$((outer_x + outer_width / 2))
  local expected_center=$((window_x + anchor_x))
  local anchor_delta=$((popup_center - expected_center))
  (( anchor_delta < 0 )) && anchor_delta=$((-anchor_delta))
  if [[ "$verify_anchor" == "yes" ]] && (( anchor_delta > 3 )); then
    echo "$shot: $popover anchor delta is ${anchor_delta}px" >&2
    exit 1
  fi

  import -window "$popup_id" "$OUT_DIR/$shot-outer.png"
  magick "$OUT_DIR/$shot-outer.png" \
    -crop "${visible_width}x${visible_height}+48+36" +repage \
    "$OUT_DIR/$shot-popover.png"

  local material_mean edge_mean
  material_mean="$(magick "$OUT_DIR/$shot-popover.png" -colorspace gray -format '%[fx:mean]' info:)"
  edge_mean="$(magick "$OUT_DIR/$shot-popover.png" -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
  local material_ok
  if [[ "$substrate" == dark && ( "$popover" == more || "$popover" == context ) ]]; then
    material_ok="$(awk -v mean="$material_mean" -v edge="$edge_mean" 'BEGIN {print (mean > 0.08 && mean < 0.45 && edge > 0.003) ? "yes" : "no"}')"
  else
    material_ok="$(awk -v mean="$material_mean" -v edge="$edge_mean" 'BEGIN {print (mean > 0.72 && edge > 0.003) ? "yes" : "no"}')"
  fi
  if [[ "$material_ok" != yes ]]; then
    echo "$shot: popover material/content failed: mean=$material_mean edge=$edge_mean" >&2
    exit 1
  fi

  local preference_variance="n/a"
  if [[ "$popover" == "subtitles" ]]; then
    local minimum_height=290
    [[ "$preview_state" == "subtitle-selected" ]] && minimum_height=380
    if (( visible_height < minimum_height )); then
      echo "$shot: subtitle preference controls are missing; height=${visible_height}, minimum=${minimum_height}" >&2
      exit 1
    fi
    local preference_y=$((visible_height - 120))
    preference_variance="$(
      magick "$OUT_DIR/$shot-popover.png" \
        -crop "250x115+7+${preference_y}" \
        -colorspace gray \
        -format '%[fx:standard_deviation]' info:
    )"
    if ! awk -v variance="$preference_variance" 'BEGIN { exit !(variance > 0.05) }'; then
      echo "$shot: subtitle Size/Style band is unexpectedly flat: variance=${preference_variance}" >&2
      exit 1
    fi
  fi

  printf '%s\n' \
    "popover=$popover" \
    "requested_width=$requested_width" \
    "visible_geometry=${visible_width}x${visible_height}" \
    "anchor_delta=$anchor_delta" \
    "material_mean=$material_mean" \
    "edge_mean=$edge_mean" \
    "preference_variance=$preference_variance" >"$OUT_DIR/$shot.txt"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 1
}

capture_set="${OKP_COMMAND_CAPTURE_SET:-all}"

if [[ "${OKP_COMMAND_SURFACES_ONLY:-0}" != 1 && "$capture_set" == all ]]; then
  capture subtitle-srt-selected-dark dark subtitles 822 262 subtitle-srt-selected
  for substrate in bright dark; do
    capture "speed-selected-$substrate" "$substrate" speed 749 120
    capture "subtitle-selected-$substrate" "$substrate" subtitles 822 262 subtitle-selected
    capture "audio-selected-$substrate" "$substrate" audio 874 248 audio-selected
  done
  capture subtitle-empty-dark dark subtitles 822 262 subtitle-empty
  capture subtitle-searchable-dark dark subtitles 822 262 subtitle-searchable
  capture audio-empty-dark dark audio 874 248 audio-empty
fi

if [[ "$capture_set" == all || "$capture_set" == more ]]; then
  for substrate in bright dark; do
    capture "more-$substrate" "$substrate" more 1072 340 "" 1 630 no
  done
  capture more-disabled-bright bright more 1072 340 more-disabled 1 630 no
  capture more-search-window-dark dark more 1072 340 "" 1 630 no window
  capture more-no-results-light bright more 1072 340 "" 1 630 no zzz-no-command
fi
# Right-click is a second presentation of the same command registry, geometry,
# search, and action dispatcher.
if [[ "$capture_set" == all || "$capture_set" == context ]]; then
  capture context-right-click-bright bright context 560 340 "" 3 340 no
  capture context-search-window-dark dark context 560 340 "" 3 340 no window
  capture context-no-results-light bright context 560 340 "" 3 340 no zzz-no-command
fi
if [[ "$capture_set" == all || "$capture_set" == narrow ]]; then
  capture more-narrow-light bright more 432 340 "" 1 220 no "" 480 270 yes
fi

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  echo "Track-popover smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

echo "Linux track-popover smoke passed. Captures: $OUT_DIR"
