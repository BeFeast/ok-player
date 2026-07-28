#!/usr/bin/env bash
# Prove the generated APT repository actually works, from a clean Debian root (issue #683).
#
# A repository that merely looks right is worthless: the failure modes that matter — a
# signature apt will not accept, a Packages entry apt cannot resolve, a package whose
# dependencies do not exist on the target — are only visible to apt itself. So this runs
# apt against the archive inside a throwaway debian:13-slim container:
#
#   * add the published keyring and source, `apt-get update`, `apt-get install ok-player`;
#   * start /usr/bin/ok-player headless and require it to reach its renderer-policy decision,
#     the same "it really started" signal the Linux smokes use (the process then dies on the
#     absent display, which is expected and not a failure);
#   * negative control 1 — a tampered InRelease must make `apt-get update` fail;
#   * negative control 2 — a keyring holding some other key must make `apt-get update` fail.
#
# Given a second, older archive it also proves the upgrade path end to end: install from the
# old archive, swap in the new one, and require `apt-get upgrade` to move to the newer version
# while the older one stays resolvable by explicit version. That is the additivity promise the
# generator makes, checked by the package manager rather than by reading the index.
#
# A missing container runtime is a hard failure, not a skip: this gate is the only thing that
# distinguishes a working archive from a plausible-looking one, and a release lane that
# quietly degrades to "we did not check" is how a broken repository reaches users.
#
# Usage: verify-apt-repo.sh <repo-root> [older-repo-root]
# Requires: docker or podman.
set -euo pipefail

REPO_ROOT="${1:?usage: verify-apt-repo.sh <repo-root> [older-repo-root]}"
OLDER_ROOT="${2:-}"
TARGET_IMAGE="${OKP_APT_VERIFY_IMAGE:-debian:13-slim}"

[[ -d "$REPO_ROOT" ]] || { echo "::error::APT repository root not found: $REPO_ROOT" >&2; exit 1; }
REPO_ROOT="$(cd -- "$REPO_ROOT" && pwd)"
if [[ -n "$OLDER_ROOT" ]]; then
  [[ -d "$OLDER_ROOT" ]] || { echo "::error::older APT repository root not found: $OLDER_ROOT" >&2; exit 1; }
  OLDER_ROOT="$(cd -- "$OLDER_ROOT" && pwd)"
fi

CONTAINER_RUNTIME=""
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=docker
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=podman
fi
if [[ -z "$CONTAINER_RUNTIME" ]]; then
  echo "::error::verifying the APT repository requires a usable docker or podman runtime; refusing to publish an archive no package manager has been asked to install from." >&2
  exit 127
fi

# The newest version the archive advertises: dpkg-scanpackages emits paragraphs in ascending
# version order, so the last Version wins. Read here rather than in the container so a
# malformed index fails before a container is even started.
PACKAGES_INDEX="$REPO_ROOT/dists/stable/main/binary-amd64/Packages"
[[ -s "$PACKAGES_INDEX" ]] || { echo "::error::no Packages index at $PACKAGES_INDEX" >&2; exit 1; }
EXPECTED_VERSION="$(awk '/^Version: / { version = $2 } END { print version }' "$PACKAGES_INDEX")"
[[ -n "$EXPECTED_VERSION" ]] || { echo "::error::the Packages index advertises no version" >&2; exit 1; }

OLDER_VERSION=""
if [[ -n "$OLDER_ROOT" ]]; then
  OLDER_VERSION="$(awk '/^Version: / { version = $2 } END { print version }' \
    "$OLDER_ROOT/dists/stable/main/binary-amd64/Packages")"
  [[ -n "$OLDER_VERSION" ]] || { echo "::error::the older archive advertises no version" >&2; exit 1; }
fi

echo "Verifying the APT archive at $REPO_ROOT (newest ${EXPECTED_VERSION}) in $TARGET_IMAGE via $CONTAINER_RUNTIME"
[[ -z "$OLDER_VERSION" ]] || echo "Upgrade path under test: ${OLDER_VERSION} -> ${EXPECTED_VERSION}"

mounts=(--mount "type=bind,src=${REPO_ROOT},dst=/repo-new,readonly")
if [[ -n "$OLDER_ROOT" ]]; then
  mounts+=(--mount "type=bind,src=${OLDER_ROOT},dst=/repo-old,readonly")
fi

"$CONTAINER_RUNTIME" run --rm -i \
  "${mounts[@]}" \
  -e EXPECTED_VERSION="$EXPECTED_VERSION" \
  -e OLDER_VERSION="$OLDER_VERSION" \
  -e DEBIAN_FRONTEND=noninteractive \
  "$TARGET_IMAGE" bash -s <<'CONTAINER'
set -euo pipefail

fail() { echo "apt repository verification: $1" >&2; exit 1; }

use_archive() {
  # use_archive <source-dir> — publish it at /work, the URI the source line points at.
  rm -rf /work
  cp -a "$1" /work
}

installed_version() {
  dpkg-query --showformat='${Version}' --show ok-player 2>/dev/null || true
}

apt-get update -qq
# gpgv is what apt uses to check InRelease; ca-certificates is unrelated to file:// but keeps
# the root honest if the source is ever pointed at the real https URI by hand.
apt-get install -y -qq --no-install-recommends gpgv ca-certificates >/dev/null

FIRST_ARCHIVE=/repo-new
[[ -z "$OLDER_VERSION" ]] || FIRST_ARCHIVE=/repo-old
use_archive "$FIRST_ARCHIVE"

install -m 0644 /work/ok-player-archive-keyring.gpg /usr/share/keyrings/ok-player-archive-keyring.gpg
cat >/etc/apt/sources.list.d/ok-player.list <<'SOURCE'
deb [signed-by=/usr/share/keyrings/ok-player-archive-keyring.gpg] file:///work stable main
SOURCE

echo "--- apt-get update against the OK Player archive ---"
apt-get update

echo "--- apt-get install ok-player ---"
apt-get install -y ok-player

if [[ -n "$OLDER_VERSION" ]]; then
  [[ "$(installed_version)" == "$OLDER_VERSION" ]] \
    || fail "expected the older archive to install $OLDER_VERSION, got $(installed_version)"
  echo "installed from the older archive: $(installed_version)"

  echo "--- publish the newer archive over the same repository and upgrade ---"
  use_archive /repo-new
  apt-get update
  apt-get upgrade -y
  [[ "$(installed_version)" == "$EXPECTED_VERSION" ]] \
    || fail "apt-get upgrade did not move to $EXPECTED_VERSION (still $(installed_version))"
  echo "upgraded to: $(installed_version)"

  echo "--- the previous version is still resolvable by explicit version ---"
  apt-cache madison ok-player | grep -F "$OLDER_VERSION" >/dev/null \
    || fail "$OLDER_VERSION disappeared from the archive after $EXPECTED_VERSION was published"
  apt-get install -y --allow-downgrades "ok-player=$OLDER_VERSION"
  [[ "$(installed_version)" == "$OLDER_VERSION" ]] \
    || fail "could not install the retained $OLDER_VERSION by explicit version"
  echo "explicit-version install of the retained release: $(installed_version)"

  apt-get install -y --allow-downgrades "ok-player=$EXPECTED_VERSION"
fi

[[ "$(installed_version)" == "$EXPECTED_VERSION" ]] \
  || fail "expected $EXPECTED_VERSION to be installed, got $(installed_version)"
echo "installed version: $(installed_version)"

echo "--- the installed binary starts ---"
# No display in a container, so the process is expected to die shortly after it decides its
# renderer policy. Reaching that decision is the signal that the packaged binary and its
# dependency closure are real; the exit status afterwards is not.
#
# Which signal is required depends on the package, and deliberately so. Current builds log
# "Renderer policy:" as the first statement of main(), before GTK, so for them that line is
# mandatory. Releases published before that line existed can only prove they reached GTK's
# display connection, and the archive has to stay verifiable over the versions it actually
# carries — so the requirement is derived from the packaged binary rather than relaxed for
# everyone. Either way an unresolved shared library is a hard failure, which is the outcome
# this check exists to catch.
INSTALLED_BINARY=/usr/lib/ok-player/ok-player
[[ -x "$INSTALLED_BINARY" ]] || fail "the package did not install $INSTALLED_BINARY"
if grep -aq 'Renderer policy:' "$INSTALLED_BINARY"; then
  LAUNCH_SIGNAL='Renderer policy:'
else
  LAUNCH_SIGNAL='Renderer policy:|Failed to open display'
  echo "note: this build predates the renderer-policy log line; requiring a GTK display attempt instead"
fi
set +e
timeout 90 /usr/bin/ok-player </dev/null >/tmp/launch.log 2>&1
set -e
if grep -q 'error while loading shared libraries' /tmp/launch.log; then
  cat /tmp/launch.log >&2
  fail "the installed binary could not resolve its shared libraries"
fi
if ! grep -Eq "$LAUNCH_SIGNAL" /tmp/launch.log; then
  echo "--- launch log ---" >&2
  cat /tmp/launch.log >&2
  fail "the installed binary never reached its GUI initialisation"
fi
grep -Em1 "$LAUNCH_SIGNAL" /tmp/launch.log

# A signature apt will not accept is only half the story. By default `apt-get update` reports
# a rejected repository as a warning, keeps the previously fetched index and still exits 0 —
# so asserting on its exit status alone would pass on a stock apt even if it had happily used
# the forged index. Each control therefore asserts both halves: the update fails once errors
# are not downgraded to warnings (APT::Update::Error-Mode=any), and, with every cached index
# discarded first, the package is genuinely not installable from the rejected archive.
negative_control() {
  # negative_control <label> <expected-message-pattern> <log>
  local label="$1" pattern="$2" log="$3"
  apt-get purge -y ok-player >/dev/null 2>&1 || true
  rm -rf /var/lib/apt/lists/*
  if apt-get update -o APT::Update::Error-Mode=any >"$log" 2>&1; then
    cat "$log" >&2
    fail "apt-get update accepted ${label}"
  fi
  grep -Eiq "$pattern" "$log" \
    || { cat "$log" >&2; fail "apt-get update failed on ${label}, but not over the signature"; }
  if apt-get install -y ok-player >"${log}.install" 2>&1; then
    cat "${log}.install" >&2
    fail "ok-player was still installable from ${label}"
  fi
  echo "apt refused ${label} and would not install from it"
}

echo "--- negative control: a tampered InRelease must be refused ---"
use_archive /repo-new
sed -i 's/^Origin: OK Player/Origin: Not OK Player/' /work/dists/stable/InRelease
negative_control "a tampered InRelease" 'manipulat|BADSIG|signature' /tmp/tampered.log

echo "--- negative control: a keyring holding another key must be refused ---"
use_archive /repo-new
apt-get install -y -qq --no-install-recommends gnupg >/dev/null
export GNUPGHOME=/tmp/wrong-key
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-generate-key 'Wrong Key <wrong@example.invalid>' ed25519 sign never
gpg --batch --quiet --export >/usr/share/keyrings/ok-player-archive-keyring.gpg
negative_control "an archive signed by a key the keyring does not hold" \
  'NO_PUBKEY|not signed|public key|signature' /tmp/wrong-key.log

echo "apt repository verification: PASS"
CONTAINER

echo "APT repository verification PASS (${EXPECTED_VERSION})"
