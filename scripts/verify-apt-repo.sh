#!/usr/bin/env bash
# Prove the generated APT repository actually works, from a clean Debian root (issues #683, #689).
#
# A repository that merely looks right is worthless: the failure modes that matter — a
# signature apt will not accept, a Packages entry apt cannot resolve, a package whose
# dependencies do not exist on the target, a QA build leaking into the release channel — are
# only visible to apt itself. So this runs apt against the archive inside a throwaway
# debian:13-slim container, once per suite the archive carries:
#
#   * add the published keyring and the suite's source, `apt-get update`, `apt-get install
#     ok-player`, and require the version apt selects to be the one that suite advertises;
#   * start /usr/bin/ok-player headless and require it to reach its renderer-policy decision,
#     the same "it really started" signal the Linux smokes use (the process then dies on the
#     absent display, which is expected and not a failure);
#   * channel isolation — a root subscribed to `stable` must never see a candidate version, in
#     `apt-cache policy` or in `apt-cache madison`. That separation is the whole reason the
#     candidate suite exists as a suite rather than as newer packages in `stable`;
#   * negative control 1 — a tampered InRelease must make `apt-get update` fail, per suite;
#   * negative control 2 — a keyring holding some other key must make `apt-get update` fail,
#     per suite, which is also what proves both suites are signed by the same key.
#
# Given a second, older archive it also proves the upgrade path end to end, per suite: install
# from the old archive, swap in the new one, and require `apt-get upgrade` to move to the newer
# version while the older one stays resolvable by explicit version. That is the additivity
# promise the generator makes, checked by the package manager rather than by reading the index.
#
# A missing container runtime is a hard failure, not a skip: this gate is the only thing that
# distinguishes a working archive from a plausible-looking one, and a release lane that
# quietly degrades to "we did not check" is how a broken repository reaches users.
#
#   * the version scheme — every version the archive advertises must be the encoding of the
#     build its pool file is named for, and must outrank everything published before the
#     encoding existed. This one is checked without a container, because it decides whether a
#     release can be shipped at all rather than whether a package installs (issue #709).
#
# Usage: verify-apt-repo.sh <repo-root> [older-repo-root]
# Requires: docker or podman, and dpkg (for version comparison).
set -euo pipefail

# The Debian version encoding, shared with the packaging that produced these files.
OKP_APT_VERIFY_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
source "$OKP_APT_VERIFY_ROOT/scripts/linux-package-version.sh"

okp_apt_verify_fail() {
  printf '::error::%s\n' "$*" >&2
  exit 1
}

# The highest version an archive advertises, by Debian version ordering.
#
# Not "the last paragraph in Packages": dpkg-scanpackages keys its index by package name and
# version and emits it in lexicographic key order, not Debian version order. `--multiversion`
# only promises to allow several versions of one package, it says nothing about sorting them.
# So 0.11.0-beta.10 is emitted before 0.11.0-beta.2 and 0.11.0-beta.9 — taking the last
# paragraph would name .9 as newest while apt installs .10, and this gate would then assert
# against a version apt never selected and fail every Pages deploy. apt itself is unaffected:
# it parses every paragraph and picks the maximum. Only the expectation was wrong.
okp_apt_newest_indexed_version() {
  local index="$1" version best=''
  while IFS= read -r version; do
    [[ -n "$version" ]] || continue
    if [[ -z "$best" ]] || dpkg --compare-versions "$version" gt "$best"; then
      best="$version"
    fi
  done < <(awk '/^Version: / { print $2 }' "$index")
  [[ -n "$best" ]] || return 1
  printf '%s' "$best"
}

okp_apt_suite_index() {
  # okp_apt_suite_index <repo-root> [suite]
  printf '%s/dists/%s/main/binary-amd64/Packages' "$1" "${2:-stable}"
}

okp_apt_has_suite() {
  # okp_apt_has_suite <repo-root> <suite>
  [[ -s "$(okp_apt_suite_index "$1" "$2")" ]]
}

# The versions one suite carries and another does not. The shared pool makes overlap legitimate —
# a candidate promoted unchanged is the same package in both suites — so "the candidate version
# appears in a stable-only policy listing" is not by itself evidence of a leak. Only a version
# that exists in `candidate` and NOT in `stable` can leak, and those are the ones worth asserting
# about. With full overlap the set is empty and there is nothing that could leak.
okp_apt_versions_only_in() {
  # okp_apt_versions_only_in <repo-root> <suite> <other suite>
  local root="$1" suite="$2" other="$3"
  comm -13 \
    <(awk '/^Version: / { print $2 }' "$(okp_apt_suite_index "$root" "$other")" | sort -u) \
    <(awk '/^Version: / { print $2 }' "$(okp_apt_suite_index "$root" "$suite")" | sort -u) \
    | tr '\n' ' '
}

# The build version a pool file is named for. `.deb` file names are not encoded — see
# scripts/linux-package-version.sh — so the file name is the archive's record of which build
# each paragraph describes, and comparing it against the paragraph's `Version:` is what makes
# the encoding checkable at all rather than merely asserted.
okp_apt_build_version_from_pool_name() {
  # okp_apt_build_version_from_pool_name <pool path or basename>
  local name="${1##*/}" build
  build="${name#ok-player_}"
  build="${build%_amd64.deb}"
  if [[ "$build" == "$name" || -z "$build" ]]; then
    okp_apt_verify_fail "the archive carries ${name}, which is not an ok-player_<version>_amd64.deb; its build version cannot be read."
  fi
  printf '%s' "$build"
}

# Every version the archive advertises must be ordered by dpkg the way the builds are ordered
# (issue #709).
#
# The defect this exists for is invisible in a passing install: a `.deb` published as
# `0.11.0-beta.0.209` installs perfectly well, and only sorts *above* the `0.11.0` release
# that follows it — so the release simply never reaches anyone, silently, months later. The
# assertion is therefore about the strings rather than about apt's behaviour today:
#
#   * a version at or below OKP_DEBIAN_LEGACY_HIGHWATER with no epoch is a package published
#     before the encoding existed. The archive is rebuilt from the release assets, so those
#     stay in the rolling window until they age out; they are counted and reported, not
#     failed. They can never outrank an encoded version — that is what the epoch buys.
#   * everything else must be exactly `okp_debian_version_for_build` of the build its pool
#     file is named for. Equality, not a pattern: an encoding that quietly disagrees with the
#     build it came from is the same class of bug in a new place.
#   * an encoded version must outrank the legacy high-water mark, or a tester already on it
#     cannot move.
#   * a prerelease must sort below its own release. `1:0.11.0~beta.0.209 lt 1:0.11.0` is the
#     whole point of the `~`, and it is asserted with dpkg rather than reasoned about.
okp_apt_assert_version_scheme() {
  # okp_apt_assert_version_scheme <repo-root> <suite>
  local root="$1" suite="$2" index
  index="$(okp_apt_suite_index "$root" "$suite")"
  [[ -s "$index" ]] || okp_apt_verify_fail "no Packages index at ${index}"

  local filename version build expected release legacy=0 encoded=0
  while IFS=$'\t' read -r filename version; do
    [[ -n "$version" ]] || continue
    [[ -n "$filename" ]] \
      || okp_apt_verify_fail "the ${suite} suite indexes ${version} with no Filename; the build it describes cannot be read."
    if [[ "$version" != *:* ]] \
      && dpkg --compare-versions "$version" le "$OKP_DEBIAN_LEGACY_HIGHWATER"; then
      legacy=$((legacy + 1))
      continue
    fi
    build="$(okp_apt_build_version_from_pool_name "$filename")"
    expected="$(okp_debian_version_for_build "$build")" \
      || okp_apt_verify_fail "the ${suite} suite carries ${filename}, whose build version ${build} this packaging cannot encode."
    [[ "$version" == "$expected" ]] \
      || okp_apt_verify_fail "the ${suite} suite advertises ${version} for ${filename}; the build ${build} encodes to ${expected}. A version apt orders differently than the builds order is how a stable release stops reaching users (issue #709)."
    dpkg --compare-versions "$version" gt "$OKP_DEBIAN_LEGACY_HIGHWATER" \
      || okp_apt_verify_fail "the ${suite} suite advertises ${version}, which does not outrank ${OKP_DEBIAN_LEGACY_HIGHWATER}; every machine already on that build would refuse it as a downgrade."
    if [[ "$build" == *-* ]]; then
      release="${OKP_DEBIAN_VERSION_EPOCH}:${build%%-*}"
      dpkg --compare-versions "$version" lt "$release" \
        || okp_apt_verify_fail "the ${suite} suite advertises the prerelease ${version}, which does not sort below its own release ${release}; the release could never be installed over it."
    fi
    encoded=$((encoded + 1))
  done < <(awk '
    /^Version: / { version = $2 }
    /^Filename: / { filename = $2 }
    /^$/ { if (version != "") print filename "\t" version; version = ""; filename = "" }
    END { if (version != "") print filename "\t" version }
  ' "$index")

  ((encoded + legacy > 0)) \
    || okp_apt_verify_fail "the ${suite} suite advertises no version at all."
  printf 'version scheme (%s): %s encoded, %s published before the encoding\n' \
    "$suite" "$encoded" "$legacy"
}

okp_apt_archive_version() {
  # okp_apt_archive_version <repo-root> <label> [suite]
  local index version
  index="$(okp_apt_suite_index "$1" "${3:-stable}")"
  [[ -s "$index" ]] || { echo "::error::no Packages index at $index" >&2; exit 1; }
  version="$(okp_apt_newest_indexed_version "$index")" \
    || { echo "::error::the $2 archive advertises no version" >&2; exit 1; }
  printf '%s' "$version"
}

# Named, not `main`, so a test can source this file alongside build-apt-repo.sh without the
# two entry points colliding.
okp_apt_verify_main() {
REPO_ROOT="${1:?usage: verify-apt-repo.sh <repo-root> [older-repo-root]}"
OLDER_ROOT="${2:-}"
TARGET_IMAGE="${OKP_APT_VERIFY_IMAGE:-debian:13-slim}"

command -v dpkg >/dev/null 2>&1 \
  || { echo "::error::verifying the APT repository requires dpkg for version comparison." >&2; exit 1; }

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

# `stable` is mandatory; `candidate` exists only once the rolling channel has published. Read
# here rather than in the container so a malformed index fails before one is started.
okp_apt_has_suite "$REPO_ROOT" stable \
  || { echo "::error::the archive at $REPO_ROOT carries no stable suite" >&2; exit 1; }
okp_apt_assert_version_scheme "$REPO_ROOT" stable
STABLE_VERSION="$(okp_apt_archive_version "$REPO_ROOT" published stable)"
CANDIDATE_VERSION=""
CANDIDATE_ONLY_VERSIONS=""
if okp_apt_has_suite "$REPO_ROOT" candidate; then
  okp_apt_assert_version_scheme "$REPO_ROOT" candidate
  CANDIDATE_VERSION="$(okp_apt_archive_version "$REPO_ROOT" published candidate)"
  CANDIDATE_ONLY_VERSIONS="$(okp_apt_versions_only_in "$REPO_ROOT" candidate stable)"
fi

OLDER_STABLE_VERSION=""
OLDER_CANDIDATE_VERSION=""
if [[ -n "$OLDER_ROOT" ]]; then
  okp_apt_has_suite "$OLDER_ROOT" stable \
    || { echo "::error::the older archive at $OLDER_ROOT carries no stable suite" >&2; exit 1; }
  OLDER_STABLE_VERSION="$(okp_apt_archive_version "$OLDER_ROOT" older stable)"
  if [[ -n "$CANDIDATE_VERSION" ]] && okp_apt_has_suite "$OLDER_ROOT" candidate; then
    OLDER_CANDIDATE_VERSION="$(okp_apt_archive_version "$OLDER_ROOT" older candidate)"
  fi
fi

echo "Verifying the APT archive at $REPO_ROOT in $TARGET_IMAGE via $CONTAINER_RUNTIME"
echo "  stable:    ${STABLE_VERSION}${OLDER_STABLE_VERSION:+ (upgrade from ${OLDER_STABLE_VERSION})}"
if [[ -n "$CANDIDATE_VERSION" ]]; then
  echo "  candidate: ${CANDIDATE_VERSION}${OLDER_CANDIDATE_VERSION:+ (upgrade from ${OLDER_CANDIDATE_VERSION})}"
else
  echo "  candidate: absent from this archive"
fi

mounts=(--mount "type=bind,src=${REPO_ROOT},dst=/repo-new,readonly")
if [[ -n "$OLDER_ROOT" ]]; then
  mounts+=(--mount "type=bind,src=${OLDER_ROOT},dst=/repo-old,readonly")
fi

"$CONTAINER_RUNTIME" run --rm -i \
  "${mounts[@]}" \
  -e STABLE_VERSION="$STABLE_VERSION" \
  -e CANDIDATE_VERSION="$CANDIDATE_VERSION" \
  -e CANDIDATE_ONLY_VERSIONS="$CANDIDATE_ONLY_VERSIONS" \
  -e OLDER_STABLE_VERSION="$OLDER_STABLE_VERSION" \
  -e OLDER_CANDIDATE_VERSION="$OLDER_CANDIDATE_VERSION" \
  -e DEBIAN_FRONTEND=noninteractive \
  "$TARGET_IMAGE" bash -s <<'CONTAINER'
set -euo pipefail

fail() { echo "apt repository verification: $1" >&2; exit 1; }

use_archive() {
  # use_archive <source-dir> — publish it at /work, the URI the source lines point at.
  rm -rf /work
  cp -a "$1" /work
}

subscribe() {
  # subscribe <suite> — exactly one suite, which is how a real machine is configured.
  printf 'deb [signed-by=/usr/share/keyrings/ok-player-archive-keyring.gpg] file:///work %s main\n' \
    "$1" >/etc/apt/sources.list.d/ok-player.list
}

install_keyring() {
  install -m 0644 /work/ok-player-archive-keyring.gpg \
    /usr/share/keyrings/ok-player-archive-keyring.gpg
}

reset_root() {
  apt-get purge -y ok-player >/dev/null 2>&1 || true
  rm -rf /var/lib/apt/lists/*
}

installed_version() {
  dpkg-query --showformat='${Version}' --show ok-player 2>/dev/null || true
}

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
launch_check() {
  local installed_binary=/usr/lib/ok-player/ok-player signal
  [[ -x "$installed_binary" ]] || fail "the package did not install $installed_binary"
  if grep -aq 'Renderer policy:' "$installed_binary"; then
    signal='Renderer policy:'
  else
    signal='Renderer policy:|Failed to open display'
    echo "note: this build predates the renderer-policy log line; requiring a GTK display attempt instead"
  fi
  set +e
  timeout 90 /usr/bin/ok-player </dev/null >/tmp/launch.log 2>&1
  set -e
  if grep -q 'error while loading shared libraries' /tmp/launch.log; then
    cat /tmp/launch.log >&2
    fail "the installed binary could not resolve its shared libraries"
  fi
  if ! grep -Eq "$signal" /tmp/launch.log; then
    echo "--- launch log ---" >&2
    cat /tmp/launch.log >&2
    fail "the installed binary never reached its GUI initialisation"
  fi
  grep -Em1 "$signal" /tmp/launch.log
}

verify_suite() {
  # verify_suite <suite> <expected version> <older version, may be empty>
  local suite="$1" expected="$2" older="$3" first=/repo-new
  echo "=== suite ${suite}: install${older:+, upgrade from ${older}} ==="
  reset_root
  [[ -z "$older" ]] || first=/repo-old
  use_archive "$first"
  install_keyring
  subscribe "$suite"

  echo "--- apt-get update against the ${suite} suite ---"
  apt-get update
  echo "--- apt-get install ok-player from ${suite} ---"
  apt-get install -y ok-player

  if [[ -n "$older" ]]; then
    [[ "$(installed_version)" == "$older" ]] \
      || fail "expected the older ${suite} archive to install ${older}, got $(installed_version)"
    echo "installed from the older ${suite} archive: $(installed_version)"

    echo "--- publish the newer archive over the same repository and upgrade ---"
    use_archive /repo-new
    apt-get update
    apt-get upgrade -y
    [[ "$(installed_version)" == "$expected" ]] \
      || fail "apt-get upgrade did not move ${suite} to ${expected} (still $(installed_version))"
    echo "upgraded to: $(installed_version)"

    echo "--- the previous ${suite} version is still resolvable by explicit version ---"
    apt-cache madison ok-player | grep -F "$older" >/dev/null \
      || fail "${older} disappeared from ${suite} after ${expected} was published"
    apt-get install -y --allow-downgrades "ok-player=$older"
    [[ "$(installed_version)" == "$older" ]] \
      || fail "could not install the retained ${older} by explicit version"
    echo "explicit-version install of the retained build: $(installed_version)"
    apt-get install -y --allow-downgrades "ok-player=$expected"
  fi

  [[ "$(installed_version)" == "$expected" ]] \
    || fail "expected ${suite} to install ${expected}, got $(installed_version)"
  echo "installed version from ${suite}: $(installed_version)"

  echo "--- the installed binary starts ---"
  launch_check
}

apt-get update -qq
# gpgv is what apt uses to check InRelease; ca-certificates is unrelated to file:// but keeps
# the root honest if the source is ever pointed at the real https URI by hand.
apt-get install -y -qq --no-install-recommends gpgv ca-certificates >/dev/null

SUITES=(stable)
[[ -z "$CANDIDATE_VERSION" ]] || SUITES+=(candidate)

verify_suite stable "$STABLE_VERSION" "$OLDER_STABLE_VERSION"

# The separation the candidate suite exists for: this root is subscribed to `stable` only, and
# the candidate packages are sitting in the very same pool it is reading from. If suite
# membership did not hold, apt would offer the higher version here — so this is the assertion
# that "a machine subscribed to stable is never offered a candidate" is a property of the
# archive, not of which packages happen to be published.
if [[ -n "$CANDIDATE_VERSION" ]]; then
  echo "--- a stable-only root is never offered the candidate ---"
  use_archive /repo-new
  apt-get update
  apt-cache policy ok-player

  # Only a version `candidate` carries and `stable` does not can leak. The shared pool makes
  # overlap legitimate — a candidate promoted unchanged is literally the same package in both
  # suites — so asserting on the candidate's version string alone would fail the whole lane the
  # first time a promotion happened, over a version that came from the stable index.
  for version in $CANDIDATE_ONLY_VERSIONS; do
    if apt-cache policy ok-player | grep -F "$version" >/dev/null; then
      fail "apt-cache policy offered the candidate-only ${version} to a stable-only root"
    fi
    if apt-cache madison ok-player | grep -F "$version" >/dev/null; then
      fail "apt-cache madison listed the candidate-only ${version} for a stable-only root"
    fi
  done
  if [[ -z "${CANDIDATE_ONLY_VERSIONS// /}" ]]; then
    echo "note: every candidate version is also a stable one, so there is nothing that could leak"
  else
    echo "none of the candidate-only versions (${CANDIDATE_ONLY_VERSIONS}) reached a stable-only root"
  fi

  # True whether or not the suites overlap: a stable-only root resolves to the stable head.
  [[ "$(apt-cache policy ok-player | awk '/Candidate:/ { print $2 }')" == "$STABLE_VERSION" ]] \
    || fail "a stable-only root does not resolve ok-player to ${STABLE_VERSION}"
  echo "stable-only root sees ${STABLE_VERSION} and nothing newer"

  verify_suite candidate "$CANDIDATE_VERSION" "$OLDER_CANDIDATE_VERSION"
fi

# A signature apt will not accept is only half the story. By default `apt-get update` reports
# a rejected repository as a warning, keeps the previously fetched index and still exits 0 —
# so asserting on its exit status alone would pass on a stock apt even if it had happily used
# the forged index. Each control therefore asserts both halves: the update fails once errors
# are not downgraded to warnings (APT::Update::Error-Mode=any), and, with every cached index
# discarded first, the package is genuinely not installable from the rejected archive.
negative_control() {
  # negative_control <label> <expected-message-pattern> <log>
  local label="$1" pattern="$2" log="$3"
  reset_root
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

for suite in "${SUITES[@]}"; do
  echo "--- negative control: a tampered ${suite} InRelease must be refused ---"
  use_archive /repo-new
  install_keyring
  subscribe "$suite"
  sed -i 's/^Origin: OK Player/Origin: Not OK Player/' "/work/dists/${suite}/InRelease"
  negative_control "a tampered ${suite} InRelease" 'manipulat|BADSIG|signature' "/tmp/tampered-${suite}.log"
done

apt-get install -y -qq --no-install-recommends gnupg >/dev/null
export GNUPGHOME=/tmp/wrong-key
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"
gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-generate-key 'Wrong Key <wrong@example.invalid>' ed25519 sign never
# Every suite must fail against this keyring. That is also the proof that no suite is signed by
# some other key: if one were, it would still be accepted here.
for suite in "${SUITES[@]}"; do
  echo "--- negative control: a keyring holding another key must be refused (${suite}) ---"
  use_archive /repo-new
  gpg --batch --quiet --export >/usr/share/keyrings/ok-player-archive-keyring.gpg
  subscribe "$suite"
  negative_control "the ${suite} suite signed by a key the keyring does not hold" \
    'NO_PUBKEY|not signed|public key|signature' "/tmp/wrong-key-${suite}.log"
done

echo "apt repository verification: PASS"
CONTAINER

echo "APT repository verification PASS (stable ${STABLE_VERSION}${CANDIDATE_VERSION:+, candidate ${CANDIDATE_VERSION}})"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  okp_apt_verify_main "$@"
fi
