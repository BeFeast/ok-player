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
done
EOF
chmod +x "$fit_series"

source_sha=1111111111111111111111111111111111111111
unsafe_output="$TEST_ROOT/unsafe-output"
mkdir -p "$unsafe_output"
unsafe_binary="$unsafe_output/ok-player"
cp "$binary" "$unsafe_binary"
set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$unsafe_binary" "$unsafe_output" >/dev/null 2>&1
unsafe_status=$?
set -e
[[ "$unsafe_status" == 2 ]] || fail "unsafe output returned $unsafe_status instead of 2"
[[ -x "$unsafe_binary" ]] || fail 'unsafe output validation deleted the candidate binary'

pass_output="$TEST_ROOT/pass"
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$pass_output" >/dev/null

assert_contains "$pass_output/metadata.env" 'operator_seat_required=false'
assert_contains "$pass_output/metadata.env" 'live_dual_head_proven=false'
assert_contains "$pass_output/metadata.env" 'status=pass'
assert_contains "$pass_output/results.tsv" $'non_osc_window_drag\tPASS'
assert_contains "$pass_output/results.tsv" $'single_monitor_initial_fit\tPASS'
assert_contains "$pass_output/SHA256SUMS" 'window-drag/results.txt'
assert_contains "$pass_output/SHA256SUMS" 'window-fit/run-3/fit-evidence.txt'

failing_drag="$TEST_ROOT/failing-drag"
cat >"$failing_drag" <<'EOF'
#!/usr/bin/env bash
exit 23
EOF
chmod +x "$failing_drag"
fit_marker="$TEST_ROOT/fit-ran"
unexpected_fit="$TEST_ROOT/unexpected-fit"
cat >"$unexpected_fit" <<EOF
#!/usr/bin/env bash
touch "$fit_marker"
EOF
chmod +x "$unexpected_fit"

set +e
OKP_WINDOW_DRAG_SMOKE="$failing_drag" \
OKP_WINDOW_FIT_SERIES="$unexpected_fit" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$TEST_ROOT/fail" >/dev/null 2>&1
failure_status=$?
set -e
[[ "$failure_status" == 23 ]] || fail "drag failure returned $failure_status instead of 23"
[[ ! -e "$fit_marker" ]] || fail 'window-fit series ran after the drag regression failed'
assert_contains "$TEST_ROOT/fail/results.tsv" $'non_osc_window_drag\tFAIL\texit=23'
assert_contains "$TEST_ROOT/fail/results.tsv" $'single_monitor_initial_fit\tNOT RUN'
assert_contains "$TEST_ROOT/fail/metadata.env" 'status=fail'

failing_fit="$TEST_ROOT/failing-fit"
cat >"$failing_fit" <<'EOF'
#!/usr/bin/env bash
exit 31
EOF
chmod +x "$failing_fit"
set +e
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$failing_fit" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$HARNESS" "$binary" "$TEST_ROOT/fit-fail" >/dev/null 2>&1
fit_failure_status=$?
set -e
[[ "$fit_failure_status" == 31 ]] ||
  fail "window-fit failure returned $fit_failure_status instead of 31"
assert_contains "$TEST_ROOT/fit-fail/results.tsv" $'non_osc_window_drag\tPASS'
assert_contains "$TEST_ROOT/fit-fail/results.tsv" $'single_monitor_initial_fit\tFAIL\texit=31'
assert_contains "$TEST_ROOT/fit-fail/metadata.env" 'status=fail'

printf 'Linux window regression harness tests passed.\n'
