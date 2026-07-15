#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-lyrics-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

# The lyrics surface is pure GTK (no GL): render the shell through the software cairo path with GLX
# disabled, so the smoke runs on headless hosts whose Xvfb has no (or a crashing) GLX. The video
# GLArea stays black here, which is exactly the audio-playback state the lyrics surface targets.
if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -extension GLX -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export GSK_RENDERER=cairo
export LIBGL_ALWAYS_SOFTWARE=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1
OKP_OPEN_LYRICS_ON_STARTUP=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 5
xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"

xwininfo -root -tree >"$OUT_DIR/tree.txt"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo"
import -window root "$OUT_DIR/root.png"
import -window "$window_id" "$OUT_DIR/window.png"

width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"
border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/window.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window.xwininfo")"

if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
  echo "Unexpected main window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
  exit 1
fi

# The lyrics overlay centres its sheet over the (audio-black) video plane: bright lines on a
# translucent dark scrim, the active line brightened to near-white. If the fixture sheet rendered,
# the central band carries that bright text, so a dark maximum there means the surface failed to
# draw.
band_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 520x360+300+90 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$band_max" 'BEGIN { exit !(max > 0.75) }'; then
  echo "Lyrics surface looks blank: content maxima=${band_max}" >&2
  exit 1
fi

# The sheet sits on the dark scrim, not a light panel, so the same band must stay mostly dark on
# average — a bright mean would mean some other (light) surface covered the plane.
band_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 520x360+300+90 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$band_mean" 'BEGIN { exit !(mean < 0.32) }'; then
  echo "Lyrics scrim missing: content mean=${band_mean}" >&2
  exit 1
fi

# The header wordmark carries the OK Player teal accent, so across a strip at the top of the sheet
# the green channel should read stronger than the red one — a stock grey header would not.
header_red="$(magick "$OUT_DIR/window.png" -crop 260x28+430+50 -format '%[fx:mean.r]' info:)"
header_green="$(magick "$OUT_DIR/window.png" -crop 260x28+430+50 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$header_red" -v g="$header_green" 'BEGIN { exit !(g - r > 0.002) }'; then
  echo "Lyrics header accent missing: red=${header_red} green=${header_green}" >&2
  exit 1
fi
SMOKE
then
  echo "Lyrics smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Lyrics smoke passed. Screenshot: $OUT_DIR/window.png"
