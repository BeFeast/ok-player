#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-main-window-smoke}"
IDLE_OSC_ASSERT="$ROOT/scripts/assert-linux-idle-osc-absent.sh"
X11_WINDOW_WAITER="$ROOT/scripts/wait-for-x11-window.sh"
X11_APP_CLEAR_WAITER="$ROOT/scripts/wait-for-x11-app-clear.sh"
DBUS_NAME_CLEAR_WAITER="$ROOT/scripts/wait-for-dbus-names-clear.sh"
ISOLATED_DBUS_SESSION="$ROOT/scripts/run-linux-isolated-dbus-session.sh"
ISOLATED_XVFB_SESSION="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
X11_CLOSE_REQUEST_SOURCE="$ROOT/scripts/send-x11-close-request.c"

for tool in Xvfb xauth flock dbus-run-session gdbus xfwm4 xdotool xwininfo xprop import magick ffmpeg ffprobe rg stat; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [[ "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" == "1" && "${OKP_MAIN_WINDOW_IDLE_ONLY:-0}" == "1" ]]; then
  echo "OKP_MAIN_WINDOW_FIT_ONLY and OKP_MAIN_WINDOW_IDLE_ONLY are mutually exclusive" >&2
  exit 2
fi
if [[ "${OKP_MAIN_WINDOW_SHUTDOWN_ONLY:-0}" == "1" \
  && "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" != "1" ]]; then
  echo "OKP_MAIN_WINDOW_SHUTDOWN_ONLY requires OKP_MAIN_WINDOW_FIT_ONLY=1" >&2
  exit 2
fi

if [[ "${OKP_MAIN_WINDOW_FIT_ONLY:-0}" != "1" ]]; then
  mkdir -p "$OUT_DIR/cache" "$OUT_DIR/runtime"
  chmod 700 "$OUT_DIR/runtime"
  if ! env XDG_CACHE_HOME="$OUT_DIR/cache" XDG_RUNTIME_DIR="$OUT_DIR/runtime" \
    __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    LIBGL_ALWAYS_SOFTWARE=1 \
    "$ISOLATED_XVFB_SESSION" "$OUT_DIR/xvfb-evidence.txt" "$OUT_DIR/xvfb.log" \
    '-screen 0 1280x900x24 -nolisten tcp -noreset' \
    "$ISOLATED_DBUS_SESSION" "$OUT_DIR/session-evidence.txt" \
    bash -s -- "$BINARY" "$OUT_DIR" "$IDLE_OSC_ASSERT" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
IDLE_OSC_ASSERT="$3"

export GDK_BACKEND=x11
# Xvfb cannot prove portal behavior. Keep its startup deterministic instead of
# allowing a document/settings portal activation from one process to overlap
# the next process's initial toplevel map.
export GDK_DEBUG=no-portals
export GIO_USE_PORTALS=0
export GTK_USE_PORTAL=0
export GTK_A11Y=none
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:?}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:?}"

mkdir -p "$XDG_CONFIG_HOME/ok-player" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" \
  "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

terminate_pid() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  for _ in $(seq 1 20); do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
      return 0
    fi
    sleep 0.05
  done
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

cleanup() {
  terminate_pid "$app_pid"
  terminate_pid "$wm_pid"
}
trap cleanup EXIT

local_contrast() {
  local image="$1" crop="$2" blur="${3:-4}"
  magick "$image" \
    -crop "$crop" +repage \
    -colorspace gray \
    \( +clone -blur "0x${blur}" \) \
    -compose difference -composite \
    -format '%[fx:mean]' info:
}

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 40s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in $(seq 1 100); do
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids" 2>/dev/null || true
  while IFS= read -r candidate; do
    [[ -n "$candidate" ]] || continue
    candidate_info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
    candidate_width="$(awk '/Width:/ { print $2; exit }' <<<"$candidate_info")"
    candidate_height="$(awk '/Height:/ { print $2; exit }' <<<"$candidate_info")"
    candidate_state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$candidate_info")"
    if [[ "$candidate_width" == "1120" && "$candidate_height" == "680" && "$candidate_state" == "IsViewable" ]]; then
      window_id="$candidate"
      break 2
    fi
  done <"$OUT_DIR/window.ids"
  sleep 0.1
done
if [[ -z "$window_id" ]]; then
  echo "Timed out waiting for the visible 1120x680 main window" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi
xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"

# Mapping precedes the first media-state projection on a cold portal startup.
# Wait for the actual Welcome identity rather than accepting a transient black
# frame as evidence that the idle surface rendered.
welcome_ready=0
for _ in $(seq 1 40); do
  if import -window "$window_id" "$OUT_DIR/window.png" 2>/dev/null; then
    ready_identity="$(local_contrast "$OUT_DIR/window.png" 300x130+410+230)"
    if awk -v residual="$ready_identity" 'BEGIN { exit !(residual > 0.015) }'; then
      welcome_ready=1
      break
    fi
  fi
  sleep 0.25
done
if [[ "$welcome_ready" != "1" ]]; then
  echo "Timed out waiting for the Welcome canvas to paint" >&2
  exit 1
fi
import -window root "$OUT_DIR/root.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/window.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"

if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected main window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# The first-run surface owns its own Open actions; the standard playback OSC
# must not be present before media is loaded. The detector measures local pill
# and glyph structure rather than absolute brightness, so both approved idle
# themes remain valid substrates.
"$IDLE_OSC_ASSERT" "$OUT_DIR/window.png" "Initial Welcome"

# Regression guard for the old native GTK caption/headerbar. The player owns
# its caption controls, so the top-center strip must remain structurally empty,
# not carry a centered system title. Local residuals are theme-independent.
top_center_residual="$(local_contrast "$OUT_DIR/window.png" 220x36+450+0)"
if ! awk -v residual="$top_center_residual" 'BEGIN { exit !(residual < 0.008) }'; then
  echo "Unexpected centered title geometry in main window: residual=${top_center_residual}" >&2
  exit 1
fi

# The custom caption keeps its brand mark and title at top-left in both themes.
top_left_residual="$(local_contrast "$OUT_DIR/window.png" 180x36+0+0)"
if ! awk -v residual="$top_left_residual" 'BEGIN { exit !(residual > 0.015) }'; then
  echo "Custom top-left caption identity is missing: residual=${top_left_residual}" >&2
  exit 1
fi

# The centered mark, title, and supporting copy must produce real structure,
# independent of whether their substrate is light or dark.
identity_residual="$(local_contrast "$OUT_DIR/window.png" 300x130+410+230)"
if ! awk -v residual="$identity_residual" 'BEGIN { exit !(residual > 0.015) }'; then
  echo "Empty Welcome identity is missing: residual=${identity_residual}" >&2
  exit 1
fi

tagline_residual="$(local_contrast "$OUT_DIR/window.png" 260x28+430+332)"
if ! awk -v residual="$tagline_residual" 'BEGIN { exit !(residual > 0.01) }'; then
  echo "Welcome supporting copy is missing: residual=${tagline_residual}" >&2
  exit 1
fi

# The in-canvas Open action keeps the teal label inside the dashed drop target.
primary_red="$(magick "$OUT_DIR/window.png" -crop 140x24+490+395 -format '%[fx:mean.r]' info:)"
primary_green="$(magick "$OUT_DIR/window.png" -crop 140x24+490+395 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$primary_red" -v g="$primary_green" 'BEGIN { exit !(g - r > 0.02) }'; then
  echo "Primary action accent missing from welcome surface: red=${primary_red} green=${primary_green}" >&2
  exit 1
fi

# The drop guidance and target border remain visible without assuming a text
# luminance direction.
drop_target_residual="$(local_contrast "$OUT_DIR/window.png" 320x100+400+365)"
drop_hint_residual="$(local_contrast "$OUT_DIR/window.png" 220x24+450+415)"
if ! awk -v target="$drop_target_residual" -v hint="$drop_hint_residual" \
  'BEGIN { exit !(target > 0.008 && hint > 0.01) }'; then
  echo "Welcome drop target or guidance is missing: target=${drop_target_residual} hint=${drop_hint_residual}" >&2
  exit 1
fi
SMOKE
  then
    echo "Main window smoke failed. Session log: $OUT_DIR/session.log" >&2
    cat "$OUT_DIR/session.log" >&2
    exit 1
  fi
fi

if [[ "${OKP_MAIN_WINDOW_IDLE_ONLY:-0}" == "1" ]]; then
  echo "Main window idle-surface smoke passed. Screenshot: $OUT_DIR/window.png"
  exit 0
fi

CC_BIN="${CC:-/usr/bin/cc}"
if ! command -v "$CC_BIN" >/dev/null 2>&1; then
  echo "Missing required C compiler: $CC_BIN" >&2
  exit 127
fi
if ! command -v pkg-config >/dev/null 2>&1 || ! pkg-config --exists x11; then
  echo "Missing required X11 development package" >&2
  exit 127
fi
read -r -a x11_compile_flags <<<"$(pkg-config --cflags --libs x11)"
X11_CLOSE_REQUEST="$OUT_DIR/send-x11-close-request"
"$CC_BIN" -Wall -Wextra -Werror "$X11_CLOSE_REQUEST_SOURCE" \
  -o "$X11_CLOSE_REQUEST" \
  "${x11_compile_flags[@]}"

"$ROOT/scripts/generate-linux-acceptance-media.sh" "$OUT_DIR/fixtures" \
  >"$OUT_DIR/fixtures.log" 2>&1

mkdir -p "$OUT_DIR/fit-cache" "$OUT_DIR/fit-runtime"
chmod 700 "$OUT_DIR/fit-runtime"
if ! env XDG_CACHE_HOME="$OUT_DIR/fit-cache" XDG_RUNTIME_DIR="$OUT_DIR/fit-runtime" \
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  "$ISOLATED_XVFB_SESSION" "$OUT_DIR/fit-xvfb-evidence.txt" \
  "$OUT_DIR/window-fit-xvfb.log" \
  '-screen 0 1280x900x24 -screen 1 1024x768x24 -nolisten tcp -noreset' \
  "$ISOLATED_DBUS_SESSION" "$OUT_DIR/fit-session-evidence.txt" \
  bash -s -- "$BINARY" "$OUT_DIR" "$X11_WINDOW_WAITER" \
  "$X11_APP_CLEAR_WAITER" "$DBUS_NAME_CLEAR_WAITER" "$X11_CLOSE_REQUEST" \
  >"$OUT_DIR/window-fit-session.log" 2>&1 <<'FIT_SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
X11_WINDOW_WAITER="$3"
X11_APP_CLEAR_WAITER="$4"
DBUS_NAME_CLEAR_WAITER="$5"
X11_CLOSE_REQUEST="$6"
FIXTURES="$OUT_DIR/fixtures"

export GDK_BACKEND=x11
# The fit lifecycle restarts the application inside one D-Bus session. Portals
# are outside this headless acceptance level and may outlive the process that
# activated them, so exclude them from this deterministic startup boundary.
export GDK_DEBUG=no-portals
export GIO_USE_PORTALS=0
export GTK_USE_PORTAL=0
export GTK_A11Y=none
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/fit-config"
export XDG_STATE_HOME="$OUT_DIR/fit-state"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:?}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:?}"

PRIMARY_DISPLAY="$DISPLAY"
SECONDARY_DISPLAY="${DISPLAY%%.*}.1"

mkdir -p "$XDG_CONFIG_HOME/ok-player" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" \
  "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
runtime_mode="$(stat -c '%a' "$XDG_RUNTIME_DIR")"
if [[ "$runtime_mode" != "700" ]]; then
  echo "XDG_RUNTIME_DIR must be private, got mode $runtime_mode" >&2
  exit 1
fi
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

wm_pid=""
app_pid=""
app_log=""
: >"$OUT_DIR/fit-lifecycle.log"
: >"$OUT_DIR/fit-evidence.txt"
printf 'xdg_cache_home_isolated=true\nxdg_runtime_dir_isolated=true\n' \
  >>"$OUT_DIR/fit-evidence.txt"
printf 'xdg_runtime_mode=%s\naccessibility_disabled=true\n' "$runtime_mode" \
  >>"$OUT_DIR/fit-evidence.txt"
DBUS_NAMES=(
  com.befeast.okplayer
  org.mpris.MediaPlayer2.okplayer
  org.a11y.Bus
  org.a11y.atspi.Registry
)

terminate_pid() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  for _ in $(seq 1 20); do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" 2>/dev/null || true
      return 0
    fi
    sleep 0.05
  done
  kill -KILL "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

cleanup() {
  terminate_pid "$app_pid"
  terminate_pid "$wm_pid"
}
trap cleanup EXIT

window_manager_ready() {
  local display="$1"
  DISPLAY="$display" xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null \
    | rg -q 'window id # 0x[1-9a-fA-F][0-9a-fA-F]*'
}

wait_for_window_manager() {
  local display="$1" label="$2"
  for attempt in $(seq 1 40); do
    if window_manager_ready "$display"; then
      printf 'window-manager display=%s label=%s attempt=%s ready=true\n' \
        "$display" "$label" "$attempt" >>"$OUT_DIR/fit-lifecycle.log"
      return 0
    fi
    sleep 0.05
  done
  echo "Timed out waiting for Xfwm on $display" >&2
  return 1
}

# Xfwm probes and manages every screen on its X server. Starting one process per
# screen lets both instances race for :N.0 and :N.1, so launch exactly one and
# wait until that process has published ownership on both roots.
DISPLAY="$PRIMARY_DISPLAY" xfwm4 --sm-client-disable \
  >"$OUT_DIR/window-fit-xfwm4-primary.log" 2>&1 &
wm_pid=$!
wait_for_window_manager "$PRIMARY_DISPLAY" primary
wait_for_window_manager "$SECONDARY_DISPLAY" secondary

start_app() {
  local log="$1"
  local startup_mode="$2"
  shift 2
  if [[ -n "$app_pid" ]]; then
    echo "Refusing to start a new player before the current PID is cleared: $app_pid" >&2
    return 1
  fi
  if ! "$X11_APP_CLEAR_WAITER" none "$OUT_DIR/pre-start-lifecycle.log"; then
    cat "$OUT_DIR/pre-start-lifecycle.log" >>"$OUT_DIR/fit-lifecycle.log"
    return 1
  fi
  cat "$OUT_DIR/pre-start-lifecycle.log" >>"$OUT_DIR/fit-lifecycle.log"
  if ! "$DBUS_NAME_CLEAR_WAITER" "$OUT_DIR/pre-start-dbus-lifecycle.log" \
    "${DBUS_NAMES[@]}"; then
    cat "$OUT_DIR/pre-start-dbus-lifecycle.log" >>"$OUT_DIR/fit-lifecycle.log"
    return 1
  fi
  cat "$OUT_DIR/pre-start-dbus-lifecycle.log" >>"$OUT_DIR/fit-lifecycle.log"

  local -a startup_env=()
  case "$startup_mode" in
    windowed) ;;
    maximized) startup_env+=(OKP_START_MAXIMIZED=1) ;;
    fullscreen) startup_env+=(OKP_START_FULLSCREEN=1) ;;
    *) echo "Unknown startup mode: $startup_mode" >&2; return 2 ;;
  esac
  env "${startup_env[@]}" \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  OKP_DEBUG_WINDOW_FIT=1 \
  OKP_DEBUG_INTERACTIONS=1 \
  OKP_COMMAND_SEARCH_QUERY='Fit window to media' \
  "$BINARY" "$@" >"$OUT_DIR/$log" 2>&1 &
  app_pid=$!
  app_log="$OUT_DIR/$log"
  printf 'start pid=%s mode=%s app_log=%s\n' "$app_pid" "$startup_mode" "$log" \
    >>"$OUT_DIR/fit-lifecycle.log"
}

wait_for_window() {
  local ids="$1"
  local diagnostics="${ids%.ids}.readiness.log"
  local selected
  selected="$("$X11_WINDOW_WAITER" "$app_pid" "$ids" "$diagnostics" "$app_log")" || return $?
  local candidate_info candidate_width candidate_height candidate_state case_name
  candidate_info="$(xwininfo -id "$selected")"
  candidate_width="$(awk '/Width:/ {print $2; exit}' <<<"$candidate_info")"
  candidate_height="$(awk '/Height:/ {print $2; exit}' <<<"$candidate_info")"
  candidate_state="$(awk -F': ' '/Map State:/ {print $2; exit}' <<<"$candidate_info")"
  case_name="$(basename "${ids%.ids}")"
  printf 'window case=%s pid=%s xid=%s state=%s width=%s height=%s\n' \
    "$case_name" "$app_pid" "$selected" "$candidate_state" "$candidate_width" \
    "$candidate_height" | tee -a "$OUT_DIR/fit-lifecycle.log" \
    >>"$OUT_DIR/fit-evidence.txt"
  printf '%s\n' "$selected"
}

stop_app() {
  local stopped_pid="$app_pid"
  terminate_pid "$stopped_pid"
  local diagnostics="$OUT_DIR/stop-${stopped_pid}-lifecycle.log"
  if ! "$X11_APP_CLEAR_WAITER" "$stopped_pid" "$diagnostics"; then
    cat "$diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
    echo "Application log: $app_log" >&2
    cat "$app_log" >&2 || true
    return 1
  fi
  cat "$diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
  local dbus_diagnostics="$OUT_DIR/stop-${stopped_pid}-dbus-lifecycle.log"
  if ! "$DBUS_NAME_CLEAR_WAITER" "$dbus_diagnostics" "${DBUS_NAMES[@]}"; then
    cat "$dbus_diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
    echo "Application log: $app_log" >&2
    cat "$app_log" >&2 || true
    return 1
  fi
  cat "$dbus_diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
  printf 'stop pid=%s clear=true\n' "$stopped_pid" >>"$OUT_DIR/fit-lifecycle.log"
  app_pid=""
  app_log=""
}

finish_app_shutdown() {
  local route="$1"
  local closed_pid="$app_pid"
  local diagnostics="$OUT_DIR/${route}-${closed_pid}-lifecycle.log"
  if ! "$X11_APP_CLEAR_WAITER" "$closed_pid" "$diagnostics"; then
    cat "$diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
    echo "Application did not exit after shutdown route: $route" >&2
    echo "Application log: $app_log" >&2
    cat "$app_log" >&2 || true
    return 1
  fi
  cat "$diagnostics" >>"$OUT_DIR/fit-lifecycle.log"

  for expected_close_stage in \
    'window close lifecycle: close-request' \
    'window close lifecycle: quit requested' \
    'window close lifecycle: engine teardown complete'; do
    if ! rg -Fx -q "$expected_close_stage" "$app_log"; then
      echo "Application exited without clean GTK shutdown stage: $expected_close_stage" >&2
      cat "$app_log" >&2 || true
      return 1
    fi
  done

  local dbus_diagnostics="$OUT_DIR/${route}-${closed_pid}-dbus-lifecycle.log"
  if ! "$DBUS_NAME_CLEAR_WAITER" "$dbus_diagnostics" "${DBUS_NAMES[@]}"; then
    cat "$dbus_diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
    echo "Application D-Bus names remained after shutdown route: $route" >&2
    cat "$app_log" >&2 || true
    return 1
  fi
  cat "$dbus_diagnostics" >>"$OUT_DIR/fit-lifecycle.log"
  local exit_status=0
  wait "$closed_pid" 2>/dev/null || exit_status=$?
  if (( exit_status != 0 )); then
    echo "Application exited with status $exit_status after shutdown route: $route" >&2
    return 1
  fi
  printf '%s pid=%s clear=true clean_teardown=true\n' "$route" "$closed_pid" \
    >>"$OUT_DIR/fit-lifecycle.log"
  printf '%s=pass\n' "$route" >>"$OUT_DIR/fit-evidence.txt"
  app_pid=""
  app_log=""
}

x11_window_state() {
  local window_id="$1"
  if xwininfo -id "$window_id" >/dev/null 2>&1; then
    printf 'present\n'
  elif xwininfo -root >/dev/null 2>&1; then
    printf 'gone\n'
  else
    printf 'unqueryable\n'
  fi
}

close_app() {
  local window_id="$1"
  local close_attempt close_error window_state
  # `windowclose` destroys the X11 window directly, while keyboard and pointer
  # routes depend on focus or hit testing. Ask Xfwm to close the exact toplevel;
  # it delivers WM_DELETE_WINDOW and GTK executes the normal close-request
  # teardown path. Retry only while the same XID remains present.
  for close_attempt in 1 2; do
    printf 'close-dispatch attempt=%s target=%s route=ewmh-close-window\n' \
      "$close_attempt" "$window_id" \
      >>"$OUT_DIR/fit-lifecycle.log"
    close_error="$OUT_DIR/close-dispatch-${close_attempt}.log"
    if ! "$X11_CLOSE_REQUEST" "$window_id" 2>"$close_error"; then
      # _NET_CLOSE_WINDOW is asynchronous. A previous request can complete
      # between the loop's geometry probe and this helper resolving the XID.
      # Treat that race as success only when the exact target is already gone.
      window_state="$(x11_window_state "$window_id")"
      case "$window_state" in
        gone)
          printf 'close-dispatch attempt=%s target=%s result=already-gone\n' \
            "$close_attempt" "$window_id" >>"$OUT_DIR/fit-lifecycle.log"
          break
          ;;
        unqueryable)
          echo "Could not verify X11 window state after close request failure" >&2
          cat "$close_error" >&2 || true
          return 1
          ;;
      esac
      echo "Could not send the X11 close request for window $window_id" >&2
      cat "$close_error" >&2 || true
      return 1
    fi
    sleep 0.2
    window_state="$(x11_window_state "$window_id")"
    case "$window_state" in
      gone) break ;;
      unqueryable)
        echo "Could not verify X11 window state after close request" >&2
        return 1
        ;;
    esac
  done
  finish_app_shutdown "last_window_close"
}

quit_app() {
  gdbus call --session \
    --dest org.mpris.MediaPlayer2.okplayer \
    --object-path /org/mpris/MediaPlayer2 \
    --method org.mpris.MediaPlayer2.Quit \
    >"$OUT_DIR/mpris-quit-call.log"
  finish_app_shutdown "mpris_quit"
}

capture_geometry() {
  local window_id="$1" stem="$2"
  xwininfo -id "$window_id" >"$OUT_DIR/$stem.xwininfo"
  import -window "$window_id" "$OUT_DIR/$stem.png"
}

activate_fit_window_command() {
  local window_id="$1"
  local width height
  width="$(xwininfo -id "$window_id" | awk '/Width:/ {print $2; exit}')"
  height="$(xwininfo -id "$window_id" | awk '/Height:/ {print $2; exit}')"
  xdotool mousemove --window "$window_id" $((width / 2)) $((height / 2)) click 3
  sleep 1
  xdotool key --clearmodifiers Return
  sleep 3
}

geometry_value() {
  local file="$1" label="$2"
  awk -v label="$label" '$1 == label ":" { print $2; exit }' "$file"
}

absolute_window_geometry() {
  local file="$1"
  awk -F': +' '
    /Absolute upper-left X:/ { x=$2 }
    /Absolute upper-left Y:/ { y=$2 }
    /^  Width:/ { w=$2 }
    /^  Height:/ { h=$2 }
    END { printf "x=%s,y=%s,w=%s,h=%s", x, y, w, h }
  ' "$file"
}

assert_monitor_fit_log() {
  local file="$1" label="$2"
  if ! rg -q \
    'window fit request: .* monitor=[^[:space:]]+ monitor_geometry=[0-9]+x[0-9]+\+[-0-9]+,[-0-9]+ workarea=[0-9]+x[0-9]+\+[-0-9]+,[-0-9]+ window=[0-9]+x[0-9]+\+[-0-9]+,[-0-9]+' \
    "$file"; then
    echo "$label did not record monitor-bound x/y/w/h fit geometry" >&2
    cat "$file" >&2
    exit 1
  fi
}

fit_log_rect() {
  local file="$1" field="$2"
  rg -m1 '^window fit request:' "$file" \
    | sed -E \
      "s/.* ${field}=([0-9]+)x([0-9]+)\+(-?[0-9]+),(-?[0-9]+).*/\1 \2 \3 \4/"
}

assert_logged_fit_containment() {
  local file="$1" label="$2"
  local monitor_width monitor_height monitor_x monitor_y
  local work_width work_height work_x work_y
  local window_width window_height window_x window_y
  read -r monitor_width monitor_height monitor_x monitor_y \
    < <(fit_log_rect "$file" monitor_geometry)
  read -r work_width work_height work_x work_y \
    < <(fit_log_rect "$file" workarea)
  read -r window_width window_height window_x window_y \
    < <(fit_log_rect "$file" window)

  if (( work_x < monitor_x || work_y < monitor_y \
    || work_x + work_width > monitor_x + monitor_width \
    || work_y + work_height > monitor_y + monitor_height )); then
    echo "$label logged a workarea outside its monitor" >&2
    rg -m1 '^window fit request:' "$file" >&2 || true
    exit 1
  fi
  if (( window_x < work_x || window_y < work_y \
    || window_x + window_width > work_x + work_width \
    || window_y + window_height > work_y + work_height )); then
    echo "$label logged a fitted window outside its workarea" >&2
    rg -m1 '^window fit request:' "$file" >&2 || true
    exit 1
  fi
}

wait_for_log() {
  local file="$1" pattern="$2"
  for _ in $(seq 1 50); do
    if rg -q "$pattern" "$file"; then
      return 0
    fi
    if [[ -n "$app_pid" ]] && ! kill -0 "$app_pid" 2>/dev/null; then
      return 1
    fi
    sleep 0.1
  done
  return 1
}

assert_single_initial_configure() {
  local file="$1" label="$2"
  local count
  count="$(rg -c 'window fit configure: kind=initial' "$file" || true)"
  if [[ "$count" != "1" ]]; then
    echo "$label issued $count initial-fit configure requests instead of one" >&2
    cat "$file" >&2
    exit 1
  fi
  if rg -q 'window fit (mapped launch|confirmed|restored after compositor|settled)' "$file"; then
    echo "$label entered the legacy visible settle sequence" >&2
    cat "$file" >&2
    exit 1
  fi
  if rg -q 'window fit (staged|reveal fallback|fallback: presenting)' "$file"; then
    echo "$label entered a hidden or fallback-map startup path" >&2
    cat "$file" >&2
    exit 1
  fi

  local map_line delivery_line launch_line
  map_line="$(rg -n -m1 '^startup launch lifecycle: window mapped$' "$file" \
    | cut -d: -f1 || true)"
  delivery_line="$(rg -n -m1 '^startup launch lifecycle: delivering after map and player readiness$' "$file" \
    | cut -d: -f1 || true)"
  launch_line="$(rg -n -m1 '^Launch request:' "$file" | cut -d: -f1 || true)"
  if [[ -z "$map_line" || -z "$delivery_line" || -z "$launch_line" \
    || "$map_line" -ge "$delivery_line" || "$delivery_line" -ge "$launch_line" ]]; then
    echo "$label did not map before delivering its startup media payload" >&2
    cat "$file" >&2
    exit 1
  fi
}

export DISPLAY="$PRIMARY_DISPLAY"
start_app "fit-small-app.log" windowed "$FIXTURES/fit-small.mkv"
small_id="$(wait_for_window "$OUT_DIR/fit-small-window.ids")"
sleep 4
xdotool mousemove 1200 850
sleep 3
capture_geometry "$small_id" "fit-small-window"
small_width="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Width)"
small_height="$(geometry_value "$OUT_DIR/fit-small-window.xwininfo" Height)"
if [[ "$small_width" != "320" || "$small_height" != "180" ]]; then
  echo "Small video did not use native size: ${small_width}x${small_height}" >&2
  exit 1
fi
for expected in \
  "mpv render context initialized before source load" \
  "headless fit smoke: audio-device observation disabled" \
  "window fit request: video=320x180" \
  "window fit configure: kind=initial target=320x180"; do
  if ! wait_for_log "$OUT_DIR/fit-small-app.log" "$expected"; then
    echo "Initial-fit sequence did not log: $expected" >&2
    cat "$OUT_DIR/fit-small-app.log" >&2
    exit 1
  fi
done
assert_single_initial_configure "$OUT_DIR/fit-small-app.log" "Small video"
assert_monitor_fit_log "$OUT_DIR/fit-small-app.log" "Small video"
assert_logged_fit_containment "$OUT_DIR/fit-small-app.log" "Small video"

# Playback and the original render context remain live through initial sizing
# and ordinary seek input. The 24-second fixture accepts +10 then -10 without
# ending the source, which catches the prior active-source teardown failure.
xdotool key --clearmodifiers Right Left
sleep 1
if ! kill -0 "$app_pid" 2>/dev/null; then
  echo "Player exited after initial sizing and seek input" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi
if rg -qi "error -15|protocol error 71" "$OUT_DIR/fit-small-app.log"; then
  echo "Initial sizing reproduced a forbidden playback/protocol error" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi

xdotool windowsize "$small_id" 700 500
sleep 2
capture_geometry "$small_id" "fit-small-manual-resize"
manual_width="$(geometry_value "$OUT_DIR/fit-small-manual-resize.xwininfo" Width)"
manual_height="$(geometry_value "$OUT_DIR/fit-small-manual-resize.xwininfo" Height)"
if [[ "$manual_width" != "700" || "$manual_height" != "500" ]]; then
  echo "Window fought manual resize after load: ${manual_width}x${manual_height}" >&2
  exit 1
fi

# A second CLI/Open-With launch must route to this primary instance, restore/present the same
# window, and switch media without creating a second hidden player process.
xdotool windowminimize "$small_id"
sleep 1
if xdotool search --onlyvisible --pid "$app_pid" --name "OK Player" >/dev/null 2>&1; then
  echo "Could not minimize the primary window before the secondary-launch check" >&2
  exit 1
fi
if ! timeout 10s "$BINARY" "$FIXTURES/fit-vertical.mkv" \
  >"$OUT_DIR/fit-secondary-launch.log" 2>&1; then
  echo "Secondary launch did not hand off to the primary instance" >&2
  cat "$OUT_DIR/fit-secondary-launch.log" >&2 || true
  exit 1
fi
secondary_presented=0
for _ in $(seq 1 60); do
  secondary_state="$(xwininfo -id "$small_id" 2>/dev/null \
    | awk -F': ' '/Map State:/ {print $2; exit}')"
  secondary_metadata="$(gdbus call --session \
    --dest org.mpris.MediaPlayer2.okplayer \
    --object-path /org/mpris/MediaPlayer2 \
    --method org.freedesktop.DBus.Properties.Get \
    org.mpris.MediaPlayer2.Player Metadata 2>/dev/null || true)"
  if [[ "$secondary_state" == "IsViewable" \
    && "$secondary_metadata" == *"fit-vertical.mkv"* ]]; then
    secondary_presented=1
    break
  fi
  sleep 0.1
done
if [[ "$secondary_presented" != "1" ]]; then
  echo "Secondary launch did not present the existing window with the new media" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi
sleep 1
xwininfo -id "$small_id" >"$OUT_DIR/fit-secondary-present-window.xwininfo"
secondary_x="$(awk -F': +' '/Absolute upper-left X:/ {print $2; exit}' \
  "$OUT_DIR/fit-secondary-present-window.xwininfo")"
secondary_y="$(awk -F': +' '/Absolute upper-left Y:/ {print $2; exit}' \
  "$OUT_DIR/fit-secondary-present-window.xwininfo")"
secondary_width="$(geometry_value "$OUT_DIR/fit-secondary-present-window.xwininfo" Width)"
secondary_height="$(geometry_value "$OUT_DIR/fit-secondary-present-window.xwininfo" Height)"
if (( secondary_x < 0 || secondary_y < 0 \
  || secondary_x + secondary_width > 1280 \
  || secondary_y + secondary_height > 900 )); then
  echo "Secondary present escaped the active monitor: ${secondary_width}x${secondary_height}+${secondary_x},${secondary_y}" >&2
  exit 1
fi
secondary_monitor_fit_count="$(rg -c \
  '^window fit request: .* monitor=[^[:space:]]+ .* workarea=1280x900' \
  "$OUT_DIR/fit-small-app.log" || true)"
if (( secondary_monitor_fit_count < 2 )); then
  echo "Secondary present did not fit against the active monitor workarea" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi
primary_window_count=0
while IFS= read -r candidate; do
  [[ -n "$candidate" ]] || continue
  candidate_state="$(xwininfo -id "$candidate" 2>/dev/null \
    | awk -F': ' '/Map State:/ {print $2; exit}')"
  candidate_type="$(xprop -id "$candidate" _NET_WM_WINDOW_TYPE 2>/dev/null || true)"
  if [[ "$candidate_state" == "IsViewable" \
    && "$candidate_type" == *"_NET_WM_WINDOW_TYPE_NORMAL"* ]]; then
    primary_window_count=$((primary_window_count + 1))
  fi
done < <(xdotool search --pid "$app_pid" --name "OK Player" 2>/dev/null \
  | sort -u || true)
if [[ "$primary_window_count" != "1" ]]; then
  echo "Secondary launch left $primary_window_count primary player windows instead of one" >&2
  exit 1
fi
launch_count="$(rg -c '^Launch request:' "$OUT_DIR/fit-small-app.log" || true)"
if [[ "$launch_count" != "2" ]]; then
  echo "Primary instance received $launch_count launch payloads instead of two" >&2
  cat "$OUT_DIR/fit-small-app.log" >&2
  exit 1
fi
printf 'secondary_launch=pass\nsecondary_presented=pass\nsingle_instance=pass\n' \
  >>"$OUT_DIR/fit-evidence.txt"
printf 'secondary_window=%s\nsecondary_monitor_fit_count=%s\nsecondary_monitor_fit=pass\n' \
  "$(absolute_window_geometry "$OUT_DIR/fit-secondary-present-window.xwininfo")" \
  "$secondary_monitor_fit_count" \
  >>"$OUT_DIR/fit-evidence.txt"
close_app "$small_id"

start_app "fit-mpris-quit-app.log" windowed "$FIXTURES/fit-small.mkv"
wait_for_window "$OUT_DIR/fit-mpris-quit-window.ids" >/dev/null
sleep 1
quit_app

if [[ "${OKP_MAIN_WINDOW_SHUTDOWN_ONLY:-0}" == "1" ]]; then
  printf 'session_process_teardown=clean\nsession_bus_teardown=clean\nstatus=pass\n' \
    >>"$OUT_DIR/fit-evidence.txt"
  echo "Main-window shutdown smoke passed. Evidence: $OUT_DIR/fit-evidence.txt"
  exit 0
fi

start_app "fit-1080p-app.log" windowed "$FIXTURES/fit-1080p.mkv"
fit_1080p_id="$(wait_for_window "$OUT_DIR/fit-1080p-window.ids")"
sleep 4
capture_geometry "$fit_1080p_id" "fit-1080p-window"
fit_1080p_width="$(geometry_value "$OUT_DIR/fit-1080p-window.xwininfo" Width)"
fit_1080p_height="$(geometry_value "$OUT_DIR/fit-1080p-window.xwininfo" Height)"
if (( fit_1080p_width < 1200 || fit_1080p_width > 1204 || fit_1080p_height < 674 || fit_1080p_height > 679 )); then
  echo "1080p video did not fit the primary workarea: ${fit_1080p_width}x${fit_1080p_height}" >&2
  exit 1
fi
assert_single_initial_configure "$OUT_DIR/fit-1080p-app.log" "1080p video"
stop_app

start_app "fit-vertical-app.log" windowed "$FIXTURES/fit-vertical.mkv"
vertical_id="$(wait_for_window "$OUT_DIR/fit-vertical-window.ids")"
sleep 4
capture_geometry "$vertical_id" "fit-vertical-window"
vertical_width="$(geometry_value "$OUT_DIR/fit-vertical-window.xwininfo" Width)"
vertical_height="$(geometry_value "$OUT_DIR/fit-vertical-window.xwininfo" Height)"
if (( vertical_width < 450 || vertical_width > 454 || vertical_height < 802 || vertical_height > 806 )); then
  echo "Vertical video did not fit the primary workarea: ${vertical_width}x${vertical_height}" >&2
  exit 1
fi
assert_single_initial_configure "$OUT_DIR/fit-vertical-app.log" "Vertical video"
stop_app

start_app "fit-maximized-app.log" maximized "$FIXTURES/fit-small.mkv"
max_id="$(wait_for_window "$OUT_DIR/fit-maximized-window.ids")"
sleep 4
capture_geometry "$max_id" "fit-maximized-before-load"
before_max_width="$(geometry_value "$OUT_DIR/fit-maximized-before-load.xwininfo" Width)"
before_max_height="$(geometry_value "$OUT_DIR/fit-maximized-before-load.xwininfo" Height)"
if (( before_max_width < 1200 || before_max_height < 840 )); then
  echo "Could not maximize the player before the load guard check: ${before_max_width}x${before_max_height}" >&2
  exit 1
fi
capture_geometry "$max_id" "fit-maximized-window"
max_width="$(geometry_value "$OUT_DIR/fit-maximized-window.xwininfo" Width)"
max_height="$(geometry_value "$OUT_DIR/fit-maximized-window.xwininfo" Height)"
if (( max_width < 1200 || max_height < 840 )); then
  echo "Media load resized a maximized window: ${max_width}x${max_height}" >&2
  exit 1
fi
if ! wait_for_log "$OUT_DIR/fit-maximized-app.log" \
  "window fit skipped: fullscreen=false maximized=true"; then
  echo "Maximized load did not exercise the window-fit guard" >&2
  exit 1
fi
activate_fit_window_command "$max_id"
capture_geometry "$max_id" "fit-maximized-explicit-command"
explicit_max_width="$(geometry_value "$OUT_DIR/fit-maximized-explicit-command.xwininfo" Width)"
explicit_max_height="$(geometry_value "$OUT_DIR/fit-maximized-explicit-command.xwininfo" Height)"
if [[ "$explicit_max_width" != "320" || "$explicit_max_height" != "180" ]]; then
  echo "Explicit Fit did not restore a maximized window: ${explicit_max_width}x${explicit_max_height}" >&2
  exit 1
fi
rg -q 'interaction: player-command=fit-window-to-media' "$OUT_DIR/fit-maximized-app.log" || {
  echo "Explicit Fit command was not dispatched from the shared menu" >&2
  exit 1
}
stop_app

start_app "fit-fullscreen-app.log" fullscreen "$FIXTURES/fit-small.mkv"
fullscreen_id="$(wait_for_window "$OUT_DIR/fit-fullscreen-window.ids")"
sleep 4
capture_geometry "$fullscreen_id" "fit-fullscreen-window"
fullscreen_width="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Width)"
fullscreen_height="$(geometry_value "$OUT_DIR/fit-fullscreen-window.xwininfo" Height)"
if (( fullscreen_width < 1270 || fullscreen_height < 890 )); then
  echo "Media load resized a fullscreen window: ${fullscreen_width}x${fullscreen_height}" >&2
  exit 1
fi
if ! wait_for_log "$OUT_DIR/fit-fullscreen-app.log" \
  "window fit skipped: fullscreen=true maximized=false"; then
  echo "Fullscreen load did not exercise the window-fit guard" >&2
  exit 1
fi
activate_fit_window_command "$fullscreen_id"
capture_geometry "$fullscreen_id" "fit-fullscreen-explicit-command"
explicit_fullscreen_width="$(geometry_value "$OUT_DIR/fit-fullscreen-explicit-command.xwininfo" Width)"
explicit_fullscreen_height="$(geometry_value "$OUT_DIR/fit-fullscreen-explicit-command.xwininfo" Height)"
if [[ "$explicit_fullscreen_width" != "320" || "$explicit_fullscreen_height" != "180" ]]; then
  echo "Explicit Fit did not restore a fullscreen window: ${explicit_fullscreen_width}x${explicit_fullscreen_height}" >&2
  exit 1
fi
stop_app

export DISPLAY="$SECONDARY_DISPLAY"
start_app "fit-4k-right-monitor-app.log" windowed "$FIXTURES/fit-4k.mkv"
right_id="$(wait_for_window "$OUT_DIR/fit-4k-right-monitor-window.ids")"
sleep 4
xdotool mousemove --window "$right_id" 480 270
sleep 0.5
capture_geometry "$right_id" "fit-4k-right-monitor-window"
fit_width="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Width)"
fit_height="$(geometry_value "$OUT_DIR/fit-4k-right-monitor-window.xwininfo" Height)"
if (( fit_width < 958 || fit_width > 964 || fit_height < 538 || fit_height > 543 )); then
  echo "4K video did not fit the active 1024x768 monitor: ${fit_width}x${fit_height}" >&2
  exit 1
fi
if ! awk -v w="$fit_width" -v h="$fit_height" \
  'BEGIN { aspect=w/h; exit !(aspect > 1.775 && aspect < 1.780) }'; then
  echo "4K fitted window lost 16:9 aspect: ${fit_width}x${fit_height}" >&2
  exit 1
fi
if ! rg -q "workarea=1024x768" "$OUT_DIR/fit-4k-right-monitor-app.log"; then
  echo "4K load did not use the monitor containing the player window" >&2
  exit 1
fi
assert_single_initial_configure "$OUT_DIR/fit-4k-right-monitor-app.log" "4K video"
assert_monitor_fit_log "$OUT_DIR/fit-4k-right-monitor-app.log" "4K video"
assert_logged_fit_containment "$OUT_DIR/fit-4k-right-monitor-app.log" "4K video"
xdotool windowsize "$right_id" 700 500
sleep 1
activate_fit_window_command "$right_id"
capture_geometry "$right_id" "fit-4k-explicit-command"
explicit_4k_width="$(geometry_value "$OUT_DIR/fit-4k-explicit-command.xwininfo" Width)"
explicit_4k_height="$(geometry_value "$OUT_DIR/fit-4k-explicit-command.xwininfo" Height)"
if (( explicit_4k_width < 958 || explicit_4k_width > 964 || explicit_4k_height < 538 || explicit_4k_height > 543 )); then
  echo "Explicit Fit did not restore the 4K media geometry: ${explicit_4k_width}x${explicit_4k_height}" >&2
  exit 1
fi
stop_app

small_window_geometry="$(absolute_window_geometry "$OUT_DIR/fit-small-window.xwininfo")"
fit_1080p_window_geometry="$(absolute_window_geometry "$OUT_DIR/fit-1080p-window.xwininfo")"
fit_4k_window_geometry="$(absolute_window_geometry "$OUT_DIR/fit-4k-right-monitor-window.xwininfo")"
small_monitor_fit="$(rg -m1 '^window fit request:' "$OUT_DIR/fit-small-app.log")"
fit_4k_monitor_fit="$(rg -m1 '^window fit request:' "$OUT_DIR/fit-4k-right-monitor-app.log")"

cat >>"$OUT_DIR/fit-evidence.txt" <<EOF
maximized_guard=pass
maximized_explicit_fit=${explicit_max_width}x${explicit_max_height}
fullscreen_guard=pass
fullscreen_explicit_fit=${explicit_fullscreen_width}x${explicit_fullscreen_height}
small_geometry=${small_width}x${small_height}
small_window=${small_window_geometry}
small_monitor_fit=${small_monitor_fit}
1080p_geometry=${fit_1080p_width}x${fit_1080p_height}
1080p_window=${fit_1080p_window_geometry}
vertical_geometry=${vertical_width}x${vertical_height}
4k_geometry=${fit_width}x${fit_height}
4k_window=${fit_4k_window_geometry}
4k_monitor_fit=${fit_4k_monitor_fit}
initial_fit_configures_per_geometry=1
logged_monitor_workarea_containment=pass
4k_explicit_fit=${explicit_4k_width}x${explicit_4k_height}
explicit_fit_dispatch=pass
status=pass
EOF
FIT_SMOKE
then
  echo "Window-fit smoke failed. Session log: $OUT_DIR/window-fit-session.log" >&2
  cat "$OUT_DIR/window-fit-session.log" >&2
  exit 1
fi
cat "$OUT_DIR/fit-xvfb-evidence.txt" >>"$OUT_DIR/fit-evidence.txt"
cat "$OUT_DIR/fit-session-evidence.txt" >>"$OUT_DIR/fit-evidence.txt"

if rg -q "org\.freedesktop\.portal\.Desktop" "$OUT_DIR/window-fit-session.log"; then
  echo "Window-fit smoke unexpectedly activated a desktop portal in the isolated X11 session" >&2
  cat "$OUT_DIR/window-fit-session.log" >&2
  exit 1
fi
printf 'portal_activation=absent\n' >>"$OUT_DIR/fit-evidence.txt"

if [[ "${OKP_MAIN_WINDOW_SHUTDOWN_ONLY:-0}" == "1" ]]; then
  echo "Main window shutdown smoke passed. Evidence: $OUT_DIR/fit-evidence.txt"
else
  echo "Main window fit smoke passed. Screenshots: $OUT_DIR/fit-small-window.png, $OUT_DIR/fit-4k-right-monitor-window.png"
fi
