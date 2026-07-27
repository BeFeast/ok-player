#!/usr/bin/env bash
# Hammer the app with varied non-OSC drags until it dies or rounds run out.
set -uo pipefail
cd "${OKP_REPRO_ROOT:-/tmp/okp-drag-repro}"
export XDG_RUNTIME_DIR="${OKP_REPRO_ROOT:-/tmp/okp-drag-repro}/xdg" WAYLAND_DISPLAY="${OKP_WAYLAND_DISPLAY:-wayland-1}"
ROUNDS="${1:-30}"
RUNTIME_DIR="/run/user/$(id -u)"
app_active() { env XDG_RUNTIME_DIR="$RUNTIME_DIR" systemctl --user is-active okp-app >/dev/null 2>&1; }
begin_count() { env XDG_RUNTIME_DIR="$RUNTIME_DIR" journalctl --user -u okp-app --no-pager 2>/dev/null | grep -c "player-window-move-begin" || true; }

gen_round() {
  local r=$1 sx sy steps dx dy pace
  case $((r % 5)) in
    0) sx=640; sy=300; steps=10; dx=18;  dy=12;  pace=70 ;;  # medium diagonal
    1) sx=300; sy=200; steps=25; dx=30;  dy=2;   pace=25 ;;  # fast long right
    2) sx=900; sy=350; steps=6;  dx=-40; dy=-20; pace=40 ;;  # up-left, big steps
    3) sx=640; sy=120; steps=14; dx=4;   dy=35;  pace=30 ;;  # down fast (toward OSC)
    4) sx=500; sy=300; steps=3;  dx=8;   dy=6;   pace=200 ;; # slow tiny (threshold edge)
  esac
  echo "abs $sx $sy"; echo "sleep 250"
  echo "btn press"; echo "sleep 150"
  local x=$sx y=$sy i
  for i in $(seq 1 $steps); do
    x=$((x+dx)); y=$((y+dy))
    [ $x -lt 2 ] && x=2; [ $x -gt 1278 ] && x=1278
    [ $y -lt 2 ] && y=2; [ $y -gt 718 ] && y=718
    echo "abs $x $y"; echo "sleep $pace"
  done
  # every third round: release mid-motion pattern (release immediately, no settle)
  if [ $((r % 3)) -eq 0 ]; then echo "btn release"; else echo "sleep 200"; echo "btn release"; fi
  echo "sleep 350"
}

for r in $(seq 1 "$ROUNDS"); do
  before=$(begin_count)
  if ! gen_round "$r" | env XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR WAYLAND_DISPLAY=$WAYLAND_DISPLAY timeout 40 python3 vptr.py >/dev/null; then
    if ! app_active; then echo "APP DIED at round $r (injector torn down with it)"; exit 0; fi
    echo "HARNESS FAULT: pointer injector failed at round $r" >&2
    exit 2
  fi
  if ! app_active; then
    echo "APP DIED at round $r"
    exit 0
  fi
  after=$(begin_count)
  if [ "$after" -le "$before" ]; then
    echo "HARNESS FAULT: round $r delivered no player-window-move-begin (input not reaching the app)" >&2
    exit 2
  fi
  echo "round $r ok (move-begin count $before -> $after)"
done
echo "survived all $ROUNDS rounds (every round verified delivered)"
