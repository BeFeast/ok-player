#!/usr/bin/env bash
# Cross-monitor drags on the scaled dual-monitor mutter layout (#627 repro).
# Primary: logical 1920x1080 (scale 2). Second: logical ~1104x621 at (1920,432).
set -uo pipefail
cd "${OKP_REPRO_ROOT:-/tmp/okp-drag-repro}"
ROUNDS="${1:-20}"
RUNTIME_DIR="/run/user/$(id -u)"

app_active() { env XDG_RUNTIME_DIR="$RUNTIME_DIR" systemctl --user is-active okp-app >/dev/null 2>&1; }
begin_count() {
  env XDG_RUNTIME_DIR="$RUNTIME_DIR" journalctl --user -u okp-app --no-pager 2>/dev/null \
    | grep -c "player-window-move-begin" || true
}

gen_round() {
  local r=$1
  case $((r % 4)) in
    0) # drag right across the boundary, release on second monitor
      echo "abs 900 500"; echo "sleep 300"; echo "btn press"; echo "sleep 150"
      local x=900
      while [ $x -lt 2300 ]; do x=$((x+50)); echo "abs $x 550"; echo "sleep 30"; done
      echo "sleep 150"; echo "btn release" ;;
    1) # drag to just past the boundary, release immediately
      echo "abs 1500 500"; echo "sleep 300"; echo "btn press"; echo "sleep 150"
      local x=1500
      while [ $x -lt 1960 ]; do x=$((x+40)); echo "abs $x 520"; echo "sleep 25"; done
      echo "btn release" ;;
    2) # start drag, cross over, come back, release on primary
      echo "abs 1200 400"; echo "sleep 300"; echo "btn press"; echo "sleep 150"
      local x=1200
      while [ $x -lt 2100 ]; do x=$((x+60)); echo "abs $x 500"; echo "sleep 25"; done
      while [ $x -gt 1000 ]; do x=$((x-60)); echo "abs $x 480"; echo "sleep 25"; done
      echo "sleep 100"; echo "btn release" ;;
    3) # drag entirely within second monitor space
      echo "abs 2400 700"; echo "sleep 400"; echo "btn press"; echo "sleep 150"
      local x=2400
      while [ $x -gt 2000 ]; do x=$((x-30)); echo "abs $x 680"; echo "sleep 30"; done
      echo "sleep 150"; echo "btn release" ;;
  esac
  echo "sleep 500"
}

for r in $(seq 1 "$ROUNDS"); do
  before=$(begin_count)
  if ! gen_round "$r" | timeout 60 python3 mptr.py >/dev/null; then
    # The app dying mid-round can tear the injector down too - that is the
    # repro, not a harness fault. Anything else is a harness fault.
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
    # A surviving app that never saw the drag proves nothing. This is exactly
    # how a mis-set-up compositor turns into a false "survived all rounds".
    echo "HARNESS FAULT: round $r delivered no player-window-move-begin (input not reaching the app)" >&2
    exit 2
  fi
  echo "round $r ok (move-begin count $before -> $after)"
done
echo "survived all $ROUNDS rounds (every round verified delivered)"
