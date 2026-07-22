#!/usr/bin/env bash
# Run the headless non-OSC drag and single-monitor fit regression gates.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-regression-smokes.sh <binary> <output-directory>}"
OUT_DIR="${2:?usage: run-linux-regression-smokes.sh <binary> <output-directory>}"
DRAG_SMOKE="${OKP_LINUX_WINDOW_DRAG_SMOKE:-$ROOT/scripts/smoke-linux-window-drag.sh}"
FIT_SERIES="${OKP_LINUX_WINDOW_FIT_SERIES:-$ROOT/scripts/run-linux-window-fit-series.sh}"
SOURCE_SHA="${OKP_LINUX_REGRESSION_SOURCE_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || printf unknown)}"
RUNNER_LABEL="${OKP_LINUX_REGRESSION_RUNNER_LABEL:-unspecified}"

[[ -x "$BINARY" ]] || {
  echo "Missing executable: $BINARY" >&2
  exit 127
}
for smoke in "$DRAG_SMOKE" "$FIT_SERIES"; do
  [[ -x "$smoke" ]] || {
    echo "Missing executable smoke: $smoke" >&2
    exit 127
  }
done
resolved_out_dir="$(realpath -m -- "$OUT_DIR")"
if [[ "$resolved_out_dir" == / || "$resolved_out_dir" == "$ROOT" ]]; then
  echo "Refusing unsafe output directory: $OUT_DIR" >&2
  exit 2
fi
if [[ ! "$SOURCE_SHA" =~ ^([0-9a-f]{40}|unknown)$ ]]; then
  echo "OKP_LINUX_REGRESSION_SOURCE_SHA must be a lowercase 40-character SHA or unknown" >&2
  exit 2
fi
if [[ ! "$RUNNER_LABEL" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "OKP_LINUX_REGRESSION_RUNNER_LABEL must contain only letters, digits, dot, underscore, or dash" >&2
  exit 2
fi

rm -rf -- "$resolved_out_dir"
OUT_DIR="$resolved_out_dir"
SUMMARY="$OUT_DIR/suite-evidence.txt"
mkdir -p "$OUT_DIR"
printf '%s\n' \
  "source_sha=$SOURCE_SHA" \
  "runner_label=$RUNNER_LABEL" \
  'environment=headless-x11' \
  'operator_seat_required=false' \
  'live_dual_head_hardware_proven=false' \
  >"$SUMMARY"

record_failure() {
  local gate="$1" status="$2"
  printf 'gate=%s status=fail exit_status=%s\nsuite_status=fail\n' \
    "$gate" "$status" >>"$SUMMARY"
  echo "Linux regression smoke failed at $gate (exit $status)" >&2
  exit "$status"
}

set +e
OKP_LINUX_REGRESSION_SOURCE_SHA="$SOURCE_SHA" \
  "$DRAG_SMOKE" "$BINARY" "$OUT_DIR/window-drag"
drag_status=$?
set -e
(( drag_status == 0 )) || record_failure window-drag "$drag_status"
for required in \
  'video_surface_handoff_survival=pass' \
  'compositor_cancel_survival=pass' \
  'post_cancel_drag=pass' \
  'idle_canvas_handoff_survival=pass' \
  'fatal_diagnostics=absent'; do
  rg -Fx -q "$required" "$OUT_DIR/window-drag/results.txt" || {
    echo "Window-drag smoke omitted required evidence: $required" >&2
    record_failure window-drag-evidence 1
  }
done
printf 'gate=window-drag status=pass evidence_sha256=%s\n' \
  "$(sha256sum "$OUT_DIR/window-drag/results.txt" | awk '{print $1}')" >>"$SUMMARY"

set +e
OKP_WINDOW_FIT_SOURCE_SHA="$SOURCE_SHA" \
  "$FIT_SERIES" "$BINARY" "$OUT_DIR/window-fit"
fit_status=$?
set -e
(( fit_status == 0 )) || record_failure window-fit "$fit_status"
for required in \
  'completed_consecutive_runs=3' \
  'status=pass'; do
  rg -Fx -q "$required" "$OUT_DIR/window-fit/series-evidence.txt" || {
    echo "Window-fit smoke omitted required evidence: $required" >&2
    record_failure window-fit-evidence 1
  }
done
printf 'gate=window-fit status=pass evidence_sha256=%s\n' \
  "$(sha256sum "$OUT_DIR/window-fit/series-evidence.txt" | awk '{print $1}')" >>"$SUMMARY"

printf '%s\n' \
  'suite_status=pass' \
  'not_proven=live GNOME Wayland, physical dual-head hardware, portal, drag-and-drop, clipboard, focus' \
  >>"$SUMMARY"
echo "Linux regression smokes passed. Evidence: $SUMMARY"
