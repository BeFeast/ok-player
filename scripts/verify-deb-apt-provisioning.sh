#!/usr/bin/env bash
# Prove that a standalone .deb arrives with the OK Player APT repository already configured
# (issue #726).
#
# The defect this gate exists for was measured, not guessed: a .deb downloaded from the
# releases page installed a player whose postinst refreshed the desktop and icon caches and
# nothing else, so the machine ended up with OK Player installed and no OK Player apt source.
# `apt-cache policy ok-player` listed exactly one origin, /var/lib/dpkg/status, and the
# operator sat on a stale build for three days while the app told him to update through apt.
#
# So the assertion here is apt's own answer on a real machine, and nothing weaker: install the
# package from a file, run `apt-get update`, and require `apt-cache policy ok-player` to name
# the archive as a source and offer a newer build — with no step in between. Everything is
# real: a signed archive built by the production generator, packages built by the production
# packaging script, apt resolving and installing across the two. Only three things are
# substituted, and each is named where it happens: the signing key is a throwaway, the archive
# is served over file:// from a bind mount instead of from Pages, and the compiled binary and
# the bundled mpv runtime are stubs, because this gate is about DEBIAN/postinst and not about
# what the player does once it starts (scripts/verify-apt-repo.sh already launches the real
# packaged binary out of the archive).
#
# Four scenarios, then the negative control:
#
#   stable           a stable package lands the machine on the stable suite, apt upgrades it,
#                    and `apt-get purge` leaves nothing behind under /etc/apt or
#                    /usr/share/keyrings.
#   candidate        the same for a candidate package, landing on the candidate suite. A tester
#                    who installed a candidate .deb must not silently end up on stable.
#   existing-choice  a machine already subscribed to candidate keeps that subscription when a
#                    stable package is installed over it.
#   no-provisioning  the negative control: the same package with the provisioning stripped out
#                    of postinst — the pre-#726 package, byte for byte where it matters — must
#                    fail at `apt-cache policy`, in exactly the state the operator was in.
#
# Usage: verify-deb-apt-provisioning.sh [scenario ...]   (default: all of them)
# Requires: docker or podman, gpg, gpgconf, dpkg-deb, dpkg-scanpackages, gzip, sha256sum,
#           md5sum. A missing container runtime is a hard failure (127), never a skip: this
#           gate is the only thing that distinguishes a package that provisions the repository
#           from one that merely looks like it does.

set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
GENERATOR="$ROOT/scripts/build-apt-repo.sh"
IMAGE="${OKP_DEB_APT_GATE_IMAGE:-debian:13-slim}"

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

for tool in gpg gpgconf dpkg-deb dpkg-scanpackages gzip sha256sum md5sum; do
  command -v "$tool" >/dev/null 2>&1 ||
    { printf 'Missing required tool: %s\n' "$tool" >&2; exit 127; }
done

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  RUNTIME=docker
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  RUNTIME=podman
else
  echo "Missing usable container runtime: docker or podman" >&2
  exit 127
fi

failures=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2" >&2; failures=$((failures + 1)); }

# shellcheck source=/dev/null
source "$GENERATOR"

# --- A throwaway signing key, used the way the real one is -----------------------------
# The production key lives in Infisical and is not available here, and asking for it would
# make this gate unrunnable on a developer machine. What matters is that the key inside the
# package is the key the archive is signed with, which this reproduces exactly: one key, put
# in both places, and the package's own build-time fingerprint gate pointed at it.
KEY_HOME="$WORK/keys"
mkdir -p "$KEY_HOME"
chmod 700 "$KEY_HOME"
PASSPHRASE='correct horse battery staple'
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --pinentry-mode loopback --passphrase "$PASSPHRASE" \
  --quick-generate-key 'OK Player Gate Signing <gate@example.invalid>' ed25519 sign never
FINGERPRINT="$(
  GNUPGHOME="$KEY_HOME" gpg --batch --with-colons --fingerprint --list-keys |
    awk -F: '$1 == "fpr" { print $10; exit }'
)"
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --pinentry-mode loopback --passphrase "$PASSPHRASE" \
  --armor --export-secret-keys --output "$WORK/private.asc" "$FINGERPRINT"
GNUPGHOME="$KEY_HOME" gpg --batch --quiet --armor --export \
  --output "$WORK/public.asc" "$FINGERPRINT"

# One key, in the packages and signing the archive — which is the property the production
# constant exists to guarantee. Both sides are held to OKP_APT_SIGNING_FINGERPRINT (#726), so
# this run says which key it is about. By assignment, since the generator is sourced here:
# there is no environment override a release build could take.
OKP_APT_SIGNING_FINGERPRINT="$FINGERPRINT"

SECRET_DIR="$WORK/secrets"
mkdir -p "$SECRET_DIR"
cp "$WORK/private.asc" "$SECRET_DIR/gpg-private-key"
cp "$WORK/public.asc" "$SECRET_DIR/gpg-public-key"
printf '%s\n' "$FINGERPRINT" >"$SECRET_DIR/gpg-fingerprint"
printf '%s\n' "$PASSPHRASE" >"$SECRET_DIR/gpg-passphrase"
gate_secret_reader() {
  [[ -f "$SECRET_DIR/$1" ]] || return 1
  cat "$SECRET_DIR/$1"
}

# --- The packaging, run for real against a fixture root --------------------------------
# The same substitution scripts/tests/linux-package-version.Tests.sh uses: the compiler, the
# bundled mpv runtime and its verifier are stubbed, so what runs is the real control-file
# emission, the real maintainer scripts, the real key handling and the real dpkg-deb.
FIXTURE="$WORK/repo"
STUB_BIN="$WORK/stub-bin"
mkdir -p \
  "$FIXTURE/scripts" \
  "$FIXTURE/rust/packaging/linux/icons/hicolor" \
  "$FIXTURE/rust/target/release" \
  "$FIXTURE/mpv-runtime" \
  "$STUB_BIN"
cp "$ROOT/scripts/package-linux-deb.sh" "$ROOT/scripts/apt-archive-identity.sh" \
  "$ROOT/scripts/linux-package-version.sh" "$ROOT/scripts/stage-license-documents.sh" \
  "$FIXTURE/scripts/"
# The packaging stages the licence documents into every package (#752), so the fixture carries
# the real ones — this suite runs the real packaging script, not a copy of part of it.
cp "$ROOT/LICENSE" "$ROOT/LICENSE.LGPL-3.0" "$ROOT/THIRD-PARTY-NOTICES.md" "$FIXTURE/"
mkdir -p "$FIXTURE/rust/packaging/linux"
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

# The archive the packages point at, as the container sees it.
MOUNT=/srv/ok-player
BASE_URL="file://${MOUNT}/site/apt"

DEBS="$WORK/debs"
mkdir -p "$DEBS"

package_deb() {
  # package_deb <build version> <suite> — writes the .deb into $DEBS and echoes its path.
  local build="$1" suite="$2"
  env -u CARGO_TARGET_DIR PATH="$STUB_BIN:$PATH" \
    OKP_TEST_MPV_RUNTIME="$FIXTURE/mpv-runtime" \
    OKP_DEB_APT_SUITE="$suite" \
    OKP_DEB_APT_BASE_URL="$BASE_URL" \
    OKP_DEB_APT_PUBLIC_KEY="$WORK/public.asc" \
    OKP_DEB_APT_FINGERPRINT="$FINGERPRINT" \
    bash "$FIXTURE/scripts/package-linux-deb.sh" "$build" >/dev/null
  local built="$FIXTURE/artifacts/linux/deb/ok-player_${build}_amd64.deb"
  cp "$built" "$DEBS/"
  printf '%s/ok-player_%s_amd64.deb' "$DEBS" "$build"
}

# Versions chosen so each suite has its own pool file names: one shared pool indexed twice is
# how the real archive works, and two different packages cannot share a file name in it.
STABLE_INSTALLED=0.11.0
STABLE_ARCHIVE=0.11.1
CANDIDATE_INSTALLED=0.11.0-beta.0.208
CANDIDATE_ARCHIVE=0.11.0-beta.0.210

package_deb "$STABLE_INSTALLED" stable >/dev/null
package_deb "$STABLE_ARCHIVE" stable >/dev/null
package_deb "$CANDIDATE_INSTALLED" candidate >/dev/null
package_deb "$CANDIDATE_ARCHIVE" candidate >/dev/null

# --- The archive, built by the production generator ------------------------------------
STAGING="$WORK/pool-staging"
mkdir -p "$STAGING"
cp "$DEBS/ok-player_${STABLE_ARCHIVE}_amd64.deb" \
  "$DEBS/ok-player_${CANDIDATE_ARCHIVE}_amd64.deb" "$STAGING/"

PLAN="$WORK/plan"
mkdir -p "$PLAN"
printf 'stable\ncandidate\n' >"$PLAN/suites"
# The Release date is an input rather than a clock reading, exactly as the real lane derives it
# from the newest retained release's publication time.
printf '1750000000\n' >"$PLAN/stable.epoch"
printf '1750000000\n' >"$PLAN/candidate.epoch"
printf 'ok-player_%s_amd64.deb\n' "$STABLE_ARCHIVE" >"$PLAN/stable.members"
printf 'ok-player_%s_amd64.deb\n' "$CANDIDATE_ARCHIVE" >"$PLAN/candidate.members"

SITE="$WORK/site"
mkdir -p "$SITE/apt"
# A subshell: okp_apt_build_signed_repo installs its own EXIT trap for the ephemeral signing
# home, and it must not take this script's trap with it.
( okp_apt_build_signed_repo "$STAGING" "$SITE/apt" "$PLAN" "$BASE_URL" gate_secret_reader ) \
  >"$WORK/generator.log" 2>&1 || {
  echo "The APT archive generator failed; the gate has nothing to verify against:" >&2
  cat "$WORK/generator.log" >&2
  exit 1
}

# --- The negative control ---------------------------------------------------------------
# Not a package with a broken postinst: the pre-#726 package. The carried repository material
# is removed and postinst is put back to the cache refresh it used to be, which is exactly the
# artifact the operator installed.
STRIPPED="$WORK/stripped"
rm -rf -- "$STRIPPED"
dpkg-deb -R "$DEBS/ok-player_${STABLE_INSTALLED}_amd64.deb" "$STRIPPED"
rm -rf "$STRIPPED/usr/share/ok-player"
cat >"$STRIPPED/DEBIAN/postinst" <<'STRIPPED_POSTINST'
#!/bin/sh
set -e

root="${DPKG_ROOT:-}"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q "$root/usr/share/applications" || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$root/usr/share/icons/hicolor" || true
fi

exit 0
STRIPPED_POSTINST
chmod 755 "$STRIPPED/DEBIAN/postinst"
mkdir -p "$WORK/stripped-deb"
dpkg-deb --root-owner-group --build "$STRIPPED" \
  "$WORK/stripped-deb/ok-player_${STABLE_INSTALLED}_amd64.deb" >/dev/null
cp "$WORK/stripped-deb/ok-player_${STABLE_INSTALLED}_amd64.deb" \
  "$DEBS/ok-player_${STABLE_INSTALLED}_amd64.deb.stripped"

# --- The container transcripts ------------------------------------------------------------
mkdir -p "$WORK/scenarios"

# Every scenario body runs under `set -e` with the archive bind-mounted read-only, so the
# assertions are apt's exit status and apt's own output and nothing else.
run_scenario() {
  # run_scenario <name> <script-body-file>
  local name="$1" body="$2"
  printf '\n--- scenario: %s ---\n' "$name"
  # apt drops privileges to `_apt` to fetch, even over file://, and mktemp -d hands back
  # a 0700 directory. Without this the archive is unreadable to the fetcher and apt
  # reports a permission error rather than anything about the package under test.
  chmod -R a+rX "$WORK"
  if "$RUNTIME" run --rm \
    --mount "type=bind,src=$WORK,dst=$MOUNT,ro" \
    -e DEBIAN_FRONTEND=noninteractive \
    "$IMAGE" bash "$MOUNT/scenarios/$(basename "$body")" 2>&1 | tee "$WORK/$name.log"; then
    return 0
  fi
  return 1
}

# Shared preamble: refresh the base indices once, and prove the starting state is the one the
# operator was in — apt has never heard of ok-player.
cat >"$WORK/scenarios/preamble" <<'PREAMBLE'
set -euo pipefail
apt-get update -qq
if apt-cache policy ok-player | grep -q 'file:'; then
  echo "the container already had an OK Player source before anything was installed" >&2
  exit 1
fi
PREAMBLE

# assert_archive_offers <suite> <expected debian version>
cat >"$WORK/scenarios/helpers" <<'HELPERS'
assert_archive_offers() {
  suite="$1"
  expected="$2"
  policy="$(apt-cache policy ok-player)"
  printf '%s\n' "$policy"
  printf '%s\n' "$policy" | grep -qF "${suite}/main amd64 Packages" || {
    echo "apt-cache policy does not name the ${suite} archive as a source" >&2
    exit 1
  }
  actual="$(printf '%s\n' "$policy" | awk '/^  Candidate:/ { print $2; exit }')"
  [ "$actual" = "$expected" ] || {
    echo "apt offers ${actual}, expected ${expected}" >&2
    exit 1
  }
}

# Which archive apt reads for this package, which is a different question from which version
# it would install. A release outranks the prereleases that led to it (#709), so a machine on
# `candidate` that has a release installed is correctly offered nothing — and that must not be
# read as having lost the subscription.
assert_subscribed_to() {
  want="$1"
  other="$2"
  policy="$(apt-cache policy ok-player)"
  printf '%s\n' "$policy"
  printf '%s\n' "$policy" | grep -qF "${want}/main amd64 Packages" || {
    echo "apt does not read the ${want} archive for this package" >&2
    exit 1
  }
  if printf '%s\n' "$policy" | grep -qF "${other}/main amd64 Packages"; then
    echo "apt also reads the ${other} archive; the machine was moved off ${want}" >&2
    exit 1
  fi
}
HELPERS

scenario_stable() {
  cat "$WORK/scenarios/preamble" "$WORK/scenarios/helpers" >"$WORK/scenarios/stable"
  cat >>"$WORK/scenarios/stable" <<STABLE
apt-get install -y -qq "$MOUNT/debs/ok-player_${STABLE_INSTALLED}_amd64.deb" >/dev/null

echo "--- the two files the package provisioned ---"
cat /etc/apt/sources.list.d/ok-player.sources
test -s /usr/share/keyrings/ok-player-archive-keyring.gpg

grep -qx 'Suites: stable' /etc/apt/sources.list.d/ok-player.sources

echo "--- apt-get update && apt-cache policy, with nothing in between ---"
apt-get update -qq
assert_archive_offers stable '1:${STABLE_ARCHIVE}'

echo "--- and it actually delivers ---"
apt-get install -y -qq --only-upgrade ok-player >/dev/null
test "\$(dpkg-query -W -f='\${Version}' ok-player)" = '1:${STABLE_ARCHIVE}'

echo "--- purge leaves nothing behind ---"
apt-get purge -y -qq ok-player >/dev/null
! test -e /etc/apt/sources.list.d/ok-player.sources
! test -e /usr/share/keyrings/ok-player-archive-keyring.gpg
! ls /etc/apt/sources.list.d/ 2>/dev/null | grep -i ok-player
echo OK
STABLE
  run_scenario stable "$WORK/scenarios/stable"
}

scenario_candidate() {
  cat "$WORK/scenarios/preamble" "$WORK/scenarios/helpers" >"$WORK/scenarios/candidate"
  cat >>"$WORK/scenarios/candidate" <<CANDIDATE
apt-get install -y -qq "$MOUNT/debs/ok-player_${CANDIDATE_INSTALLED}_amd64.deb" >/dev/null

echo "--- the suite a candidate package subscribes to ---"
cat /etc/apt/sources.list.d/ok-player.sources
grep -qx 'Suites: candidate' /etc/apt/sources.list.d/ok-player.sources

apt-get update -qq
assert_archive_offers candidate '1:0.11.0~beta.0.210'

apt-get install -y -qq --only-upgrade ok-player >/dev/null
test "\$(dpkg-query -W -f='\${Version}' ok-player)" = '1:0.11.0~beta.0.210'
echo OK
CANDIDATE
  run_scenario candidate "$WORK/scenarios/candidate"
}

scenario_existing_choice() {
  cat "$WORK/scenarios/preamble" "$WORK/scenarios/helpers" >"$WORK/scenarios/existing-choice"
  cat >>"$WORK/scenarios/existing-choice" <<EXISTING
echo "--- a tester installs a candidate .deb and is subscribed to candidate ---"
apt-get install -y -qq "$MOUNT/debs/ok-player_${CANDIDATE_INSTALLED}_amd64.deb" >/dev/null
grep -qx 'Suites: candidate' /etc/apt/sources.list.d/ok-player.sources

echo "--- then installs a stable .deb over it ---"
apt-get install -y -qq "$MOUNT/debs/ok-player_${STABLE_INSTALLED}_amd64.deb" >/dev/null
cat /etc/apt/sources.list.d/ok-player.sources
grep -qx 'Suites: candidate' /etc/apt/sources.list.d/ok-player.sources || {
  echo "the reinstall moved a candidate subscriber back to stable" >&2
  exit 1
}

echo "--- and apt still reads candidate, and only candidate ---"
apt-get update -qq
assert_subscribed_to candidate stable

echo "--- the same for a subscription the user wrote into a file of their own ---"
apt-get purge -y -qq ok-player >/dev/null
install -Dm644 /dev/stdin /etc/apt/sources.list.d/ok-player-candidate.sources <<'STANZA'
Types: deb
URIs: ${BASE_URL}
Suites: candidate
Components: main
Architectures: amd64
Signed-By: /usr/share/keyrings/ok-player-archive-keyring.gpg
STANZA
before="\$(sha256sum /etc/apt/sources.list.d/ok-player-candidate.sources)"
apt-get install -y -qq "$MOUNT/debs/ok-player_${STABLE_INSTALLED}_amd64.deb" >/dev/null
test "\$(sha256sum /etc/apt/sources.list.d/ok-player-candidate.sources)" = "\$before" || {
  echo "the package rewrote the subscription the user had chosen" >&2
  exit 1
}
if [ -e /etc/apt/sources.list.d/ok-player.sources ]; then
  echo "the package added a stable source beside the candidate one the user picked" >&2
  exit 1
fi
apt-get update -qq
assert_subscribed_to candidate stable

echo "--- purging with that subscription in place leaves apt working ---"
apt-get purge -y -qq ok-player >/dev/null
test -f /usr/share/keyrings/ok-player-archive-keyring.gpg || {
  echo "purge removed a keyring the surviving source still names" >&2
  exit 1
}
apt-get update -qq
echo OK
EXISTING
  run_scenario existing-choice "$WORK/scenarios/existing-choice"
}

scenario_no_provisioning() {
  cat "$WORK/scenarios/preamble" "$WORK/scenarios/helpers" >"$WORK/scenarios/no-provisioning"
  cat >>"$WORK/scenarios/no-provisioning" <<NEGATIVE
cp "$MOUNT/debs/ok-player_${STABLE_INSTALLED}_amd64.deb.stripped" /tmp/ok-player.deb
apt-get install -y -qq /tmp/ok-player.deb >/dev/null

echo "--- the state the operator was in ---"
apt-get update -qq
apt-cache policy ok-player

# A subshell: the helper reports a failed assertion by exiting, and here a failed
# assertion is the expected outcome rather than the end of the scenario.
if ( assert_archive_offers stable '1:${STABLE_ARCHIVE}' ) >/dev/null 2>&1; then
  echo "the stripped package still provisioned the repository; the gate proves nothing" >&2
  exit 1
fi
apt-cache policy ok-player | grep -q '/var/lib/dpkg/status' || {
  echo "expected the stripped package to leave dpkg's status file as the only origin" >&2
  exit 1
}
echo OK
NEGATIVE
  # This one is expected to pass: the scenario body itself asserts the failure.
  run_scenario no-provisioning "$WORK/scenarios/no-provisioning"
}

SCENARIOS=("$@")
if [[ ${#SCENARIOS[@]} -eq 0 ]]; then
  SCENARIOS=(stable candidate existing-choice no-provisioning)
fi

for scenario in "${SCENARIOS[@]}"; do
  case "$scenario" in
    stable) scenario_stable && pass 'a stable .deb lands the machine on the stable suite, upgrades, and purges clean' || fail 'stable' 'see the transcript above' ;;
    candidate) scenario_candidate && pass 'a candidate .deb lands the machine on the candidate suite and upgrades' || fail 'candidate' 'see the transcript above' ;;
    existing-choice) scenario_existing_choice && pass 'an existing candidate subscription survives installing a stable package' || fail 'existing-choice' 'see the transcript above' ;;
    no-provisioning) scenario_no_provisioning && pass 'negative control: stripping the provisioning puts the machine back in the operator state' || fail 'no-provisioning' 'the stripped package did not fail the check' ;;
    *) fail "$scenario" 'unknown scenario' ;;
  esac
done

printf '\n'
if [[ $failures -eq 0 ]]; then
  echo "Debian APT provisioning gate: every scenario passed."
else
  printf 'Debian APT provisioning gate: %d scenario(s) failed.\n' "$failures" >&2
fi
exit $((failures > 0))
