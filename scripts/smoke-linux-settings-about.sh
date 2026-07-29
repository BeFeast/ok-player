#!/usr/bin/env bash
# X11/Xvfb regression for the Settings/About surface (#711).
#
# Three defects the operator reported at once, all of them geometry: the window opened at a
# constant height and cut its own page off while the desktop was still half empty; the rule
# over the rail's About entry and the rule over the page footer sat a couple of dozen pixels
# apart, so the eye read two nearly-aligned lines instead of one; and the footer row had no
# declared vertical alignment at all.
#
# All three are rectangles, so this checks numbers rather than pixels-by-eye: the Settings
# window publishes its own planes on the `interaction:` stream (#690, extended for this
# surface) and the assertions below subtract them. The window is opened on About on a
# 1080p-class screen and on a 4K screen, because a page that fits must open whole on both.
#
# The screenshot is not decoration either: the record could describe a widget that draws
# nothing, so both rules are also read back off the captured image.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

# One screen where the reference window is a small part of the desktop, and one where it is
# a very small part. About fits inside the work area of both, so on both it must open whole.
SCREENS=("1920x1080" "3840x2160")

if [[ "${1:-}" == "--inner" ]]; then
  shift
  BINARY="${1:?missing binary}"
  OUT_DIR="${2:?missing output directory}"
  LABEL="${3:?missing screen label}"

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
  mkdir -p "$XDG_STATE_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"

  # ImageMagick 7 renamed the entry point; the version Debian and Ubuntu package is 6.
  if command -v magick >/dev/null 2>&1; then
    magick_cmd=(magick)
  else
    magick_cmd=(convert)
  fi

  xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
  wm_pid=$!
  app_pid=""
  cleanup() {
    [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
    kill "$wm_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for _ in $(seq 1 100); do
    xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' && break
    sleep 0.05
  done
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' || {
    echo "xfwm4 did not become ready" >&2
    exit 75
  }

  # Newest value of one key on one part of the Settings geometry record, rounded to whole
  # pixels. Rounding is the "within a pixel" tolerance every claim below is stated with.
  field() {
    awk -v part="$1" -v key="$2" '
      index($0, "interaction: settings-geometry part=" part " ") == 1 {
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == key) { value = pair[2] }
        }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$3"
  }

  dump_settings_state() {
    local log="$1" part
    echo "--- Settings geometry as last reported ---" >&2
    echo "window: $(field window w "$log")x$(field window h "$log") \
work-area-h=$(field window work-area-h "$log") overflow=$(field window overflow "$log")" >&2
    for part in rail-rule content-rule footer-action footer-links content-column; do
      echo "$part: at $(field "$part" x "$log"),$(field "$part" y "$log") \
size $(field "$part" w "$log")x$(field "$part" h "$log") center-y=$(field "$part" center-y "$log")" >&2
    done
  }

  # Block until the record stops changing. The reporter publishes only on change, so a
  # sequence number that stops advancing is the layout settling; asserting before it has
  # would test whatever GTK happened to have allocated while the page was still building.
  wait_for_settled() {
    local log="$1" last="" current="" stable=0
    for _ in $(seq 1 240); do
      current="$(field window seq "$log")"
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

  : >"$OUT_DIR/results.txt"

  OKP_DEBUG_INTERACTIONS=1 \
  OKP_OPEN_SETTINGS_ON_STARTUP=1 \
  OKP_OPEN_SETTINGS_PAGE_ON_STARTUP=about \
  OKP_SETTINGS_COLOR_SCHEME=light \
  OKP_DISABLE_MPRIS=1 \
  OKP_SKIP_UPDATE_CHECK=1 \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 90s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!

  settings_id=""
  for _ in $(seq 1 160); do
    if xdotool search --onlyvisible --name '^Settings$' >"$OUT_DIR/settings.ids" 2>/dev/null \
      && [[ -s "$OUT_DIR/settings.ids" ]]; then
      while IFS= read -r candidate; do
        candidate_info="$(xwininfo -id "$candidate" 2>/dev/null || true)"
        candidate_width="$(awk '/Width:/ { print $2; exit }' <<<"$candidate_info")"
        candidate_state="$(awk -F': ' '/Map State:/ { print $2; exit }' <<<"$candidate_info")"
        if [[ "$candidate_width" -gt 1 && "$candidate_state" == "IsViewable" ]]; then
          settings_id="$candidate"
          break 2
        fi
      done <"$OUT_DIR/settings.ids"
    fi
    sleep 0.25
  done
  [[ -n "$settings_id" ]] || {
    echo "the Settings window never appeared" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
  xdotool windowactivate --sync "$settings_id" >/dev/null 2>&1 || true

  wait_for_settled "$OUT_DIR/app.log" || {
    echo "the Settings layout never settled" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }

  page="$(awk '
    index($0, "interaction: settings-geometry part=window ") == 1 {
      for (i = 1; i <= NF; i++) { split($i, pair, "="); if (pair[1] == "page") { value = pair[2] } }
    } END { print value }' "$OUT_DIR/app.log")"
  [[ "$page" == "about" ]] || {
    echo "Settings settled on '$page' rather than on About" >&2
    exit 1
  }

  window_h="$(field window h "$OUT_DIR/app.log")"
  work_area_h="$(field window work-area-h "$OUT_DIR/app.log")"
  overflow="$(field window overflow "$OUT_DIR/app.log")"
  for value in window_h work_area_h overflow; do
    [[ -n "${!value}" ]] || {
      echo "the Settings window published no $value" >&2
      exit 1
    }
  done

  # 1. The page opens whole. About is far shorter than either work area here, so any
  #    scrollable remainder means the window was sized by something other than its content.
  (( overflow == 0 )) || {
    echo "About opened with ${overflow}px of the page out of view on a ${LABEL} screen" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }
  # ... and inside the room it was given, so a panel or a dock stays uncovered.
  (( window_h <= work_area_h )) || {
    echo "the Settings window is ${window_h}px tall inside a ${work_area_h}px work area" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }

  rail_rule_y="$(field rail-rule y "$OUT_DIR/app.log")"
  content_rule_y="$(field content-rule y "$OUT_DIR/app.log")"
  rail_rule_x="$(field rail-rule x "$OUT_DIR/app.log")"
  content_rule_x="$(field content-rule x "$OUT_DIR/app.log")"
  action_center="$(field footer-action center-y "$OUT_DIR/app.log")"
  links_center="$(field footer-links center-y "$OUT_DIR/app.log")"
  action_x="$(field footer-action x "$OUT_DIR/app.log")"
  column_x="$(field content-column x "$OUT_DIR/app.log")"
  links_bottom=$(( $(field footer-links y "$OUT_DIR/app.log") + $(field footer-links h "$OUT_DIR/app.log") ))
  for value in rail_rule_y content_rule_y rail_rule_x content_rule_x \
    action_center links_center action_x column_x; do
    [[ -n "${!value}" ]] || {
      echo "the Settings window published no $value" >&2
      dump_settings_state "$OUT_DIR/app.log"
      exit 1
    }
  done

  # 2. The two rules share a baseline. They are insets of different columns, so only the
  #    baseline is shared - and a single rule spanning both columns would be a different
  #    design, not this one, so their x must still differ.
  rule_offset=$(( rail_rule_y - content_rule_y ))
  (( rule_offset >= -1 && rule_offset <= 1 )) || {
    echo "the rail rule sits at y=${rail_rule_y} and the page rule at y=${content_rule_y}" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }
  (( rail_rule_x != content_rule_x )) || {
    echo "both rules start at x=${rail_rule_x}; the two columns collapsed into one" >&2
    exit 1
  }

  # 3. The footer row shares one centre line, and starts at the content column's left edge.
  center_offset=$(( action_center - links_center ))
  (( center_offset >= -1 && center_offset <= 1 )) || {
    echo "the footer button centres on y=${action_center} and its links on y=${links_center}" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }
  left_offset=$(( action_x - column_x ))
  (( left_offset >= -1 && left_offset <= 1 )) || {
    echo "the footer starts at x=${action_x} against a content column at x=${column_x}" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }
  # Nothing the page ends with may fall below the window it opened at.
  (( links_bottom <= window_h )) || {
    echo "the footer links end at y=${links_bottom} inside a ${window_h}px window" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }

  # 4. The rules are painted where the record says they are. A rectangle can be reported
  #    for a widget that draws nothing, so read the captured window back.
  info="$(xwininfo -id "$settings_id")"
  win_x="$(awk '/Absolute upper-left X:/ { print $4; exit }' <<<"$info")"
  win_y="$(awk '/Absolute upper-left Y:/ { print $4; exit }' <<<"$info")"
  win_w="$(awk '/Width:/ { print $2; exit }' <<<"$info")"
  win_h="$(awk '/Height:/ { print $2; exit }' <<<"$info")"
  import -window root "$OUT_DIR/root.png"
  "${magick_cmd[@]}" "$OUT_DIR/root.png" -crop "${win_w}x${win_h}+${win_x}+${win_y}" +repage \
    "$OUT_DIR/settings-about.png"
  rm -f "$OUT_DIR/root.png"

  # A hairline is darker than the surface it separates. Six pixels above the rule is inside
  # the gap the band keeps clear, so the comparison is against blank surface either way.
  row_mean() {
    "${magick_cmd[@]}" "$OUT_DIR/settings-about.png" -crop "$1" +repage -colorspace gray \
      -format '%[fx:mean]' info:
  }
  assert_painted_rule() {
    local label="$1" x="$2" width="$3" y="$4" rule_mean above_mean
    rule_mean="$(row_mean "${width}x1+${x}+${y}")"
    above_mean="$(row_mean "${width}x1+${x}+$((y - 6))")"
    awk -v rule="$rule_mean" -v above="$above_mean" 'BEGIN { exit !(rule < above - 0.004) }' || {
      echo "$label: no rule is painted at y=${y} (row=${rule_mean}, surface above=${above_mean})" >&2
      return 1
    }
    printf '%s\n' "${label}_painted_at=${y}" "${label}_row_mean=${rule_mean}" \
      >>"$OUT_DIR/results.txt"
  }
  assert_painted_rule rail_rule "$rail_rule_x" "$(field rail-rule w "$OUT_DIR/app.log")" \
    "$rail_rule_y" || exit 1
  assert_painted_rule content_rule "$content_rule_x" \
    "$(field content-rule w "$OUT_DIR/app.log")" "$content_rule_y" || exit 1

  # 5. A height the reader chose outranks a height a page wants. Resize the window by hand
  #    and then page to a surface long enough to want the whole work area: the window must
  #    not move, because automatic sizing ends for the session the moment someone else sizes
  #    it. This is checked while the window is still shorter than that page wants, since a
  #    window already at the work-area cap has no growth left to be caught making.
  HAND_HEIGHT=620
  xdotool windowsize --sync "$settings_id" 760 "$HAND_HEIGHT"
  sleep 1
  # Shortcuts is the sixth rail row and is longer than every work area this runs on.
  xdotool mousemove --window "$settings_id" 90 302 click 1
  sleep 2
  wait_for_settled "$OUT_DIR/app.log" || {
    echo "the Settings layout never settled after a resize by hand" >&2
    exit 1
  }
  paged_page="$(awk '
    index($0, "interaction: settings-geometry part=window ") == 1 {
      for (i = 1; i <= NF; i++) { split($i, pair, "="); if (pair[1] == "page") { value = pair[2] } }
    } END { print value }' "$OUT_DIR/app.log")"
  paged_h="$(field window h "$OUT_DIR/app.log")"
  [[ "$paged_page" != "about" ]] || {
    echo "the rail click did not leave About; the resize regression tested nothing" >&2
    exit 1
  }
  (( paged_h == HAND_HEIGHT )) || {
    echo "a page change resized a hand-sized window from ${HAND_HEIGHT}px to ${paged_h}px" >&2
    dump_settings_state "$OUT_DIR/app.log"
    exit 1
  }

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$OUT_DIR/app.log"; then
    echo "the Settings/About smoke observed a fatal process diagnostic" >&2
    exit 1
  fi

  printf '%s\n' \
    "screen=${LABEL}" \
    "settled_page=${page}" \
    "window_height=${window_h}" \
    "work_area_height=${work_area_h}" \
    "content_overflow=${overflow}" \
    "rule_baseline_offset=${rule_offset}" \
    "footer_center_offset=${center_offset}" \
    "footer_left_offset=${left_offset}" \
    "hand_sized_height_kept=${paged_h}" \
    "hand_sized_page=${paged_page}" \
    'fatal_diagnostics=absent' \
    'status=pass' >>"$OUT_DIR/results.txt"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-settings-about-smoke}"

for tool in xfwm4 xdotool xprop xwininfo import awk timeout; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
command -v magick >/dev/null 2>&1 || command -v convert >/dev/null 2>&1 || {
  echo "Missing required tool: magick or convert" >&2
  exit 127
}
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

for screen in "${SCREENS[@]}"; do
  run_dir="$OUT_DIR/$screen"
  mkdir -p "$run_dir"
  __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
    "$ISOLATED_XVFB" \
    "$run_dir/xvfb-evidence.txt" \
    "$run_dir/xvfb.log" \
    "-screen 0 ${screen}x24 -nolisten tcp" \
    "$ISOLATED_DBUS" \
    "$run_dir/dbus-evidence.txt" \
    "$0" --inner "$BINARY" "$run_dir" "$screen"
done

echo "Settings/About smoke passed. Results: $OUT_DIR/*/results.txt"
