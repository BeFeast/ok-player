#!/usr/bin/env bash
# Xvfb interaction smoke for the capture-phase Space contract. This proves
# deterministic GTK routing, not live GNOME/Wayland focus or compositor behavior.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="$(realpath -m "${2:-$ROOT/artifacts/linux-acceptance/space-shortcut}")"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
if [[ ! -x "$BINARY" ]]; then
  echo "Missing player binary: $BINARY" >&2
  exit 127
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
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
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export LIBGL_ALWAYS_SOFTWARE=1

mkdir -p "$XDG_CONFIG_HOME/ok-player"
printf '%s\n' '{"version":2,"updates":{"auto_check":false}}' \
  >"$XDG_CONFIG_HOME/ok-player/settings.json"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

OKP_DEBUG_INTERACTIONS=1 \
OKP_DISABLE_MPRIS=1 \
OKP_FIXED_VIEWPORT_SMOKE=1 \
OKP_SKIP_UPDATE_CHECK=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 45s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in $(seq 1 120); do
  for candidate in $(xdotool search --onlyvisible --name '^OK Player$' 2>/dev/null || true); do
    width="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Width:/ { print $2; exit }')"
    height="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Height:/ { print $2; exit }')"
    if [[ "${width:-0}" -ge 1000 && "${height:-0}" -ge 600 ]]; then
      window_id="$candidate"
    fi
  done
  [[ -n "$window_id" ]] && break
  sleep 0.1
done
if [[ -z "$window_id" ]]; then
  echo "main window did not appear" >&2
  exit 1
fi
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 3

focus_play_button() {
  local focus_before focus_after
  focus_before="$(rg -c 'interaction: outside-target=play-focused' "$OUT_DIR/app.log" || true)"
  for _ in $(seq 1 24); do
    xdotool key --clearmodifiers Tab
    sleep 0.05
    focus_after="$(rg -c 'interaction: outside-target=play-focused' "$OUT_DIR/app.log" || true)"
    if [[ "${focus_after:-0}" -gt "${focus_before:-0}" ]]; then
      return
    fi
  done
  echo "keyboard traversal did not focus Play/Pause" >&2
  exit 1
}

focus_play_button

player_dispatches_before="$(rg -c 'keyboard-play-pause-dispatch context=player' "$OUT_DIR/app.log" || true)"
play_activations_before="$(rg -c 'interaction: play-button-activate' "$OUT_DIR/app.log" || true)"
xdotool keydown --clearmodifiers space
sleep 1
xdotool keyup --clearmodifiers space
sleep 0.5
player_dispatches_after="$(rg -c 'keyboard-play-pause-dispatch context=player' "$OUT_DIR/app.log" || true)"
play_activations_after="$(rg -c 'interaction: play-button-activate' "$OUT_DIR/app.log" || true)"
if [[ $((player_dispatches_after - player_dispatches_before)) -ne 1 ]]; then
  echo "held Space did not dispatch exactly one player Play/Pause command" >&2
  exit 1
fi
if [[ "$play_activations_after" -ne "$play_activations_before" ]]; then
  echo "Space reactivated the focused Play/Pause button" >&2
  exit 1
fi

focus_play_button
xdotool key --clearmodifiers Return
sleep 0.5
play_activations_after_return="$(rg -c 'interaction: play-button-activate' "$OUT_DIR/app.log" || true)"
if [[ $((play_activations_after_return - play_activations_after)) -ne 1 ]]; then
  echo "Return did not activate the focused Play/Pause button" >&2
  exit 1
fi

xdotool key --clearmodifiers ctrl+comma
settings_window_id=""
for _ in $(seq 1 40); do
  settings_window_id="$(xdotool search --onlyvisible --name '^Settings$' 2>/dev/null | tail -n1 || true)"
  [[ -n "$settings_window_id" ]] && break
  sleep 0.1
done
if [[ -z "$settings_window_id" ]]; then
  echo "Settings companion did not open" >&2
  exit 1
fi
xdotool windowactivate "$settings_window_id" >/dev/null 2>&1 || true
sleep 0.5

settings_dispatches_before="$(rg -c 'keyboard-play-pause-dispatch context=settings' "$OUT_DIR/app.log" || true)"
xdotool keydown --clearmodifiers space
sleep 1
xdotool keyup --clearmodifiers space
sleep 0.5
settings_dispatches_after="$(rg -c 'keyboard-play-pause-dispatch context=settings' "$OUT_DIR/app.log" || true)"
if [[ $((settings_dispatches_after - settings_dispatches_before)) -ne 1 ]]; then
  echo "held Space did not dispatch exactly once from Settings" >&2
  exit 1
fi
if ! xwininfo -id "$settings_window_id" >/dev/null 2>&1; then
  echo "Space activated a Settings control instead of playback" >&2
  exit 1
fi

echo "Space shortcut smoke passed"
SMOKE
then
  echo "Space shortcut smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Space shortcut smoke passed. Evidence: $OUT_DIR"
