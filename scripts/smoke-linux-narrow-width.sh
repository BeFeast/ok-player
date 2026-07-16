#!/usr/bin/env bash
# Visual smoke guard for the PRD §14 narrow-width acceptance (issue #235): the
# OSC bar and side panel can crowd at small window widths, so text/buttons must
# not clip or overlap. This script loads real media (to hide the welcome surface
# and give a clean dark video plane), opens the Up Next side panel (which pins
# the OSC visible for the duration), resizes the window down to a narrow floor
# where the side panel still fits without clipping, and asserts on regions that
# the OSC controls and side-panel rows render without clipping and that the
# panel does not slide down over the bar. Guards use regions and derived layout
# boundaries rather than any exact decorative pixel.
#
# Needs real media plus a window resize, which is why it is tracked separately
# from the preview-fixture smokes. The tiny synthetic clip in
# tests/OkPlayer.IntegrationTests/fixtures is a dark 1280x720 H.264 stream, so a
# near-black maximum in an OSC control band means a control was clipped or
# covered by the panel; a bright maximum means the white icon glyph drew.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-narrow-width-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$FIXTURE" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
FIXTURE="$3"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

sleep 1

# Load the fixture clip via the command line so the welcome surface hides and
# the video plane is a clean dark background, and open the Up Next panel so the
# chrome is pinned (the OSC stays visible for the duration) and the side panel
# is the chrome element that can crowd the bar at narrow widths.
OKP_OPEN_SIDE_PANEL_ON_STARTUP=up-next \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 30s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

sleep 6

xdotool search --name "OK Player" >"$OUT_DIR/window.ids"
window_id="$(head -n1 "$OUT_DIR/window.ids")"
if [[ -z "$window_id" ]]; then
  echo "main window did not appear" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi

# Confirm the default geometry before shrinking, so a resize that silently
# no-ops is caught.
xwininfo -id "$window_id" >"$OUT_DIR/window-default.xwininfo"
default_width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window-default.xwininfo")"
default_height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window-default.xwininfo")"
if [[ "$default_width" != "1120" || "$default_height" != "680" ]]; then
  echo "unexpected default geometry: ${default_width}x${default_height}" >&2
  exit 1
fi

# Shrink to a narrow floor: 480x540 is well below the default 1120x680 (so the
# narrow-width surface is actually exercised) but wide enough that the side
# panel (316 px) still fits without its rows clipping
# off the left edge — the acceptance is "side-panel rows do not clip", so the
# floor must stay just clear of the panel's own minimum.
xdotool windowsize "$window_id" 480 540
sleep 1

xwininfo -id "$window_id" >"$OUT_DIR/window-narrow.xwininfo"
width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/window-narrow.xwininfo")"
height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/window-narrow.xwininfo")"
state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/window-narrow.xwininfo")"
if [[ "$state" != "IsViewable" ]]; then
  echo "narrow window not viewable: state=${state}" >&2
  exit 1
fi
if (( width >= 1000 )); then
  echo "resize to narrow floor did not shrink the window: ${width}x${height}" >&2
  exit 1
fi
if (( width < 400 )); then
  echo "narrow floor too small for the side panel to fit: ${width}x${height}" >&2
  exit 1
fi

import -window "$window_id" "$OUT_DIR/narrow.png"

# The OSC bar lives at the bottom (valign End, 18 px margins, ~50 px tall); its
# interior row sits roughly at y = height-66 .. height-18.
osc_top=$((height - 66))
osc_h=$((height - 18 - osc_top))

# The side panel is anchored flush to the right at the canonical 316 px width,
# so its horizontal extent is [width-316, width].
panel_left=$((width - 316))
panel_right=$width
panel_w=$((panel_right - panel_left))
panel_bottom=$((height - 80))

if (( panel_left < 0 || panel_right > width || panel_bottom > osc_top - 12 )); then
  echo "derived narrow layout overlaps: panel=[${panel_left},${panel_right}] bottom=${panel_bottom}, osc-top=${osc_top}" >&2
  exit 1
fi

# The unified OSC is one continuous elevated surface. Its full-width region
# must be visibly separated from the near-black video plane without relying on
# a single corner/background color.
osc_mean="$(
  magick "$OUT_DIR/narrow.png" \
    -crop "$((width - 28))x${osc_h}+14+${osc_top}" \
    -colorspace gray \
    -format '%[fx:mean]' info:
)"
if ! awk -v mean="$osc_mean" 'BEGIN { exit !(mean > 0.055) }'; then
  echo "OSC surface lacks contrast at narrow width: mean=${osc_mean}" >&2
  exit 1
fi

# Left controls not clipped: the primary group (open + transport) sits at the
# far left of the bar, left of the side panel. A bright glyph there means the
# leftmost controls drew and were not squeezed off-screen.
left_w=$((panel_left - 20))
left_max="$(
  magick "$OUT_DIR/narrow.png" \
    -crop ${left_w}x${osc_h}+20+${osc_top} \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$left_max" 'BEGIN { exit !(max > 0.4) }'; then
  echo "OSC left controls clipped at narrow width: maxima=${left_max}" >&2
  exit 1
fi

# No panel-over-OSC overlap: the side panel renders above the bar (z-order)
# with an 80 px bottom inset, so the OSC controls in the panel's horizontal
# extent stay visible. If that margin regresses the panel slides over the bar and
# dims/covers these glyphs — so this same band guards the overlap. It also
# catches the controls being clipped out of the panel's horizontal extent.
panel_osc_max="$(
  magick "$OUT_DIR/narrow.png" \
    -crop ${panel_w}x${osc_h}+${panel_left}+${osc_top} \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$panel_osc_max" 'BEGIN { exit !(max > 0.4) }'; then
  echo "OSC controls covered by side panel or clipped at narrow width: maxima=${panel_osc_max}" >&2
  exit 1
fi

# Side-panel rows not clipped: the panel header (title + tabs) sits at the
# top-right of the panel. A bright maximum there means the panel rows rendered
# and were not squeezed off-screen by the narrow width.
panel_header_x=$((panel_left + 28))
panel_header_w=$((panel_right - panel_header_x))
panel_max="$(
  magick "$OUT_DIR/narrow.png" \
    -crop ${panel_header_w}x76+${panel_header_x}+44 \
    -colorspace gray \
    -format '%[fx:maxima]' info:
)"
if ! awk -v max="$panel_max" 'BEGIN { exit !(max > 0.5) }'; then
  echo "side panel rows clipped at narrow width: maxima=${panel_max}" >&2
  exit 1
fi

echo "narrow floor: ${width}x${height}"
echo "osc-mean=${osc_mean} osc-left=${left_max} osc-panel=${panel_osc_max} panel=${panel_max}"
SMOKE
then
  echo "Narrow-width smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Narrow-width smoke passed. Screenshot: $OUT_DIR/narrow.png"
