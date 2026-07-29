#!/usr/bin/env bash
# Produce the live GNOME/Wayland acceptance rows a machine is allowed to attest (#691).
#
# Every check here reads the outcome back out of the session rather than trusting that a
# gesture had an effect: the Wayland clipboard through a separate client, the portal call
# on the session bus, the compositor's own fullscreen configure and composited frame, the
# shell's focus and always-on-top diagnostics. A check that cannot observe its outcome
# fails; none of them can report PASS by default.
#
# Input is real: uinput events through ydotool, aimed with the geometry the app publishes
# (#690) and only pressed once the app has confirmed where the pointer landed.
#
#   OKP_ACCEPTANCE_NEGATIVE_CONTROL=<row>   withhold that row's stimulus. The observation
#                                           is unchanged, so the row must FAIL. This is
#                                           how the run proves a row can fail at all.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
HARNESS="$ROOT/scripts/gnome-wayland-harness"
AIM="$ROOT/scripts/wayland-drag-harness/aim.py"

BINARY=""
FIXTURE=""
OUT_DIR=""
ROWS="gnome-file-chooser,wayland-clipboard,desktop-portal,wayland-compositor-fullscreen,wayland-double-click-fullscreen,wayland-always-on-top-unavailable,keyboard-focus-navigation"
NEGATIVE_CONTROL="${OKP_ACCEPTANCE_NEGATIVE_CONTROL:-}"

usage() {
  cat >&2 <<'EOF'
usage: run-linux-gnome-wayland-acceptance.sh --binary PATH --fixture PATH --out DIR [--rows a,b,c]

Runs on a live GNOME/Wayland session. Refuses anything else.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY="${2:?}"; shift 2 ;;
    --fixture) FIXTURE="${2:?}"; shift 2 ;;
    --out) OUT_DIR="${2:?}"; shift 2 ;;
    --rows) ROWS="${2:?}"; shift 2 ;;
    --negative-control) NEGATIVE_CONTROL="${2:?}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 64 ;;
  esac
done

[[ -x "$BINARY" ]] || { echo "missing or non-executable --binary: $BINARY" >&2; exit 64; }
[[ -f "$FIXTURE" ]] || { echo "missing --fixture: $FIXTURE" >&2; exit 64; }
[[ -n "$OUT_DIR" ]] || { usage; exit 64; }

for tool in ydotool ydotoold wl-paste wl-copy dbus-monitor python3 gst-launch-1.0 sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "missing required tool: $tool" >&2; exit 127; }
done

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"
RESULTS="$OUT_DIR/results"
ARTIFACTS="$OUT_DIR/artifacts"
RUN="$OUT_DIR/session"
rm -rf "$RESULTS" "$ARTIFACTS" "$RUN"
mkdir -p "$RESULTS" "$ARTIFACTS" "$RUN"

APP_LOG="$RUN/app.log"
PORTAL_LOG="$RUN/portal.log"
YDOTOOL_SOCKET="$RUN/ydotool.sock"
export YDOTOOL_SOCKET

say() { printf '\n== %s\n' "$*"; }

# A row records exactly what the check observed. `pass` is only ever written by a check
# that saw the outcome; everything else is a failure with the reason attached.
record() {
  local row="$1" status="$2" note="$3"
  printf '%s\n%s\n' "$status" "$note" >"$RESULTS/$row.result"
  printf '  %s: %s (%s)\n' "$row" "$status" "$note"
}

wants() {
  [[ ",$ROWS," == *",$1,"* ]]
}

# The stimulus is withheld for the row under negative control; nothing else changes, so a
# row that still reports PASS is not observing anything.
stimulus_withheld() {
  [[ -n "$NEGATIVE_CONTROL" && "$NEGATIVE_CONTROL" == "$1" ]]
}

say "session facts"
python3 "$HARNESS/session-facts.py" >"$OUT_DIR/session-facts.json"
cat "$OUT_DIR/session-facts.json"

cleanup() {
  local status=$?
  [[ -n "${APP_PID:-}" ]] && kill "$APP_PID" 2>/dev/null || true
  [[ -n "${MONITOR_PID:-}" ]] && kill "$MONITOR_PID" 2>/dev/null || true
  [[ -n "${YDOTOOLD_PID:-}" ]] && sudo -n kill "$YDOTOOLD_PID" 2>/dev/null || true
  return $status
}
trap cleanup EXIT

say "input injector"
# uinput needs privilege; the daemon is the only privileged part and it owns a socket in
# this run's directory rather than a shared well-known path.
sudo -n sh -c "nohup ydotoold --socket-path='$YDOTOOL_SOCKET' --socket-perm=0660 \
  --socket-own=$(id -u):$(id -g) >'$RUN/ydotoold.log' 2>&1 & echo \$! >'$RUN/ydotoold.pid'"
for _ in $(seq 1 40); do [[ -S "$YDOTOOL_SOCKET" ]] && break; sleep 0.1; done
[[ -S "$YDOTOOL_SOCKET" ]] || { echo "ydotoold never created its socket" >&2; exit 75; }
YDOTOOLD_PID="$(cat "$RUN/ydotoold.pid")"

# Absolute injection is only faithful with a flat pointer profile; ydotool says so itself.
PREVIOUS_ACCEL="$(gsettings get org.gnome.desktop.peripherals.mouse accel-profile)"
gsettings set org.gnome.desktop.peripherals.mouse accel-profile flat
# A deterministic double-click window, or the double-click row races the GTK default.
PREVIOUS_A11Y="$(gsettings get org.gnome.desktop.interface toolkit-accessibility)"
gsettings set org.gnome.desktop.interface toolkit-accessibility true
restore_desktop() {
  gsettings set org.gnome.desktop.peripherals.mouse accel-profile "$PREVIOUS_ACCEL" || true
  gsettings set org.gnome.desktop.interface toolkit-accessibility "$PREVIOUS_A11Y" || true
}
trap 'restore_desktop; cleanup' EXIT

say "portal monitor"
dbus-monitor --session "interface='org.freedesktop.portal.FileChooser'" >"$PORTAL_LOG" 2>&1 &
MONITOR_PID=$!

say "player"
export OKP_DEBUG_INTERACTIONS=1
export OKP_DEBUG_WINDOW_FIT=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export XDG_CONFIG_HOME="$RUN/config"
export XDG_STATE_HOME="$RUN/state"
export XDG_CACHE_HOME="$RUN/cache"
export XDG_DATA_HOME="$RUN/data"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME"
"$BINARY" "$FIXTURE" >"$APP_LOG" 2>&1 &
APP_PID=$!

for _ in $(seq 1 120); do
  grep -q 'interaction: geometry part=window' "$APP_LOG" 2>/dev/null && break
  kill -0 "$APP_PID" 2>/dev/null || { echo "the player exited before it published geometry" >&2; exit 75; }
  sleep 0.25
done
grep -q 'interaction: geometry part=window' "$APP_LOG" || {
  echo "the player never published a geometry record: is this build older than #690?" >&2
  exit 75
}

key() { ydotool key "$@"; }
aim_click() { python3 "$AIM" --log "$APP_LOG" --part "${1:-video}" click; }
aim_target() { python3 "$AIM" --log "$APP_LOG" --part "${1:-video}" target; }
capture() { python3 "$HARNESS/capture-screen.py" "$ARTIFACTS/$1"; }
excerpt() { grep -E "$1" "$APP_LOG" | tail -40 >"$ARTIFACTS/$2" || true; }

# Newest value of one key on one part of the geometry record.
geometry_field() {
  awk -v part="$1" -v key="$2" '
    index($0, "interaction: geometry part=" part " ") == 1 {
      for (i = 1; i <= NF; i++) { split($i, pair, "="); if (pair[1] == key) value = pair[2] }
    }
    END { print (value == "" ? "unknown" : value) }
  ' "$APP_LOG"
}

focus_player() { aim_click video >/dev/null 2>&1 || true; }

#-------------------------------------------------------------------------------- rows --

if wants wayland-clipboard; then
  say "wayland-clipboard"
  # The outcome is read by a separate Wayland client, so a PASS means the compositor
  # really carried the selection between two processes.
  wl-copy --clear || true
  sleep 0.5
  focus_player
  if stimulus_withheld wayland-clipboard; then
    echo "negative control: the copy gesture is withheld"
  else
    key 42:1 46:1 46:0 42:0   # Shift+C, the Copy Frame binding
  fi
  sleep 3
  if wl-paste --type image/png >"$ARTIFACTS/wayland-clipboard.png" 2>"$RUN/clipboard.err"; then
    size="$(stat -c %s "$ARTIFACTS/wayland-clipboard.png")"
    header="$(head -c 8 "$ARTIFACTS/wayland-clipboard.png" | od -An -tx1 | tr -d ' \n')"
    if [[ "$header" == "89504e470d0a1a0a" && "$size" -gt 4096 ]]; then
      record wayland-clipboard pass \
        "a separate wl-paste client read a ${size}-byte image/png frame off the Wayland clipboard"
    else
      record wayland-clipboard fail \
        "the clipboard offered image/png but the payload is not a usable PNG (${size} bytes)"
    fi
  else
    rm -f "$ARTIFACTS/wayland-clipboard.png"
    record wayland-clipboard fail "no image/png offer on the Wayland clipboard after the copy gesture"
  fi
  grep -F 'Frame copied to clipboard' "$APP_LOG" | tail -5 >"$ARTIFACTS/wayland-clipboard.log" || true
fi

if wants gnome-file-chooser || wants desktop-portal; then
  say "gnome-file-chooser / desktop-portal"
  CHOSEN="$RUN/chosen-through-the-chooser.mp4"
  cp "$FIXTURE" "$CHOSEN"
  portal_before="$(grep -c 'member=OpenFile' "$PORTAL_LOG" || true)"
  focus_player
  if stimulus_withheld gnome-file-chooser || stimulus_withheld desktop-portal; then
    echo "negative control: the open-file gesture is withheld"
  else
    key 24:1 24:0                       # O, the Open File binding
    sleep 3
    key 29:1 38:1 38:0 29:0             # Ctrl+L, the chooser's location entry
    sleep 1
    ydotool type "$CHOSEN"
    sleep 1
    key 28:1 28:0                       # Enter commits the typed location
    sleep 2
    key 28:1 28:0                       # Enter accepts the selection
    sleep 4
  fi

  python3 "$HARNESS/atspi-windows.py" >"$ARTIFACTS/gnome-file-chooser.json" 2>/dev/null || true
  portal_after="$(grep -c 'member=OpenFile' "$PORTAL_LOG" || true)"
  grep -A20 'interface=org.freedesktop.portal.FileChooser' "$PORTAL_LOG" | tail -60 \
    >"$ARTIFACTS/desktop-portal.log" || true

  history="$XDG_STATE_HOME/ok-player/history.json"
  loaded=0
  if [[ -f "$history" ]]; then
    loaded="$(python3 - "$history" "$CHOSEN" <<'PY'
import json, pathlib, sys
entry = json.loads(pathlib.Path(sys.argv[1]).read_text()).get("files", {}).get(sys.argv[2])
# A duration only exists if the demuxer actually opened the file the chooser returned.
print(1 if entry and float(entry.get("duration") or 0) > 0 else 0)
PY
)"
  fi

  if wants gnome-file-chooser; then
    if [[ "$loaded" == "1" ]]; then
      record gnome-file-chooser pass \
        "the native chooser returned the selected file and the player demuxed it"
    else
      record gnome-file-chooser fail \
        "the player never loaded a file through the chooser (no demuxed history entry)"
    fi
  fi
  if wants desktop-portal; then
    if (( portal_after > portal_before )); then
      record desktop-portal pass \
        "the open request travelled over org.freedesktop.portal.FileChooser.OpenFile on the session bus"
    else
      record desktop-portal fail \
        "no org.freedesktop.portal.FileChooser traffic: the chooser did not go through the portal"
    fi
  fi
fi

if wants wayland-compositor-fullscreen; then
  say "wayland-compositor-fullscreen"
  focus_player
  monitor_w="$(geometry_field window monitor-w)"
  monitor_h="$(geometry_field window monitor-h)"
  if stimulus_withheld wayland-compositor-fullscreen; then
    echo "negative control: the fullscreen gesture is withheld"
  else
    key 33:1 33:0   # F
  fi
  sleep 3
  capture wayland-compositor-fullscreen.png >/dev/null
  fullscreen="$(geometry_field window fullscreen)"
  window_w="$(geometry_field window w)"
  window_h="$(geometry_field window h)"
  origin="$(geometry_field window origin)"
  excerpt 'fullscreen geometry: boundary=fullscreen-(request|ack)' wayland-compositor-fullscreen.log
  if [[ "$fullscreen" == "1" && "$window_w" == "$monitor_w" && "$window_h" == "$monitor_h" \
        && "$origin" == "fullscreen-monitor" ]]; then
    record wayland-compositor-fullscreen pass \
      "the compositor configured the surface fullscreen at the monitor size ${window_w}x${window_h}"
  else
    record wayland-compositor-fullscreen fail \
      "no compositor fullscreen configure (fullscreen=$fullscreen window=${window_w}x${window_h} monitor=${monitor_w}x${monitor_h} origin=$origin)"
  fi
  # Leave fullscreen so the following rows start from the same state.
  key 1:1 1:0
  sleep 2
fi

if wants wayland-double-click-fullscreen; then
  say "wayland-double-click-fullscreen"
  before="$(geometry_field window fullscreen)"
  target="$(aim_target drag-target | awk '/^target /{print $3}')"
  x="${target#global=(}"; x="${x%%,*}"
  y="${target##*,}"; y="${y%)}"
  transitions=0
  for round in 1 2; do
    if stimulus_withheld wayland-double-click-fullscreen; then
      echo "negative control: round $round sends a single click instead of a double"
      ydotool click 0xC0
    else
      # Two presses inside the double-click window, from the point aiming just verified.
      ydotool click -D 40 0xC0 0xC0
    fi
    sleep 3
    state="$(geometry_field window fullscreen)"
    if [[ "$state" != "$before" ]]; then
      transitions=$((transitions + 1))
      before="$state"
    fi
    capture "wayland-double-click-fullscreen-$round.png" >/dev/null
  done
  excerpt 'interaction: video-(double|single)-click' wayland-double-click-fullscreen.log
  if (( transitions == 2 )); then
    record wayland-double-click-fullscreen pass \
      "two double-clicks on the video plane at ($x,$y) drove two compositor fullscreen transitions"
  else
    record wayland-double-click-fullscreen fail \
      "expected two fullscreen transitions from two double-clicks, observed $transitions"
  fi
  if [[ "$(geometry_field window fullscreen)" == "1" ]]; then key 1:1 1:0; sleep 2; fi
fi

if wants wayland-always-on-top-unavailable; then
  say "wayland-always-on-top-unavailable"
  # The pin control is aimed from the geometry record, not guessed at in the titlebar.
  before="$(grep -c 'interaction: always-on-top=' "$APP_LOG" || true)"
  if stimulus_withheld wayland-always-on-top-unavailable; then
    echo "negative control: the pin gesture is withheld"
  else
    aim_click window-pin-0 || true
  fi
  sleep 2
  capture wayland-always-on-top-unavailable.png >/dev/null
  excerpt 'interaction: always-on-top=' wayland-always-on-top-unavailable.log
  after="$(grep -c 'interaction: always-on-top=' "$APP_LOG" || true)"
  verdict="$(grep 'interaction: always-on-top=' "$APP_LOG" | tail -1 || true)"
  if (( after > before )) && [[ "$verdict" == *"always-on-top=unavailable"* ]]; then
    record wayland-always-on-top-unavailable pass \
      "the pin control reported the state as unavailable on this Wayland session"
  elif (( after > before )); then
    record wayland-always-on-top-unavailable fail \
      "the pin control reported ${verdict##*interaction: }, but Wayland exposes no such protocol"
  else
    record wayland-always-on-top-unavailable fail \
      "the pin control never reported an always-on-top result"
  fi
fi

if wants keyboard-focus-navigation; then
  say "keyboard-focus-navigation"
  focus_player
  before="$(grep -c 'interaction: focus target=' "$APP_LOG" || true)"
  if stimulus_withheld keyboard-focus-navigation; then
    echo "negative control: the Tab traversal is withheld"
  else
    for _ in 1 2 3 4 5 6; do key 15:1 15:0; sleep 0.4; done   # Tab
    for _ in 1 2 3; do key 42:1 15:1 15:0 42:0; sleep 0.4; done  # Shift+Tab
  fi
  sleep 1
  grep 'interaction: focus target=' "$APP_LOG" | tail -40 >"$ARTIFACTS/keyboard-focus-navigation.log" || true
  capture keyboard-focus-navigation.png >/dev/null
  if verdict="$(python3 "$HARNESS/focus-traversal.py" --log "$APP_LOG" \
      --from-line "$before" --forward 6 --backward 3)"; then
    record keyboard-focus-navigation pass "Tab traversal observed: ${verdict#pass }"
  else
    record keyboard-focus-navigation fail "focus did not traverse: ${verdict#fail }"
  fi
fi

#---------------------------------------------------------------------------- manifest --

say "rows"
# CI runs from the checkout; a live-desktop rehearsal outside one names the tree it used.
HARNESS_REVISION="${OKP_HARNESS_REVISION:-$(git -C "$ROOT" rev-parse HEAD)}"
EXECUTION_SEED="gnome-wayland-automated:${GITHUB_RUN_ID:-local}:$(python3 -c 'import uuid;print(uuid.uuid4())')"
EXECUTION_SHA256="$(printf '%s' "$EXECUTION_SEED" | sha256sum | awk '{print $1}')"
python3 "$HARNESS/emit-rows.py" \
  --results "$RESULTS" \
  --artifacts "$ARTIFACTS" \
  --facts "$OUT_DIR/session-facts.json" \
  --harness-revision "$HARNESS_REVISION" \
  --execution-environment-sha256 "$EXECUTION_SHA256" \
  --output "$OUT_DIR/gnome-wayland-rows.json"
