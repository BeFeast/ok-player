#!/usr/bin/env bash
# Behavioural tests for scripts/build-apt-repo.sh (issue #683).
#
# The APT archive is only safe while four properties hold, and all four are invisible in a
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
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

# Named up front so a runner image without dpkg-dev says so, instead of failing somewhere
# inside the generator with a message about OpenPGP.
for tool in gpg gpgconf dpkg-deb dpkg-scanpackages gzip sha256sum md5sum; do
  command -v "$tool" >/dev/null 2>&1 \
    || { printf 'APT repository generator tests require %s, which is not on PATH\n' "$tool" >&2; exit 1; }
done

# shellcheck source=/dev/null
source "$GENERATOR"

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

# --- Real .deb packages ---------------------------------------------------------------
make_deb() {
  # make_deb <version> <output-dir>
  local version="$1" destination="$2"
  local build="$WORK/build/$version"
  rm -rf -- "$build"
  mkdir -p "$build/DEBIAN" "$build/usr/bin"
  printf '#!/bin/sh\necho ok-player %s\n' "$version" >"$build/usr/bin/ok-player"
  chmod 755 "$build/usr/bin/ok-player"
  {
    printf 'Package: ok-player\n'
    printf 'Version: %s\n' "$version"
    printf 'Architecture: amd64\n'
    printf 'Maintainer: OK Player <noreply@example.invalid>\n'
    printf 'Description: APT repository generator test package\n'
  } >"$build/DEBIAN/control"
  mkdir -p "$destination"
  dpkg-deb --build --root-owner-group "$build" \
    "$destination/ok-player_${version}_amd64.deb" >/dev/null
}

# Fixed epoch: the Release date is an input, not a clock reading, and pinning it here is what
# lets two runs be compared byte for byte at all.
EPOCH=1750000000
BASE_URL='https://example.invalid/ok-player/apt'

# A subshell on purpose: okp_apt_build_signed_repo installs its own EXIT trap for the
# ephemeral signing home, and an abort inside it must end that build, not this suite.
build_repo() {
  # build_repo <staging> <repo-root> [epoch]
  ( okp_apt_build_signed_repo "$1" "$2" "${3:-$EPOCH}" "$BASE_URL" test_secret_reader )
}

# Everything except the two OpenPGP signatures, which carry their own creation time and are
# expected to differ between runs.
content_diff() {
  diff -r --exclude=InRelease --exclude=Release.gpg "$1" "$2"
}

packages_index() {
  printf '%s/dists/stable/main/binary-amd64/Packages' "$1"
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

if ((failures > 0)); then
  printf '%s APT repository generator assertion(s) failed\n' "$failures" >&2
  exit 1
fi
printf 'APT repository generator policy tests passed\n'
