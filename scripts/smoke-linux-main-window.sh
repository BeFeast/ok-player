#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-main-window-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"

mkdir -p "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 1,
  "updates": { "auto_check": false }
}
JSON

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
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 4
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

# The first-run surface owns its own Open actions; the standard playback OSC
# must not be present before media is loaded.
empty_bottom_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 1088x70+16+592 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$empty_bottom_max" 'BEGIN { exit !(max < 0.08) }'; then
  echo "Standard OSC leaked into the empty state: bottom maxima=${empty_bottom_max}" >&2
  exit 1
fi

# Regression guard for the old native GTK caption/headerbar. The player owns
# its caption controls, so the top-center strip should remain dark video/chrome,
# not a light system titlebar with centered text.
top_center_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 220x36+450+0 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$top_center_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Unexpected bright top-center caption strip in main window: mean=${top_center_mean}" >&2
  exit 1
fi

# Guard the captionless dark shell as a region rather than pinning one
# decorative pixel. Small renderer/color-management differences may shift an
# individual RGB value while the perceived surface remains identical.
top_left_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 180x36+0+0 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$top_left_mean" 'BEGIN { exit !(mean < 0.08) }'; then
  echo "Unexpected bright top-left chrome region: mean=${top_left_mean}" >&2
  exit 1
fi

# The empty player shell must render the OK Player welcome surface, not a blank
# or overlapped viewport. The centered identity band (app mark + wordmark) is
# the brightest thing in the middle of the window, so a near-black maximum there
# means the welcome surface failed to draw.
identity_max="$(
  magick "$OUT_DIR/window.png" \
    -crop 260x150+430+165 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
identity_mean="$(
  magick "$OUT_DIR/window.png" \
    -crop 280x150+420+160 \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v max="$identity_max" -v mean="$identity_mean" \
  'BEGIN { exit !(max > 0.6 && max - mean > 0.4) }'; then
  echo "Empty welcome identity lacks contrast: maxima=${identity_max} mean=${identity_mean}" >&2
  exit 1
fi

# Supporting copy must produce a meaningful bright-text area, not merely exist
# as near-black pixels over the video plane.
tagline_bright_pixels="$(
  magick "$OUT_DIR/window.png" \
    -crop 280x34+420+275 \
    -colorspace gray \
    -threshold 42% \
    -format '%[fx:mean*w*h]' info:
)"
tagline_bright_pixels="${tagline_bright_pixels%.*}"
if (( tagline_bright_pixels < 180 )); then
  echo "Welcome supporting copy is missing or too dim: bright pixels=${tagline_bright_pixels}" >&2
  exit 1
fi

# The primary "Open media" action carries the OK Player accent, so its band must
# be clearly green-dominant (teal) rather than a stock grey GTK button.
primary_red="$(magick "$OUT_DIR/window.png" -crop 130x60+372+296 -format '%[fx:mean.r]' info:)"
primary_green="$(magick "$OUT_DIR/window.png" -crop 130x60+372+296 -format '%[fx:mean.g]' info:)"
if ! awk -v r="$primary_red" -v g="$primary_green" 'BEGIN { exit !(g - r > 0.12) }'; then
  echo "Primary action accent missing from welcome surface: red=${primary_red} green=${primary_green}" >&2
  exit 1
fi

# Secondary actions carry readable light labels and borders beside the CTA.
secondary_bright_pixels="$(
  magick "$OUT_DIR/window.png" \
    -crop 260x70+495+320 \
    -colorspace gray \
    -threshold 55% \
    -format '%[fx:mean*w*h]' info:
)"
secondary_bright_pixels="${secondary_bright_pixels%.*}"
if (( secondary_bright_pixels < 300 )); then
  echo "Welcome secondary actions are missing or unreadable: bright pixels=${secondary_bright_pixels}" >&2
  exit 1
fi
SMOKE
then
  echo "Main window smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Main window smoke passed. Screenshot: $OUT_DIR/window.png"
