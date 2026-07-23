#!/usr/bin/env bash
# Headless X11 regression gate for window-drag survival and monitor-local initial fit.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
OUT_DIR="${2:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
DRAG_SMOKE="${OKP_WINDOW_DRAG_SMOKE:-$ROOT/scripts/smoke-linux-window-drag.sh}"
FIT_SERIES="${OKP_WINDOW_FIT_SERIES:-$ROOT/scripts/run-linux-window-fit-series.sh}"
SOURCE_SHA="${OKP_WINDOW_REGRESSION_SOURCE_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)}"

OUT_DIR="$(realpath -m -- "$OUT_DIR")"
BINARY_PATH="$(realpath -m -- "$BINARY")"
if [[ "$OUT_DIR" == / || "$OUT_DIR" == "$ROOT" || "$BINARY_PATH" == "$OUT_DIR"/* ]]; then
  echo "Output directory must be a dedicated directory that does not contain the candidate binary" >&2
  exit 2
fi

[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }
[[ -x "$DRAG_SMOKE" ]] || { echo "Missing executable: $DRAG_SMOKE" >&2; exit 127; }
[[ -x "$FIT_SERIES" ]] || { echo "Missing executable: $FIT_SERIES" >&2; exit 127; }
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "OKP_WINDOW_REGRESSION_SOURCE_SHA must be a 40-character lowercase commit SHA" >&2
  exit 2
}

rm -rf -- "$OUT_DIR"
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
  printf 'status=%s\n' "$status" >>"$OUT_DIR/metadata.env"
  (
    cd "$OUT_DIR"
    shopt -s globstar nullglob
    for evidence_file in **/*; do
      [[ -f "$evidence_file" && "$evidence_file" != SHA256SUMS ]] || continue
      sha256sum "$evidence_file"
    done
  ) >"$OUT_DIR/SHA256SUMS"
}

require_line() {
  local file="$1" line="$2"
  [[ -f "$file" ]] && rg -Fxq "$line" "$file"
}

drag_dir="$OUT_DIR/window-drag"
set +e
"$DRAG_SMOKE" "$BINARY" "$drag_dir" >"$OUT_DIR/window-drag-command.log" 2>&1
drag_status=$?
set -e
if (( drag_status != 0 )); then
  record_result non_osc_window_drag FAIL "exit=$drag_status; log=window-drag-command.log"
  record_result single_monitor_initial_fit 'NOT RUN' 'window-drag regression did not pass'
  finish_evidence fail
  echo "Window-drag regression failed. Evidence: $OUT_DIR" >&2
  exit "$drag_status"
fi

for required in \
  video_surface_handoff_survival=pass \
  compositor_cancel_survival=pass \
  post_cancel_drag=pass \
  idle_canvas_handoff_survival=pass \
  fatal_diagnostics=absent; do
  if ! require_line "$drag_dir/results.txt" "$required"; then
    record_result non_osc_window_drag FAIL "missing=$required"
    record_result single_monitor_initial_fit 'NOT RUN' 'window-drag evidence was incomplete'
    finish_evidence fail
    echo "Window-drag regression omitted required evidence: $required" >&2
    exit 1
  fi
done
record_result non_osc_window_drag PASS 'evidence=window-drag/results.txt'

fit_dir="$OUT_DIR/window-fit"
set +e
OKP_WINDOW_FIT_SOURCE_SHA="$SOURCE_SHA" \
  "$FIT_SERIES" "$BINARY" "$fit_dir" >"$OUT_DIR/window-fit-command.log" 2>&1
fit_status=$?
set -e
if (( fit_status != 0 )); then
  record_result single_monitor_initial_fit FAIL "exit=$fit_status; log=window-fit-command.log"
  finish_evidence fail
  echo "Single-monitor window-fit regression failed. Evidence: $OUT_DIR" >&2
  exit "$fit_status"
fi

for required in completed_consecutive_runs=3 status=pass; do
  if ! require_line "$fit_dir/series-evidence.txt" "$required"; then
    record_result single_monitor_initial_fit FAIL "missing=$required"
    finish_evidence fail
    echo "Window-fit series omitted required evidence: $required" >&2
    exit 1
  fi
done
for run in 1 2 3; do
  for required in logged_monitor_workarea_containment=pass status=pass; do
    if ! require_line "$fit_dir/run-$run/fit-evidence.txt" "$required"; then
      record_result single_monitor_initial_fit FAIL "run=$run; missing=$required"
      finish_evidence fail
      echo "Window-fit run $run omitted required evidence: $required" >&2
      exit 1
    fi
  done
done
record_result single_monitor_initial_fit PASS 'evidence=window-fit/series-evidence.txt'

finish_evidence pass
echo "Linux window regression smokes passed. Evidence: $OUT_DIR"
