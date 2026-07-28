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

  wait_for_geometry() {
    for _ in $(seq 1 60); do
      if [[ -n "$(field drag-target center-x "$1")" || -n "$(field drag-target local-x "$1")" ]]; then
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
  wait_for_geometry "$OUT_DIR/app.log" || {
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
      "$OUT_DIR/app.log" "$OUT_DIR/quiet-app.log"; then
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
