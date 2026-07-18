#!/usr/bin/env bash
# Deterministic X11/Xvfb render smoke for issue #394's persistent updater card.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-update-surface-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
export GDK_BACKEND=x11
export GSK_RENDERER=cairo
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

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":2,"updates":{"auto_check":false}}
JSON

capture() {
  local mode="$1" theme="$2" shot="$3" expected_width="${4:-1120}" expected_height="${5:-680}"
  local narrow_env=()
  if (( expected_width < 620 )); then
    narrow_env=(OKP_NARROW_COMMAND_PREVIEW=1)
  fi
  env \
    OKP_SKIP_UPDATE_CHECK=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    OKP_IDLE_THEME="$theme" \
    OKP_WELCOME_STATE=empty \
    OKP_UPDATE_SURFACE_PREVIEW="$mode" \
    "${narrow_env[@]}" \
    timeout 15s "$BINARY" >"$OUT_DIR/$shot.log" 2>&1 &
  app_pid=$!

  for _ in $(seq 1 30); do
    if xdotool search --name '^OK Player$' >"$OUT_DIR/$shot.ids" 2>/dev/null \
      && [[ -s "$OUT_DIR/$shot.ids" ]]; then
      break
    fi
    sleep 0.25
  done
  local window_id
  window_id="$(head -n1 "$OUT_DIR/$shot.ids" 2>/dev/null || true)"
  if [[ -z "$window_id" ]]; then
    echo "$shot: player window did not appear" >&2
    exit 1
  fi

  # The former update toast was gone after 1.7 seconds. Waiting five seconds
  # proves this card is governed by update state rather than that timeout.
  sleep 5
  import -window "$window_id" "$OUT_DIR/$shot.png"
  xwininfo -id "$window_id" >"$OUT_DIR/$shot.xwininfo"

  local width height
  width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/$shot.xwininfo")"
  height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/$shot.xwininfo")"
  if [[ "$width" != "$expected_width" || "$height" != "$expected_height" ]]; then
    echo "$shot: unexpected geometry ${width}x${height}, expected ${expected_width}x${expected_height}" >&2
    exit 1
  fi

  local card_crop card_variance
  if (( expected_width < 620 )); then
    card_crop="$((expected_width - 32))x150+16+6"
  else
    card_crop="600x110+260+48"
  fi
  card_variance="$(magick "$OUT_DIR/$shot.png" -crop "$card_crop" -colorspace gray -format '%[fx:standard_deviation]' info:)"
  if ! awk -v variance="$card_variance" 'BEGIN {exit !(variance > 0.07)}'; then
    echo "$shot: persistent update card is unexpectedly flat: variance=$card_variance" >&2
    exit 1
  fi

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture available light available-light
capture available dark available-dark
capture install-error light install-error-light
capture install-error dark install-error-dark
capture available dark available-narrow 480 270
SMOKE
then
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Linux persistent update surface smoke passed: $OUT_DIR"
