#!/usr/bin/env bash
# X11/Xvfb regression for the interaction geometry diagnostic (#690).
#
# The real player is launched with OKP_DEBUG_INTERACTIONS=1, and the geometry record it
# publishes is checked against the window the display server actually has: the reported
# client size must match xwininfo, the drag target must sit inside the video plane and
# clear of every interactive chrome plane, and a press aimed from the record alone must
# reach the video surface on the first attempt. A second run with the variable unset must
# publish nothing at all.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

if [[ "${1:-}" == "--inner" ]]; then
  shift
  BINARY="${1:?missing binary}"
  FIXTURE="${2:?missing fixture}"
  OUT_DIR="${3:?missing output directory}"

  export GDK_BACKEND=x11
  export GTK_USE_PORTAL=0
  export NO_AT_BRIDGE=1
  export XDG_SESSION_TYPE=x11
  export XDG_CURRENT_DESKTOP=XFCE
  export XDG_STATE_HOME="$OUT_DIR/state"
  export XDG_CONFIG_HOME="$OUT_DIR/config"
  export XDG_CACHE_HOME="$OUT_DIR/cache"
  export LIBGL_ALWAYS_SOFTWARE=1
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
  export OKP_FIXED_VIEWPORT_SMOKE=1
  export OKP_DISABLE_MPRIS=1
  export OKP_SKIP_OPEN_INSTALLER=1
  export OKP_SKIP_DEB_SELF_INSTALL=1
  export OKP_SKIP_UPDATE_CHECK=1

  xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
  wm_pid=$!
  app_pid=""
  cleanup() {
    [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
    kill "$wm_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for _ in $(seq 1 100); do
    if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; then
      break
    fi
    sleep 0.05
  done
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' || {
    echo "xfwm4 did not become ready" >&2
    exit 75
  }

  wait_for_window() {
    local id=""
    for _ in $(seq 1 80); do
      id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | head -n1 || true)"
      [[ -n "$id" ]] && break
      sleep 0.25
    done
    [[ -n "$id" ]] || return 1
    printf '%s\n' "$id"
  }

  # Newest value of one key on one part of the geometry record, rounded to whole pixels.
  field() {
    awk -v part="$1" -v key="$2" '
      index($0, "interaction: geometry part=" part " ") == 1 {
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == key) { value = pair[2] }
        }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$3"
  }

  # Wait for one plane of the record. A cold GTK/libmpv start under Xvfb can take a while,
  # so this is patient; every caller names the plane its checks actually need.
  wait_for_plane() {
    local log="$1" part="$2"
    for _ in $(seq 1 160); do
      if [[ -n "$(field "$part" local-x "$log")" ]]; then
        return 0
      fi
      sleep 0.25
    done
    return 1
  }

  timeout 70s env OKP_DEBUG_INTERACTIONS=1 "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
  window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  sleep 3
  wait_for_plane "$OUT_DIR/app.log" drag-target || {
    echo "no geometry record was published with OKP_DEBUG_INTERACTIONS=1" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }

  read -r actual_width actual_height < <(xwininfo -id "$window_id" | awk '
    /^  Width:/ { w = $2 }
    /^  Height:/ { h = $2 }
    END { print w, h }
  ')
  reported_width="$(field window w "$OUT_DIR/app.log")"
  reported_height="$(field window h "$OUT_DIR/app.log")"
  [[ "$reported_width" == "$actual_width" && "$reported_height" == "$actual_height" ]] || {
    echo "reported client size ${reported_width}x${reported_height} does not match the server's ${actual_width}x${actual_height}" >&2
    exit 1
  }

  video_x="$(field video local-x "$OUT_DIR/app.log")"
  video_y="$(field video local-y "$OUT_DIR/app.log")"
  video_w="$(field video w "$OUT_DIR/app.log")"
  video_h="$(field video h "$OUT_DIR/app.log")"
  drag_x="$(field drag-target local-x "$OUT_DIR/app.log")"
  drag_y="$(field drag-target local-y "$OUT_DIR/app.log")"
  drag_w="$(field drag-target w "$OUT_DIR/app.log")"
  drag_h="$(field drag-target h "$OUT_DIR/app.log")"
  for value in "$video_w" "$video_h" "$drag_w" "$drag_h"; do
    [[ -n "$value" && "$value" -gt 0 ]] || {
      echo "the geometry record is missing a plane rectangle" >&2
      exit 1
    }
  done
  (( drag_x >= video_x && drag_y >= video_y )) || {
    echo "the drag target starts outside the video plane" >&2
    exit 1
  }
  (( drag_x + drag_w <= video_x + video_w && drag_y + drag_h <= video_y + video_h )) || {
    echo "the drag target extends past the video plane" >&2
    exit 1
  }

  center_x=$((drag_x + drag_w / 2))
  center_y=$((drag_y + drag_h / 2))

  # The titlebar is an interactive chrome plane over the video: the drag target must
  # already exclude it, so the aim point must miss it.
  titlebar_h="$(field titlebar h "$OUT_DIR/app.log")"
  titlebar_interactive="$(field titlebar interactive "$OUT_DIR/app.log")"
  if [[ -n "$titlebar_h" && "$titlebar_interactive" == "1" ]]; then
    (( center_y >= titlebar_h )) || {
      echo "the aim point lands on the titlebar chrome (${center_y} < ${titlebar_h})" >&2
      exit 1
    }
  fi

  clicks_before="$(grep -c 'interaction: video-single-click-scheduled' "$OUT_DIR/app.log" || true)"
  xdotool mousemove --window "$window_id" "$center_x" "$center_y" click 1
  landed=0
  for _ in $(seq 1 40); do
    clicks_now="$(grep -c 'interaction: video-single-click-scheduled' "$OUT_DIR/app.log" || true)"
    if (( clicks_now > clicks_before )); then
      landed=1
      break
    fi
    sleep 0.1
  done
  (( landed == 1 )) || {
    echo "a press aimed at the reported drag target never reached the video surface" >&2
    tail -n 40 "$OUT_DIR/app.log" >&2
    exit 1
  }

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  # Audio lyrics put a full-window targetable surface over the video; it must be reported
  # or a drag target would be published under a lyric row.
  timeout 40s env OKP_DEBUG_INTERACTIONS=1 OKP_OPEN_LYRICS_ON_STARTUP=synced \
    "$BINARY" "$FIXTURE" >"$OUT_DIR/lyrics-app.log" 2>&1 &
  app_pid=$!
  lyrics_window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/lyrics-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$lyrics_window_id" >/dev/null 2>&1 || true
  sleep 3
  xdotool mousemove --window "$lyrics_window_id" 60 200
  sleep 1
  wait_for_plane "$OUT_DIR/lyrics-app.log" lyrics || {
    echo "no lyrics geometry record was published" >&2
    cat "$OUT_DIR/lyrics-app.log" >&2
    exit 1
  }
  lyrics_x="$(field lyrics local-x "$OUT_DIR/lyrics-app.log")"
  lyrics_y="$(field lyrics local-y "$OUT_DIR/lyrics-app.log")"
  lyrics_w="$(field lyrics w "$OUT_DIR/lyrics-app.log")"
  lyrics_h="$(field lyrics h "$OUT_DIR/lyrics-app.log")"
  lyrics_interactive="$(field lyrics interactive "$OUT_DIR/lyrics-app.log")"
  [[ -n "$lyrics_w" && "$lyrics_w" -gt 0 && "$lyrics_interactive" == "1" ]] || {
    echo "the lyrics surface was not reported as an interactive plane" >&2
    exit 1
  }
  lyrics_drag_w="$(field drag-target w "$OUT_DIR/lyrics-app.log")"
  if [[ -n "$lyrics_drag_w" && "$lyrics_drag_w" -gt 0 ]]; then
    lyrics_drag_x="$(field drag-target local-x "$OUT_DIR/lyrics-app.log")"
    lyrics_drag_y="$(field drag-target local-y "$OUT_DIR/lyrics-app.log")"
    lyrics_drag_h="$(field drag-target h "$OUT_DIR/lyrics-app.log")"
    if (( lyrics_drag_x < lyrics_x + lyrics_w && lyrics_x < lyrics_drag_x + lyrics_drag_w \
          && lyrics_drag_y < lyrics_y + lyrics_h && lyrics_y < lyrics_drag_y + lyrics_drag_h )); then
      echo "a drag target was published under the lyrics surface" >&2
      exit 1
    fi
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  # With no media the welcome canvas owns the whole window and takes input, so it must be
  # reported: a drag target published underneath it would be unpressable.
  timeout 40s env OKP_DEBUG_INTERACTIONS=1 "$BINARY" >"$OUT_DIR/idle-app.log" 2>&1 &
  app_pid=$!
  idle_window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/idle-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$idle_window_id" >/dev/null 2>&1 || true
  sleep 3
  xdotool mousemove --window "$idle_window_id" 60 200
  sleep 1
  wait_for_plane "$OUT_DIR/idle-app.log" welcome || {
    echo "no idle geometry record was published" >&2
    cat "$OUT_DIR/idle-app.log" >&2
    exit 1
  }
  welcome_x="$(field welcome local-x "$OUT_DIR/idle-app.log")"
  welcome_y="$(field welcome local-y "$OUT_DIR/idle-app.log")"
  welcome_w="$(field welcome w "$OUT_DIR/idle-app.log")"
  welcome_h="$(field welcome h "$OUT_DIR/idle-app.log")"
  welcome_interactive="$(field welcome interactive "$OUT_DIR/idle-app.log")"
  [[ -n "$welcome_w" && "$welcome_w" -gt 0 && "$welcome_interactive" == "1" ]] || {
    echo "the welcome surface was not reported as an interactive plane" >&2
    exit 1
  }
  # The titlebar sits above the welcome canvas, and both take input here. The plane that
  # owns a point must be the one GTK would deliver the press to, which only holds if the
  # planes are reported in the order the overlay stacks them.
  titlebar_center_x=$((actual_width / 2))
  titlebar_center_y=$(( "$(field titlebar h "$OUT_DIR/idle-app.log")" / 2 ))
  xdotool mousemove --window "$idle_window_id" "$titlebar_center_x" "$titlebar_center_y"
  sleep 1
  titlebar_owner="$(awk '
    index($0, "interaction: geometry part=pointer ") == 1 {
      for (i = 1; i <= NF; i++) {
        split($i, pair, "=")
        if (pair[1] == "over") { value = pair[2] }
      }
    }
    END { print value }
  ' "$OUT_DIR/idle-app.log")"
  [[ "$titlebar_owner" == "titlebar" ]] || {
    echo "a point on the titlebar was reported as owned by ${titlebar_owner:-nothing}" >&2
    exit 1
  }

  idle_drag_w="$(field drag-target w "$OUT_DIR/idle-app.log")"
  if [[ -n "$idle_drag_w" && "$idle_drag_w" -gt 0 ]]; then
    idle_drag_x="$(field drag-target local-x "$OUT_DIR/idle-app.log")"
    idle_drag_y="$(field drag-target local-y "$OUT_DIR/idle-app.log")"
    idle_drag_h="$(field drag-target h "$OUT_DIR/idle-app.log")"
    if (( idle_drag_x < welcome_x + welcome_w && welcome_x < idle_drag_x + idle_drag_w \
          && idle_drag_y < welcome_y + welcome_h && welcome_y < idle_drag_y + idle_drag_h )); then
      echo "a drag target was published under the welcome surface" >&2
      exit 1
    fi
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  # Compact mode floats its own controls over the video, the play button in the middle of
  # it. Those planes must be reported too, or the drag target would cover a control.
  timeout 50s env OKP_DEBUG_INTERACTIONS=1 OKP_START_COMPACT=1 "$BINARY" "$FIXTURE" \
    >"$OUT_DIR/compact-app.log" 2>&1 &
  app_pid=$!
  compact_window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/compact-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$compact_window_id" >/dev/null 2>&1 || true
  sleep 3
  xdotool mousemove --window "$compact_window_id" 40 40
  sleep 1
  wait_for_plane "$OUT_DIR/compact-app.log" compact-play || {
    echo "no compact geometry record was published" >&2
    cat "$OUT_DIR/compact-app.log" >&2
    exit 1
  }
  play_x="$(field compact-play local-x "$OUT_DIR/compact-app.log")"
  play_y="$(field compact-play local-y "$OUT_DIR/compact-app.log")"
  play_w="$(field compact-play w "$OUT_DIR/compact-app.log")"
  play_h="$(field compact-play h "$OUT_DIR/compact-app.log")"
  [[ -n "$play_w" && "$play_w" -gt 0 && -n "$play_h" && "$play_h" -gt 0 ]] || {
    echo "the compact play control was not reported as a plane" >&2
    exit 1
  }
  compact_drag_x="$(field drag-target local-x "$OUT_DIR/compact-app.log")"
  compact_drag_y="$(field drag-target local-y "$OUT_DIR/compact-app.log")"
  compact_drag_w="$(field drag-target w "$OUT_DIR/compact-app.log")"
  compact_drag_h="$(field drag-target h "$OUT_DIR/compact-app.log")"
  if (( compact_drag_x < play_x + play_w && play_x < compact_drag_x + compact_drag_w \
        && compact_drag_y < play_y + play_h && play_y < compact_drag_y + compact_drag_h )); then
    echo "the compact drag target overlaps the compact play control" >&2
    exit 1
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  timeout 40s "$BINARY" "$FIXTURE" >"$OUT_DIR/quiet-app.log" 2>&1 &
  app_pid=$!
  quiet_window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/quiet-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$quiet_window_id" >/dev/null 2>&1 || true
  sleep 3
  xdotool mousemove --window "$quiet_window_id" "$center_x" "$center_y" click 1
  sleep 1
  quiet_lines="$(grep -c 'interaction: geometry' "$OUT_DIR/quiet-app.log" || true)"
  (( quiet_lines == 0 )) || {
    echo "the geometry diagnostic published $quiet_lines lines without OKP_DEBUG_INTERACTIONS" >&2
    exit 1
  }
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$OUT_DIR/app.log" "$OUT_DIR/idle-app.log" "$OUT_DIR/lyrics-app.log" \
      "$OUT_DIR/compact-app.log" "$OUT_DIR/quiet-app.log"; then
    echo "window-geometry smoke observed a fatal process diagnostic" >&2
    exit 1
  fi

  printf '%s\n' \
    "reported_client_size=${reported_width}x${reported_height}" \
    "server_client_size=${actual_width}x${actual_height}" \
    "video_plane=${video_w}x${video_h}+${video_x},${video_y}" \
    "drag_target=${drag_w}x${drag_h}+${drag_x},${drag_y}" \
    "aim_point=${center_x},${center_y}" \
    'aimed_press_reached_video=pass' \
    "compact_play_plane=${play_w}x${play_h}+${play_x},${play_y}" \
    "compact_drag_target=${compact_drag_w}x${compact_drag_h}+${compact_drag_x},${compact_drag_y}" \
    'compact_drag_target_clears_controls=pass' \
    "welcome_plane=${welcome_w}x${welcome_h}+${welcome_x},${welcome_y}" \
    'welcome_surface_owns_the_idle_window=pass' \
    "titlebar_point_owner=${titlebar_owner}" \
    "lyrics_plane=${lyrics_w}x${lyrics_h}+${lyrics_x},${lyrics_y}" \
    'lyrics_surface_owns_the_video_area=pass' \
    'silent_without_diagnostic=pass' \
    'fatal_diagnostics=absent' >"$OUT_DIR/results.txt"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-window-geometry-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xfwm4 xdotool xwininfo xprop awk timeout; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }
[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  "$ISOLATED_XVFB" \
  "$OUT_DIR/xvfb-evidence.txt" \
  "$OUT_DIR/xvfb.log" \
  '-screen 0 1440x900x24 -nolisten tcp' \
  "$ISOLATED_DBUS" \
  "$OUT_DIR/dbus-evidence.txt" \
  "$0" --inner "$BINARY" "$FIXTURE" "$OUT_DIR"

echo "Window-geometry smoke passed. Results: $OUT_DIR/results.txt"
