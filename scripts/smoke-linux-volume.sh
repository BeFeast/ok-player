#!/usr/bin/env bash
# Deterministic canonical volume-control captures for issue #262.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-volume-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"
read -r -a BACKGROUNDS <<<"${OKP_VOLUME_BACKGROUNDS:-light bright dark}"
INTERACTION_ONLY="${OKP_VOLUME_INTERACTION_ONLY:-0}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick compare jq gdbus; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ "$BINARY" == */* && "$BINARY" != /* ]]; then
  BINARY="$ROOT/$BINARY"
elif [[ "$BINARY" != */* ]]; then
  BINARY="$(command -v "$BINARY")"
fi
if [[ ! -x "$BINARY" ]]; then
  echo "Player binary is not executable: $BINARY" >&2
  exit 127
fi
if [[ ! -f "$FIXTURE" ]]; then
  echo "Missing media fixture: $FIXTURE" >&2
  exit 127
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/config/ok-player" "$OUT_DIR/state"
cat >"$OUT_DIR/config/ok-player/settings.json" <<'JSON'
{
  "version": 2,
  "playback": { "volume": 78.0 },
  "updates": { "auto_check": false }
}
JSON

capture_state() {
  local background="$1" state="$2"
  local shot="$OUT_DIR/$background-$state.png"
  local log="$OUT_DIR/app-$background-$state.log"

  if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
    dbus-run-session -- bash -s -- \
      "$BINARY" "$FIXTURE" "$OUT_DIR" "$background" "$state" "$shot" \
      >"$log" 2>&1 <<'CAPTURE'
set -euo pipefail
BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"
BACKGROUND="$4"
STATE="$5"
SHOT="$6"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export OKP_PLAYBACK_FRAME_PREVIEW="$BACKGROUND"
export OKP_VOLUME_PREVIEW="$STATE"
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1

stop_process() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  for _ in {1..20}; do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.05
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
}

xfwm4 --sm-client-disable >/dev/null 2>&1 &
wm_pid=$!
"$BINARY" "$FIXTURE" &
app_pid=$!

window_id=""
for _ in {1..24}; do
  window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | tail -n1 || true)"
  [[ -n "$window_id" ]] && break
  sleep 0.5
done
if [[ -z "$window_id" ]]; then
  echo "main window did not appear" >&2
  exit 1
fi

xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 1
xdotool key --window "$window_id" --clearmodifiers space
sleep 1

geometry="$(xwininfo -id "$window_id")"
width="$(awk '/Width:/ { print $2; exit }' <<<"$geometry")"
height="$(awk '/Height:/ { print $2; exit }' <<<"$geometry")"
x="$(awk '/Absolute upper-left X:/ { print $4; exit }' <<<"$geometry")"
y="$(awk '/Absolute upper-left Y:/ { print $4; exit }' <<<"$geometry")"
if [[ "$width" != "1120" || "$height" != "680" ]]; then
  echo "unexpected geometry ${width}x${height}" >&2
  exit 1
fi
root_shot="${SHOT%.png}-root.png"
import -window root "$root_shot"
magick "$root_shot" -crop "${width}x${height}+${x}+${y}" +repage "$SHOT"
rm -f "$root_shot"
stop_process "$app_pid"
stop_process "$wm_pid"
CAPTURE
  then
    echo "Failed to capture $background-$state. Log: $log" >&2
    cat "$log" >&2
    exit 1
  fi
}

if [[ "$INTERACTION_ONLY" != "1" ]]; then
  for background in "${BACKGROUNDS[@]}"; do
    for state in rest focus zero normal unity boost muted; do
      capture_state "$background" "$state"
    done
  done
fi

run_interaction_verification() {
  local log="$OUT_DIR/app-interaction.log"

  cat >"$OUT_DIR/config/ok-player/settings.json" <<'JSON'
{
  "version": 2,
  "playback": { "volume": 78.0 },
  "updates": { "auto_check": false }
}
JSON

  if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
    dbus-run-session -- bash -s -- \
      "$BINARY" "$FIXTURE" "$OUT_DIR" \
      >"$log" 2>&1 <<'INTERACTION'
set -euo pipefail
BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"
SETTINGS="$OUT_DIR/config/ok-player/settings.json"
REPORT="$OUT_DIR/interaction-results.txt"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export OKP_PLAYBACK_FRAME_PREVIEW=light
export OKP_DEBUG_INTERACTIONS=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1

cleanup() {
  stop_process "${app_pid:-}"
  stop_process "${wm_pid:-}"
}
trap cleanup EXIT

stop_process() {
  local pid="$1"
  [[ -n "$pid" ]] || return 0
  kill "$pid" 2>/dev/null || true
  for _ in {1..20}; do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.05
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -KILL "$pid" 2>/dev/null || true
  fi
  wait "$pid" 2>/dev/null || true
}

xfwm4 --sm-client-disable >/dev/null 2>&1 &
wm_pid=$!
"$BINARY" "$FIXTURE" &
app_pid=$!

window_id=""
for _ in {1..24}; do
  window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | tail -n1 || true)"
  [[ -n "$window_id" ]] && break
  sleep 0.5
done
if [[ -z "$window_id" ]]; then
  echo "main window did not appear" >&2
  exit 1
fi

xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
sleep 1
xdotool key --window "$window_id" --clearmodifiers space
sleep 1

geometry="$(xwininfo -id "$window_id")"
width="$(awk '/Width:/ { print $2; exit }' <<<"$geometry")"
height="$(awk '/Height:/ { print $2; exit }' <<<"$geometry")"
x="$(awk '/Absolute upper-left X:/ { print $4; exit }' <<<"$geometry")"
y="$(awk '/Absolute upper-left Y:/ { print $4; exit }' <<<"$geometry")"
if [[ "$width" != "1120" || "$height" != "680" ]]; then
  echo "unexpected geometry ${width}x${height}" >&2
  exit 1
fi

capture_window() {
  local shot="$1" root_shot="${1%.png}-root.png"
  import -window root "$root_shot"
  magick "$root_shot" -crop "${width}x${height}+${x}+${y}" +repage "$shot"
  rm -f "$root_shot"
}

settings_volume() {
  jq -er '.playback.volume' "$SETTINGS" 2>/dev/null
}

mpris_volume() {
  local ratio
  ratio="$(gdbus call --session \
    --dest org.mpris.MediaPlayer2.okplayer \
    --object-path /org/mpris/MediaPlayer2 \
    --method org.freedesktop.DBus.Properties.Get \
    org.mpris.MediaPlayer2.Player Volume 2>/dev/null \
    | sed -E 's/.*<([-+0-9.eE]+)>.*/\1/')"
  awk -v ratio="$ratio" 'BEGIN { printf "%.3f", ratio * 100.0 }'
}

wait_for_volume() {
  local expected="$1" label="$2" settings="" observed=""
  for _ in {1..50}; do
    settings="$(settings_volume || true)"
    observed="$(mpris_volume || true)"
    if awk -v expected="$expected" -v settings="$settings" -v observed="$observed" \
      'BEGIN { exit !(settings != "" && observed != "" &&
        (settings - expected) < 0.051 && (expected - settings) < 0.051 &&
        (observed - expected) < 0.051 && (expected - observed) < 0.051) }'; then
      printf '%s: settings=%.3f observed-mpv=%.3f\n' \
        "$label" "$settings" "$observed" >>"$REPORT"
      return 0
    fi
    sleep 0.1
  done
  echo "$label did not settle at $expected (settings=$settings observed-mpv=$observed)" >&2
  return 1
}

wait_for_settled_range() {
  local settings="" observed=""
  for _ in {1..50}; do
    settings="$(settings_volume || true)"
    observed="$(mpris_volume || true)"
    if awk -v settings="$settings" -v observed="$observed" \
      'BEGIN { exit !(settings > 0.5 && settings < 129.5 &&
        (settings - observed) < 0.051 && (observed - settings) < 0.051) }'; then
      printf '%s\n' "$settings"
      return 0
    fi
    sleep 0.1
  done
  echo "pointer-selected level did not settle (settings=$settings observed-mpv=$observed)" >&2
  return 1
}

: >"$REPORT"
wait_for_volume 78.0 initial

xdotool mousemove --sync --window "$window_id" 100 100
capture_window "$OUT_DIR/interaction-rest.png"
xdotool mousemove --sync --window "$window_id" 660 629
sleep 0.4
capture_window "$OUT_DIR/interaction-open.png"

# Five rapid pointer-wheel events complete inside one 200ms poll interval. The
# final 83% assertion proves stale observations cannot reset the nudge base.
xdotool click --repeat 5 --delay 20 4
wait_for_volume 83.0 rapid-pointer-wheel

xdotool keydown Shift_L
xdotool click 4
xdotool keyup Shift_L
wait_for_volume 83.1 shift-fine-wheel

# Enter exact-value editing through the real readout, replace the selected text with
# a changed decimal, and press Return. This proves keyboard events reach GtkEntry's
# internal editable child and activation persists through mpv/MPRIS.
xdotool mousemove --sync --window "$window_id" 745 587
xdotool click 1
# Xvfb finalizes the popup's native focus on the next seat event. Prime it with a
# modifier so the first editable key is never sacrificed to that transition.
sleep 2
xdotool key --clearmodifiers Shift_R
sleep 1
xdotool key --clearmodifiers ctrl+a
xdotool type --clearmodifiers --delay 40 '54.7'
xdotool key --clearmodifiers Return
wait_for_volume 54.7 exact-input-return-commit

# Reopen the editor, type a different decimal, and click Play directly. The changed
# value must commit on blur without reclaiming focus; Play retains the click target
# and the capsule completes its timed collapse.
xdotool mousemove --sync --window "$window_id" 676 638
sleep 0.4
xdotool mousemove --sync --window "$window_id" 745 587
xdotool click 1
sleep 0.5
xdotool key --clearmodifiers Shift_R
sleep 0.5
xdotool key --clearmodifiers ctrl+a
xdotool type --clearmodifiers --delay 40 '88.4'
xdotool mousemove --sync --window "$window_id" 52 629
xdotool click 1
wait_for_volume 88.4 exact-input-blur-commit
sleep 0.5

# Pointer selection establishes a high real value before wheel and keyboard clamp checks.
xdotool mousemove --sync --window "$window_id" 676 638
sleep 0.4
xdotool mousemove --sync --window "$window_id" 702 579
xdotool click 1
upper_selected="$(wait_for_settled_range)"
if ! awk -v level="$upper_selected" 'BEGIN { exit !(level > 120.0) }'; then
  echo "pointer high selection was unexpectedly low: $upper_selected" >&2
  exit 1
fi
printf 'pointer-high-selection: settings=%.3f observed-mpv=%.3f\n' \
  "$upper_selected" "$(mpris_volume)" >>"$REPORT"

# Traverse real GTK focus until the volume button receives Right and applies +1%.
arrow_expected="$(awk -v base="$upper_selected" 'BEGIN { printf "%.3f", base + 1.0 }')"
focused_tab=""
for tab_index in {1..24}; do
  xdotool key --clearmodifiers Tab
  xdotool key --clearmodifiers Right
  sleep 0.15
  current="$(settings_volume)"
  if awk -v current="$current" -v expected="$arrow_expected" \
    'BEGIN { exit !((current - expected) < 0.051 && (expected - current) < 0.051) }'; then
    focused_tab="$tab_index"
    break
  fi
done
if [[ -z "$focused_tab" ]]; then
  echo "Tab traversal did not reach the volume arrow-key handler" >&2
  exit 1
fi
wait_for_volume "$arrow_expected" keyboard-arrow-step
printf 'keyboard-focus-traversal: tabs=%s\n' "$focused_tab" >>"$REPORT"

xdotool mousemove --sync --window "$window_id" 660 629
xdotool click --repeat 5 --delay 20 4
wait_for_volume 130.0 wheel-upper-clamp

# The focused volume button uses the same 1% arrow step and shared clamp.
xdotool key --clearmodifiers --repeat 140 --delay 5 Left
wait_for_volume 0.0 keyboard-lower-clamp
xdotool key --clearmodifiers --repeat 3 --delay 20 Left
wait_for_volume 0.0 keyboard-repeat-lower-clamp

xdotool keydown Shift_L
xdotool click 4
xdotool keyup Shift_L
wait_for_volume 0.1 focused-shift-fine-wheel

# The real M shortcut must mute and restore the last nonzero projected level.
restore_level="$(settings_volume)"
xdotool key --clearmodifiers m
wait_for_volume 0.0 keyboard-mute
xdotool key --clearmodifiers m
wait_for_volume "$restore_level" keyboard-mute-restore
INTERACTION
  then
    echo "Failed real volume interaction verification. Log: $log" >&2
    cat "$log" >&2
    exit 1
  fi

  if ! awk '
    /interaction: volume-exact-focus=entry/ {
      focus_count += 1
      if (focus_count == 1) first_focus = NR
      if (focus_count == 2) second_focus = NR
    }
    /interaction: volume-exact-commit=activate/ { activate = NR }
    /interaction: volume-exact-commit=blur/ { blur = NR }
    /interaction: outside-target=play-focused/ { target = NR }
    /interaction: volume-capsule=closed/ {
      if (first_focus && activate > first_focus && second_focus > activate &&
          blur > second_focus && target > blur && NR > target) passed = 1
    }
    END { exit !passed }
  ' "$log"; then
    echo "Exact-volume Return/blur routing or outside-target collapse ordering failed" >&2
    cat "$log" >&2
    exit 1
  fi
  printf 'exact-input-return: decimal persisted through activation\n' \
    >>"$OUT_DIR/interaction-results.txt"
  printf 'exact-input-blur-collapse: changed decimal persisted, outside target focused, capsule closed\n' \
    >>"$OUT_DIR/interaction-results.txt"

  magick "$OUT_DIR/interaction-rest.png" -crop 382x49+706+613 "$OUT_DIR/interaction-rest-row.png"
  magick "$OUT_DIR/interaction-open.png" -crop 382x49+706+613 "$OUT_DIR/interaction-open-row.png"
  set +e
  local row_diff
  row_diff="$(compare -metric AE "$OUT_DIR/interaction-rest-row.png" "$OUT_DIR/interaction-open-row.png" null: 2>&1)"
  local compare_status=$?
  set -e
  if [[ $compare_status -gt 1 ]] || ! awk -v diff="$row_diff" 'BEGIN { exit !(diff + 0 < 5) }'; then
    echo "real hover opening reflowed the OSC row: diff=$row_diff" >&2
    exit 1
  fi
  printf 'actual-hover-row-reflow: changed-pixels=%s\n' "${row_diff%% *}" >>"$OUT_DIR/interaction-results.txt"
}

run_interaction_verification

color_fraction() {
  local image="$1" color="$2"
  magick "$image" -crop 1088x150+16+510 -alpha off -fuzz 8% \
    -transparent "$color" -alpha extract -negate -format '%[fx:mean]' info:
}

if [[ "$INTERACTION_ONLY" != "1" ]]; then
  for background in "${BACKGROUNDS[@]}"; do
    teal="$(color_fraction "$OUT_DIR/$background-normal.png" '#28B3AA')"
    amber_boost="$(color_fraction "$OUT_DIR/$background-boost.png" '#F0B840')"
    amber_muted="$(color_fraction "$OUT_DIR/$background-muted.png" '#F0B840')"
    if ! awk -v teal="$teal" -v boost="$amber_boost" -v muted="$amber_muted" \
      'BEGIN { exit !(teal > 0.000005 && boost > 0.000005 && muted > 0.000005) }'; then
      echo "$background: canonical volume colors missing: teal=$teal boost=$amber_boost muted=$amber_muted" >&2
      exit 1
    fi

    set +e
    total_diff="$(compare -metric AE "$OUT_DIR/$background-rest.png" "$OUT_DIR/$background-focus.png" null: 2>&1)"
    compare_status=$?
    set -e
    if [[ $compare_status -gt 1 ]] || ! awk -v diff="$total_diff" 'BEGIN { exit !(diff + 0 > 500) }'; then
      echo "$background: focus capsule did not produce a visible floating surface: diff=$total_diff" >&2
      exit 1
    fi

    # The speed button and every command to its right are downstream of volume.
    # Their pixels must remain fixed when the capsule opens; the seek thumb to the
    # left is intentionally excluded because separate launches can pause a frame apart.
    magick "$OUT_DIR/$background-rest.png" -crop 382x49+706+613 "$OUT_DIR/$background-rest-row.png"
    magick "$OUT_DIR/$background-focus.png" -crop 382x49+706+613 "$OUT_DIR/$background-focus-row.png"
    set +e
    row_diff="$(compare -metric AE "$OUT_DIR/$background-rest-row.png" "$OUT_DIR/$background-focus-row.png" null: 2>&1)"
    compare_status=$?
    set -e
    if [[ $compare_status -gt 1 ]] || ! awk -v diff="$row_diff" 'BEGIN { exit !(diff + 0 < 5) }'; then
      echo "$background: opening volume reflowed the OSC row: diff=$row_diff" >&2
      exit 1
    fi
  done
fi

if [[ "$INTERACTION_ONLY" == "1" ]]; then
  echo "Volume interaction smoke passed in $OUT_DIR"
else
  echo "Volume smoke passed. Captured $((${#BACKGROUNDS[@]} * 7)) states and verified real GTK interactions in $OUT_DIR"
fi
