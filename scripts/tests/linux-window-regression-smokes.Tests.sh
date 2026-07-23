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

binary="$TEST_ROOT/okp-linux-gtk"
drag_smoke="$TEST_ROOT/drag-smoke"
fit_series="$TEST_ROOT/fit-series"
source_sha=1111111111111111111111111111111111111111
cat >"$binary" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$drag_smoke" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'video_surface_handoff_survival=pass\n' >"$2/results.txt"
printf 'status=pass\n' >"$2/xvfb-evidence.txt"
printf 'status=pass\n' >"$2/dbus-evidence.txt"
EOF
cat >"$fit_series" <<'EOF'
#!/usr/bin/env bash
mkdir -p "$2"
printf 'source_sha=%s\nstatus=pass\n' "$OKP_WINDOW_FIT_SOURCE_SHA" >"$2/series-evidence.txt"
for run in 1 2 3; do
  mkdir -p "$2/run-$run"
  printf 'run=%s\nstatus=pass\n' "$run" >"$2/run-$run/fit-evidence.txt"
  printf 'run=%s\nstatus=pass\n' "$run" >"$2/run-$run/fit-session-evidence.txt"
  printf 'run=%s\nstatus=pass\n' "$run" >"$2/run-$run/fit-xvfb-evidence.txt"
done
EOF
chmod +x "$binary" "$drag_smoke" "$fit_series"

if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  "$RUNNER" "$binary" "$TEST_ROOT/missing-source" >/dev/null 2>&1; then
  fail 'runner accepted evidence without the tested binary source SHA'
fi
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA=not-a-commit \
  "$RUNNER" "$binary" "$TEST_ROOT/invalid-source" >/dev/null 2>&1; then
  fail 'runner accepted an invalid tested binary source SHA'
fi

pass_output="$TEST_ROOT/pass"
OKP_WINDOW_DRAG_SMOKE="$drag_smoke" \
OKP_WINDOW_FIT_SERIES="$fit_series" \
OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$RUNNER" "$binary" "$pass_output" >/dev/null
assert_contains "$pass_output/results.tsv" $'non_osc_window_drag\tPASS'
assert_contains "$pass_output/results.tsv" $'single_monitor_window_fit\tPASS'
assert_contains "$pass_output/summary.env" 'status=pass'
assert_contains "$pass_output/window-fit/series-evidence.txt" \
  "source_sha=$source_sha"
[[ -s "$pass_output/SHA256SUMS" ]] || fail 'runner did not bind its evidence files'
for run in 1 2 3; do
  assert_contains "$pass_output/SHA256SUMS" "window-fit/run-$run/fit-evidence.txt"
  assert_contains "$pass_output/SHA256SUMS" "window-fit/run-$run/fit-session-evidence.txt"
  assert_contains "$pass_output/SHA256SUMS" "window-fit/run-$run/fit-xvfb-evidence.txt"
done

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
printf 'status=pass\n' >"\$2/series-evidence.txt"
printf 'ran\n' >"$fit_marker"
EOF
chmod +x "$drag_smoke" "$fit_series"

fail_output="$TEST_ROOT/fail"
if OKP_WINDOW_DRAG_SMOKE="$drag_smoke" OKP_WINDOW_FIT_SERIES="$fit_series" \
  OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
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
  OKP_WINDOW_REGRESSION_SOURCE_SHA="$source_sha" \
  "$RUNNER" "$binary" "$missing_output" >/dev/null 2>&1; then
  fail 'runner passed when a successful smoke omitted its evidence file'
fi
assert_contains "$missing_output/results.tsv" \
  $'non_osc_window_drag\tFAIL\tmissing evidence=window-drag/results.txt'

printf 'Linux window regression smoke runner tests passed.\n'
