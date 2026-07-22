#!/usr/bin/env bash
# Package-bound EOF/Close Media regression smoke. Xvfb proves deterministic
# rendering and state projection; native Wayland compositor behavior remains
# an explicit live-desktop acceptance row.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-idle-return-smoke}"
IDLE_OSC_ASSERT="$ROOT/scripts/assert-linux-idle-osc-absent.sh"
if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi
OUT_DIR="$(realpath -m "$OUT_DIR")"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffmpeg realpath rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'color=c=0xff00ff:s=1120x680:r=24:d=4' \
  -map 0:v:0 -c:v libx264 -preset ultrafast -tune stillimage \
  -pix_fmt yuv420p -g 24 -an "$OUT_DIR/idle-return.mkv"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$IDLE_OSC_ASSERT" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
IDLE_OSC_ASSERT="$3"
FIXTURE="$OUT_DIR/idle-return.mkv"

export GDK_BACKEND=x11
export GDK_DEBUG=no-portals
export GSK_RENDERER=cairo
export GIO_USE_PORTALS=0
export GTK_USE_PORTAL=0
export GTK_A11Y=none
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_DISABLE_MPRIS=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CACHE_HOME="$OUT_DIR/cache"

mkdir -p "$XDG_CONFIG_HOME/ok-player" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 2,
  "playback": { "auto_advance": false },
  "updates": { "auto_check": false }
}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

wait_for_window() {
  local window_id=""
  for _ in $(seq 1 120); do
    for candidate in $(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null || true); do
      local width height
      width="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Width:/ { print $2; exit }')"
      height="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Height:/ { print $2; exit }')"
      if [[ "${width:-0}" -ge 1000 && "${height:-0}" -ge 600 ]]; then
        window_id="$candidate"
        break 2
      fi
    done
    sleep 0.1
  done
  [[ -n "$window_id" ]] || return 1
  printf '%s\n' "$window_id"
}

capture_metrics() {
  local window_id="$1" image="$2"
  import -window "$window_id" "$image" || return 1

  local alpha_min identity_residual magenta_mean
  alpha_min="$(magick "$image" -alpha extract -format '%[fx:minima]' info:)"
  identity_residual="$(magick "$image" \
    -crop 300x170+410+180 +repage -colorspace gray \
    \( +clone -blur 0x4 \) -compose difference -composite \
    -format '%[fx:mean]' info:)"
  magenta_mean="$(magick "$image" -crop 260x140+430+220 \
    -format '%[fx:(mean.r+mean.b)/2-mean.g]' info:)"

  printf '%s %s %s\n' "$alpha_min" "$identity_residual" "$magenta_mean"
}

assert_idle_capture() {
  local window_id="$1" name="$2" label="$3"
  local alpha_min=0 identity_residual=0 magenta_mean=0 ready=0

  # Startup and package extraction can make media initialization variable. Poll the
  # rendered identity instead of taking one timing-dependent sample; a blank or
  # retained fixture frame still times out and fails the unchanged assertions below.
  for _ in $(seq 1 120); do
    if read -r alpha_min identity_residual magenta_mean \
      < <(capture_metrics "$window_id" "$OUT_DIR/$name.png") \
      && awk -v alpha="$alpha_min" -v identity="$identity_residual" -v magenta="$magenta_mean" \
        'BEGIN { exit !(alpha > 0.999 && identity > 0.012 && magenta < 0.35) }'
    then
      ready=1
      break
    fi
    sleep 0.25
  done

  [[ "$ready" == "1" ]] || {
    echo "$label did not become a complete Welcome surface: alpha minimum=$alpha_min residual=$identity_residual magenta mean=$magenta_mean" >&2
    exit 1
  }

  "$IDLE_OSC_ASSERT" "$OUT_DIR/$name.png" "$label"

  awk -v value="$alpha_min" 'BEGIN { exit !(value > 0.999) }' || {
    echo "$label capture contains transparent pixels: alpha minimum=$alpha_min" >&2
    exit 1
  }
  awk -v value="$identity_residual" 'BEGIN { exit !(value > 0.012) }' || {
    echo "$label did not restore the Welcome identity: residual=$identity_residual" >&2
    exit 1
  }
  awk -v value="$magenta_mean" 'BEGIN { exit !(value < 0.35) }' || {
    echo "$label retained the magenta fixture frame: magenta mean=$magenta_mean" >&2
    exit 1
  }

  printf '%s_alpha_min=%s\n%s_identity_residual=%s\n%s_magenta_mean=%s\n' \
    "$name" "$alpha_min" "$name" "$identity_residual" "$name" "$magenta_mean" \
    >>"$OUT_DIR/results.txt"
}

wait_for_log_marker() {
  local marker="$1" log_name="$2" label="$3"
  for _ in $(seq 1 180); do
    if rg -q -F "$marker" "$OUT_DIR/$log_name.log"; then
      printf '%s=%s\n' "$label" pass >>"$OUT_DIR/results.txt"
      return
    fi
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 0.25
  done
  echo "$label did not reach marker: $marker" >&2
  cat "$OUT_DIR/$log_name.log" >&2 || true
  exit 1
}

launch_fixture() {
  local log_name="$1"
  OKP_DEBUG_IDLE_RETURN_SMOKE=1 \
    timeout 60s "$BINARY" "$FIXTURE" >"$OUT_DIR/$log_name.log" 2>&1 &
  app_pid=$!
  window_id="$(wait_for_window)"
}

launch_empty() {
  local log_name="$1"
  timeout 30s "$BINARY" >"$OUT_DIR/$log_name.log" 2>&1 &
  app_pid=$!
  window_id="$(wait_for_window)"
}

stop_app() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 0.5
}

window_id=""
launch_empty initial-app || {
  cat "$OUT_DIR/initial-app.log" >&2 || true
  exit 1
}
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
assert_idle_capture "$window_id" initial-idle "Initial idle canvas"
stop_app

launch_fixture eof-app || {
  cat "$OUT_DIR/eof-app.log" >&2 || true
  exit 1
}
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
wait_for_log_marker 'idle-return-smoke: file-loaded' eof-app eof_file_loaded
wait_for_log_marker 'idle-return-smoke: eof-idle' eof-app eof_idle_transition
assert_idle_capture "$window_id" eof-idle "EOF idle canvas"
stop_app

launch_fixture close-app || {
  cat "$OUT_DIR/close-app.log" >&2 || true
  exit 1
}
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
wait_for_log_marker 'idle-return-smoke: file-loaded' close-app close_media_file_loaded
xdotool windowfocus "$window_id"
xdotool key --clearmodifiers x
wait_for_log_marker 'idle-return-smoke: close-idle' close-app close_media_transition
assert_idle_capture "$window_id" close-media-idle "Close Media idle canvas"

for name in eof-idle; do
  magick "$OUT_DIR/initial-idle.png" -crop 1120x638+0+42 +repage \
    "$OUT_DIR/initial-idle-content.png"
  magick "$OUT_DIR/$name.png" -crop 1120x638+0+42 +repage \
    "$OUT_DIR/$name-content.png"
  rmse="$(magick compare -metric RMSE \
    "$OUT_DIR/initial-idle-content.png" "$OUT_DIR/$name-content.png" null: 2>&1 || true)"
  normalized="$(sed -n 's/.*(\([^()]*\)).*/\1/p' <<<"$rmse")"
  [[ -n "$normalized" ]] && awk -v value="$normalized" 'BEGIN { exit !(value < 0.002) }' || {
    echo "$name diverged from the canonical initial idle content: RMSE=$rmse" >&2
    exit 1
  }
  printf '%s_reference_rmse=%s\n' "$name" "$normalized" >>"$OUT_DIR/results.txt"
done

printf '%s\n' \
  'evidence_level=package-payload-xvfb-render' \
  'package_bound=pass' \
  'eof_idle=pass' \
  'close_media_idle=pass' \
  'not_proven=GNOME Wayland compositor, native subsurface retirement, focus, portals' \
  >>"$OUT_DIR/results.txt"
SMOKE
then
  echo "Idle-return smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Idle-return smoke passed. Results: $OUT_DIR/results.txt"
