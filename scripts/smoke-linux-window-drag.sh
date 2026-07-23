#!/usr/bin/env bash
# X11/Xvfb regression for thresholded player-surface window movement.
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
  export OKP_DEBUG_INTERACTIONS=1
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

  window_geometry() {
    xwininfo -id "$1" | awk '
      /Absolute upper-left X:/ { x = $4 }
      /Absolute upper-left Y:/ { y = $4 }
      /Width:/ { w = $2 }
      /Height:/ { h = $2 }
      END { print x, y, w, h }
    '
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

  assert_alive() {
    kill -0 "$app_pid" 2>/dev/null || {
      echo "player exited during $1" >&2
      exit 1
    }
  }

  drag_completion_count() {
    awk '/interaction: player-window-move-(end|cancel)/ { count++ } END { print count + 0 }' "$1"
  }

  drag_handoff_count() {
    awk '/interaction: player-window-move$/ { count++ } END { print count + 0 }' "$1"
  }

  drag_begin_count() {
    awk '/interaction: player-window-move-begin / { count++ } END { print count + 0 }' "$1"
  }

  wait_for_new_drag_handoff() {
    local log="$1" previous_handoffs="$2"
    for _ in $(seq 1 40); do
      if [[ "$(drag_handoff_count "$log")" -gt "$previous_handoffs" ]]; then
        return 0
      fi
      sleep 0.05
    done
    return 1
  }

  drag_and_assert_handoff() {
    local id="$1" x="$2" y="$3" label="$4" log="$5"
    local previous_handoffs
    previous_handoffs="$(drag_handoff_count "$log")"
    xdotool mousemove --window "$id" "$x" "$y" mousedown 1
    sleep 0.2
    xdotool mousemove_relative --sync 20 15
    sleep 0.2
    xdotool mousemove_relative --sync 30 20
    sleep 0.2
    xdotool mousemove_relative --sync 40 30
    sleep 0.2
    xdotool mouseup 1
    sleep 0.8
    assert_alive "$label"
    wait_for_new_drag_handoff "$log" "$previous_handoffs" || {
      echo "$label did not produce a native handoff" >&2
      exit 1
    }
  }

  timeout 70s "$BINARY" "$FIXTURE" >"$OUT_DIR/playback-app.log" 2>&1 &
  app_pid=$!
  window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/playback-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  sleep 3
  read -r _ _ window_width window_height < <(window_geometry "$window_id")
  center_x=$((window_width / 2))
  center_y=$((window_height / 2))

  drag_and_assert_handoff \
    "$window_id" "$center_x" "$center_y" video-surface-drag "$OUT_DIR/playback-app.log"
  xdotool windowmove "$window_id" 80 80
  sleep 0.5

  # Cross the threshold, then cancel the compositor-owned move with Escape.
  cancel_previous_handoffs="$(drag_handoff_count "$OUT_DIR/playback-app.log")"
  xdotool mousemove --window "$window_id" "$center_x" "$center_y" mousedown 1
  sleep 0.2
  xdotool mousemove_relative --sync 70 45
  sleep 0.5
  xdotool key --clearmodifiers Escape
  xdotool mouseup 1
  sleep 0.8
  assert_alive compositor-cancel
  wait_for_new_drag_handoff "$OUT_DIR/playback-app.log" "$cancel_previous_handoffs" || {
    echo "Escape-cancelled drag did not produce a native handoff" >&2
    exit 1
  }

  # A fresh drag must still work after cancellation.
  drag_and_assert_handoff \
    "$window_id" "$center_x" "$center_y" post-cancel-drag "$OUT_DIR/playback-app.log"
  xdotool windowmove "$window_id" 80 80
  sleep 0.5

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  playback_moves="$(awk '/interaction: player-window-move$/ { count++ } END { print count + 0 }' "$OUT_DIR/playback-app.log")"
  [[ "$playback_moves" -ge 3 ]] || {
    echo "expected all three playback-surface move handoffs, observed $playback_moves" >&2
    exit 1
  }
  playback_begins="$(drag_begin_count "$OUT_DIR/playback-app.log")"
  [[ "$playback_begins" -ge 3 ]] || {
    echo "expected three fresh GTK drag begin boundaries, observed $playback_begins" >&2
    exit 1
  }
  playback_completions="$(drag_completion_count "$OUT_DIR/playback-app.log")"
  [[ "$playback_completions" -ge 1 ]] || {
    echo "expected at least one GTK end/cancel edge, observed $playback_completions" >&2
    exit 1
  }
  timeout 40s "$BINARY" >"$OUT_DIR/idle-app.log" 2>&1 &
  app_pid=$!
  idle_window_id="$(wait_for_window)" || {
    cat "$OUT_DIR/idle-app.log" >&2
    exit 1
  }
  xdotool windowactivate "$idle_window_id" >/dev/null 2>&1 || true
  sleep 2
  # The idle process has its own log; retry once because Xvfb pointer delivery is
  # synthetic, then require a native handoff after exit.
  idle_previous_handoffs="$(drag_handoff_count "$OUT_DIR/idle-app.log")"
  xdotool mousemove --window "$idle_window_id" 100 300 mousedown 1
  sleep 0.2
  xdotool mousemove_relative --sync 90 65
  sleep 0.2
  xdotool mouseup 1
  sleep 0.8
  assert_alive idle-canvas-drag
  if ! wait_for_new_drag_handoff "$OUT_DIR/idle-app.log" "$idle_previous_handoffs"; then
    xdotool mousemove --window "$idle_window_id" 100 300 mousedown 1
    sleep 0.2
    xdotool mousemove_relative --sync 90 65
    sleep 0.2
    xdotool mouseup 1
    sleep 0.8
    assert_alive idle-canvas-retry
    wait_for_new_drag_handoff "$OUT_DIR/idle-app.log" "$idle_previous_handoffs" || {
      echo "idle-canvas drag did not produce a native handoff" >&2
      exit 1
    }
  fi
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  idle_moves="$(awk '/interaction: player-window-move$/ { count++ } END { print count + 0 }' "$OUT_DIR/idle-app.log")"
  [[ "$idle_moves" -ge 1 ]] || {
    echo "expected an idle-canvas move handoff, observed $idle_moves" >&2
    exit 1
  }
  idle_completions="$(drag_completion_count "$OUT_DIR/idle-app.log")"
  [[ "$idle_completions" -ge 1 ]] || {
    echo "expected GTK to finish or cancel the idle-canvas drag, observed $idle_completions" >&2
    exit 1
  }
  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$OUT_DIR/playback-app.log" "$OUT_DIR/idle-app.log"; then
    echo "window-drag smoke observed a fatal process diagnostic" >&2
    exit 1
  fi

  printf '%s\n' \
    'video_surface_handoff_survival=pass' \
    'video_surface_drag_handoff=observed' \
    'compositor_cancel_survival=pass' \
    'compositor_cancel_drag_handoff=observed' \
    'post_cancel_drag=pass' \
    'post_cancel_drag_handoff=observed' \
    'fresh_drag_begin_boundaries=observed' \
    'gtk_completion_edge=observed' \
    'idle_canvas_handoff_survival=pass' \
    'idle_canvas_drag_handoff=observed' \
    'fatal_diagnostics=absent' >"$OUT_DIR/results.txt"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-window-drag-smoke}"
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

echo "Window-drag smoke passed. Results: $OUT_DIR/results.txt"
