#!/usr/bin/env bash
# X11/Xvfb regression for the idle canvas at a narrow window, and for the geometry a
# playback session borrows from it (#716).
#
# The operator's report was one screenshot with two defects in it. Playing a portrait clip
# fits the window to the clip - correct while it plays - and leaving playback used to leave
# the idle Continue-watching surface inside that tall narrow shape, where it was simply
# cropped: the subtitle ran off the right edge and the settings button in the bottom bar was
# cut in half. Neither half of that substitutes for the other, so this guards both:
#
#   Part A  The idle canvas fits inside the window at every width down to the window's own
#           minimum. Not by hiding anything and not by refusing to be narrow - the portrait
#           fit is a feature - but by reflowing. Proven at the width the portrait fit
#           produces on this display, at a width below it, at the width the same clip would
#           get on the smallest desktop in the tester base, and at the canvas' own floor.
#   Part B  Leaving playback restores the geometry the window had before that media opened,
#           and a portrait file followed by a landscape file returns to the same one.
#
# Everything is read from the per-plane rectangles the geometry diagnostic publishes (#690)
# rather than from pixels, so a failure names the widget that left the window and by how
# much. Readiness is a published condition rather than a sleep (#704): the reporter emits
# only on change, so a sequence number that stops advancing is the layout settling.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

# How far below the portrait width the second narrow measurement sits. Enough to change the
# shelf's column count, so the check covers a reflow rather than one lucky width.
NARROWER_STEP=80

# Every part of the idle canvas that has to stay inside the window. The two footer buttons
# are indexed in on-screen order: 0 opens History, 1 is the settings button the operator's
# screenshot showed cut in half.
IDLE_PLANES=(
  welcome
  idle-headline-0
  idle-subtitle-0
  idle-shelf-0
  idle-actions-0
  idle-footer-0
  idle-footer-button-0
  idle-footer-button-1
  recent-card-0
  recent-card-1
  recent-card-2
  recents-more-0
)

# Card geometry the shelf lays out on, from history_view.rs. Used to work out how many
# columns a given shelf width has room for, which is the difference between a shelf that
# uses its width and one that abandons it.
RECENT_CARD_WIDTH=194
RECENT_GAP=14
WELCOME_ITEM_LIMIT=3

# The narrowest window a portrait clip can legitimately produce on hardware this player
# supports, and therefore the widest the idle canvas' own minimum is allowed to be. A
# 1366x768 laptop is the smallest desktop in the Debian/Ubuntu tester base; okp-core's fit
# spends WORK_AREA_FILL (0.94) of the work area and reserves PLAYER_CHROME_RESERVE (42) from
# its height, so a 9:16 clip lands in floor(floor(768 * 0.94) - 42) * 9 / 16 = 381px. The
# operator's report is what happens when the canvas insists on more than that: the fit is a
# feature, and putting a wider floor under the window to make the crop go away would defeat
# it rather than fix it.
SMALLEST_DESKTOP_WIDTH=1366
SMALLEST_DESKTOP_HEIGHT=768
PORTRAIT_FIT_FLOOR=381

if [[ "${1:-}" == "--inner" ]]; then
  shift
  BINARY="${1:?missing binary}"
  OUT_DIR="${2:?missing output directory}"

  export GDK_BACKEND=x11
  export GTK_USE_PORTAL=0
  export NO_AT_BRIDGE=1
  export XDG_SESSION_TYPE=x11
  export XDG_CURRENT_DESKTOP=XFCE
  export XDG_STATE_HOME="$OUT_DIR/state"
  export XDG_CONFIG_HOME="$OUT_DIR/config"
  export XDG_CACHE_HOME="$OUT_DIR/cache"
  export LIBGL_ALWAYS_SOFTWARE=1
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
  export OKP_SKIP_OPEN_INSTALLER=1
  export OKP_SKIP_DEB_SELF_INSTALL=1
  export OKP_SKIP_UPDATE_CHECK=1
  # Posters are placed by file stem, so the shelf renders real thumbnails without a decoder.
  export OKP_POSTER_FIXTURE_DIR="$OUT_DIR/posters"
  # Deliberately NOT OKP_FIXED_VIEWPORT_SMOKE: this smoke is about the production fit.

  xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
  wm_pid=$!
  app_pid=""
  cleanup() {
    [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
    kill "$wm_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for _ in $(seq 1 100); do
    if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; then
      break
    fi
    sleep 0.05
  done
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' || {
    echo "xfwm4 did not become ready" >&2
    exit 75
  }

  LOG="$OUT_DIR/app.log"
  RESULTS="$OUT_DIR/results.txt"
  : >"$RESULTS"

  # Newest value of one key on one plane of the geometry record, rounded to whole pixels.
  field() {
    awk -v part="$1" -v key="$2" '
      index($0, "interaction: geometry part=" part " ") == 1 {
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == key) { value = pair[2] }
        }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$LOG"
  }

  wait_for_marker() {
    local marker="$1" limit="${2:-160}"
    for _ in $(seq 1 "$limit"); do
      grep -q "$marker" "$LOG" && return 0
      sleep 0.25
    done
    echo "never observed: $marker" >&2
    tail -40 "$LOG" >&2
    return 1
  }

  # Block until the record stops changing. Asserting before it has would test whatever GTK
  # happened to have allocated mid-reflow.
  wait_for_settled() {
    local last="" current="" stable=0
    for _ in $(seq 1 240); do
      current="$(field window seq)"
      if [[ -n "$current" && "$current" == "$last" ]]; then
        stable=$((stable + 1))
        (( stable >= 8 )) && return 0
      else
        stable=0
      fi
      last="$current"
      sleep 0.25
    done
    return 1
  }

  dump_idle_planes() {
    echo "--- idle canvas as last reported (window $(field window w)x$(field window h)) ---" >&2
    for part in "${IDLE_PLANES[@]}"; do
      local w
      w="$(field "$part" w)"
      [[ -z "$w" ]] && continue
      echo "  $part: at $(field "$part" local-x),$(field "$part" local-y) size ${w}x$(field "$part" h)" >&2
    done
  }

  # Fail unless every reported part of the idle canvas is drawn inside the window. This is
  # the whole of the first defect: content wider than the window must wrap, ellipsize or
  # scroll, and must never be silently cut off.
  assert_nothing_clipped() {
    local label="$1" window_w present=0
    window_w="$(field window w)"
    [[ -n "$window_w" ]] || { echo "$label: the window published no rectangle" >&2; return 1; }

    for part in "${IDLE_PLANES[@]}"; do
      local x w right
      x="$(field "$part" local-x)"
      w="$(field "$part" w)"
      [[ -n "$x" && -n "$w" ]] || continue
      present=$((present + 1))
      right=$((x + w))
      # One pixel of slack absorbs the rounding in the reported rectangles; anything more is
      # a widget hanging outside the window.
      if (( x < -1 || right > window_w + 1 )); then
        echo "$label: $part spans ${x}..${right} outside a ${window_w}px window" >&2
        dump_idle_planes
        return 1
      fi
    done

    # A run that reported nothing would pass every check above, so require the surface to
    # have actually been on screen.
    (( present >= 8 )) || {
      echo "$label: only $present idle planes were reported; the surface was not up" >&2
      dump_idle_planes
      return 1
    }
    printf '%s\n' "${label}_window_width=${window_w}" "${label}_planes_inside=${present}" >>"$RESULTS"
  }

  # Fail unless the shelf laid out every column its width had room for, and unless those
  # cards are still the uniform tiles #702 established. A shelf that drops to one column
  # while two fit is the "wide empty gutter" side of the report.
  assert_shelf_uses_its_width() {
    local label="$1"
    local shelf_w row_y first_width index=0 on_row=0
    shelf_w="$(field idle-shelf-0 w)"
    row_y="$(field recent-card-0 local-y)"
    first_width="$(field recent-card-0 w)"
    [[ -n "$shelf_w" && -n "$row_y" && -n "$first_width" ]] || {
      echo "$label: the shelf published no rectangle" >&2
      return 1
    }

    while (( index < WELCOME_ITEM_LIMIT )); do
      local w y
      w="$(field "recent-card-$index" w)"
      y="$(field "recent-card-$index" local-y)"
      [[ -n "$w" && -n "$y" ]] || break
      (( w >= first_width - 1 && w <= first_width + 1 )) || {
        echo "$label: card $index is ${w}px wide but card 0 is ${first_width}px" >&2
        dump_idle_planes
        return 1
      }
      (( y == row_y )) && on_row=$((on_row + 1))
      index=$((index + 1))
    done

    local fits=0 used=0
    while :; do
      local next=$(( used == 0 ? RECENT_CARD_WIDTH : used + RECENT_GAP + RECENT_CARD_WIDTH ))
      (( next <= shelf_w )) || break
      used="$next"
      fits=$((fits + 1))
      (( fits >= WELCOME_ITEM_LIMIT )) && break
    done

    (( on_row == fits )) || {
      echo "$label: ${on_row} card(s) share the top row but a ${shelf_w}px shelf has room for ${fits}" >&2
      dump_idle_planes
      return 1
    }
    printf '%s\n' \
      "${label}_shelf_width=${shelf_w}" \
      "${label}_cards_on_row=${on_row}" \
      "${label}_card_width=${first_width}" >>"$RESULTS"
  }

  capture() {
    command -v import >/dev/null 2>&1 || return 0
    import -window "$window_id" "$OUT_DIR/$1.png" || true
  }

  resize_to() {
    local width="$1" height="$2"
    xdotool windowsize "$window_id" "$width" "$height"
    for _ in $(seq 1 80); do
      [[ "$(field window w)" == "$width" ]] && break
      sleep 0.25
    done
    wait_for_settled || {
      echo "the layout never settled at ${width}px" >&2
      return 1
    }
  }

  window_geometry() { printf '%sx%s\n' "$(field window w)" "$(field window h)"; }

  open_media() {
    local uri
    uri="$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve().as_uri())' "$1")"
    gdbus call --session \
      --dest org.mpris.MediaPlayer2.okplayer \
      --object-path /org/mpris/MediaPlayer2 \
      --method org.mpris.MediaPlayer2.Player.OpenUri \
      "$uri" >>"$OUT_DIR/open-calls.log"
  }

  timeout 300s env OKP_DEBUG_INTERACTIONS=1 OKP_DEBUG_WINDOW_FIT=1 \
    OKP_DEBUG_IDLE_RETURN_SMOKE=1 "$BINARY" >"$LOG" 2>&1 &
  app_pid=$!
  window_id=""
  for _ in $(seq 1 120); do
    window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | head -n1 || true)"
    [[ -n "$window_id" ]] && break
    sleep 0.25
  done
  [[ -n "$window_id" ]] || { cat "$LOG" >&2; exit 1; }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true

  # The shelf only exists once the welcome model has been built.
  shelf_ready=0
  for _ in $(seq 1 200); do
    if [[ -n "$(field recent-card-2 w)" ]]; then
      shelf_ready=1
      break
    fi
    sleep 0.25
  done
  (( shelf_ready == 1 )) || {
    echo "the shelf never published a card rectangle" >&2
    cat "$LOG" >&2
    exit 1
  }
  wait_for_settled || { echo "the idle layout never settled at the default width" >&2; exit 1; }

  idle_geometry="$(window_geometry)"
  capture 00-idle-default
  assert_nothing_clipped default || exit 1
  assert_shelf_uses_its_width default || exit 1
  echo "idle_geometry_before_playback=${idle_geometry}" >>"$RESULTS"

  # --- Part B: the geometry a playback session borrows ------------------------------------
  wait_for_marker 'startup launch lifecycle: player ready' || exit 1
  for _ in $(seq 1 120); do
    gdbus introspect --session --dest org.mpris.MediaPlayer2.okplayer \
      --object-path /org/mpris/MediaPlayer2 >/dev/null 2>&1 && break
    sleep 0.25
  done

  open_media "$OUT_DIR/media/portrait.mkv"
  wait_for_marker 'idle-return-smoke: file-loaded' || exit 1
  wait_for_marker 'window fit configure: ' || exit 1
  wait_for_settled || { echo "the window never settled on the portrait fit" >&2; exit 1; }
  portrait_geometry="$(window_geometry)"
  portrait_width="$(field window w)"
  portrait_height="$(field window h)"
  capture 01-playing-portrait
  (( portrait_height > portrait_width )) || {
    echo "the portrait clip did not produce a portrait window: ${portrait_geometry}" >&2
    exit 1
  }
  echo "portrait_fit_geometry=${portrait_geometry}" >>"$RESULTS"

  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  xdotool key --clearmodifiers x
  wait_for_marker 'idle-return-smoke: close-idle' || exit 1
  wait_for_settled || { echo "the window never settled after leaving playback" >&2; exit 1; }
  restored_geometry="$(window_geometry)"
  capture 02-idle-after-portrait
  [[ "$restored_geometry" == "$idle_geometry" ]] || {
    echo "leaving playback left the window at ${restored_geometry}, not the pre-playback ${idle_geometry}" >&2
    exit 1
  }
  assert_nothing_clipped after-portrait || exit 1
  echo "idle_geometry_after_portrait=${restored_geometry}" >>"$RESULTS"

  # A portrait file followed by a landscape file must not accumulate drift: the second
  # session has to return to the same geometry the first one did.
  open_media "$OUT_DIR/media/landscape.mkv"
  wait_for_settled || { echo "the window never settled on the landscape fit" >&2; exit 1; }
  landscape_geometry="$(window_geometry)"
  capture 03-playing-landscape
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  xdotool key --clearmodifiers x
  wait_for_settled || { echo "the window never settled after the second close" >&2; exit 1; }
  drifted_geometry="$(window_geometry)"
  capture 04-idle-after-landscape
  [[ "$drifted_geometry" == "$idle_geometry" ]] || {
    echo "a portrait file followed by a landscape file drifted to ${drifted_geometry}, not ${idle_geometry}" >&2
    exit 1
  }
  printf '%s\n' \
    "landscape_fit_geometry=${landscape_geometry}" \
    "idle_geometry_after_landscape=${drifted_geometry}" >>"$RESULTS"

  # --- Part A: the idle canvas at narrow widths -------------------------------------------
  # The portrait width the fit above actually produced, a width below it, and the floor the
  # window settles on when asked for far less than it can be. The last one is the "from the
  # window's own minimum upward" clause: whatever that minimum turns out to be, the canvas
  # has to fit inside it.
  narrower_width=$((portrait_width - NARROWER_STEP))
  resize_to "$portrait_width" 700 || exit 1
  capture 05-narrow-portrait-width
  assert_nothing_clipped portrait-width || exit 1
  assert_shelf_uses_its_width portrait-width || exit 1

  resize_to "$narrower_width" 700 || exit 1
  capture 06-below-portrait-width
  assert_nothing_clipped below-portrait-width || exit 1
  assert_shelf_uses_its_width below-portrait-width || exit 1

  # The width the same portrait clip would get on the smallest desktop in the tester base.
  # A CI screen is roomy, so measuring only at the fit this display happens to produce would
  # leave the operator's own case - a laptop, where the fit is 166px narrower - untested.
  resize_to "$PORTRAIT_FIT_FLOOR" 700 || exit 1
  capture 07-smallest-desktop-portrait-width
  assert_nothing_clipped smallest-desktop-portrait-width || exit 1
  assert_shelf_uses_its_width smallest-desktop-portrait-width || exit 1

  # "The window's own minimum" is not a number this script gets to pick. Ask for a width
  # nothing could honour and read back how wide the canvas insisted on being: that is the
  # floor, whatever the layout currently makes it. Then measure at exactly that width, so
  # the floor case cannot quietly stop being the floor when the layout changes.
  xdotool windowsize "$window_id" 120 700
  wait_for_settled || { echo "the layout never settled below the canvas floor" >&2; exit 1; }
  canvas_floor="$(field welcome w)"
  [[ -n "$canvas_floor" ]] || { echo "the idle canvas published no rectangle" >&2; exit 1; }

  # That floor is the whole of the second half of the report: a canvas that cannot be
  # narrower than PORTRAIT_FIT_FLOOR gets cropped by the very window a portrait clip fits
  # into on the smallest desktop this player supports, and no amount of reflow above that
  # width helps. The bound is derived rather than chosen - see the constant.
  (( canvas_floor <= PORTRAIT_FIT_FLOOR )) || {
    echo "the idle canvas cannot be narrower than ${canvas_floor}px, so a portrait fit on a" \
      "${SMALLEST_DESKTOP_WIDTH}x${SMALLEST_DESKTOP_HEIGHT} desktop (${PORTRAIT_FIT_FLOOR}px) would crop it" >&2
    dump_idle_planes
    exit 1
  }

  resize_to "$canvas_floor" 700 || exit 1
  capture 08-canvas-floor
  assert_nothing_clipped canvas-floor || exit 1
  assert_shelf_uses_its_width canvas-floor || exit 1
  echo "canvas_floor_width=${canvas_floor}" >>"$RESULTS"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$LOG"; then
    echo "idle narrow-canvas smoke observed a fatal process diagnostic" >&2
    exit 1
  fi
  echo 'fatal_diagnostics=absent' >>"$RESULTS"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-idle-narrow-canvas-smoke}"

for tool in xfwm4 xdotool xprop xwininfo awk timeout python3 ffmpeg gdbus dbus-run-session; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# The same hostile Continue-watching fixture the shelf smoke uses, so the surface under test
# is the one the operator sees rather than a friendly stand-in.
python3 "$ROOT/scripts/make-linux-recents-fixture.py" "$OUT_DIR"
python3 "$ROOT/scripts/make-linux-portrait-fixture.py" "$OUT_DIR"

__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  "$ISOLATED_XVFB" \
  "$OUT_DIR/xvfb-evidence.txt" \
  "$OUT_DIR/xvfb.log" \
  '-screen 0 1920x1080x24 -nolisten tcp' \
  "$ISOLATED_DBUS" \
  "$OUT_DIR/dbus-evidence.txt" \
  "$0" --inner "$BINARY" "$OUT_DIR"

echo "Idle narrow-canvas smoke passed. Results: $OUT_DIR/results.txt"
