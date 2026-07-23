#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNNER="$ROOT/scripts/run-linux-window-regression-smokes.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local file="$1" expected="$2"
  local contents
  contents="$(<"$file")"
  [[ "$contents" == *"$expected"* ]] || fail "$file does not contain: $expected"
}

assert_not_contains() {
  local file="$1" unexpected="$2"
  local contents
  contents="$(<"$file")"
  [[ "$contents" != *"$unexpected"* ]] || fail "$file unexpectedly contains: $unexpected"
}

drag_smoke_source="$ROOT/scripts/smoke-linux-window-drag.sh"
assert_contains "$drag_smoke_source" 'begin_drag_sequence()'
assert_contains "$drag_smoke_source" 'wait_for_drag_sequence_handoff()'
assert_contains "$drag_smoke_source" '$0 == "interaction: player-window-move sequence=" expected'
[[ "$(grep -Fc 'idle_sequence="$(begin_drag_sequence' "$drag_smoke_source")" -ge 2 ]] || \
  fail 'idle retry does not start and bind a fresh GTK drag sequence'
assert_not_contains "$drag_smoke_source" 'idle_previous_handoffs='
assert_not_contains "$drag_smoke_source" 'idle_completions='

binary="$TEST_ROOT/okp-linux-gtk"
drag_smoke="$TEST_ROOT/drag-smoke"
fit_series="$TEST_ROOT/fit-series"
cat >"$binary" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf '%s\n' \
  'video_surface_handoff_survival=pass' \
  'video_surface_drag_handoff=observed' \
  'compositor_cancel_survival=pass' \
  'compositor_cancel_drag_handoff=observed' \
  'post_cancel_drag=pass' \
  'post_cancel_drag_handoff=observed' \
  'fresh_drag_begin_boundaries=observed' \
  'gtk_completion_edge=observed' \
  'idle_canvas_handoff_survival=pass' \
  'idle_canvas_drag_handoff=observed' \
  'fatal_diagnostics=absent' >"$2/results.txt"
printf 'status=pass\n' >"$2/xvfb-evidence.txt"
printf 'status=pass\n' >"$2/dbus-evidence.txt"
EOF
cat >"$fit_series" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'source_sha=%s\nrequired_consecutive_runs=3\ncompleted_consecutive_runs=3\nstatus=pass\n' \
  "$OKP_WINDOW_FIT_SOURCE_SHA" >"$2/series-evidence.txt"
for run in 1 2 3; do
  mkdir -p "$2/run-$run"
  printf 'logged_monitor_workarea_containment=pass\nstatus=pass\n' \
    >"$2/run-$run/fit-evidence.txt"
  printf 'status=pass\n' >"$2/run-$run/fit-session-evidence.txt"
  printf 'status=pass\n' >"$2/run-$run/fit-xvfb-evidence.txt"
done
EOF
chmod +x "$binary" "$drag_smoke" "$fit_series"

existing_output="$TEST_ROOT/existing"
mkdir -p "$existing_output"
printf 'preserve\n' >"$existing_output/sentinel"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=1111111111111111111111111111111111111111 \
  "$RUNNER" "$binary" "$existing_output" >/dev/null 2>&1; then
  fail 'runner accepted a pre-existing output directory'
fi
assert_contains "$existing_output/sentinel" 'preserve'

exported_root="$TEST_ROOT/exported"
mkdir -p "$exported_root/scripts"
cp "$RUNNER" "$exported_root/scripts/run-linux-window-regression-smokes.sh"
missing_sha_error="$TEST_ROOT/missing-sha.error"
if env -u OKP_WINDOW_REGRESSION_SOURCE_SHA \
  OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  "$exported_root/scripts/run-linux-window-regression-smokes.sh" \
  "$binary" "$TEST_ROOT/exported-output" >/dev/null 2>"$missing_sha_error"; then
  fail 'runner passed without an exact source SHA or Git metadata'
fi
assert_contains "$missing_sha_error" \
  'Set OKP_WINDOW_REGRESSION_SOURCE_SHA when Git metadata is unavailable'

pass_output="$TEST_ROOT/pass"
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA=1111111111111111111111111111111111111111 \
  "$RUNNER" "$binary" "$pass_output" >/dev/null
assert_contains "$pass_output/results.tsv" $'non_osc_window_drag\tPASS'
assert_contains "$pass_output/results.tsv" $'single_monitor_window_fit\tPASS'
assert_contains "$pass_output/summary.env" 'status=pass'
assert_contains "$pass_output/window-fit/series-evidence.txt" \
  'source_sha=1111111111111111111111111111111111111111'
[[ -s "$pass_output/SHA256SUMS" ]] || fail 'runner did not bind its evidence files'
assert_contains "$pass_output/SHA256SUMS" 'window-fit/run-3/fit-xvfb-evidence.txt'

cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'video_surface_handoff_survival=pass\n' >"$2/results.txt"
printf 'status=pass\n' >"$2/xvfb-evidence.txt"
printf 'status=pass\n' >"$2/dbus-evidence.txt"
EOF
chmod +x "$drag_smoke"
incomplete_drag_output="$TEST_ROOT/incomplete-drag"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=1111111111111111111111111111111111111111 \
  "$RUNNER" "$binary" "$incomplete_drag_output" >/dev/null 2>&1; then
  fail 'runner passed when zero-exit drag evidence omitted required assertions'
fi
assert_contains "$incomplete_drag_output/results.tsv" \
  $'non_osc_window_drag\tFAIL\tmissing exact evidence=video_surface_drag_handoff=observed'

cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf '%s\n' \
  'video_surface_handoff_survival=pass' \
  'video_surface_drag_handoff=observed' \
  'compositor_cancel_survival=pass' \
  'post_cancel_drag=pass' \
  'post_cancel_drag_handoff=observed' \
  'fresh_drag_begin_boundaries=observed' \
  'gtk_completion_edge=observed' \
  'idle_canvas_handoff_survival=pass' \
  'idle_canvas_drag_handoff=observed' \
  'fatal_diagnostics=absent' >"$2/results.txt"
printf 'status=pass\n' >"$2/xvfb-evidence.txt"
printf 'status=pass\n' >"$2/dbus-evidence.txt"
EOF
chmod +x "$drag_smoke"
missing_phase_output="$TEST_ROOT/missing-drag-handoff-phase"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=1111111111111111111111111111111111111111 \
  "$RUNNER" "$binary" "$missing_phase_output" >/dev/null 2>&1; then
  fail 'runner passed when drag evidence omitted a phase-specific native handoff'
fi
assert_contains "$missing_phase_output/results.tsv" \
  $'non_osc_window_drag\tFAIL\tmissing exact evidence=compositor_cancel_drag_handoff=observed'

cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf '%s\n' \
  'video_surface_handoff_survival=pass' \
  'video_surface_drag_handoff=observed' \
  'compositor_cancel_survival=pass' \
  'compositor_cancel_drag_handoff=observed' \
  'post_cancel_drag=pass' \
  'post_cancel_drag_handoff=observed' \
  'fresh_drag_begin_boundaries=observed' \
  'gtk_completion_edge=observed' \
  'idle_canvas_handoff_survival=pass' \
  'idle_canvas_drag_handoff=observed' \
  'fatal_diagnostics=absent' >"$2/results.txt"
printf 'status=pass\n' >"$2/xvfb-evidence.txt"
printf 'status=pass\n' >"$2/dbus-evidence.txt"
EOF
cat >"$fit_series" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'source_sha=%s\nrequired_consecutive_runs=3\nstatus=pass\n' \
  "$OKP_WINDOW_FIT_SOURCE_SHA" >"$2/series-evidence.txt"
for run in 1 2 3; do
  mkdir -p "$2/run-$run"
  printf 'logged_monitor_workarea_containment=pass\nstatus=pass\n' \
    >"$2/run-$run/fit-evidence.txt"
  printf 'status=pass\n' >"$2/run-$run/fit-session-evidence.txt"
  printf 'status=pass\n' >"$2/run-$run/fit-xvfb-evidence.txt"
done
EOF
chmod +x "$drag_smoke" "$fit_series"
incomplete_fit_output="$TEST_ROOT/incomplete-fit"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=1111111111111111111111111111111111111111 \
  "$RUNNER" "$binary" "$incomplete_fit_output" >/dev/null 2>&1; then
  fail 'runner passed when zero-exit fit evidence omitted the completion marker'
fi
assert_contains "$incomplete_fit_output/results.tsv" \
  $'single_monitor_window_fit\tFAIL\tmissing exact evidence=completed_consecutive_runs=3'

cat >"$fit_series" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'source_sha=3333333333333333333333333333333333333333\nstatus=pass\n' \
  >"$2/series-evidence.txt"
EOF
chmod +x "$fit_series"
mismatched_output="$TEST_ROOT/mismatched-source"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=2222222222222222222222222222222222222222 \
  "$RUNNER" "$binary" "$mismatched_output" >/dev/null 2>&1; then
  fail 'runner passed when fit evidence named a different source SHA'
fi
assert_contains "$mismatched_output/results.tsv" \
  $'single_monitor_window_fit\tFAIL\tmissing exact evidence=source_sha=2222222222222222222222222222222222222222'
assert_contains "$mismatched_output/summary.env" 'status=fail'

cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'video_surface_handoff_survival=fail\n' >"$2/results.txt"
exit 19
EOF
fit_marker="$TEST_ROOT/fit-ran"
cat >"$fit_series" <<EOF
#!/usr/bin/env bash
mkdir -p "\$2"
printf 'source_sha=%s\nrequired_consecutive_runs=3\ncompleted_consecutive_runs=3\nstatus=pass\n' \
  "\$OKP_WINDOW_FIT_SOURCE_SHA" \
  >"\$2/series-evidence.txt"
for run in 1 2 3; do
  mkdir -p "\$2/run-\$run"
  printf 'logged_monitor_workarea_containment=pass\nstatus=pass\n' \
    >"\$2/run-\$run/fit-evidence.txt"
  printf 'status=pass\n' >"\$2/run-\$run/fit-session-evidence.txt"
  printf 'status=pass\n' >"\$2/run-\$run/fit-xvfb-evidence.txt"
done
printf 'ran\n' >"$fit_marker"
EOF
chmod +x "$drag_smoke" "$fit_series"

fail_output="$TEST_ROOT/fail"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  "$RUNNER" "$binary" "$fail_output" >/dev/null 2>&1; then
  fail 'runner passed when the drag regression smoke failed'
fi
assert_contains "$fail_output/results.tsv" $'non_osc_window_drag\tFAIL\texit=19'
assert_contains "$fail_output/results.tsv" $'single_monitor_window_fit\tPASS'
assert_contains "$fail_output/summary.env" 'status=fail'
[[ -f "$fit_marker" ]] || fail 'fit smoke did not run after the drag smoke failed'

cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
exit 0
EOF
chmod +x "$drag_smoke"
missing_output="$TEST_ROOT/missing-evidence"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  "$RUNNER" "$binary" "$missing_output" >/dev/null 2>&1; then
  fail 'runner passed when a successful smoke omitted its evidence file'
fi
assert_contains "$missing_output/results.tsv" \
  $'non_osc_window_drag\tFAIL\tmissing evidence=window-drag/results.txt'

printf 'Linux window regression smoke runner tests passed.\n'
