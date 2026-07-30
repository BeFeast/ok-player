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
# The last section runs `scripts/package-linux-deb.sh` for real against a fixture root — the
# compiler, the bundled mpv runtime and its verifier stubbed, the control-file emission and
# `dpkg-deb` genuine — and reads the version back out of the `.deb` it wrote. What the rpm lane
# emits is asserted the same way, out of the SRPM, in scripts/run-linux-rpm-checks.sh, which is
# where an rpm is actually built.
#
# Runs on any Linux host with dpkg and dpkg-deb. No network, no container, no compiler.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
HELPER="$ROOT/scripts/linux-package-version.sh"

for tool in dpkg dpkg-deb; do
  command -v "$tool" >/dev/null 2>&1 \
    || { printf 'the package version tests require %s, which is not on PATH\n' "$tool" >&2; exit 1; }
done

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

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

# --- 5. The package the packaging actually produces -----------------------------------------
# Not a reading of the script: `scripts/package-linux-deb.sh` is run, and the `Version:` field
# is read back out of the `.deb` it wrote. Everything the packaging needs but this assertion
# does not is stubbed in a fixture root — the compiler, the bundled mpv runtime and its
# verifier — so what runs is the real control-file emission and the real `dpkg-deb`.
FIXTURE="$WORK/repo"
STUB_BIN="$WORK/stub-bin"
mkdir -p \
  "$FIXTURE/scripts" \
  "$FIXTURE/rust/packaging/linux/icons/hicolor" \
  "$FIXTURE/rust/target/release" \
  "$FIXTURE/mpv-runtime" \
  "$STUB_BIN"
cp "$ROOT/scripts/package-linux-deb.sh" "$ROOT/scripts/linux-package-version.sh" \
  "$ROOT/scripts/stage-license-documents.sh" "$FIXTURE/scripts/"
# The real licence documents, not stubs: this fixture runs the real packaging
# and the real dpkg-deb, so it is also the cheapest place to prove the shipped
# artifact carries them (issue #743).
cp "$ROOT/LICENSE" "$ROOT/LICENSE.LGPL-3.0" "$ROOT/THIRD-PARTY-NOTICES.md" "$FIXTURE/"
cp "$ROOT/rust/packaging/linux/copyright" "$FIXTURE/rust/packaging/linux/copyright"
cat >"$FIXTURE/scripts/linux-bundled-mpv-env.sh" <<'STUB'
okp_use_linux_bundled_mpv() { export OKP_BUNDLED_MPV_RUNTIME_DIR="$OKP_TEST_MPV_RUNTIME"; }
STUB
printf '#!/usr/bin/env bash\nexit 0\n' >"$FIXTURE/scripts/verify-linux-bundled-mpv.sh"
printf '#!/bin/sh\nexit 0\n' >"$STUB_BIN/cargo"
chmod 755 "$FIXTURE/scripts/verify-linux-bundled-mpv.sh" "$STUB_BIN/cargo"
printf '[Desktop Entry]\nName=OK Player\n' \
  >"$FIXTURE/rust/packaging/linux/com.befeast.okplayer.desktop"
printf '<component type="desktop-application"/>\n' \
  >"$FIXTURE/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"
printf '<svg/>\n' >"$FIXTURE/rust/packaging/linux/com.befeast.okplayer.svg"
for size in 16 24 32 48 64; do
  mkdir -p "$FIXTURE/rust/packaging/linux/icons/hicolor/${size}x${size}/apps"
  printf '<svg/>\n' \
    >"$FIXTURE/rust/packaging/linux/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done
printf '#!/bin/sh\nexit 0\n' >"$FIXTURE/rust/target/release/okp-linux-gtk"
chmod 755 "$FIXTURE/rust/target/release/okp-linux-gtk"
printf 'bundled runtime\n' >"$FIXTURE/mpv-runtime/libmpv.so.2"

package_deb() {
  # package_deb <build version> — returns the path of the .deb the packaging wrote.
  local build="$1"
  env -u CARGO_TARGET_DIR PATH="$STUB_BIN:$PATH" \
    OKP_TEST_MPV_RUNTIME="$FIXTURE/mpv-runtime" \
    bash "$FIXTURE/scripts/package-linux-deb.sh" "$build" >/dev/null 2>"$WORK/package.err" || {
    fail "packaging run" "package-linux-deb.sh failed for ${build}: $(cat "$WORK/package.err")"
    return 1
  }
  printf '%s/artifacts/linux/deb/ok-player_%s_amd64.deb' "$FIXTURE" "$build"
}

CANDIDATE_DEB="$(package_deb 0.11.0-beta.0.209)"
RELEASE_DEB="$(package_deb 0.11.0)"

if [[ -f "$CANDIDATE_DEB" && -f "$RELEASE_DEB" ]]; then
  pass "the packaging names its artifacts by the build version"
else
  fail "artifact naming" "expected $CANDIDATE_DEB and $RELEASE_DEB"
fi

# Issue #743: the .deb reached the public candidate channel carrying no licence
# document at all. Asked of the artifact dpkg-deb just wrote, not of the script
# that wrote it.
if [[ -f "$CANDIDATE_DEB" ]]; then
  PACKAGED_CONTENTS="$(dpkg-deb -c "$CANDIDATE_DEB")"
  missing_documents=()
  for document in copyright LICENSE LICENSE.LGPL-3.0 THIRD-PARTY-NOTICES.md; do
    grep -q " ./usr/share/doc/ok-player/${document}\$" <<<"$PACKAGED_CONTENTS" \
      || missing_documents+=("$document")
  done
  if (( ${#missing_documents[@]} == 0 )); then
    pass "the produced .deb installs its licence documents under /usr/share/doc/ok-player"
  else
    fail "packaged licence documents" \
      "the .deb ships no ${missing_documents[*]} under /usr/share/doc/ok-player"
  fi
fi

PACKAGED_CANDIDATE="$(dpkg-deb -f "$CANDIDATE_DEB" Version)"
PACKAGED_RELEASE="$(dpkg-deb -f "$RELEASE_DEB" Version)"
if [[ "$PACKAGED_CANDIDATE" == "$(okp_debian_version_for_build 0.11.0-beta.0.209)" ]] \
  && [[ "$PACKAGED_RELEASE" == "$(okp_debian_version_for_build 0.11.0)" ]]; then
  pass "the produced .deb carries the encoded version ($PACKAGED_CANDIDATE, $PACKAGED_RELEASE)"
else
  fail "packaged version" \
    "the .deb files carry $PACKAGED_CANDIDATE and $PACKAGED_RELEASE"
fi

# The properties that matter, asked of the packages the packaging produced rather than of
# strings this test composed.
expect_order "the produced release outranks the produced candidate" \
  "$PACKAGED_CANDIDATE" "$PACKAGED_RELEASE"
expect_order "the produced candidate outranks what testers have installed" \
  "$INSTALLED" "$PACKAGED_CANDIDATE"
if [[ "$(okp_build_version_from_debian "$PACKAGED_CANDIDATE")" == "0.11.0-beta.0.209" ]]; then
  pass "the produced .deb reads back as the build it was made from"
else
  fail "packaged round trip" "$PACKAGED_CANDIDATE reads back as $(okp_build_version_from_debian "$PACKAGED_CANDIDATE")"
fi

if ((failures > 0)); then
  printf '%s package version assertion(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'Linux package version scheme tests passed\n'
