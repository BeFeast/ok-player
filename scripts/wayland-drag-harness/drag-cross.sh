#!/usr/bin/env bash
# Cross-monitor drags on the scaled dual-monitor mutter layout (#627 repro).
# Primary: logical 1920x1080 (scale 2). Second: logical ~1104x621 at (1920,432).
set -uo pipefail
cd /tmp/okp-drag-repro
ROUNDS="${1:-20}"

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
  gen_round "$r" | timeout 60 python3 mptr.py >/dev/null 2>&1
  if ! env XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active okp-app >/dev/null 2>&1; then
    echo "APP DIED at round $r"
    exit 0
  fi
  echo "round $r ok"
done
echo "survived all $ROUNDS rounds"
