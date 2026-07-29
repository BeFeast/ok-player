#!/usr/bin/env bash
# Behavioural tests for scripts/linux-package-version.sh (issue #709).
#
# The encoding is only worth having while dpkg agrees with it, so every ordering claim below
# is asked of `dpkg --compare-versions` rather than reasoned about. The four properties the
# scheme has to satisfy, and which nothing else in the gate would notice breaking:
#
#   1. a new candidate must be an upgrade from 0.11.0-beta.0.208, the newest build published
#      before the encoding existed — otherwise every tester on the candidate suite is stranded;
#   2. a release must outrank every candidate that preceded it — otherwise the APT lane can
#      never ship a stable version at all, which is the shipping blocker;
#   3. candidates must still order among themselves, including across a decimal boundary;
#   4. the encoding must round-trip, so the version a user sees and the version apt compares
#      are provably the same build.
#
# Each is followed by its negative control: the same claim under the scheme it replaces, shown
# to be false. Without those the assertions would pass just as happily over a scheme that
# never fixed anything.
#
# Runs on any Linux host with dpkg. No network, no container.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
HELPER="$ROOT/scripts/linux-package-version.sh"

command -v dpkg >/dev/null 2>&1 \
  || { printf 'the package version tests require dpkg, which is not on PATH\n' >&2; exit 1; }

# shellcheck source=/dev/null
source "$HELPER"

failures=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2" >&2; failures=$((failures + 1)); }

expect_order() {
  # expect_order <label> <lower> <higher>
  local label="$1" lower="$2" higher="$3"
  if dpkg --compare-versions "$lower" lt "$higher"; then
    pass "$label ($lower < $higher, per dpkg)"
  else
    fail "$label" "dpkg does not order $lower below $higher"
  fi
}

expect_not_order() {
  # expect_not_order <label> <lower> <higher> — the negative control of the claim above.
  local label="$1" lower="$2" higher="$3"
  if dpkg --compare-versions "$lower" lt "$higher"; then
    fail "$label" "$lower already sorts below $higher, so the control proves nothing"
  else
    pass "$label ($lower is NOT below $higher, which is why the scheme changed)"
  fi
}

# --- 1. The encoding, both ways ------------------------------------------------------------
# The same table okp_core::package_version drives, so the two implementations cannot drift.
ENCODINGS=(
  '0.11.0-beta.0.209|1:0.11.0~beta.0.209'
  '0.11.0-beta.0.9|1:0.11.0~beta.0.9'
  '0.11.0-beta.0.10|1:0.11.0~beta.0.10'
  '0.11.0-beta.1|1:0.11.0~beta.1'
  '0.11.0-alpha.109|1:0.11.0~alpha.109'
  '0.11.0|1:0.11.0'
  '1.0.0|1:1.0.0'
  '0.1.0-linux-alpha.112|1:0.1.0~linux~alpha.112'
)
encoding_failures=0
for pair in "${ENCODINGS[@]}"; do
  build="${pair%%|*}"
  expected="${pair##*|}"
  actual="$(okp_debian_version_for_build "$build")"
  if [[ "$actual" != "$expected" ]]; then
    fail "encoding" "$build encodes to $actual, expected $expected"
    encoding_failures=$((encoding_failures + 1))
    continue
  fi
  back="$(okp_build_version_from_debian "$expected")"
  if [[ "$back" != "$build" ]]; then
    fail "round trip" "$expected reads back as $back, expected $build"
    encoding_failures=$((encoding_failures + 1))
    continue
  fi
  # Nothing this packaging emits may carry a Debian revision: the tail after a `-` is what
  # would outrank the release, which is the whole defect.
  if [[ "${expected#*:}" == *-* ]]; then
    fail "no revision" "$expected carries a Debian revision"
    encoding_failures=$((encoding_failures + 1))
  fi
done
((encoding_failures > 0)) \
  || pass "every published shape encodes, round-trips, and carries no Debian revision"

# rpm shares the substitution and takes no epoch: it forbids `-` outright and already read `~`
# correctly, so its ordering was never wrong and an epoch would be a permanent cost for nothing.
if [[ "$(okp_rpm_version_for_build 0.11.0-beta.1)" == "0.11.0~beta.1" ]] \
  && [[ "$(okp_rpm_version_for_build 0.11.0)" == "0.11.0" ]]; then
  pass "the rpm lane shares the substitution without taking the epoch"
else
  fail "rpm encoding" "got $(okp_rpm_version_for_build 0.11.0-beta.1)"
fi

# --- 2. Refusals ----------------------------------------------------------------------------
# A build version that could be read back as a different one is refused at package time, which
# is the only place it can still be fixed.
refusals=0
for rejected in '' '0.11.0~beta.1' '1:0.11.0' 'v0.11.0' '0.11.0-' '0.11.0 beta' '0.11.0_beta.1'; do
  if okp_debian_version_for_build "$rejected" >/dev/null 2>&1; then
    fail "build version refusal" "$rejected was accepted"
    refusals=$((refusals + 1))
  fi
done
((refusals > 0)) || pass "a build version that would not round-trip is refused"

# A Debian version this packaging never emits establishes nothing, and saying nothing is the
# only honest answer: this is the shape (#708) whose two comparators disagree.
read_refusals=0
for rejected in '' '0.11.0-beta.0.208' '0.11.0~beta.0.209' '1.0.0-1' '1:1.0.0-1' '2:0.11.0~beta.1'; do
  if okp_build_version_from_debian "$rejected" >/dev/null 2>&1; then
    fail "read-back refusal" "$rejected was read back as a build version"
    read_refusals=$((read_refusals + 1))
  fi
done
((read_refusals > 0)) \
  || pass "a Debian version this packaging never emits is not read back as a build"

# --- 3. The four ordering properties, asked of dpkg -----------------------------------------
INSTALLED="$OKP_DEBIAN_LEGACY_HIGHWATER"
NEXT_CANDIDATE="$(okp_debian_version_for_build 0.11.0-beta.0.209)"
RELEASE="$(okp_debian_version_for_build 0.11.0)"
NINE="$(okp_debian_version_for_build 0.11.0-beta.0.9)"
TEN="$(okp_debian_version_for_build 0.11.0-beta.0.10)"

expect_order "a stranded tester can move" "$INSTALLED" "$NEXT_CANDIDATE"
expect_order "the release outranks the candidate that preceded it" "$NEXT_CANDIDATE" "$RELEASE"
expect_order "the release outranks what is installed today" "$INSTALLED" "$RELEASE"
expect_order "candidates order across a decimal boundary" "$NINE" "$TEN"
expect_order "the next release outranks this one" \
  "$RELEASE" "$(okp_debian_version_for_build 0.11.1-beta.0.1)"
expect_order "and a prerelease of it still sorts below it" \
  "$(okp_debian_version_for_build 0.11.1-beta.0.1)" "$(okp_debian_version_for_build 0.11.1)"

# The stable suite's own history: the legacy release naming must also be outranked, or a
# machine on the last published release could not take the first encoded one either.
expect_order "the encoded release outranks the last legacy stable" \
  "0.1.0-linux-alpha.112" "$RELEASE"

# --- 4. The negative controls ---------------------------------------------------------------
# Why the encoding is not simply the build version:
expect_not_order "the raw build version does not sort below its own release" \
  "0.11.0-beta.0.208" "0.11.0"
# Why `~` alone is not enough — the naive fix strands everybody already installed:
expect_not_order "the tilde alone does not reach what is already installed" \
  "$INSTALLED" "0.11.0~beta.0.209"
# Why the epoch alone is not enough either — it lifts the candidate over the release again:
expect_not_order "the epoch alone does not order the release above the candidate" \
  "1:0.11.0-beta.0.208" "1:0.11.0"

# --- 5. The lanes that have to stay in step -------------------------------------------------
DEB_SCRIPT="$(cat "$ROOT/scripts/package-linux-deb.sh")"
if [[ "$DEB_SCRIPT" == *'DEB_VERSION="$(okp_debian_version_for_build "$VERSION")"'* ]] \
  && [[ "$DEB_SCRIPT" == *'Version: $DEB_VERSION'* ]] \
  && [[ "$DEB_SCRIPT" != *'Version: $VERSION'* ]]; then
  pass "the Debian packaging stamps the encoded version"
else
  fail "deb packaging" "package-linux-deb.sh does not stamp the encoded version"
fi

RPM_SCRIPT="$(cat "$ROOT/scripts/package-linux-rpm-source.sh")"
if [[ "$RPM_SCRIPT" == *'RPM_VERSION="$(okp_rpm_version_for_build "$UPSTREAM_VERSION")"'* ]] \
  && [[ "$RPM_SCRIPT" == *'--define "rpm_version $RPM_VERSION"'* ]]; then
  pass "the rpm source lane derives its version instead of inheriting a stale default"
else
  fail "rpm packaging" "package-linux-rpm-source.sh does not derive rpm_version"
fi

# The spec's own fallback defaults only apply to a bare `rpmbuild`, and nothing else would ever
# notice them drifting apart.
SPEC="$ROOT/rust/packaging/fedora/ok-player.spec"
spec_default() {
  awk -v name="$1" '$0 ~ "^%\\{!\\?" name ":%global " name " " { print $3; exit }' "$SPEC" \
    | tr -d '}'
}
SPEC_UPSTREAM="$(spec_default upstream_version)"
SPEC_RPM="$(spec_default rpm_version)"
if [[ -n "$SPEC_UPSTREAM" && "$SPEC_RPM" == "$(okp_rpm_version_for_build "$SPEC_UPSTREAM")" ]]; then
  pass "the spec's two version defaults are the same build under the shared rule"
else
  fail "spec defaults" \
    "upstream_version=$SPEC_UPSTREAM encodes to $(okp_rpm_version_for_build "$SPEC_UPSTREAM"), but rpm_version=$SPEC_RPM"
fi

if ((failures > 0)); then
  printf '%s package version assertion(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'Linux package version scheme tests passed\n'
