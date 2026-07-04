#!/usr/bin/env bash
set -euo pipefail

# Guards the §2.1 error state of the surface-state matrix: when a file fails to
# load, the player must recover to the OK Player welcome surface (its tested
# no-media state) rather than stranding the user on a black video plane behind a
# live OSC reading 00:00 / 00:00. OKP_SIMULATE_LOAD_ERROR_ON_STARTUP drives the
# real recovery path (unload media, queue the error toast) without needing a GL
# video pipeline, so the recovered surface can be screenshot-tested headlessly.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-playback-error-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick timeout head awk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
OKP_SIMULATE_LOAD_ERROR_ON_STARTUP="Broken Episode.mkv" \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 4
xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"

xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
import -window "$window_id" "$OUT_DIR/window.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"

if [[ "$width" != "1120" || "$height" != "680" || "$state" != "IsViewable" ]]; then
  echo "Unexpected window geometry after load error: ${width}x${height}, state=${state}" >&2
  exit 1
fi

# The recovery target is the welcome surface, whose centered identity band (app
# mark + wordmark) sits over the middle of the window. This guard only needs to
# tell a rendered welcome surface apart from a dead black video plane (the media
# never unloaded): the failure mode reads as the ~srgb(5,5,7) chrome black
# (maxima ~0.03), while any rendered welcome band clears it with room to spare,
# so the low floor stays robust to overlay dimming and the software renderer.
identity_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 260x150+430+165 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$identity_max" 'BEGIN { exit !(max > 0.08) }'; then
  echo "Failed load did not recover to the welcome surface: identity band maxima=${identity_max}" >&2
  exit 1
fi
SMOKE
then
  echo "Playback-error recovery smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Playback-error recovery smoke passed. Screenshot: $OUT_DIR/window.png"
