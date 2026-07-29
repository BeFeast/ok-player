#!/usr/bin/env bash
# X11/Xvfb regression for the playback OSC at a narrow window (#729).
#
# The operator played a portrait clip, the window fit itself to the clip, and the control
# bar kept the width it wanted instead of the width it had: the volume control was cut in
# half at the window edge, the row carried on past it, and the `…` entry - the one place
# the folded controls live - was off screen entirely. #716 fixed the same defect class on
# the idle canvas; this is the playback chrome, which was never covered.
#
# The bar already folds its secondary controls (#328). What it could not do was fit: the
# floor it reported as its horizontal minimum was wider than a portrait-fit window, and GTK
# never allocates a widget less than the minimum it reports, so the surplus was handed
# straight back and clipped. So this guards two things:
#
#   Part A  Nothing in the OSC is ever drawn outside the window, and the controls the
#           pillars protect - transport, seek, volume and the `…` entry - are all still
#           there. Proven at the portrait width this display produces, at a width below it,
#           at the width the same clip gets on the smallest desktop in the tester base, and
#           at the bar's own floor.
#   Part B  That floor is narrow enough to be reachable: a bar that cannot be narrower than
#           the window a portrait clip fits into is cropped by the fit itself, and no amount
#           of reflow above that width helps.
#
# Everything is read from the per-plane rectangles the geometry diagnostic publishes (#690),
# so a failure names the control that left the window and by how much. Only the newest
# published snapshot is read, so a control that has folded is genuinely absent rather than
# remembered from a wider window. Readiness is a published condition rather than a sleep
# (#704).
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

# How far below the portrait width the second narrow measurement sits. Enough to fold
# another control, so the check covers a reflow rather than one lucky width.
NARROWER_STEP=60

# The controls the four pillars protect. They are the floor of the collapse order: playback
# control outranks secondary affordances, so transport and seek survive longest, and the
# volume must never be dropped without a home - here it is never dropped at all.
REQUIRED_CONTROLS=(osc-pill-0 osc-play-0 osc-timeline-0 osc-volume-0 osc-overflow-0)

# Horizontal margin the pill is inset from the window on each side (controls_bar in
# controls.rs). The pill's own floor plus both margins is the narrowest window that can hold
# the bar.
OSC_MARGIN=16

# The narrowest window a portrait clip can legitimately produce on hardware this player
# supports. A 1366x768 laptop is the smallest desktop in the Debian/Ubuntu tester base;
# okp-core's fit spends WORK_AREA_FILL (0.94) of the work area and reserves
# PLAYER_CHROME_RESERVE (42) from its height, so a 9:16 clip lands in
# floor(floor(768 * 0.94) - 42) * 9 / 16 = 381px. Derived identically in
# smoke-linux-idle-narrow-canvas.sh, for the same reason: the fit is a feature, and putting
# a wider floor under the window to stop the crop would defeat it rather than fix it.
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

  # Sequence number of the newest published snapshot. The reporter emits the whole window
  # on every change, so one sequence number is one consistent picture of the layout.
  newest_seq() {
    awk '
      index($0, "interaction: geometry part=window ") == 1 {
        for (i = 1; i <= NF; i++) { split($i, pair, "="); if (pair[1] == "seq") { seq = pair[2] } }
      }
      END { print seq }
    ' "$LOG"
  }

  # One key of one plane, from the newest snapshot only, rounded to whole pixels. A plane
  # that is not in that snapshot returns empty - which is how a folded control, unmapped by
  # the bar, reports that it is no longer on screen.
  field() {
    awk -v part="$1" -v key="$2" -v want="$3" '
      index($0, "interaction: geometry part=" part " ") == 1 {
        seq = ""; found = ""
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == "seq") { seq = pair[2] }
          if (pair[1] == key) { found = pair[2] }
        }
        if (want == "" || seq == want) { value = found }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$LOG"
  }

  # Every OSC plane present in the newest snapshot, in publication order.
  osc_parts() {
    local seq="$1"
    awk -v want="$seq" '
      index($0, "interaction: geometry part=osc") == 1 {
        part = ""; seq = ""
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == "part") { part = pair[2] }
          if (pair[1] == "seq") { seq = pair[2] }
        }
        if (seq == want && part != "osc") { print part }
      }
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
      current="$(newest_seq)"
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

  dump_osc() {
    local seq="$1"
    echo "--- OSC as last reported (window $(field window w "$seq")x$(field window h "$seq")) ---" >&2
    local part
    while read -r part; do
      [[ -z "$part" ]] && continue
      echo "  $part: at $(field "$part" local-x "$seq"),$(field "$part" local-y "$seq")" \
        "size $(field "$part" w "$seq")x$(field "$part" h "$seq")" >&2
    done < <(osc_parts "$seq")
  }

  # The whole of the defect: a control wider than the room it has must fold or shorten, and
  # must never be drawn past the window edge where it cannot be clicked.
  assert_osc_fits() {
    local label="$1" seq window_w present=0
    seq="$(newest_seq)"
    window_w="$(field window w "$seq")"
    [[ -n "$window_w" ]] || { echo "$label: the window published no rectangle" >&2; return 1; }

    local part x w right
    while read -r part; do
      [[ -z "$part" ]] && continue
      x="$(field "$part" local-x "$seq")"
      w="$(field "$part" w "$seq")"
      [[ -n "$x" && -n "$w" ]] || continue
      present=$((present + 1))
      right=$((x + w))
      # One pixel of slack absorbs the rounding in the reported rectangles; anything more is
      # a control hanging outside the window.
      if (( x < -1 || right > window_w + 1 )); then
        echo "$label: $part spans ${x}..${right} outside a ${window_w}px window" >&2
        dump_osc "$seq"
        return 1
      fi
    done < <(osc_parts "$seq")

    # A run that reported nothing would pass the loop above, so require the controls the
    # pillars protect to have actually been on screen. This is also what keeps the fix
    # honest: folding the volume away to make the row fit would satisfy "nothing outside the
    # window" and fail here, because a hidden volume has no home in this bar.
    local required
    for required in "${REQUIRED_CONTROLS[@]}"; do
      [[ -n "$(field "$required" w "$seq")" ]] || {
        echo "$label: $required is not on screen at ${window_w}px" >&2
        dump_osc "$seq"
        return 1
      }
    done

    printf '%s\n' \
      "${label}_window_width=${window_w}" \
      "${label}_osc_planes=${present}" \
      "${label}_pill_width=$(field osc-pill-0 w "$seq")" \
      "${label}_seek_width=$(field osc-timeline-0 w "$seq")" >>"$RESULTS"
  }

  capture() {
    command -v import >/dev/null 2>&1 || return 0
    import -window "$window_id" "$OUT_DIR/$1.png" || true
  }

  # The bar hides itself while a clip plays; keep the pointer inside the window so the
  # chrome is up whatever the auto-hide timer is doing.
  nudge() { xdotool mousemove --window "$window_id" 40 40 >/dev/null 2>&1 || true; }

  resize_to() {
    local width="$1" height="$2"
    xdotool windowsize "$window_id" "$width" "$height"
    for _ in $(seq 1 80); do
      [[ "$(field window w "$(newest_seq)")" == "$width" ]] && break
      nudge
      sleep 0.25
    done
    nudge
    wait_for_settled || {
      echo "the layout never settled at ${width}px" >&2
      return 1
    }
  }

  open_media() {
    local uri
    uri="$(python3 -c 'from pathlib import Path; import sys; print(Path(sys.argv[1]).resolve().as_uri())' "$1")"
    gdbus call --session \
      --dest org.mpris.MediaPlayer2.okplayer \
      --object-path /org/mpris/MediaPlayer2 \
      --method org.mpris.MediaPlayer2.Player.OpenUri \
      "$uri" >>"$OUT_DIR/open-calls.log"
  }

  timeout 300s env OKP_DEBUG_INTERACTIONS=1 OKP_DEBUG_WINDOW_FIT=1 OKP_DEBUG_OSC_LAYOUT=1 \
    "$BINARY" >"$LOG" 2>&1 &
  app_pid=$!
  window_id=""
  for _ in $(seq 1 120); do
    window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | head -n1 || true)"
    [[ -n "$window_id" ]] && break
    sleep 0.25
  done
  [[ -n "$window_id" ]] || { cat "$LOG" >&2; exit 1; }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true

  wait_for_marker 'startup launch lifecycle: player ready' || exit 1
  for _ in $(seq 1 120); do
    gdbus introspect --session --dest org.mpris.MediaPlayer2.okplayer \
      --object-path /org/mpris/MediaPlayer2 >/dev/null 2>&1 && break
    sleep 0.25
  done

  open_media "$OUT_DIR/media/portrait.mkv"
  wait_for_marker 'window fit configure: ' || exit 1
  # A paused clip is the state the operator screenshotted, and it is also the state in which
  # the chrome does not auto-hide, so the bar can be measured through a resize.
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  xdotool key --clearmodifiers space
  nudge
  wait_for_settled || { echo "the window never settled on the portrait fit" >&2; exit 1; }

  portrait_width="$(field window w "$(newest_seq)")"
  portrait_height="$(field window h "$(newest_seq)")"
  [[ -n "$portrait_width" ]] || { echo "the window published no rectangle" >&2; exit 1; }
  (( portrait_height > portrait_width )) || {
    echo "the portrait clip did not produce a portrait window: ${portrait_width}x${portrait_height}" >&2
    exit 1
  }
  capture 00-portrait-fit
  assert_osc_fits portrait-fit || exit 1
  echo "portrait_fit_geometry=${portrait_width}x${portrait_height}" >>"$RESULTS"

  # --- Part A: the bar at narrow widths ----------------------------------------------------
  resize_to $((portrait_width - NARROWER_STEP)) 700 || exit 1
  capture 01-below-portrait-width
  assert_osc_fits below-portrait-width || exit 1

  # The width the same portrait clip gets on the smallest desktop in the tester base. A CI
  # screen is roomy, so measuring only at the fit this display happens to produce would leave
  # the operator's own case - a laptop, where the fit is much narrower - untested.
  resize_to "$PORTRAIT_FIT_FLOOR" 700 || exit 1
  capture 02-smallest-desktop-portrait-width
  assert_osc_fits smallest-desktop-portrait-width || exit 1

  # --- Part B: the bar's own floor ---------------------------------------------------------
  # "The narrowest the bar can be" is not a number this script gets to pick. Ask for a width
  # nothing could honour and read back how wide the pill insisted on being: that is the floor,
  # whatever the layout currently makes it.
  xdotool windowsize "$window_id" 120 700
  nudge
  wait_for_settled || { echo "the layout never settled below the bar's floor" >&2; exit 1; }
  pill_floor="$(field osc-pill-0 w "$(newest_seq)")"
  [[ -n "$pill_floor" ]] || { echo "the OSC published no rectangle" >&2; exit 1; }
  osc_floor=$((pill_floor + OSC_MARGIN * 2))

  # A bar that cannot be narrower than the window a portrait clip fits into on the smallest
  # desktop this player supports is cropped by the fit itself. The bound is derived rather
  # than chosen - see the constant.
  (( osc_floor <= PORTRAIT_FIT_FLOOR )) || {
    echo "the OSC cannot be narrower than ${osc_floor}px, so a portrait fit on a" \
      "${SMALLEST_DESKTOP_WIDTH}x${SMALLEST_DESKTOP_HEIGHT} desktop (${PORTRAIT_FIT_FLOOR}px) would clip it" >&2
    dump_osc "$(newest_seq)"
    exit 1
  }

  resize_to "$osc_floor" 700 || exit 1
  capture 03-osc-floor
  assert_osc_fits osc-floor || exit 1
  printf '%s\n' "osc_floor_width=${osc_floor}" "osc_pill_floor=${pill_floor}" >>"$RESULTS"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$LOG"; then
    echo "OSC narrow-bar smoke observed a fatal process diagnostic" >&2
    exit 1
  fi
  echo 'fatal_diagnostics=absent' >>"$RESULTS"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-osc-narrow-bar-smoke}"

for tool in xfwm4 xdotool xprop awk timeout python3 ffmpeg gdbus dbus-run-session; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# The same portrait clip the idle-geometry smoke plays, so both surfaces are measured against
# the one shape the operator actually reported.
python3 "$ROOT/scripts/make-linux-portrait-fixture.py" "$OUT_DIR"

__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  "$ISOLATED_XVFB" \
  "$OUT_DIR/xvfb-evidence.txt" \
  "$OUT_DIR/xvfb.log" \
  '-screen 0 1920x1080x24 -nolisten tcp' \
  "$ISOLATED_DBUS" \
  "$OUT_DIR/dbus-evidence.txt" \
  "$0" --inner "$BINARY" "$OUT_DIR"

echo "OSC narrow-bar smoke passed. Results: $OUT_DIR/results.txt"
