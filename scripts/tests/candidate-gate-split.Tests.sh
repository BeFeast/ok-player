#!/usr/bin/env bash
# The candidate builder's gate split: an advisory gate may withhold the *verdict*
# on a build, never the build itself.
#
# This exists because the alternative cost a night. A screenshot check aiming at
# the wrong eight pixels aborted after packaging, so a correct, installable .deb
# sat finished on the builder while the operator waiting for it got nothing.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
BUILDER=scripts/build-linux-candidate.sh
WRAPPER=scripts/run-linux-candidate-workflow.sh
CORE=rust/crates/okp-core/src/candidate_build.rs

pass_count=0
fail_count=0
pass() { printf 'PASS %s\n' "$1"; pass_count=$((pass_count + 1)); }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; fail_count=$((fail_count + 1)); }

# The advisory set, read from the Rust source of truth rather than restated here,
# so the two cannot drift.
mapfile -t advisory < <(
  awk '/^pub const ADVISORY_CANDIDATE_GATES/,/^\];/' "$CORE" |
    grep -oE '"[a-z-]+"' | tr -d '"'
)
if ((${#advisory[@]} == 0)); then
  fail "the advisory set is declared in okp-core" "none found"
  exit 1
fi
pass "the advisory set is declared in okp-core (${#advisory[@]} gates)"

for gate in "${advisory[@]}"; do
  if grep -qE "^run_advisory_gate ${gate}\b" "$BUILDER"; then
    pass "$gate runs as advisory in the builder"
  else
    fail "$gate must run as advisory in the builder" \
      "it aborts the build, so a finished package would be withheld"
  fi
  if grep -qE "^run_gate ${gate}\b" "$BUILDER"; then
    fail "$gate must not also run as blocking" "both forms present"
  fi
done

# The blocking set must stay blocking: without these there is no artifact, or one
# that cannot start, and shipping that would be worse than shipping nothing.
mapfile -t required < <(
  awk '/^pub const REQUIRED_CANDIDATE_GATES/,/^\];/' "$CORE" |
    grep -oE '"[a-z-]+"' | tr -d '"'
)
for gate in "${required[@]}"; do
  if grep -qE "^run_advisory_gate ${gate}\b" "$BUILDER"; then
    fail "$gate is required and must not be advisory" "found as advisory"
  fi
done
pass "no blocking gate was made advisory (${#required[@]} checked)"

# A red run is how the morning report learns about an advisory failure: the split
# trades "no package" for "a package plus a loud complaint", not for silence.
# The status is its own value, not a bare 1, so the wrapper can tell "staged but
# not vouched for" from "failed before there was anything to hand off".
builder_status="$(sed -n 's/^ADVISORY_EXIT_STATUS=\([0-9][0-9]*\).*/\1/p' "$BUILDER" | head -n1)"
wrapper_status="$(sed -n 's/^ADVISORY_EXIT_STATUS=\([0-9][0-9]*\).*/\1/p' "$WRAPPER" | head -n1)"
if grep -q 'ADVISORY_FAILURES\[@\]} > 0' "$BUILDER" &&
  [[ -n "$builder_status" && "$builder_status" != 0 ]]; then
  pass "an advisory failure still exits nonzero (status $builder_status)"
else
  fail "an advisory failure must still exit nonzero" "silence would hide a real regression"
fi

if [[ -n "$wrapper_status" && "$wrapper_status" == "$builder_status" ]]; then
  pass "the builder and the wrapper agree on the advisory exit status"
else
  fail "the builder and the wrapper must agree on the advisory exit status" \
    "builder=${builder_status:-none} wrapper=${wrapper_status:-none}; drift withholds the package again"
fi

# The wrapper must carry that verdict *past* the publisher. Aborting at the
# builder call leaves the staged package unpublished and the split buys nothing.
publish_line="$(grep -n 'publish-linux-candidate.sh' "$WRAPPER" | cut -d: -f1 | head -n1)"
verdict_report_line="$(grep -n 'exit "\$BUILD_STATUS"' "$WRAPPER" | cut -d: -f1 | tail -n1)"
if [[ -n "$publish_line" && -n "$verdict_report_line" ]] && ((verdict_report_line > publish_line)); then
  pass "the wrapper publishes before it reports the advisory verdict"
else
  fail "the wrapper must publish before it reports the advisory verdict" \
    "publish=${publish_line:-none} report=${verdict_report_line:-none}"
fi

# And it must do so only after the bundle is staged, or nothing was delivered.
staged_line="$(grep -n 'Candidate bundle:' "$BUILDER" | cut -d: -f1)"
verdict_line="$(grep -n 'ADVISORY_FAILURES\[@\]} > 0' "$BUILDER" | cut -d: -f1)"
if [[ -n "$staged_line" && -n "$verdict_line" ]] && ((verdict_line > staged_line)); then
  pass "the verdict comes after the bundle is staged"
else
  fail "the verdict must come after the bundle is staged" \
    "staged=${staged_line:-none} verdict=${verdict_line:-none}"
fi

# The feed refresh is where the package reaches apt subscribers. It `needs` the
# build job, so a bare gate skips it whenever that job is red - including the
# red-but-published state this split exists to produce.
#
# That property was asserted here by grepping the workflow for one exact condition
# string, which fits badly in both directions: a comment quoting the string would
# satisfy it, and an equivalent rewrite - `(success() || failure()) && ...` - would
# fail it while delivering correctly. It now lives in
# scripts/tests/feeds-workflow.Tests.sh, which parses release-linux-candidate.yml and
# evaluates the condition against the states this lane actually reaches, including
# promoted-then-reddened. Asserted in one place so the two cannot disagree about what
# counts as delivered.

printf '\n%s passed, %s failed\n' "$pass_count" "$fail_count"
((fail_count == 0)) || exit 1
echo "Candidate gate split tests passed"
