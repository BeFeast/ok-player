#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"
BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-update-surface-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

mkdir -p "$OUT_DIR"
STATE_DIR="$(okp_make_scratch_dir update-surface)"
trap 'rm -rf -- "$STATE_DIR"' EXIT

env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$STATE_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
STATE_DIR="$3"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export XDG_CONFIG_HOME="$STATE_DIR/config"
export XDG_DATA_HOME="$STATE_DIR/data"
export XDG_STATE_HOME="$STATE_DIR/state"
export XDG_CACHE_HOME="$STATE_DIR/cache"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_SETTINGS_UPDATE_PREVIEW=available \
OKP_SKIP_UPDATE_CHECK=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 30s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

player_id=""
for _ in $(seq 1 40); do
  player_id="$(xdotool search --onlyvisible --name '^OK Player$' 2>/dev/null | head -n1 || true)"
  [[ -n "$player_id" ]] && break
  sleep 0.25
done
if [[ -z "$player_id" ]]; then
  echo "Player window did not appear" >&2
  exit 1
fi

xwininfo -id "$player_id" >"$OUT_DIR/player.xwininfo"
sleep 5
import -window "$player_id" "$OUT_DIR/available-before-timeout.png"
sleep 3
import -window "$player_id" "$OUT_DIR/available-after-timeout.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/player.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/player.xwininfo")"
if [[ "$width" != "1120" || "$height" != "680" ]]; then
  echo "Unexpected player geometry: ${width}x${height}" >&2
  exit 1
fi

surface_variance="$(
  magick "$OUT_DIR/available-after-timeout.png" \
    -crop 520x120+300+48 \
    -colorspace gray \
    -format '%[fx:standard_deviation]' info:
)"
if ! awk -v variance="$surface_variance" 'BEGIN { exit !(variance > 0.08) }'; then
  echo "Persistent update surface is unexpectedly flat: variance=${surface_variance}" >&2
  exit 1
fi

timeout_difference="$(
  magick "$OUT_DIR/available-before-timeout.png" "$OUT_DIR/available-after-timeout.png" \
    -compose difference -composite \
    -crop 520x120+300+48 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v difference="$timeout_difference" 'BEGIN { exit !(difference < 0.003) }'; then
  echo "Update surface changed after the old toast timeout: difference=${timeout_difference}" >&2
  exit 1
fi

xdotool mousemove --window "$player_id" 528 135 click 1
sleep 1
import -window "$player_id" "$OUT_DIR/after-skip.png"
if ! rg -q '"public": "0\.11\.0-beta\.2"' \
  "$XDG_CONFIG_HOME/ok-player/settings.json"; then
  echo "Skip this version did not persist the public-channel version" >&2
  exit 1
fi

skip_difference="$(
  magick "$OUT_DIR/available-after-timeout.png" "$OUT_DIR/after-skip.png" \
    -compose difference -composite \
    -crop 520x120+300+48 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v difference="$skip_difference" 'BEGIN { exit !(difference > 0.04) }'; then
  echo "Skip action did not dismiss the persistent decision surface: difference=${skip_difference}" >&2
  exit 1
fi

kill "$app_pid" 2>/dev/null || true
wait "$app_pid" 2>/dev/null || true

OKP_OPEN_SETTINGS_ON_STARTUP=1 \
OKP_OPEN_SETTINGS_PAGE_ON_STARTUP=updates \
OKP_SETTINGS_UPDATE_PREVIEW=available \
OKP_SKIP_UPDATE_CHECK=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 30s "$BINARY" >"$OUT_DIR/restart.log" 2>&1 &
app_pid=$!

settings_id=""
for _ in $(seq 1 40); do
  settings_id="$(xdotool search --onlyvisible --name '^Settings$' 2>/dev/null | head -n1 || true)"
  [[ -n "$settings_id" ]] && break
  sleep 0.25
done
if [[ -z "$settings_id" ]]; then
  echo "Settings window did not appear after restart" >&2
  exit 1
fi

sleep 5
import -window "$settings_id" "$OUT_DIR/settings-skipped.png"

# Manual Check for updates keeps the exact skipped result visible and exposes
# Install anyway instead of silently reporting Up to date.
xdotool mousemove --window "$settings_id" 495 521 click 1
sleep 1
import -window "$settings_id" "$OUT_DIR/settings-skipped-after-check.png"

manual_check_difference="$(
  magick "$OUT_DIR/settings-skipped.png" "$OUT_DIR/settings-skipped-after-check.png" \
    -compose difference -composite \
    -crop 500x150+216+340 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v difference="$manual_check_difference" 'BEGIN { exit !(difference < 0.02) }'; then
  echo "Manual check did not preserve the skipped-version decision surface: difference=${manual_check_difference}" >&2
  exit 1
fi

xdotool mousemove --window "$settings_id" 620 458 click 1
sleep 2
import -window "$settings_id" "$OUT_DIR/settings-install-failure.png"

failure_difference="$(
  magick "$OUT_DIR/settings-skipped.png" "$OUT_DIR/settings-install-failure.png" \
    -compose difference -composite \
    -crop 500x150+216+340 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v difference="$failure_difference" 'BEGIN { exit !(difference > 0.01) }'; then
  echo "Install anyway did not enter a visible retryable failure state: difference=${failure_difference}" >&2
  exit 1
fi

printf 'surface_variance=%s\ntimeout_difference=%s\n' \
  "$surface_variance" "$timeout_difference" >"$OUT_DIR/measurements.txt"
printf 'skip_difference=%s\nfailure_difference=%s\n' \
  "$skip_difference" "$failure_difference" >>"$OUT_DIR/measurements.txt"
printf 'manual_check_difference=%s\n' \
  "$manual_check_difference" >>"$OUT_DIR/measurements.txt"
SMOKE

echo "Update surface smoke passed. Screenshots: $OUT_DIR"
