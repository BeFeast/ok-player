#!/usr/bin/env bash
# Headless X11 regression gate for window-drag survival and monitor-local initial fit.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-window-regression-smokes.sh <binary> <new-output-directory>}"
OUT_DIR="${2:?usage: run-linux-window-regression-smokes.sh <binary> <new-output-directory>}"
DRAG_SMOKE="${OKP_WINDOW_DRAG_SMOKE:-$ROOT/scripts/smoke-linux-window-drag.sh}"
FIT_SERIES="${OKP_WINDOW_FIT_SERIES:-$ROOT/scripts/run-linux-window-fit-series.sh}"
SOURCE_SHA="${OKP_WINDOW_REGRESSION_SOURCE_SHA:-}"

[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }
[[ -x "$DRAG_SMOKE" ]] || { echo "Missing executable: $DRAG_SMOKE" >&2; exit 127; }
[[ -x "$FIT_SERIES" ]] || { echo "Missing executable: $FIT_SERIES" >&2; exit 127; }
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "OKP_WINDOW_REGRESSION_SOURCE_SHA must identify the tested candidate with a lowercase 40-character commit SHA" >&2
  exit 2
}

BINARY_PATH="$(realpath -e -- "$BINARY")"
OUT_DIR="$(realpath -m -- "$OUT_DIR")"
if [[ "$OUT_DIR" == "$BINARY_PATH" || "$BINARY_PATH" == "$OUT_DIR"/* ]]; then
  echo "Output directory must not be the candidate binary or contain it" >&2
  exit 2
fi
if [[ -e "$OUT_DIR" || -L "$OUT_DIR" ]]; then
  echo "Output directory must not already exist: $OUT_DIR" >&2
  exit 2
fi
mkdir -p -- "$OUT_DIR"

cat >"$OUT_DIR/metadata.env" <<EOF
schema=1
source_sha=$SOURCE_SHA
evidence_level=xvfb-x11-automation
operator_seat_required=false
live_gnome_wayland_proven=false
live_dual_head_proven=false
EOF
: >"$OUT_DIR/results.tsv"

record_result() {
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$OUT_DIR/results.tsv"
}

finish_evidence() {
  local status="$1" evidence_file
  printf '%s\n' \
    "window_drag_status=${drag_result,,}" \
    "window_fit_status=${fit_result,,}" \
    "status=$status" >>"$OUT_DIR/metadata.env"
  (
    cd "$OUT_DIR"
    shopt -s globstar nullglob
    for evidence_file in **/*; do
      [[ -f "$evidence_file" && "$evidence_file" != SHA256SUMS ]] || continue
      sha256sum "$evidence_file"
    done
  ) >"$OUT_DIR/SHA256SUMS"
}

missing_evidence=""
require_lines() {
  local file="$1" line
  shift
  if [[ ! -f "$file" ]]; then
    missing_evidence="missing file=${file#"$OUT_DIR"/}"
    return 1
  fi
  for line in "$@"; do
    if ! awk -v expected="$line" '$0 == expected { found = 1 } END { exit !found }' "$file"; then
      missing_evidence="file=${file#"$OUT_DIR"/}; missing=$line"
      return 1
    fi
  done
}

failed=0
drag_result=FAIL
drag_dir="$OUT_DIR/window-drag"
set +e
"$DRAG_SMOKE" "$BINARY" "$drag_dir" >"$OUT_DIR/window-drag-command.log" 2>&1
drag_status=$?
set -e
if (( drag_status != 0 )); then
  record_result non_osc_window_drag FAIL "exit=$drag_status; log=window-drag-command.log"
  failed=1
elif ! require_lines "$drag_dir/results.txt" \
  video_surface_handoff_survival=pass \
  compositor_cancel_survival=pass \
  post_cancel_drag=pass \
  idle_canvas_handoff_survival=pass \
  fatal_diagnostics=absent; then
  record_result non_osc_window_drag FAIL "$missing_evidence"
  failed=1
elif ! require_lines "$drag_dir/dbus-evidence.txt" \
  session_bus_ready=true \
  session_bus_teardown=clean \
  session_process_teardown=clean \
  command_status=0 \
  status=pass; then
  record_result non_osc_window_drag FAIL "$missing_evidence"
  failed=1
elif ! require_lines "$drag_dir/xvfb-evidence.txt" \
  xvfb_ready=true \
  xvfb_alive_before_teardown=true \
  xvfb_teardown=clean \
  command_status=0 \
  status=pass; then
  record_result non_osc_window_drag FAIL "$missing_evidence"
  failed=1
else
  drag_result=PASS
  record_result non_osc_window_drag PASS 'evidence=window-drag/results.txt'
fi

fit_result=FAIL
fit_dir="$OUT_DIR/window-fit"
set +e
OKP_WINDOW_FIT_SOURCE_SHA="$SOURCE_SHA" \
  "$FIT_SERIES" "$BINARY" "$fit_dir" >"$OUT_DIR/window-fit-command.log" 2>&1
fit_status=$?
set -e
if (( fit_status != 0 )); then
  record_result single_monitor_initial_fit FAIL "exit=$fit_status; log=window-fit-command.log"
  failed=1
elif ! require_lines "$fit_dir/series-evidence.txt" \
  "source_sha=$SOURCE_SHA" \
  completed_consecutive_runs=3 \
  status=pass; then
  record_result single_monitor_initial_fit FAIL "$missing_evidence"
  failed=1
else
  fit_evidence_complete=1
  for run in 1 2 3; do
    if ! require_lines "$fit_dir/run-$run/fit-evidence.txt" \
      logged_monitor_workarea_containment=pass \
      status=pass; then
      fit_evidence_complete=0
      break
    fi
    if ! require_lines "$fit_dir/run-$run/fit-session-evidence.txt" \
      session_bus_ready=true \
      session_bus_teardown=clean \
      session_process_teardown=clean \
      command_status=0 \
      status=pass; then
      fit_evidence_complete=0
      break
    fi
    if ! require_lines "$fit_dir/run-$run/fit-xvfb-evidence.txt" \
      xvfb_ready=true \
      xvfb_alive_before_teardown=true \
      xvfb_teardown=clean \
      command_status=0 \
      status=pass; then
      fit_evidence_complete=0
      break
    fi
  done
  if (( fit_evidence_complete == 0 )); then
    record_result single_monitor_initial_fit FAIL "$missing_evidence"
    failed=1
  else
    fit_result=PASS
    record_result single_monitor_initial_fit PASS 'evidence=window-fit/series-evidence.txt'
  fi
fi

if (( failed != 0 )); then
  finish_evidence fail
  echo "Linux window regression smokes failed. Evidence: $OUT_DIR" >&2
  exit 1
fi

finish_evidence pass
echo "Linux window regression smokes passed. Evidence: $OUT_DIR"
