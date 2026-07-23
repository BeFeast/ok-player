#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS="$ROOT/scripts/run-linux-window-regression-smokes.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local file="$1" expected="$2"
  [[ -f "$file" ]] || fail "missing file: $file"
  [[ "$(<"$file")" == *"$expected"* ]] || fail "$file does not contain: $expected"
}

binary="$TEST_ROOT/ok-player"
printf '#!/usr/bin/env bash\nexit 0\n' >"$binary"
chmod +x "$binary"

drag_smoke="$TEST_ROOT/drag-smoke"
cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$2"
cat >"$2/results.txt" <<'RESULTS'
video_surface_handoff_survival=pass
compositor_cancel_survival=pass
post_cancel_drag=pass
idle_canvas_handoff_survival=pass
fatal_diagnostics=absent
RESULTS
cat >"$2/dbus-evidence.txt" <<'EVIDENCE'
session_bus_ready=true
session_bus_teardown=clean
session_process_teardown=clean
command_status=0
status=pass
EVIDENCE
cat >"$2/xvfb-evidence.txt" <<'EVIDENCE'
xvfb_ready=true
xvfb_alive_before_teardown=true
xvfb_teardown=clean
command_status=0
status=pass
EVIDENCE
EOF
chmod +x "$drag_smoke"

fit_series="$TEST_ROOT/fit-series"
cat >"$fit_series" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mkdir -p "$2"
printf 'source_sha=%s\ncompleted_consecutive_runs=3\nstatus=pass\n' \
  "$OKP_WINDOW_FIT_SOURCE_SHA" >"$2/series-evidence.txt"
for run in 1 2 3; do
  mkdir -p "$2/run-$run"
  printf 'logged_monitor_workarea_containment=pass\nstatus=pass\n' \
    >"$2/run-$run/fit-evidence.txt"
  cat >"$2/run-$run/fit-session-evidence.txt" <<'EVIDENCE'
session_bus_ready=true
session_bus_teardown=clean
session_process_teardown=clean
command_status=0
status=pass
EVIDENCE
  cat >"$2/run-$run/fit-xvfb-evidence.txt" <<'EVIDENCE'
xvfb_ready=true
xvfb_alive_before_teardown=true
xvfb_teardown=clean
command_status=0
status=pass
EVIDENCE
done
EOF
chmod +x "$fit_series"

source_sha=1111111111111111111111111111111111111111

existing_output="$TEST_ROOT/existing-output"
mkdir -p "$existing_output"
printf 'keep\n' >"$existing_output/sentinel"
set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$existing_output" >/dev/null 2>&1
existing_status=$?
set -e
[[ "$existing_status" == 2 ]] || fail "existing output returned $existing_status instead of 2"
assert_contains "$existing_output/sentinel" keep

set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$binary" >/dev/null 2>&1
binary_output_status=$?
set -e
[[ "$binary_output_status" == 2 ]] || fail "binary output returned $binary_output_status instead of 2"
[[ -x "$binary" ]] || fail 'output validation deleted the candidate binary'

set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  "$HARNESS" "$binary" "$TEST_ROOT/missing-source" >/dev/null 2>&1
missing_source_status=$?
set -e
[[ "$missing_source_status" == 2 ]] || fail "missing source SHA returned $missing_source_status instead of 2"

pass_output="$TEST_ROOT/pass"
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$pass_output" >/dev/null

assert_contains "$pass_output/metadata.env" "source_sha=$source_sha"
assert_contains "$pass_output/metadata.env" 'operator_seat_required=false'
assert_contains "$pass_output/metadata.env" 'live_dual_head_proven=false'
assert_contains "$pass_output/metadata.env" 'status=pass'
assert_contains "$pass_output/results.tsv" $'non_osc_window_drag\tPASS'
assert_contains "$pass_output/results.tsv" $'single_monitor_initial_fit\tPASS'
assert_contains "$pass_output/SHA256SUMS" 'window-drag/results.txt'
assert_contains "$pass_output/SHA256SUMS" 'window-fit/run-3/fit-session-evidence.txt'
assert_contains "$pass_output/SHA256SUMS" 'window-fit/run-3/fit-xvfb-evidence.txt'

failing_drag="$TEST_ROOT/failing-drag"
cat >"$failing_drag" <<'EOF'
#!/usr/bin/env bash
exit 23
EOF
chmod +x "$failing_drag"
fit_marker="$TEST_ROOT/fit-ran"
marked_fit="$TEST_ROOT/marked-fit"
sed "2a touch '$fit_marker'" "$fit_series" >"$marked_fit"
chmod +x "$marked_fit"

set +e
OKP_WINDOW_DRAG_SMOKE="$failing_drag" \
OKP_WINDOW_FIT_SERIES="$marked_fit" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$TEST_ROOT/drag-fail" >/dev/null 2>&1
drag_failure_status=$?
set -e
[[ "$drag_failure_status" == 1 ]] || fail "drag failure returned $drag_failure_status instead of 1"
[[ -e "$fit_marker" ]] || fail 'window-fit series did not run after the drag regression failed'
assert_contains "$TEST_ROOT/drag-fail/results.tsv" $'non_osc_window_drag\tFAIL\texit=23'
assert_contains "$TEST_ROOT/drag-fail/results.tsv" $'single_monitor_initial_fit\tPASS'
assert_contains "$TEST_ROOT/drag-fail/metadata.env" 'status=fail'

mismatched_fit="$TEST_ROOT/mismatched-fit"
# shellcheck disable=SC2016 # The generated script must replace the literal variable reference.
sed 's/"$OKP_WINDOW_FIT_SOURCE_SHA"/2222222222222222222222222222222222222222/' \
  "$fit_series" >"$mismatched_fit"
chmod +x "$mismatched_fit"
set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$mismatched_fit" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$TEST_ROOT/source-mismatch" >/dev/null 2>&1
mismatch_status=$?
set -e
[[ "$mismatch_status" == 1 ]] || fail "source mismatch returned $mismatch_status instead of 1"
assert_contains "$TEST_ROOT/source-mismatch/results.tsv" \
  $'single_monitor_initial_fit\tFAIL\tfile=window-fit/series-evidence.txt; missing=source_sha='

printf 'Linux window regression harness tests passed.\n'
