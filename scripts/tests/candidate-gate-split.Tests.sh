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
if grep -q 'ADVISORY_FAILURES\[@\]} > 0' "$BUILDER" && grep -q 'exit 1' "$BUILDER"; then
  pass "an advisory failure still exits nonzero"
else
  fail "an advisory failure must still exit nonzero" "silence would hide a real regression"
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

printf '\n%s passed, %s failed\n' "$pass_count" "$fail_count"
((fail_count == 0)) || exit 1
echo "Candidate gate split tests passed"
