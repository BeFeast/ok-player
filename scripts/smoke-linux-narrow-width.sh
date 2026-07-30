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
# from the preview-fixture smokes. The smoke uses a long dark H.264 stream so the
# delayed capture cannot outlive the video track and accidentally measure the
# idle surface. A near-black maximum in an OSC control band means a control was
# clipped or covered by the panel; a bright maximum means the white icon glyph
# drew.
#
# Readiness is a published condition, not a sleep (#704). The shell reports every
# plane's rectangle under OKP_DEBUG_INTERACTIONS (#690), so this waits until the
# app has published a settled record at the narrow width with the OSC and the
# side panel in it, and then re-captures a bounded number of times: layout being
# done is not the same fact as the X server having painted it, and a capture of
# an unpainted window used to read as a layout failure. Every rejected capture is
# kept next to the accepted one so a failure carries its own evidence.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-narrow-width-smoke}"
FIXTURE="${3:-}"

for tool in Xvfb dbus-run-session ffmpeg gdbus python3 xauth xprop flock mcookie xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if [[ -z "$FIXTURE" ]]; then
  FIXTURE="$OUT_DIR/dark.mkv"
  # Long enough that the readiness wait and the bounded capture retries below can
  # spend their whole budget on a loaded builder and still be measuring playback.
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i 'color=c=0x101010:s=640x360:r=2:d=120' \
    -c:v libx264 -preset ultrafast -tune stillimage -pix_fmt yuv420p -g 4 -an \
    "$FIXTURE"
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi

set +e
env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  "$ROOT/scripts/run-linux-isolated-xvfb-session.sh" \
  "$OUT_DIR/xvfb-session.txt" "$OUT_DIR/xvfb.log" \
  '-screen 0 1280x900x24 -nolisten tcp' \
  "$ROOT/scripts/run-linux-isolated-dbus-session.sh" "$OUT_DIR/dbus-session.txt" \
  bash -s -- "$BINARY" "$OUT_DIR" "$FIXTURE" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
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

# Narrow floor the window is resized to. Well below the default 1120x680 (so the
# narrow-width surface is actually exercised) but wide enough that the side panel
# (316 px) still fits without its rows clipping off the left edge — the
# acceptance is "side-panel rows do not clip", so the floor must stay just clear
# of the panel's own minimum.
NARROW_WIDTH=480
NARROW_HEIGHT=540

# How many fresh captures the assertions may be re-run over. Bounded on purpose:
# a slow paint costs a few hundred milliseconds, a real layout defect still fails
# — every attempt, with all of them kept.
CAPTURE_ATTEMPTS=15
CAPTURE_RETRY_DELAY=0.4

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

cleanup() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

window_manager_ready() {
  local state
  state="$(xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null || true)"
  [[ "$state" =~ window\ id\ \#\ 0x[1-9a-fA-F][0-9a-fA-F]* ]]
}

wm_ready=false
for attempt in $(seq 1 100); do
  if window_manager_ready; then
    printf 'attempt=%s\nready=true\n' "$attempt" >"$OUT_DIR/xfwm4-ready.txt"
    wm_ready=true
    break
  fi
  if ! kill -0 "$wm_pid" 2>/dev/null; then
    break
  fi
  sleep 0.05
done
if [[ "$wm_ready" != true ]]; then
  printf 'ready=false\n' >"$OUT_DIR/xfwm4-ready.txt"
  echo "Timed out waiting for Xfwm readiness" >&2
  exit "${OKP_SESSION_INFRA_EXIT_CODE:-75}"
fi

# --- Published geometry, the readiness signal --------------------------------

# Newest value of one key on one part of the record the shell publishes under
# OKP_DEBUG_INTERACTIONS. The reporter prints only when the snapshot changes, so
# the last line for a part is the state currently on screen.
geometry_field() {
  [[ -s "$OUT_DIR/app.log" ]] || return 0
  awk -v part="$1" -v key="$2" '
    index($0, "interaction: geometry part=" part " ") == 1 {
      for (i = 1; i <= NF; i++) {
        split($i, pair, "=")
        if (pair[1] == key) { value = pair[2] }
      }
    }
    END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
  ' "$OUT_DIR/app.log"
}

# Whether a plane belongs to the newest published record rather than to an older
# one. Every publish emits the whole record, so a plane that has since unmapped
# simply stops appearing — matching sequence numbers is what tells the two apart.
plane_is_current() {
  local window_seq plane_seq
  window_seq="$(geometry_field window seq)"
  plane_seq="$(geometry_field "$1" seq)"
  [[ -n "$window_seq" && -n "$plane_seq" && "$window_seq" == "$plane_seq" ]]
}

# The window has been mapped and has published at least one record.
window_is_mapped() {
  [[ -n "$(geometry_field window w)" ]]
}

# Whether media is still on screen. The welcome surface stays in the record while
# a file plays - it is mapped underneath - but it only becomes targetable once
# playback has ended and the idle canvas is back. That flag is what distinguishes
# the two surfaces, and it matters because every assertion below is about the OSC
# over a video plane: a capture taken after the fixture ran out would be measuring
# the welcome canvas instead. Callers may supply their own fixture, so the smoke
# checks the surface rather than trusting a duration.
playback_surface_is_live() {
  plane_is_current welcome || return 1
  [[ "$(geometry_field welcome interactive)" == "0" ]]
}

# The narrow surface this smoke is about: the window is down at the narrow floor
# and both surfaces the assertions read — the OSC bar and the pinned side panel —
# are in the record with real rectangles.
narrow_chrome_is_laid_out() {
  local window_width osc_width panel_width
  window_width="$(geometry_field window w)"
  [[ -n "$window_width" ]] || return 1
  (( window_width <= NARROW_WIDTH + 40 )) || return 1
  playback_surface_is_live || return 1
  plane_is_current osc || return 1
  plane_is_current side-panel || return 1
  osc_width="$(geometry_field osc w)"
  panel_width="$(geometry_field side-panel w)"
  [[ -n "$osc_width" && -n "$panel_width" ]] || return 1
  (( osc_width > 0 && panel_width > 0 ))
}

# Block until `predicate` holds over a record that has stopped changing. GTK
# settles a reflow over several frames, so a predicate that first holds mid-reflow
# is not yet the layout the assertions are about.
wait_for_geometry() {
  local label="$1" predicate="$2" budget="${3:-300}"
  local last="" current="" stable=0
  for _ in $(seq 1 "$budget"); do
    if "$predicate"; then
      current="$(geometry_field window seq)"
      if [[ -n "$current" && "$current" == "$last" ]]; then
        stable=$((stable + 1))
        if (( stable >= 5 )); then
          printf '%s: settled at seq=%s\n' "$label" "$current"
          return 0
        fi
      else
        stable=0
      fi
      last="$current"
    else
      stable=0
      last=""
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      echo "player exited while waiting for $label" >&2
      cat "$OUT_DIR/app.log" >&2 || true
      return 1
    fi
    sleep 0.1
  done
  echo "timed out waiting for $label" >&2
  # Name what the X server has, so a startup that never mapped is distinguishable
  # from one that mapped and never published.
  echo "windows X knows about: $(xdotool search --name 'OK Player' 2>/dev/null | tr '\n' ' ')" >&2
  tail -n 40 "$OUT_DIR/app.log" >&2 || true
  return 1
}

# Load the fixture clip via the command line so the welcome surface hides and
# the video plane is a clean dark background, and open the Up Next panel so the
# chrome is pinned (the OSC stays visible for the duration) and the side panel
# is the chrome element that can crowd the bar at narrow widths.
OKP_DEBUG_INTERACTIONS=1 \
OKP_OPEN_SIDE_PANEL_ON_STARTUP=up-next \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 180s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!

# Startup is the slowest, least predictable stretch - a software renderer on a
# loaded builder, media initialisation, and a package payload being paged in - so
# it gets a budget of its own rather than the one a settled reflow needs.
wait_for_geometry "startup layout" window_is_mapped 600

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

xdotool windowsize "$window_id" "$NARROW_WIDTH" "$NARROW_HEIGHT"
wait_for_geometry "narrow reflow" narrow_chrome_is_laid_out

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

left_w=$((panel_left - 20))
panel_header_x=$((panel_left + 28))
panel_header_w=$((panel_right - panel_header_x))
# Overflow entry (issue #328): the adaptive OSC folds lower-priority controls
# into the `…` menu, keeping the overflow entry as the final in-flow action at
# the far right; a dark seam must separate it from its neighbour.
#
# Both the entry and the seam are read from the rectangles the bar publishes,
# not computed from the window width. The bar spends two levers as the window
# narrows - gaps 16->8 and pill inset 14->8 (#730) - so an offset measured from
# the right edge lands on a glyph rather than in the gap at exactly the widths
# this smoke exercises, and then reports a missing seam that is really a
# mis-aimed crop.
#
# The record publishes window-local `local-x` and `w` (not `x`/`width`, which are
# screen coordinates), and the planes are indexed - `osc-overflow-0`. The capture
# below is `import -window`, so window-local is the frame the crops need.
overflow_x="$(geometry_field osc-overflow-0 local-x)"
overflow_w="$(geometry_field osc-overflow-0 w)"
if [[ -z "$overflow_x" || -z "$overflow_w" ]]; then
  echo "the bar published no osc-overflow rectangle; the overflow entry cannot be located" >&2
  exit 1
fi

# The seam is the gap the bar actually left: from the right edge of the nearest
# control left of the overflow entry to the entry itself, sampled in the middle.
neighbour_right=0
for plane in osc-play-0 osc-timeline-0 osc-volume-0 osc-slot-0; do
  nx="$(geometry_field "$plane" local-x)"
  nw="$(geometry_field "$plane" w)"
  [[ -n "$nx" && -n "$nw" ]] || continue
  right=$((nx + nw))
  if (( right <= overflow_x && right > neighbour_right )); then
    neighbour_right=$right
  fi
done
if (( neighbour_right == 0 )); then
  echo "no control left of the overflow entry published a rectangle" >&2
  exit 1
fi
gap=$((overflow_x - neighbour_right))
if (( gap < 4 )); then
  echo "the overflow entry shares bounds with its neighbour: gap=${gap}px" >&2
  exit 1
fi
if (( gap >= 8 )); then
  seam_w=4
else
  seam_w=2
fi
seam_x=$(( neighbour_right + (gap - seam_w) / 2 ))
echo "overflow=${overflow_x}+${overflow_w} neighbour-right=${neighbour_right} gap=${gap}px seam=${seam_x}+${seam_w}"

# --- The narrow-width acceptance, over one capture ----------------------------
#
# Prints the measured metrics and returns 0 when the capture satisfies every
# region guard; otherwise prints the first violated guard and returns 1. Every
# threshold and message here is the acceptance itself — only *which* capture it
# reads changed (#704).
evaluate_capture() {
  local image="$1"
  local osc_mean left_max panel_osc_max panel_max overflow_max seam_max

  # The unified OSC is one continuous elevated surface. Its full-width region
  # must be visibly separated from the near-black video plane without relying on
  # a single corner/background color.
  osc_mean="$(
    magick "$image" \
      -crop "$((width - 28))x${osc_h}+14+${osc_top}" \
      -colorspace gray \
      -format '%[fx:mean]' info:
  )"
  if ! awk -v mean="$osc_mean" 'BEGIN { exit !(mean > 0.055) }'; then
    echo "OSC surface lacks contrast at narrow width: mean=${osc_mean}"
    return 1
  fi

  # Left controls not clipped: the primary group (open + transport) sits at the
  # far left of the bar, left of the side panel. A bright glyph there means the
  # leftmost controls drew and were not squeezed off-screen.
  left_max="$(
    magick "$image" \
      -crop ${left_w}x${osc_h}+20+${osc_top} \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$left_max" 'BEGIN { exit !(max > 0.4) }'; then
    echo "OSC left controls clipped at narrow width: maxima=${left_max}"
    return 1
  fi

  # No panel-over-OSC overlap: the side panel renders above the bar (z-order)
  # with an 80 px bottom inset, so the OSC controls in the panel's horizontal
  # extent stay visible. If that margin regresses the panel slides over the bar
  # and dims/covers these glyphs — so this same band guards the overlap. It also
  # catches the controls being clipped out of the panel's horizontal extent.
  panel_osc_max="$(
    magick "$image" \
      -crop ${panel_w}x${osc_h}+${panel_left}+${osc_top} \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$panel_osc_max" 'BEGIN { exit !(max > 0.4) }'; then
    echo "OSC controls covered by side panel or clipped at narrow width: maxima=${panel_osc_max}"
    return 1
  fi

  # Side-panel rows not clipped: the panel header (title + tabs) sits at the
  # top-right of the panel. A bright maximum there means the panel rows rendered
  # and were not squeezed off-screen by the narrow width.
  panel_max="$(
    magick "$image" \
      -crop ${panel_header_w}x76+${panel_header_x}+44 \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$panel_max" 'BEGIN { exit !(max > 0.5) }'; then
    echo "side panel rows clipped at narrow width: maxima=${panel_max}"
    return 1
  fi

  # Overflow entry reachable and unobstructed at narrow width: it must render a
  # bright glyph (not be clipped) and must not share bounds with its neighbour —
  # a dark seam separates them.
  overflow_max="$(
    magick "$image" \
      -crop ${overflow_w}x${osc_h}+${overflow_x}+${osc_top} \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$overflow_max" 'BEGIN { exit !(max > 0.4) }'; then
    echo "overflow entry clipped or occluded at narrow width: maxima=${overflow_max}"
    return 1
  fi
  seam_max="$(
    magick "$image" \
      -crop ${seam_w}x${osc_h}+${seam_x}+${osc_top} \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$seam_max" 'BEGIN { exit !(max < 0.35) }'; then
    echo "no disjoint seam left of the overflow entry (neighbour shares bounds): maxima=${seam_max}"
    return 1
  fi

  printf 'osc-mean=%s osc-left=%s osc-panel=%s panel=%s\n' \
    "$osc_mean" "$left_max" "$panel_osc_max" "$panel_max"
  printf 'overflow-glyph=%s overflow-seam=%s\n' "$overflow_max" "$seam_max"
}

: >"$OUT_DIR/capture-attempts.txt"
accepted=""
metrics=""
attempt=0
while (( attempt < CAPTURE_ATTEMPTS )); do
  attempt=$((attempt + 1))
  capture="$OUT_DIR/narrow-attempt-${attempt}.png"
  if ! import -window "$window_id" "$capture"; then
    printf 'attempt=%s capture failed\n' "$attempt" >>"$OUT_DIR/capture-attempts.txt"
    sleep "$CAPTURE_RETRY_DELAY"
    continue
  fi
  if metrics="$(evaluate_capture "$capture")"; then
    accepted="$capture"
    printf 'attempt=%s accepted\n' "$attempt" >>"$OUT_DIR/capture-attempts.txt"
    break
  fi
  printf 'attempt=%s rejected: %s\n' "$attempt" "$metrics" >>"$OUT_DIR/capture-attempts.txt"
  if ! playback_surface_is_live; then
    echo "playback ended before a capture was accepted; the fixture is shorter than this smoke's readiness budget" >&2
    cat "$OUT_DIR/capture-attempts.txt" >&2
    exit 1
  fi
  sleep "$CAPTURE_RETRY_DELAY"
done

if [[ -z "$accepted" ]]; then
  echo "narrow-width acceptance failed on every one of ${CAPTURE_ATTEMPTS} captures" >&2
  cat "$OUT_DIR/capture-attempts.txt" >&2
  echo "captures kept:" >&2
  ls -1 "$OUT_DIR"/narrow-attempt-*.png >&2
  # The last capture is the one a reader wants first; keep it under the stable
  # name so the failure ships an image instead of only a number.
  cp "$OUT_DIR/narrow-attempt-${attempt}.png" "$OUT_DIR/narrow.png" 2>/dev/null || true
  exit 1
fi

cp "$accepted" "$OUT_DIR/narrow.png"
echo "narrow floor: ${width}x${height}"
echo "capture attempts: ${attempt}"
printf '%s\n' "$metrics"
SMOKE
session_status=$?
set -e
if (( session_status != 0 )); then
  echo "Narrow-width smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit "$session_status"
fi

echo "Narrow-width smoke passed. Screenshot: $OUT_DIR/narrow.png"
