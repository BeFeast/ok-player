#!/usr/bin/env bash
# Behavioural tests for scripts/build-apt-repo.sh (issues #683, #689).
#
# The APT archive is only safe while these properties hold, and none of them is visible in a
# single successful run:
#   1. Re-running over an unchanged release set reproduces the archive content byte for byte,
#      so a rebuild triggered by anything else (a docs push, a Windows release) cannot rewrite
#      what apt clients already have.
#   2. Adding a version is additive: the previous version keeps its pool file and its exact
#      Size/SHA256, so a client mid-download is never handed a changed checksum.
#   3. A key whose fingerprint is not the expected one aborts the lane. Signing with an
#      unexpected key is worse than not publishing at all.
#   4. A missing or unreadable secret aborts the lane, naming the secret, instead of falling
#      through to an unsigned archive.
#   5. The two suites share one pool: a package both carry is stored once, indexed with the
#      same bytes in both, and charged to the rolling-window budget once.
#   6. The rolling window is per suite, and neither suite may starve the other or drop its own
#      current build.
#   7. Both suites are signed by the same key, in the same generator path — the `candidate`
#      suite is not a second, parallel archive.
#
# These drive the real script: they source it and call okp_apt_build_signed_repo — the same
# function main() calls — with a throwaway signing key and a test secret reader passed as its
# parameter. Nothing in the script's executed path is redirected by an environment variable,
# so the abort messages asserted below are the ones production emits. The packages are real
# .deb files built here with dpkg-deb, so dpkg-scanpackages indexes them for real.
#
# Runs on any Linux host with gpg, dpkg-deb, dpkg-scanpackages and gzip. No network, no
# Infisical, no container.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
GENERATOR="$ROOT/scripts/build-apt-repo.sh"
VERIFIER="$ROOT/scripts/verify-apt-repo.sh"
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

# Named up front so a runner image without dpkg-dev says so, instead of failing somewhere
# inside the generator with a message about OpenPGP.
for tool in gpg gpgconf dpkg dpkg-deb dpkg-scanpackages gzip sha256sum md5sum jq; do
  command -v "$tool" >/dev/null 2>&1 \
    || { printf 'APT repository generator tests require %s, which is not on PATH\n' "$tool" >&2; exit 1; }
done

# shellcheck source=/dev/null
source "$GENERATOR"
# The verifier derives "which version should be installed" from the index it is handed, and that
# derivation is asserted below without a container. It exposes okp_apt_verify_main rather than
# main so both files can be sourced here.
# shellcheck source=/dev/null
source "$VERIFIER"

failures=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2" >&2; failures=$((failures + 1)); }

# --- A throwaway signing key, used the way the real one is ----------------------------
# ed25519 so key generation is instant and the suite stays runnable on a headless CI box
# with little entropy; the code under test cares about the fingerprint gate and the loopback
# passphrase, not the algorithm.
KEY_HOME="$WORK/keys"
mkdir -p "$KEY_HOME"
chmod 700 "$KEY_HOME"
TEST_PASSPHRASE='correct horse battery staple'
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --pinentry-mode loopback \
  --passphrase "$TEST_PASSPHRASE" \
  --quick-generate-key 'OK Player Test Signing <test@example.invalid>' ed25519 sign never
TEST_FINGERPRINT="$(
  GNUPGHOME="$KEY_HOME" gpg --batch --with-colons --fingerprint --list-keys \
    | awk -F: '$1 == "fpr" { print $10; exit }'
)"
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --pinentry-mode loopback \
  --passphrase "$TEST_PASSPHRASE" --armor --export-secret-keys \
  --output "$WORK/private.asc" "$TEST_FINGERPRINT"
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --armor --export \
  --output "$WORK/public.asc" "$TEST_FINGERPRINT"

# A second, unrelated key: the material the fingerprint gate must reject.
OTHER_HOME="$WORK/other-keys"
mkdir -p "$OTHER_HOME"
chmod 700 "$OTHER_HOME"
GNUPGHOME="$OTHER_HOME" gpg --batch --quiet --pinentry-mode loopback \
  --passphrase "$TEST_PASSPHRASE" \
  --quick-generate-key 'Somebody Else <other@example.invalid>' ed25519 sign never
OTHER_FINGERPRINT="$(
  GNUPGHOME="$OTHER_HOME" gpg --batch --with-colons --fingerprint --list-keys \
    | awk -F: '$1 == "fpr" { print $10; exit }'
)"
GNUPGHOME="$OTHER_HOME" gpg --batch --quiet --pinentry-mode loopback \
  --passphrase "$TEST_PASSPHRASE" --armor --export-secret-keys \
  --output "$WORK/other-private.asc" "$OTHER_FINGERPRINT"

# The generator now also holds the signing key to the fingerprint the packaging ships
# (#726). These tests sign with the throwaway key above, so they say so — by assignment, since
# the suite sources the generator; there is no environment override a release build could take.
OKP_APT_SIGNING_FINGERPRINT="$TEST_FINGERPRINT"

# --- The test secret source, pinned in place of the Infisical reader -------------------
# SECRET_DIR holds one file per secret name; deleting one models a secret that Infisical
# cannot serve, which is what the missing-secret test does.
SECRET_DIR="$WORK/secrets"
mkdir -p "$SECRET_DIR"
seed_secrets() {
  local private="${1:-$WORK/private.asc}" fingerprint="${2:-$TEST_FINGERPRINT}"
  cp "$private" "$SECRET_DIR/gpg-private-key"
  cp "$WORK/public.asc" "$SECRET_DIR/gpg-public-key"
  printf '%s\n' "$fingerprint" >"$SECRET_DIR/gpg-fingerprint"
  printf '%s\n' "$TEST_PASSPHRASE" >"$SECRET_DIR/gpg-passphrase"
}
test_secret_reader() {
  local name="$1"
  [[ -f "$SECRET_DIR/$name" ]] || return 1
  cat "$SECRET_DIR/$name"
}

# The archive may only be signed by the key every .deb carries (#726). A rotation that moved
# the Infisical secrets without the committed key would otherwise publish an archive that no
# installed keyring can verify, with both builds green.
seed_secrets "$WORK/other-private.asc" "$OTHER_FINGERPRINT"
OKP_APT_SIGNING_FINGERPRINT="$TEST_FINGERPRINT"
OKP_APT_ADDITIONAL_TRUSTED_FINGERPRINTS=''
if ( okp_apt_import_signing_key "$(okp_apt_make_gnupghome)" \
  "$WORK/other-private.asc" <(printf '%s\n' "$OTHER_FINGERPRINT") ) >/dev/null 2>&1; then
  fail 'committed fingerprint' \
    'the generator signed with a key the packaging does not ship, so no installed client could verify the archive'
else
  pass 'the generator refuses a signing key the packaging does not ship'
fi

# ...and accepts it once the packaging ships it, which is what makes a rotation stageable: the
# keyring carrying both keys is published first, and only then may the signer change. Requiring
# the signer to equal the one committed key would permit only the order that strands every
# installed client, since apt must verify InRelease before it can fetch the new keyring.
OKP_APT_ADDITIONAL_TRUSTED_FINGERPRINTS="$OTHER_FINGERPRINT"
if ( okp_apt_import_signing_key "$(okp_apt_make_gnupghome)" \
  "$WORK/other-private.asc" <(printf '%s\n' "$OTHER_FINGERPRINT") ) >/dev/null 2>&1; then
  pass 'the generator signs with a staged key once the packaging ships it'
else
  fail 'staged rotation' \
    'a key the packaging already ships was refused, so a rotation could only be done in the order that strands clients'
fi
OKP_APT_ADDITIONAL_TRUSTED_FINGERPRINTS=''
seed_secrets
OKP_APT_SIGNING_FINGERPRINT="$TEST_FINGERPRINT"

# --- Real .deb packages ---------------------------------------------------------------
make_deb() {
  # make_deb <build version> <output-dir> [control version]
  #
  # The file name is the build version and the control `Version:` is its Debian encoding, which
  # is how the real lane names things (issue #709). The third argument overrides the control
  # version so a test can build the archive a regression would produce.
  local version="$1" destination="$2" control_version="${3:-$1}"
  local build="$WORK/build/$version"
  rm -rf -- "$build"
  mkdir -p "$build/DEBIAN" "$build/usr/bin"
  printf '#!/bin/sh\necho ok-player %s\n' "$version" >"$build/usr/bin/ok-player"
  chmod 755 "$build/usr/bin/ok-player"
  {
    printf 'Package: ok-player\n'
    printf 'Version: %s\n' "$control_version"
    printf 'Architecture: amd64\n'
    printf 'Maintainer: OK Player <noreply@example.invalid>\n'
    printf 'Description: APT repository generator test package\n'
  } >"$build/DEBIAN/control"
  mkdir -p "$destination"
  dpkg-deb --build --root-owner-group "$build" \
    "$destination/ok-player_${version}_amd64.deb" >/dev/null
}

deb_name() { printf 'ok-player_%s_amd64.deb' "$1"; }

# Fixed epoch: the Release date is an input, not a clock reading, and pinning it here is what
# lets two runs be compared byte for byte at all.
EPOCH=1750000000
BASE_URL='https://example.invalid/ok-player/apt'

# --- Archive plans ---------------------------------------------------------------------
# The generator takes the suite layout as a plan directory, so a test can describe an archive
# without going near GitHub. main() builds the same directory out of the rolling window.
write_plan() {
  # write_plan <plan-dir> <suite>=<epoch>=<comma-separated pool basenames> ...
  local plan="$1"
  shift
  rm -rf -- "$plan"
  mkdir -p "$plan"
  : >"$plan/suites"
  local spec suite epoch members
  for spec in "$@"; do
    suite="${spec%%=*}"
    spec="${spec#*=}"
    epoch="${spec%%=*}"
    members="${spec#*=}"
    printf '%s\n' "$suite" >>"$plan/suites"
    printf '%s\n' "$epoch" >"$plan/$suite.epoch"
    tr ',' '\n' <<<"$members" >"$plan/$suite.members"
  done
}

stage_from_plan() {
  # stage_from_plan <staging-out> <source-dir> <plan-dir>
  # main() downloads exactly the packages the window retained, so a test that wants to observe
  # what the generator does with a plan has to stage exactly the same set.
  local out="$1" source="$2" plan="$3" suite name
  rm -rf -- "$out"
  mkdir -p "$out"
  while IFS= read -r suite; do
    [[ -n "$suite" ]] || continue
    while IFS= read -r name; do
      [[ -n "$name" ]] || continue
      cp -- "$source/$name" "$out/$name"
    done <"$plan/$suite.members"
  done <"$plan/suites"
}

stable_plan_for_staging() {
  # stable_plan_for_staging <plan-dir> <staging> [epoch] — one stable suite over everything staged
  local plan="$1" staging="$2" epoch="${3:-$EPOCH}"
  rm -rf -- "$plan"
  mkdir -p "$plan"
  printf 'stable\n' >"$plan/suites"
  printf '%s\n' "$epoch" >"$plan/stable.epoch"
  ( cd "$staging" && ls -1 -- *.deb ) >"$plan/stable.members"
}

# A subshell on purpose: okp_apt_build_signed_repo installs its own EXIT trap for the
# ephemeral signing home, and an abort inside it must end that build, not this suite.
build_planned() {
  # build_planned <staging> <repo-root> <plan-dir>
  ( okp_apt_build_signed_repo "$1" "$2" "$3" "$BASE_URL" test_secret_reader )
}

build_repo() {
  # build_repo <staging> <repo-root> [epoch] — stable-only over everything staged
  local plan
  plan="$WORK/plan-$(basename -- "$2")"
  stable_plan_for_staging "$plan" "$1" "${3:-$EPOCH}"
  build_planned "$1" "$2" "$plan"
}

# Everything except the two OpenPGP signatures, which carry their own creation time and are
# expected to differ between runs.
content_diff() {
  diff -r --exclude=InRelease --exclude=Release.gpg "$1" "$2"
}

packages_index() {
  # packages_index <repo-root> [suite]
  printf '%s/dists/%s/main/binary-amd64/Packages' "$1" "${2:-stable}"
}

paragraph_for() {
  # paragraph_for <version> <packages-index>
  awk -v want="$1" 'BEGIN { RS = ""; FS = "\n" }
    { for (i = 1; i <= NF; i++) if ($i == "Version: " want) { print; exit } }' "$2"
}

# --- 1. Idempotent re-run --------------------------------------------------------------
seed_secrets
STAGE_ONE="$WORK/stage-one"
make_deb 1.0.0 "$STAGE_ONE"
make_deb 1.1.0 "$STAGE_ONE"
build_repo "$STAGE_ONE" "$WORK/run-a" >/dev/null
build_repo "$STAGE_ONE" "$WORK/run-b" >/dev/null
if content_diff "$WORK/run-a" "$WORK/run-b" >"$WORK/idempotence.diff" 2>&1; then
  pass "re-running over an unchanged release set reproduces the archive byte for byte"
else
  fail "idempotent re-run" "the second run differs: $(head -n 20 "$WORK/idempotence.diff")"
fi

# Regenerating in place, over an already-populated directory, must reach the same state too:
# that is what the lane actually does when Pages is rebuilt.
build_repo "$STAGE_ONE" "$WORK/run-a" >/dev/null
if content_diff "$WORK/run-a" "$WORK/run-b" >"$WORK/idempotence-inplace.diff" 2>&1; then
  pass "regenerating in place over an existing archive reaches the same state"
else
  fail "in-place regeneration" "$(head -n 20 "$WORK/idempotence-inplace.diff")"
fi

# --- 2. The signature is real and made by the expected key ------------------------------
VERIFY_HOME="$WORK/verify"
mkdir -p "$VERIFY_HOME"
chmod 700 "$VERIFY_HOME"
GNUPGHOME="$VERIFY_HOME" gpg --batch --quiet --import "$WORK/run-a/ok-player-archive-keyring.asc"
if GNUPGHOME="$VERIFY_HOME" gpg --batch --status-fd 1 --verify \
     "$WORK/run-a/dists/stable/InRelease" 2>/dev/null \
     | grep -q "VALIDSIG ${TEST_FINGERPRINT}"; then
  pass "InRelease carries a valid signature from the published public key"
else
  fail "InRelease signature" "InRelease does not verify against the published keyring"
fi
if GNUPGHOME="$VERIFY_HOME" gpg --batch --status-fd 1 --verify \
     "$WORK/run-a/dists/stable/Release.gpg" "$WORK/run-a/dists/stable/Release" 2>/dev/null \
     | grep -q "VALIDSIG ${TEST_FINGERPRINT}"; then
  pass "Release.gpg verifies against the published public key"
else
  fail "Release.gpg signature" "the detached signature does not verify"
fi

# --- 3. Adding a version keeps the previous one ------------------------------------------
BEFORE_PARAGRAPH="$(paragraph_for 1.1.0 "$(packages_index "$WORK/run-a")")"
BEFORE_SHA="$(sha256sum "$WORK/run-a/pool/main/o/ok-player/ok-player_1.1.0_amd64.deb" | cut -d' ' -f1)"
STAGE_TWO="$WORK/stage-two"
mkdir -p "$STAGE_TWO"
cp -a "$STAGE_ONE/." "$STAGE_TWO/"
make_deb 1.2.0 "$STAGE_TWO"
build_repo "$STAGE_TWO" "$WORK/run-c" >/dev/null

AFTER_INDEX="$(packages_index "$WORK/run-c")"
if [[ -f "$WORK/run-c/pool/main/o/ok-player/ok-player_1.1.0_amd64.deb" ]]; then
  pass "publishing a new version leaves the previous pool file in place"
else
  fail "additivity (pool)" "ok-player_1.1.0_amd64.deb disappeared from the pool"
fi
if [[ "$(paragraph_for 1.1.0 "$AFTER_INDEX")" == "$BEFORE_PARAGRAPH" ]]; then
  pass "the previous version's Packages paragraph is unchanged, checksums included"
else
  fail "additivity (index)" "the 1.1.0 paragraph changed when 1.2.0 was added"
fi
AFTER_SHA="$(sha256sum "$WORK/run-c/pool/main/o/ok-player/ok-player_1.1.0_amd64.deb" | cut -d' ' -f1)"
if [[ "$AFTER_SHA" == "$BEFORE_SHA" ]]; then
  pass "the previous version's pool bytes are identical after the new release"
else
  fail "additivity (bytes)" "ok-player_1.1.0_amd64.deb was rewritten"
fi
if [[ -n "$(paragraph_for 1.2.0 "$AFTER_INDEX")" ]]; then
  pass "the new version is indexed alongside the ones already published"
else
  fail "additivity (new version)" "1.2.0 is missing from the Packages index"
fi
# An index that mentions a file the pool does not carry is exactly the orphan this archive
# must never produce, so check the whole index, not just the version under test.
orphans=0
while IFS= read -r filename; do
  [[ -f "$WORK/run-c/$filename" ]] || { orphans=$((orphans + 1)); printf 'orphan: %s\n' "$filename" >&2; }
done < <(awk '/^Filename: / { print $2 }' "$AFTER_INDEX")
if ((orphans == 0)); then
  pass "every indexed package resolves to a file in the pool"
else
  fail "orphan check" "$orphans indexed packages have no pool file"
fi

# --- 4. A key that is not the expected one aborts ----------------------------------------
seed_secrets "$WORK/other-private.asc" "$TEST_FINGERPRINT"
set +e
mismatch_output="$(build_repo "$STAGE_ONE" "$WORK/run-mismatch" 2>&1)"
mismatch_status=$?
set -e
if ((mismatch_status != 0)); then
  pass "a signing key whose fingerprint is not the expected one aborts with a non-zero exit"
else
  fail "fingerprint mismatch (exit)" "the build succeeded with an unexpected signing key"
fi
if grep -q 'fingerprint mismatch' <<<"$mismatch_output" \
  && grep -q "$OTHER_FINGERPRINT" <<<"$mismatch_output" \
  && grep -q "$TEST_FINGERPRINT" <<<"$mismatch_output"; then
  pass "the fingerprint-mismatch abort names both the imported and the expected key"
else
  fail "fingerprint mismatch (message)" "message did not name the cause: $mismatch_output"
fi
if [[ ! -e "$WORK/run-mismatch/dists/stable/InRelease" ]]; then
  pass "the fingerprint-mismatch abort publishes nothing"
else
  fail "fingerprint mismatch (output)" "an InRelease was written despite the mismatch"
fi

# --- 5. A secret Infisical cannot serve aborts, naming it ---------------------------------
for missing in gpg-private-key gpg-public-key gpg-fingerprint gpg-passphrase; do
  seed_secrets
  rm -f "$SECRET_DIR/$missing"
  set +e
  absent_output="$(build_repo "$STAGE_ONE" "$WORK/run-absent-$missing" 2>&1)"
  absent_status=$?
  set -e
  if ((absent_status != 0)) && grep -q "services/prod/ok-player:${missing}" <<<"$absent_output"; then
    pass "an unreadable ${missing} aborts with a non-zero exit naming the secret"
  else
    fail "missing secret ${missing}" "status ${absent_status}, output: ${absent_output}"
  fi
  if [[ -e "$WORK/run-absent-$missing/dists/stable/InRelease" ]]; then
    fail "missing secret ${missing}" "an InRelease was written without the signing material"
  fi
done

# --- 6. The ephemeral signing home does not survive the build -----------------------------
# Same base the script picks, so this still observes the right directory under Actions.
SIGNING_BASE="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
list_signing_homes() {
  find "${SIGNING_BASE%/}" -maxdepth 1 -name 'okp-apt-gnupg.*' 2>/dev/null | sort
}
seed_secrets
homes_before="$(list_signing_homes)"
build_repo "$STAGE_ONE" "$WORK/run-cleanup" >/dev/null
seed_secrets "$WORK/other-private.asc" "$TEST_FINGERPRINT"
set +e
build_repo "$STAGE_ONE" "$WORK/run-cleanup-fail" >/dev/null 2>&1
set -e
if [[ "$(list_signing_homes)" == "$homes_before" ]]; then
  pass "the ephemeral GNUPGHOME is removed after both a successful and a failed build"
else
  fail "ephemeral GNUPGHOME" "signing homes leaked under ${SIGNING_BASE}"
fi

# --- 7. "Newest" means the highest Debian version, not the last paragraph -----------------
# dpkg-scanpackages emits its index in lexicographic key order, so 0.11.0-beta.10 lands before
# 0.11.0-beta.2 and 0.11.0-beta.9. Taking the last paragraph would name .9 as the newest while
# apt installs .10, and the container gate would then assert against a version apt never
# selected — failing every Pages deploy the first time a version crosses a decimal boundary.
seed_secrets
STAGE_ORDER="$WORK/stage-order"
make_deb 0.11.0-beta.9 "$STAGE_ORDER"
make_deb 0.11.0-beta.10 "$STAGE_ORDER"
make_deb 0.11.0-beta.2 "$STAGE_ORDER"
build_repo "$STAGE_ORDER" "$WORK/run-order" >/dev/null
ORDER_INDEX="$(packages_index "$WORK/run-order")"
if [[ "$(okp_apt_newest_indexed_version "$ORDER_INDEX")" == "0.11.0-beta.10" ]]; then
  pass "the newest advertised version is the highest Debian version, not the last paragraph"
else
  fail "newest version derivation" \
    "got $(okp_apt_newest_indexed_version "$ORDER_INDEX"), expected 0.11.0-beta.10 from: $(awk '/^Version: /{printf "%s ", $2}' "$ORDER_INDEX")"
fi
# The premise the test above exists for: the index really is not in version order, so a naive
# derivation really would be wrong. If dpkg-scanpackages ever starts sorting by version this
# assertion fails and the comment above can be retired.
if [[ "$(awk '/^Version: / { v = $2 } END { print v }' "$ORDER_INDEX")" != "0.11.0-beta.10" ]]; then
  pass "the Packages index is not emitted in Debian version order (why the comparison is needed)"
else
  fail "index ordering premise" "the index is now version-ordered; the derivation comment is stale"
fi

# --- 8. The rolling window is per suite, and the budget is shared -------------------------
# Rows are what the two fetchers emit: <published>\t<label>\t<pool basename>\t<size>.
row() { printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$(deb_name "$3")" "$4"; }

plan_window() {
  # plan_window <stable max> <candidate max> <budget> <stable rows> <candidate rows> \
  #             <stable out> <candidate out>
  ( OKP_APT_MAX_VERSIONS="$1" OKP_APT_CANDIDATE_MAX_VERSIONS="$2" OKP_APT_POOL_BUDGET_BYTES="$3" \
    okp_apt_plan_suites "$4" "$5" "$6" "$7" )
}

STABLE_ROWS="$WORK/rows-stable"
CAND_ROWS="$WORK/rows-candidate"
STABLE_OUT="$WORK/selected-stable"
CAND_OUT="$WORK/selected-candidate"
: >"$CAND_ROWS"
{
  row '2026-07-15T19:35:55Z' linux-v0.3.0 0.3.0 900
  row '2026-07-14T19:35:55Z' linux-v0.2.0 0.2.0 100
  row '2026-07-13T19:35:55Z' linux-v0.1.0 0.1.0 100
} >"$STABLE_ROWS"

# The current release is not optional: skipping it because it does not fit, and keeping older,
# smaller ones, would publish a signed archive advertising an older version than the JSON feeds
# do. Clients would accept it, which makes it worse than not publishing.
set +e
oversize_output="$(plan_window 10 6 500 "$STABLE_ROWS" "$CAND_ROWS" "$STABLE_OUT" "$CAND_OUT" 2>&1)"
oversize_status=$?
set -e
if ((oversize_status != 0)); then
  pass "a current release that does not fit the pool budget aborts instead of being skipped"
else
  fail "current release budget (exit)" "the window succeeded without the current release"
fi
if grep -q 'linux-v0.3.0' <<<"$oversize_output" \
  && grep -q 'refusing to publish' <<<"$oversize_output"; then
  pass "the oversize-current-release abort names the release it refused to drop"
else
  fail "current release budget (message)" "$oversize_output"
fi
if grep -q 'linux-v0.2.0' <<<"$oversize_output"; then
  fail "current release budget (fallthrough)" "an older release was retained in place of the current one"
else
  pass "no older release is retained in place of the current one"
fi

set +e
nowindow_output="$(plan_window 0 6 100000 "$STABLE_ROWS" "$CAND_ROWS" "$STABLE_OUT" "$CAND_OUT" 2>&1)"
nowindow_status=$?
set -e
if ((nowindow_status != 0)) && grep -q 'linux-v0.3.0' <<<"$nowindow_output"; then
  pass "a version count too small to carry the current release aborts, naming it"
else
  fail "zero-width window" "status ${nowindow_status}, output: ${nowindow_output}"
fi

# Positive control: with room for the current release the window still trims the tail rather
# than failing, and reports what it dropped.
set +e
trimmed_output="$(plan_window 10 6 1000 "$STABLE_ROWS" "$CAND_ROWS" "$STABLE_OUT" "$CAND_OUT" 2>&1)"
trimmed_status=$?
set -e
if ((trimmed_status == 0)) \
  && grep -q 'linux-v0.3.0' "$STABLE_OUT" \
  && grep -q 'does not fit' <<<"$trimmed_output"; then
  pass "the window keeps the current release and drops the tail with a stated reason"
else
  fail "window trimming" "status ${trimmed_status}, output: ${trimmed_output}"
fi

# The candidate suite is optional: no rolling candidate means a stable-only archive, not a
# failed lane.
set +e
plan_window 10 6 100000 "$STABLE_ROWS" "$CAND_ROWS" "$STABLE_OUT" "$CAND_OUT" >/dev/null 2>&1
stable_only_status=$?
set -e
if ((stable_only_status == 0)) && [[ -s "$STABLE_OUT" && ! -s "$CAND_OUT" ]]; then
  pass "an archive with no rolling candidate plans the stable suite alone"
else
  fail "stable-only plan" "status ${stable_only_status}, candidate rows: $(cat "$CAND_OUT")"
fi

# A package both suites carry is charged once. Three distinct files of 100 bytes fit a 300-byte
# budget exactly; counting the shared one twice would need 400 and would push the candidate's
# oldest build out of the window.
{
  row '2026-07-15T00:00:00Z' linux-v1.0.0 1.0.0 100
  row '2026-07-14T00:00:00Z' linux-v0.9.0 0.9.0 100
} >"$WORK/rows-stable-shared"
{
  row '2026-07-16T00:00:00Z' 0.9.0 0.9.0 100
  row '2026-07-15T00:00:00Z' 0.8.0 0.8.0 100
} >"$WORK/rows-candidate-shared"
set +e
shared_output="$(plan_window 10 6 300 "$WORK/rows-stable-shared" "$WORK/rows-candidate-shared" \
  "$STABLE_OUT" "$CAND_OUT" 2>&1)"
shared_status=$?
set -e
if ((shared_status == 0)) && [[ "$(wc -l <"$STABLE_OUT")" == 2 && "$(wc -l <"$CAND_OUT")" == 2 ]]; then
  pass "a package carried by both suites is charged to the pool budget once"
else
  fail "shared pool accounting" \
    "status ${shared_status}, stable $(wc -l <"$STABLE_OUT") candidate $(wc -l <"$CAND_OUT"): ${shared_output}"
fi

# Neither suite may starve the other. Heads take 200 of a 500-byte budget; the remaining 300
# goes to the tails in turn, so stable keeps two tail builds and candidate one. Draining
# stable's tail first would leave the candidate suite with nothing but its current build.
{
  row '2026-07-15T00:00:00Z' linux-v3.0.0 3.0.0 100
  row '2026-07-14T00:00:00Z' linux-v2.0.0 2.0.0 100
  row '2026-07-13T00:00:00Z' linux-v1.0.0 1.0.0 100
  row '2026-07-12T00:00:00Z' linux-v0.9.0 0.9.0 100
} >"$WORK/rows-stable-fair"
{
  row '2026-07-16T00:00:00Z' 9.3.0 9.3.0 100
  row '2026-07-16T00:00:00Z' 9.2.0 9.2.0 100
  row '2026-07-16T00:00:00Z' 9.1.0 9.1.0 100
  row '2026-07-16T00:00:00Z' 9.0.0 9.0.0 100
} >"$WORK/rows-candidate-fair"
set +e
plan_window 10 6 500 "$WORK/rows-stable-fair" "$WORK/rows-candidate-fair" \
  "$STABLE_OUT" "$CAND_OUT" >/dev/null 2>&1
fair_status=$?
set -e
if ((fair_status == 0)) && [[ "$(wc -l <"$STABLE_OUT")" == 3 && "$(wc -l <"$CAND_OUT")" == 2 ]]; then
  pass "a tight shared budget is offered to both tails in turn instead of draining one suite"
else
  fail "shared budget fairness" \
    "status ${fair_status}, stable $(wc -l <"$STABLE_OUT") candidate $(wc -l <"$CAND_OUT")"
fi

# The per-suite version cap is per suite: trimming the candidate tail leaves stable untouched.
set +e
plan_window 10 2 100000 "$WORK/rows-stable-fair" "$WORK/rows-candidate-fair" \
  "$STABLE_OUT" "$CAND_OUT" >/dev/null 2>&1
cap_status=$?
set -e
if ((cap_status == 0)) && [[ "$(wc -l <"$STABLE_OUT")" == 4 && "$(wc -l <"$CAND_OUT")" == 2 ]]; then
  pass "the rolling window applies per suite: the candidate cap does not shorten stable"
else
  fail "per-suite window" \
    "status ${cap_status}, stable $(wc -l <"$STABLE_OUT") candidate $(wc -l <"$CAND_OUT")"
fi

# A candidate current build that cannot fit aborts for the same reason stable's does: the
# candidate suite would otherwise advertise an older build than candidate.linux.json does.
{
  row '2026-07-16T00:00:00Z' 9.3.0 9.3.0 100000
  row '2026-07-16T00:00:00Z' 9.2.0 9.2.0 10
} >"$WORK/rows-candidate-huge"
set +e
huge_output="$(plan_window 10 6 500 "$WORK/rows-stable-fair" "$WORK/rows-candidate-huge" \
  "$STABLE_OUT" "$CAND_OUT" 2>&1)"
huge_status=$?
set -e
if ((huge_status != 0)) && grep -q 'candidate' <<<"$huge_output" && grep -q '9.3.0' <<<"$huge_output"; then
  pass "a current candidate build that does not fit aborts, naming it"
else
  fail "candidate current build budget" "status ${huge_status}, output: ${huge_output}"
fi

# --- 9. The candidate suite is derived from the published pointer -------------------------
# candidate.linux.json is the pointer okp-core uploads last, after the artifacts it names, so
# the candidate suite advertises exactly what the candidate feed advertises.
POINTER="$WORK/candidate.linux.json"
ASSETS="$WORK/candidate-assets.json"
cat >"$POINTER" <<'JSON'
{
  "channel": "candidate",
  "version": "0.11.0-beta.0.197",
  "build": 197,
  "timestamp_utc": "2026-07-28T00:36:09Z",
  "acceptance": "accepted",
  "package": {
    "name": "ok-player_0.11.0-beta.0.197_amd64.deb",
    "sha256": "bbab4a5a0cc80b335b3965e6e4c74093e958bb6ad41012570e844292dcd17457"
  },
  "history": [
    { "version": "0.11.0-beta.0.193",
      "package": { "name": "ok-player_0.11.0-beta.0.193_amd64.deb",
                   "sha256": "3a46972dc90806c3351695de8987b8903019e418382398af0cdb262bfdc8d607" } },
    { "version": "0.11.0-beta.0.187",
      "package": { "name": "ok-player_0.11.0-beta.0.187_amd64.deb",
                   "sha256": "536e4bb36836ce5ff038e3952ce9fd3a470306cdc3806cdb17c3d30ba2ebf632" } }
  ]
}
JSON
cat >"$ASSETS" <<'JSON'
[
  { "name": "candidate.linux.json", "size": 7067 },
  { "name": "ok-player_0.11.0-beta.0.197_amd64.deb", "size": 76237684 },
  { "name": "ok-player_0.11.0-beta.0.193_amd64.deb", "size": 76225892 },
  { "name": "ok-player_0.11.0-beta.0.187_amd64.deb", "size": 83929784 }
]
JSON
POINTER_ROWS="$WORK/pointer.rows"
set +e
pointer_output="$(okp_apt_candidate_rows_from_pointer "$POINTER" "$ASSETS" "$POINTER_ROWS" 2>&1)"
pointer_status=$?
set -e
EXPECTED_ROWS="$(printf '%s\n' \
  '2026-07-28T00:36:09Z	0.11.0-beta.0.197	ok-player_0.11.0-beta.0.197_amd64.deb	76237684	bbab4a5a0cc80b335b3965e6e4c74093e958bb6ad41012570e844292dcd17457' \
  '2026-07-28T00:36:09Z	0.11.0-beta.0.193	ok-player_0.11.0-beta.0.193_amd64.deb	76225892	3a46972dc90806c3351695de8987b8903019e418382398af0cdb262bfdc8d607' \
  '2026-07-28T00:36:09Z	0.11.0-beta.0.187	ok-player_0.11.0-beta.0.187_amd64.deb	83929784	536e4bb36836ce5ff038e3952ce9fd3a470306cdc3806cdb17c3d30ba2ebf632')"
if ((pointer_status == 0)) && [[ "$(cat "$POINTER_ROWS")" == "$EXPECTED_ROWS" ]]; then
  pass "the candidate window is the pointer's current build followed by its history, newest first"
else
  fail "pointer rows" "status ${pointer_status}, output: ${pointer_output}, rows: $(cat "$POINTER_ROWS")"
fi

# A history entry whose asset has been pruned from the rolling release is dropped rather than
# indexed: an index entry apt cannot download is worse than a shorter window.
jq 'del(.[] | select(.name == "ok-player_0.11.0-beta.0.187_amd64.deb"))' "$ASSETS" \
  >"$WORK/candidate-assets-pruned.json"
set +e
pruned_output="$(okp_apt_candidate_rows_from_pointer "$POINTER" "$WORK/candidate-assets-pruned.json" \
  "$POINTER_ROWS" 2>&1)"
pruned_status=$?
set -e
if ((pruned_status == 0)) && [[ "$(wc -l <"$POINTER_ROWS")" == 2 ]] \
  && ! grep -q 'beta.0.187' "$POINTER_ROWS"; then
  pass "a history build whose asset is gone drops out of the candidate suite"
else
  fail "pruned history asset" "status ${pruned_status}, rows: $(cat "$POINTER_ROWS"), output: ${pruned_output}"
fi

# The current build is different: if the pointer names a package the release does not carry,
# the archive cannot agree with the feed, so the lane aborts.
jq 'del(.[] | select(.name == "ok-player_0.11.0-beta.0.197_amd64.deb"))' "$ASSETS" \
  >"$WORK/candidate-assets-nocurrent.json"
set +e
nocurrent_output="$(okp_apt_candidate_rows_from_pointer "$POINTER" \
  "$WORK/candidate-assets-nocurrent.json" "$POINTER_ROWS" 2>&1)"
nocurrent_status=$?
set -e
if ((nocurrent_status != 0)) && grep -q 'beta.0.197' <<<"$nocurrent_output" \
  && grep -q 'candidate feed' <<<"$nocurrent_output"; then
  pass "a pointer naming a package the rolling release does not carry aborts the lane"
else
  fail "missing current candidate asset" "status ${nocurrent_status}, output: ${nocurrent_output}"
fi

# The rolling release is mutable: assets are replaced in place, so the pointer's digest is the
# only thing tying the archive to the artifact the candidate feed authenticated. A pointer entry
# without one cannot be published, and bytes that do not match it must never be signed.
jq 'del(.package.sha256)' "$POINTER" >"$WORK/pointer-nodigest.json"
set +e
nodigest_output="$(okp_apt_candidate_rows_from_pointer "$WORK/pointer-nodigest.json" "$ASSETS" \
  "$WORK/rows-nodigest" 2>&1)"
nodigest_status=$?
set -e
if ((nodigest_status != 0)) && grep -q 'without a sha256' <<<"$nodigest_output"; then
  pass "a candidate the pointer does not authenticate cannot be published"
else
  fail "missing candidate digest" "status ${nodigest_status}, output: ${nodigest_output}"
fi

DIGEST_FILE="$STAGE_ONE/$(deb_name 1.0.0)"
REAL_DIGEST="$(sha256sum "$DIGEST_FILE" | cut -d' ' -f1)"
set +e
okp_apt_require_digest "$DIGEST_FILE" "$REAL_DIGEST" candidate 0.11.0-beta.0.197
matching_status=$?
mismatch_digest_output="$(okp_apt_require_digest "$DIGEST_FILE" \
  '0000000000000000000000000000000000000000000000000000000000000000' candidate 0.11.0-beta.0.197 2>&1)"
mismatch_digest_status=$?
skipped_status=0
okp_apt_require_digest "$DIGEST_FILE" '' stable linux-v0.1.0 || skipped_status=$?
set -e
if ((matching_status == 0)) && ((mismatch_digest_status != 0)) && ((skipped_status == 0)) \
  && grep -q 'the update feed did not vouch for' <<<"$mismatch_digest_output" \
  && grep -q "$REAL_DIGEST" <<<"$mismatch_digest_output"; then
  pass "a downloaded package whose bytes do not match the feed's digest aborts before signing"
else
  fail "digest verification" \
    "match ${matching_status}, mismatch ${mismatch_digest_status}, skipped ${skipped_status}: ${mismatch_digest_output}"
fi

# The acceptance gate. okp-core::candidate_channel::select_candidate_update_from_feed refuses a
# pointer that is not Accepted, so the installed .deb never offers a pending or rejected build.
# apt must not be the one channel that ships it anyway: `apt upgrade` would push a build that
# failed acceptance to every subscribed tester without asking.
for state in pending rejected; do
  jq --arg state "$state" '.acceptance = $state' "$POINTER" >"$WORK/pointer-$state.json"
  set +e
  gated_output="$(okp_apt_candidate_rows_from_pointer "$WORK/pointer-$state.json" "$ASSETS" \
    "$WORK/rows-$state" 2>&1)"
  gated_status=$?
  set -e
  if ((gated_status == 0)) && ! grep -q 'beta.0.197' "$WORK/rows-$state" \
    && [[ "$(head -n 1 "$WORK/rows-$state" | cut -f2)" == "0.11.0-beta.0.193" ]]; then
    pass "an acceptance=${state} candidate is withheld and the suite falls back to the newest accepted build"
  else
    fail "acceptance gate (${state})" "status ${gated_status}, rows: $(cat "$WORK/rows-$state"), output: ${gated_output}"
  fi
  if grep -q "acceptance=${state}" <<<"$gated_output"; then
    pass "the withheld ${state} candidate is reported with its acceptance state"
  else
    fail "acceptance gate message (${state})" "$gated_output"
  fi
done

# ...and with nothing accepted to fall back to, the archive is stable-only rather than carrying a
# candidate suite nobody vouched for.
jq '.acceptance = "rejected" | .history = []' "$POINTER" >"$WORK/pointer-nothing.json"
set +e
nothing_output="$(okp_apt_candidate_rows_from_pointer "$WORK/pointer-nothing.json" "$ASSETS" \
  "$WORK/rows-nothing" 2>&1)"
nothing_status=$?
set -e
if ((nothing_status == 0)) && [[ ! -s "$WORK/rows-nothing" ]] \
  && grep -q 'no accepted build' <<<"$nothing_output"; then
  pass "a pointer with no accepted build at all publishes a stable-only archive"
else
  fail "acceptance gate (nothing accepted)" "status ${nothing_status}, output: ${nothing_output}"
fi

# A pointer that names no package at all is a different thing: that is a malformed input, not a
# channel with nothing to say, and it must not pass silently.
printf '{"acceptance":"accepted","timestamp_utc":"2026-07-28T00:36:09Z","history":[]}\n' \
  >"$WORK/pointer-malformed.json"
set +e
malformed_output="$(okp_apt_candidate_rows_from_pointer "$WORK/pointer-malformed.json" "$ASSETS" \
  "$WORK/rows-malformed" 2>&1)"
malformed_status=$?
set -e
if ((malformed_status != 0)) && grep -q 'names no Debian package' <<<"$malformed_output"; then
  pass "a pointer that names no Debian package aborts instead of quietly publishing nothing"
else
  fail "malformed pointer" "status ${malformed_status}, output: ${malformed_output}"
fi

# "No rolling candidate yet" and "GitHub did not answer" must not look the same. The first is a
# legitimate stable-only archive; the second silently un-publishing the QA channel that testers
# are subscribed to is a failure, not a degraded mode. `gh` is shadowed by a shell function here,
# so the real discovery path runs against a canned API failure without a network.
set +e
absent_release_output="$(
  ( gh() { printf 'gh: Not Found (HTTP 404)\n' >&2; return 1; }
    okp_apt_fetch_candidate_rows BeFeast/ok-player "$WORK" "$WORK/rows-no-release" ) 2>&1
)"
absent_release_status=$?
set -e
if ((absent_release_status == 0)) && [[ ! -s "$WORK/rows-no-release" ]] \
  && grep -q 'stable suite only' <<<"$absent_release_output"; then
  pass "a repository with no rolling candidate release plans a stable-only archive"
else
  fail "absent candidate release" "status ${absent_release_status}, output: ${absent_release_output}"
fi

set +e
outage_output="$(
  ( gh() { printf 'gh: Server Error (HTTP 503)\n' >&2; return 1; }
    okp_apt_fetch_candidate_rows BeFeast/ok-player "$WORK" "$WORK/rows-outage" ) 2>&1
)"
outage_status=$?
set -e
if ((outage_status != 0)) && grep -q 'refusing to drop' <<<"$outage_output"; then
  pass "an API failure that is not a 404 aborts instead of quietly dropping the candidate suite"
else
  fail "candidate discovery outage" "status ${outage_status}, output: ${outage_output}"
fi

# --- 10. Two suites, one pool, one key -----------------------------------------------------
seed_secrets
STAGE_SUITES="$WORK/stage-suites"
make_deb 0.1.0-linux-alpha.112 "$STAGE_SUITES"     # stable only
make_deb 0.11.0-beta.0.193 "$STAGE_SUITES"         # candidate only
make_deb 0.11.0-beta.0.197 "$STAGE_SUITES"         # candidate only
# A promoted build: the same package file is a release AND still a retained candidate, which is
# the case "stored once, referenced twice" exists for.
SHARED=0.11.0-beta.0.187
make_deb "$SHARED" "$STAGE_SUITES"
CANDIDATE_EPOCH=1750500000
write_plan "$WORK/plan-two" \
  "stable=${EPOCH}=$(deb_name 0.1.0-linux-alpha.112),$(deb_name "$SHARED")" \
  "candidate=${CANDIDATE_EPOCH}=$(deb_name 0.11.0-beta.0.197),$(deb_name 0.11.0-beta.0.193),$(deb_name "$SHARED")"
stage_from_plan "$WORK/staged-two" "$STAGE_SUITES" "$WORK/plan-two"
build_planned "$WORK/staged-two" "$WORK/run-two" "$WORK/plan-two" >/dev/null

STABLE_INDEX="$(packages_index "$WORK/run-two" stable)"
CANDIDATE_INDEX="$(packages_index "$WORK/run-two" candidate)"

if [[ -s "$CANDIDATE_INDEX" && -s "$STABLE_INDEX" ]]; then
  pass "the generator emits dists/stable and dists/candidate from one run"
else
  fail "two suites" "one of the suites has no Packages index"
fi

# The separation the issue exists for, at the index level: the release channel must not carry
# candidate builds, and the QA channel must carry them.
if ! grep -q '0.11.0-beta.0.197' "$STABLE_INDEX" && ! grep -q '0.11.0-beta.0.193' "$STABLE_INDEX"; then
  pass "the stable index carries no candidate build"
else
  fail "channel separation" "a candidate build leaked into dists/stable"
fi
if grep -q '0.11.0-beta.0.197' "$CANDIDATE_INDEX" && grep -q '0.11.0-beta.0.193' "$CANDIDATE_INDEX"; then
  pass "the candidate index carries the rolling builds"
else
  fail "candidate index" "the candidate suite is missing a rolling build"
fi
if ! grep -q '0.1.0-linux-alpha.112' "$CANDIDATE_INDEX"; then
  pass "the candidate index carries no stable-only release"
else
  fail "channel separation" "a stable release leaked into dists/candidate"
fi

# Shared, not duplicated: one file in the pool, and the same paragraph bytes in both indices.
if [[ "$(find "$WORK/run-two/pool" -name "$(deb_name "$SHARED")" | wc -l)" == 1 ]]; then
  pass "a package carried by both suites is stored once in the shared pool"
else
  fail "shared pool" "$(deb_name "$SHARED") is not stored exactly once"
fi
if [[ -n "$(paragraph_for "$SHARED" "$STABLE_INDEX")" \
  && "$(paragraph_for "$SHARED" "$STABLE_INDEX")" == "$(paragraph_for "$SHARED" "$CANDIDATE_INDEX")" ]]; then
  pass "a package carried by both suites is indexed with identical bytes in both"
else
  fail "shared paragraph" "the ${SHARED} paragraph differs between the suites"
fi
# Every index entry resolves, in both suites — the same orphan check the stable suite gets.
suite_orphans=0
for suite in stable candidate; do
  while IFS= read -r filename; do
    [[ -f "$WORK/run-two/$filename" ]] || suite_orphans=$((suite_orphans + 1))
  done < <(awk '/^Filename: / { print $2 }' "$(packages_index "$WORK/run-two" "$suite")")
done
if ((suite_orphans == 0)); then
  pass "every package indexed by either suite resolves to a file in the shared pool"
else
  fail "two-suite orphan check" "${suite_orphans} indexed packages have no pool file"
fi

# One key, both suites. A second signing path is exactly what this archive must not grow.
for suite in stable candidate; do
  if GNUPGHOME="$VERIFY_HOME" gpg --batch --status-fd 1 --verify \
       "$WORK/run-two/dists/${suite}/InRelease" 2>/dev/null \
       | grep -q "VALIDSIG ${TEST_FINGERPRINT}"; then
    pass "the ${suite} InRelease is signed by the archive key"
  else
    fail "${suite} signature" "dists/${suite}/InRelease does not verify against the published keyring"
  fi
  if GNUPGHOME="$VERIFY_HOME" gpg --batch --status-fd 1 --verify \
       "$WORK/run-two/dists/${suite}/Release.gpg" "$WORK/run-two/dists/${suite}/Release" 2>/dev/null \
       | grep -q "VALIDSIG ${TEST_FINGERPRINT}"; then
    pass "the ${suite} Release.gpg is signed by the archive key"
  else
    fail "${suite} detached signature" "dists/${suite}/Release.gpg does not verify"
  fi
  if grep -qx "Suite: ${suite}" "$WORK/run-two/dists/${suite}/Release" \
    && grep -qx "Codename: ${suite}" "$WORK/run-two/dists/${suite}/Release"; then
    pass "the ${suite} Release names its own suite"
  else
    fail "${suite} Release identity" "$(grep -E '^(Suite|Codename):' "$WORK/run-two/dists/${suite}/Release" | tr '\n' ' ')"
  fi
done
if grep -qx "Date: $(LC_ALL=C date -u -d "@${CANDIDATE_EPOCH}" '+%a, %d %b %Y %H:%M:%S UTC')" \
     "$WORK/run-two/dists/candidate/Release"; then
  pass "each suite's Release is dated from its own newest build, not from the clock"
else
  fail "candidate Release date" "$(grep '^Date:' "$WORK/run-two/dists/candidate/Release")"
fi

# The published source stanzas: the default one must stay stable-only, or a user who follows the
# README would be enrolled in QA builds by accident.
if grep -qx 'Suites: stable' "$WORK/run-two/ok-player.sources"; then
  pass "ok-player.sources subscribes to stable alone"
else
  fail "ok-player.sources" "$(cat "$WORK/run-two/ok-player.sources")"
fi
if grep -qx 'Suites: candidate' "$WORK/run-two/ok-player-candidate.sources"; then
  pass "ok-player-candidate.sources subscribes to candidate alone"
else
  fail "ok-player-candidate.sources" "$(cat "$WORK/run-two/ok-player-candidate.sources" 2>&1)"
fi
# A stable-only archive must not publish a candidate stanza pointing at a suite that is not there:
# apt fails hard on a missing suite.
if [[ ! -e "$WORK/run-a/ok-player-candidate.sources" ]]; then
  pass "a stable-only archive publishes no candidate source stanza"
else
  fail "stray candidate stanza" "ok-player-candidate.sources exists without a candidate suite"
fi

# Per-suite version derivation, which is what the container gate asserts against.
if [[ "$(okp_apt_archive_version "$WORK/run-two" test candidate)" == "0.11.0-beta.0.197" \
  && "$(okp_apt_archive_version "$WORK/run-two" test stable)" == "$SHARED" ]]; then
  pass "the verifier derives each suite's newest version from that suite's index"
else
  fail "per-suite version derivation" \
    "stable $(okp_apt_archive_version "$WORK/run-two" test stable), candidate $(okp_apt_archive_version "$WORK/run-two" test candidate)"
fi

# Overlap is legitimate — a candidate promoted unchanged is the same package in both suites — so
# the leak check must be about versions `candidate` carries and `stable` does not, never about the
# candidate version string. Otherwise the first promotion fails the whole lane over a version that
# came from the stable index.
if [[ "$(okp_apt_versions_only_in "$WORK/run-two" candidate stable)" \
      == "0.11.0-beta.0.193 0.11.0-beta.0.197 " ]]; then
  pass "the leak check targets the versions candidate carries and stable does not"
else
  fail "candidate-only versions" "got '$(okp_apt_versions_only_in "$WORK/run-two" candidate stable)'"
fi
write_plan "$WORK/plan-overlap" \
  "stable=${EPOCH}=$(deb_name "$SHARED")" \
  "candidate=${CANDIDATE_EPOCH}=$(deb_name "$SHARED")"
stage_from_plan "$WORK/staged-overlap" "$STAGE_SUITES" "$WORK/plan-overlap"
build_planned "$WORK/staged-overlap" "$WORK/run-overlap" "$WORK/plan-overlap" >/dev/null
if [[ -z "$(okp_apt_versions_only_in "$WORK/run-overlap" candidate stable)" ]] \
  && [[ "$(okp_apt_archive_version "$WORK/run-overlap" test candidate)" \
        == "$(okp_apt_archive_version "$WORK/run-overlap" test stable)" ]]; then
  pass "two suites whose heads are the same promoted build leave nothing that could leak"
else
  fail "fully overlapping suites" \
    "candidate-only: '$(okp_apt_versions_only_in "$WORK/run-overlap" candidate stable)'"
fi

# The cross-suite integrity guard: two suites must never advertise one Filename with two different
# sets of bytes, because exactly one of them would be lying about what apt is about to download.
COLLIDE_A="$WORK/collide-a"
COLLIDE_B="$WORK/collide-b"
COLLIDE_POOL="$WORK/collide-pool"
rm -rf -- "$COLLIDE_A" "$COLLIDE_B" "$COLLIDE_POOL"
mkdir -p "$COLLIDE_A" "$COLLIDE_B" "$COLLIDE_POOL"
cp "$STAGE_SUITES/$(deb_name "$SHARED")" "$COLLIDE_A/$(deb_name "$SHARED")"
cp "$STAGE_SUITES/$(deb_name 0.11.0-beta.0.197)" "$COLLIDE_B/$(deb_name "$SHARED")"
set +e
collision_output="$( ( okp_apt_merge_into_pool "$COLLIDE_A" "$COLLIDE_POOL" stable
                       okp_apt_merge_into_pool "$COLLIDE_B" "$COLLIDE_POOL" candidate ) 2>&1 )"
collision_status=$?
set -e
if ((collision_status != 0)) && grep -q 'two packages under one Filename' <<<"$collision_output"; then
  pass "the same Filename with different bytes in two suites aborts the lane"
else
  fail "filename collision" "status ${collision_status}, output: ${collision_output}"
fi
# The benign case is the whole point of sharing: identical bytes merge into one pool file.
rm -rf -- "$COLLIDE_A" "$COLLIDE_B" "$COLLIDE_POOL"
mkdir -p "$COLLIDE_A" "$COLLIDE_B" "$COLLIDE_POOL"
cp "$STAGE_SUITES/$(deb_name "$SHARED")" "$COLLIDE_A/$(deb_name "$SHARED")"
cp "$STAGE_SUITES/$(deb_name "$SHARED")" "$COLLIDE_B/$(deb_name "$SHARED")"
okp_apt_merge_into_pool "$COLLIDE_A" "$COLLIDE_POOL" stable
okp_apt_merge_into_pool "$COLLIDE_B" "$COLLIDE_POOL" candidate
if [[ "$(find "$COLLIDE_POOL" -name '*.deb' | wc -l)" == 1 ]] \
  && cmp -s "$COLLIDE_POOL/$(deb_name "$SHARED")" "$STAGE_SUITES/$(deb_name "$SHARED")"; then
  pass "a package both suites downloaded identically merges into one pool file"
else
  fail "shared merge" "$(find "$COLLIDE_POOL" -type f | tr '\n' ' ')"
fi

# --- 11. A candidate that ages out leaves the archive completely ---------------------------
# The next run's plan no longer names beta.0.193, and it is carried by no other suite, so it
# must disappear from dists/candidate AND from the pool. A pool file nothing indexes is dead
# weight against the shared budget.
make_deb 0.11.0-beta.0.201 "$STAGE_SUITES"
write_plan "$WORK/plan-rolled" \
  "stable=${EPOCH}=$(deb_name 0.1.0-linux-alpha.112),$(deb_name "$SHARED")" \
  "candidate=${CANDIDATE_EPOCH}=$(deb_name 0.11.0-beta.0.201),$(deb_name 0.11.0-beta.0.197),$(deb_name "$SHARED")"
stage_from_plan "$WORK/staged-rolled" "$STAGE_SUITES" "$WORK/plan-rolled"
build_planned "$WORK/staged-rolled" "$WORK/run-rolled" "$WORK/plan-rolled" >/dev/null
ROLLED_INDEX="$(packages_index "$WORK/run-rolled" candidate)"
if [[ ! -e "$WORK/run-rolled/pool/main/o/ok-player/$(deb_name 0.11.0-beta.0.193)" ]] \
  && ! grep -q '0.11.0-beta.0.193' "$ROLLED_INDEX"; then
  pass "a candidate that ages out disappears from dists/candidate and from the pool together"
else
  fail "candidate ageing" "beta.0.193 survived the window it left"
fi
if grep -q '0.11.0-beta.0.197' "$ROLLED_INDEX" \
  && [[ "$(paragraph_for "$SHARED" "$(packages_index "$WORK/run-rolled" stable)")" \
        == "$(paragraph_for "$SHARED" "$STABLE_INDEX")" ]]; then
  pass "rolling the candidate window leaves the retained builds and the stable suite untouched"
else
  fail "candidate ageing (additivity)" "a retained build changed when the window rolled"
fi

# --- 12. A plan the pool cannot satisfy aborts ---------------------------------------------
# Both directions are planning bugs that would ship a broken archive: an index entry apt cannot
# download, or bytes no client can reach.
write_plan "$WORK/plan-dangling" \
  "stable=${EPOCH}=$(deb_name 0.1.0-linux-alpha.112)" \
  "candidate=${CANDIDATE_EPOCH}=$(deb_name 9.9.9)"
set +e
dangling_output="$(build_planned "$STAGE_SUITES" "$WORK/run-dangling" "$WORK/plan-dangling" 2>&1)"
dangling_status=$?
set -e
if ((dangling_status != 0)) && grep -q 'not in the pool' <<<"$dangling_output"; then
  pass "a suite that references a package the pool does not carry aborts the lane"
else
  fail "dangling member" "status ${dangling_status}, output: ${dangling_output}"
fi

write_plan "$WORK/plan-unreferenced" \
  "stable=${EPOCH}=$(deb_name 0.1.0-linux-alpha.112)"
set +e
unreferenced_output="$(build_planned "$STAGE_SUITES" "$WORK/run-unreferenced" \
  "$WORK/plan-unreferenced" 2>&1)"
unreferenced_status=$?
set -e
if ((unreferenced_status != 0)) && grep -q 'no suite indexes' <<<"$unreferenced_output"; then
  pass "a pool file no suite indexes aborts the lane instead of wasting the byte budget"
else
  fail "unreferenced pool file" "status ${unreferenced_status}, output: ${unreferenced_output}"
fi

# --- 13. Two suites are as idempotent as one ------------------------------------------------
build_planned "$WORK/staged-two" "$WORK/run-two-again" "$WORK/plan-two" >/dev/null
if content_diff "$WORK/run-two" "$WORK/run-two-again" >"$WORK/two-suite-idempotence.diff" 2>&1; then
  pass "a two-suite archive reproduces byte for byte over an unchanged plan"
else
  fail "two-suite idempotence" "$(head -n 20 "$WORK/two-suite-idempotence.diff")"
fi

# --- 14. The archive may only advertise versions apt orders the way builds order ------------
# The defect this guards is invisible in a passing install (issue #709): a `.deb` published as
# `0.11.0-beta.0.209` installs perfectly, and only sorts *above* the `0.11.0` release that
# follows it — so the release silently never reaches anyone, months later. The assertion is in
# the verifier, and it runs before any container, so it is exercised here directly.
#
# The verifier aborts by exiting, which is right for the lane and would end this suite, so each
# case is run inside a command substitution.
assert_scheme() {
  # assert_scheme <repo-root> [suite]
  ( okp_apt_assert_version_scheme "$1" "${2:-stable}" 2>&1 )
}

seed_secrets
STAGE_SCHEME="$WORK/stage-scheme"
ENCODED_209="$(okp_debian_version_for_build 0.11.0-beta.0.209)"
make_deb 0.11.0-beta.0.209 "$STAGE_SCHEME" "$ENCODED_209"
make_deb 0.11.0 "$STAGE_SCHEME" "$(okp_debian_version_for_build 0.11.0)"
# The tail the archive keeps carrying: builds published before the encoding existed. They are
# rebuilt into the archive from their release assets until they age out of the window, so
# tolerating them is not a loophole — the epoch is what keeps them below everything encoded.
make_deb 0.11.0-beta.0.208 "$STAGE_SCHEME" 0.11.0-beta.0.208
make_deb 0.1.0-linux-alpha.112 "$STAGE_SCHEME" 0.1.0-linux-alpha.112
build_repo "$STAGE_SCHEME" "$WORK/run-scheme" >/dev/null
set +e
scheme_output="$(assert_scheme "$WORK/run-scheme")"
scheme_status=$?
set -e
if ((scheme_status == 0)) && grep -q '2 encoded, 2 published before the encoding' <<<"$scheme_output"; then
  pass "an archive of encoded versions passes, and the pre-encoding tail is counted, not failed"
else
  fail "version scheme" "status ${scheme_status}, output: ${scheme_output}"
fi

# apt's own view of that archive: the release is what a client would be offered, over both the
# candidate that preceded it and everything published before the encoding.
if [[ "$(okp_apt_newest_indexed_version "$(packages_index "$WORK/run-scheme")")" \
  == "$(okp_debian_version_for_build 0.11.0)" ]]; then
  pass "the release is the newest version the archive advertises"
else
  fail "release ordering" \
    "newest is $(okp_apt_newest_indexed_version "$(packages_index "$WORK/run-scheme")")"
fi

# --- Negative control: the scheme regresses ------------------------------------------------
# The same build, stamped the way it was stamped before this change. Nothing about the archive
# differs except one string, and that string is the whole defect.
STAGE_REGRESSED="$WORK/stage-regressed"
make_deb 0.11.0-beta.0.209 "$STAGE_REGRESSED" 0.11.0-beta.0.209
build_repo "$STAGE_REGRESSED" "$WORK/run-regressed" >/dev/null
set +e
regressed_output="$(assert_scheme "$WORK/run-regressed")"
regressed_status=$?
set -e
if ((regressed_status != 0)) && grep -q 'encodes to' <<<"$regressed_output"; then
  pass "an unencoded version newer than the pre-encoding tail fails the archive"
else
  fail "version scheme control" "status ${regressed_status}, output: ${regressed_output}"
fi

# The regression really would ship: apt orders that package above the release, which is what
# makes the assertion above the only thing standing between it and a release nobody receives.
if dpkg --compare-versions 0.11.0-beta.0.209 gt "$(okp_debian_version_for_build 0.11.0)"; then
  fail "regression premise" "the unencoded candidate no longer outranks the release"
else
  if dpkg --compare-versions 0.11.0-beta.0.209 gt 0.11.0; then
    pass "the premise: without the encoding the candidate outranks the release it precedes"
  else
    fail "regression premise" "dpkg no longer ranks 0.11.0-beta.0.209 above 0.11.0"
  fi
fi

# --- Negative control: the release itself, unencoded -----------------------------------------
# The case the "published before the encoding" exception must not swallow. dpkg ranks a raw
# `0.11.0` *below* `0.11.0-beta.0.208` — that is the defect — so an exception phrased as "at or
# below the high-water mark" would wave through exactly the release that cannot be installed by
# anyone on the candidate suite. It is admitted only for a prerelease whose Version is literally
# its build version, which no release ever is.
if dpkg --compare-versions 0.11.0 le 0.11.0-beta.0.208; then
  pass "the premise: dpkg ranks a raw 0.11.0 below the last pre-encoding candidate"
else
  fail "raw release premise" "dpkg no longer ranks 0.11.0 below 0.11.0-beta.0.208"
fi
STAGE_RAW_RELEASE="$WORK/stage-raw-release"
make_deb 0.11.0 "$STAGE_RAW_RELEASE" 0.11.0
make_deb 0.11.0-beta.0.208 "$STAGE_RAW_RELEASE" 0.11.0-beta.0.208
build_repo "$STAGE_RAW_RELEASE" "$WORK/run-raw-release" >/dev/null
set +e
raw_release_output="$(assert_scheme "$WORK/run-raw-release")"
raw_release_status=$?
set -e
if ((raw_release_status != 0)) && grep -q 'encodes to 1:0.11.0' <<<"$raw_release_output"; then
  pass "an unencoded release is not admitted as a package from before the encoding"
else
  fail "raw release control" "status ${raw_release_status}, output: ${raw_release_output}"
fi

# --- Negative control: an unencoded prerelease that was never published ----------------------
# The exception is a closed list, not a rule, and this is why. `0.11.0-alpha.999` is a
# prerelease, is unencoded, and dpkg sorts it below the pre-encoding high-water mark — so a rule
# phrased in those terms admits it, even though no lane ever published it. Admitting it would
# mean a package from a regressed or unrecognised lane bypasses every encoding and ordering
# assertion in this function.
if dpkg --compare-versions 0.11.0-alpha.999 le 0.11.0-beta.0.208; then
  pass "the premise: dpkg sorts a never-published raw prerelease below the pre-encoding tail"
else
  fail "unpublished prerelease premise" "dpkg no longer sorts 0.11.0-alpha.999 low"
fi
if okp_is_pre_encoding_version 0.11.0-alpha.999; then
  fail "published set" "0.11.0-alpha.999 is not a version this archive ever published"
else
  pass "a never-published version is not in the pre-encoding set"
fi
STAGE_UNPUBLISHED="$WORK/stage-unpublished"
make_deb 0.11.0-alpha.999 "$STAGE_UNPUBLISHED" 0.11.0-alpha.999
build_repo "$STAGE_UNPUBLISHED" "$WORK/run-unpublished" >/dev/null
set +e
unpublished_output="$(assert_scheme "$WORK/run-unpublished")"
unpublished_status=$?
set -e
if ((unpublished_status != 0)) && grep -q 'encodes to' <<<"$unpublished_output"; then
  pass "an unencoded version that was never published is not admitted as legacy"
else
  fail "unpublished control" "status ${unpublished_status}, output: ${unpublished_output}"
fi

# --- Negative control: the encoding disagrees with the build it is named for ----------------
STAGE_MISMATCH="$WORK/stage-mismatch"
make_deb 0.11.0-beta.0.209 "$STAGE_MISMATCH" "$(okp_debian_version_for_build 0.11.0-beta.0.210)"
build_repo "$STAGE_MISMATCH" "$WORK/run-mismatch" >/dev/null
set +e
mismatch_output="$(assert_scheme "$WORK/run-mismatch")"
mismatch_status=$?
set -e
if ((mismatch_status != 0)) && grep -q 'encodes to' <<<"$mismatch_output"; then
  pass "a version that disagrees with the build its pool file names fails the archive"
else
  fail "version fidelity control" "status ${mismatch_status}, output: ${mismatch_output}"
fi

# --- Negative control: a Debian revision reappears -------------------------------------------
# The shape that started all of this: a tail dpkg reads as a revision, so it outranks the
# release of the same upstream version.
STAGE_REVISION="$WORK/stage-revision"
make_deb 0.11.0-beta.0.209 "$STAGE_REVISION" '1:0.11.0~beta.0.209-1'
build_repo "$STAGE_REVISION" "$WORK/run-revision" >/dev/null
set +e
revision_output="$(assert_scheme "$WORK/run-revision")"
revision_status=$?
set -e
if ((revision_status != 0)) && grep -q 'encodes to' <<<"$revision_output"; then
  pass "a version carrying a Debian revision fails the archive"
else
  fail "revision control" "status ${revision_status}, output: ${revision_output}"
fi

if ((failures > 0)); then
  printf '%s APT repository generator assertion(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'APT repository generator policy tests passed\n'
