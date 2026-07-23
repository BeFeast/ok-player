#!/usr/bin/env bash
# Run the headless non-OSC drag and single-monitor-fit regression smokes.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
OUT_DIR="${2:?usage: run-linux-window-regression-smokes.sh <binary> <output-directory>}"
DRAG_SMOKE="${OKP_WINDOW_DRAG_SMOKE:-$ROOT/scripts/smoke-linux-window-drag.sh}"
FIT_SERIES="${OKP_WINDOW_FIT_SERIES:-$ROOT/scripts/run-linux-window-fit-series.sh}"
SOURCE_SHA="${OKP_WINDOW_REGRESSION_SOURCE_SHA:-}"

if [[ -z "$SOURCE_SHA" ]]; then
  if ! SOURCE_SHA="$(git -C "$ROOT" rev-parse --verify "HEAD^{commit}" 2>/dev/null)"; then
    echo "Set OKP_WINDOW_REGRESSION_SOURCE_SHA when Git metadata is unavailable" >&2
    exit 2
  fi
fi
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "OKP_WINDOW_REGRESSION_SOURCE_SHA must be a lowercase 40-character commit SHA" >&2
  exit 2
}

[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }
[[ -x "$DRAG_SMOKE" ]] || { echo "Missing executable: $DRAG_SMOKE" >&2; exit 127; }
[[ -x "$FIT_SERIES" ]] || { echo "Missing executable: $FIT_SERIES" >&2; exit 127; }

[[ ! -e "$OUT_DIR" && ! -L "$OUT_DIR" ]] || {
  echo "Output directory already exists: $OUT_DIR" >&2
  exit 2
}
mkdir -p "$OUT_DIR"
: >"$OUT_DIR/results.tsv"

failed=0
evidence_has_exact_line() {
  local file="$1" expected="$2" line
  while IFS= read -r line; do
    [[ "$line" == "$expected" ]] && return 0
  done <"$file"
  return 1
}

run_smoke() {
  local name="$1" primary_evidence="$2"
  shift 2
  local -a evidence_files=()
  local -a expected_lines=()
  local evidence failure_detail="" index rc

  while [[ "${1:-}" != '--' ]]; do
    (( $# >= 2 )) || {
      echo "Internal error: incomplete evidence requirement for $name" >&2
      exit 2
    }
    evidence_files+=("$1")
    expected_lines+=("$2")
    shift 2
  done
  shift

  set +e
  "$@"
  rc=$?
  set -e

  if (( rc != 0 )); then
    printf '%s\tFAIL\texit=%s; evidence=%s\n' "$name" "$rc" "$primary_evidence" \
      >>"$OUT_DIR/results.tsv"
    failed=1
    return
  fi

  for index in "${!evidence_files[@]}"; do
    evidence="${evidence_files[$index]}"
    if [[ ! -s "$OUT_DIR/$evidence" ]]; then
      failure_detail="missing evidence=$evidence"
      break
    fi
    if ! evidence_has_exact_line \
      "$OUT_DIR/$evidence" "${expected_lines[$index]}"; then
      failure_detail="missing exact evidence=${expected_lines[$index]}; file=$evidence"
      break
    fi
  done

  if [[ -n "$failure_detail" ]]; then
    printf '%s\tFAIL\t%s\n' "$name" "$failure_detail" >>"$OUT_DIR/results.tsv"
    failed=1
  else
    printf '%s\tPASS\t%s\n' "$name" "$primary_evidence" >>"$OUT_DIR/results.tsv"
  fi
}

run_smoke \
  non_osc_window_drag \
  window-drag/results.txt \
  window-drag/results.txt video_surface_handoff_survival=pass \
  window-drag/results.txt video_surface_drag_handoff=observed \
  window-drag/results.txt compositor_cancel_survival=pass \
  window-drag/results.txt compositor_cancel_drag_handoff=observed \
  window-drag/results.txt post_cancel_drag=pass \
  window-drag/results.txt post_cancel_drag_handoff=observed \
  window-drag/results.txt fresh_drag_begin_boundaries=observed \
  window-drag/results.txt gtk_completion_edge=observed \
  window-drag/results.txt idle_canvas_handoff_survival=pass \
  window-drag/results.txt idle_canvas_drag_handoff=observed \
  window-drag/results.txt fatal_diagnostics=absent \
  window-drag/xvfb-evidence.txt status=pass \
  window-drag/dbus-evidence.txt status=pass \
  -- \
  "$DRAG_SMOKE" "$BINARY" "$OUT_DIR/window-drag"
run_smoke \
  single_monitor_window_fit \
  window-fit/series-evidence.txt \
  window-fit/series-evidence.txt "source_sha=$SOURCE_SHA" \
  window-fit/series-evidence.txt required_consecutive_runs=3 \
  window-fit/series-evidence.txt completed_consecutive_runs=3 \
  window-fit/series-evidence.txt status=pass \
  window-fit/run-1/fit-evidence.txt logged_monitor_workarea_containment=pass \
  window-fit/run-1/fit-session-evidence.txt status=pass \
  window-fit/run-1/fit-xvfb-evidence.txt status=pass \
  window-fit/run-2/fit-evidence.txt logged_monitor_workarea_containment=pass \
  window-fit/run-2/fit-session-evidence.txt status=pass \
  window-fit/run-2/fit-xvfb-evidence.txt status=pass \
  window-fit/run-3/fit-evidence.txt logged_monitor_workarea_containment=pass \
  window-fit/run-3/fit-session-evidence.txt status=pass \
  window-fit/run-3/fit-xvfb-evidence.txt status=pass \
  -- \
  env OKP_WINDOW_FIT_SOURCE_SHA="$SOURCE_SHA" \
  "$FIT_SERIES" "$BINARY" "$OUT_DIR/window-fit"

{
  printf 'source_sha=%s\n' "$SOURCE_SHA"
  printf 'window_drag_status=%s\n' \
    "$(awk -F '\t' '$1 == "non_osc_window_drag" { print tolower($2) }' "$OUT_DIR/results.tsv")"
  printf 'window_fit_status=%s\n' \
    "$(awk -F '\t' '$1 == "single_monitor_window_fit" { print tolower($2) }' "$OUT_DIR/results.tsv")"
  if (( failed == 0 )); then
    printf 'status=pass\n'
  else
    printf 'status=fail\n'
  fi
} >"$OUT_DIR/summary.env"

(
  cd "$OUT_DIR"
  for evidence_file in \
    results.tsv \
    summary.env \
    window-drag/results.txt \
    window-drag/xvfb-evidence.txt \
    window-drag/dbus-evidence.txt \
    window-fit/series-evidence.txt \
    window-fit/run-{1,2,3}/fit-evidence.txt \
    window-fit/run-{1,2,3}/fit-session-evidence.txt \
    window-fit/run-{1,2,3}/fit-xvfb-evidence.txt; do
    [[ -f "$evidence_file" ]] || continue
    sha256sum "$evidence_file"
  done
) >"$OUT_DIR/SHA256SUMS"

if (( failed != 0 )); then
  echo "Linux window regression smokes failed. Results: $OUT_DIR/results.tsv" >&2
  exit 1
fi
echo "Linux window regression smokes passed. Results: $OUT_DIR/results.tsv"
