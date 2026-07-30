#!/usr/bin/env bash
# What a standalone .deb carries and what its maintainer scripts do with it (issue #726).
#
# scripts/verify-deb-apt-provisioning.sh is the acceptance: real apt, in a real container,
# answering `apt-cache policy`. This suite is the fast half that runs on every push — it needs
# no container and no network — and it pins the decisions that container cannot see, plus the
# ones a container run would only discover after several minutes of downloading:
#
#   * the package carries the key and the stanza, so install time needs no network at all;
#   * the key it carries is the one the archive is signed with, asserted by fingerprint, and
#     the packaging refuses to build with any other;
#   * the stanza it installs is byte for byte the stanza the archive publishes for that suite;
#   * the suite matches the artifact, and an undeclared build is a candidate;
#   * postinst installs both files, and leaves an existing subscription alone;
#   * purge removes both, and a plain remove does not.
#
# The maintainer scripts are run rather than read: DPKG_ROOT is what dpkg itself gives them
# when it configures into a root, so a temporary directory is a faithful stand-in for the
# decisions they make.

set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

for tool in gpg dpkg-deb; do
  command -v "$tool" >/dev/null 2>&1 ||
    { printf 'Debian APT provisioning tests require %s, which is not on PATH\n' "$tool" >&2; exit 1; }
done

failures=0
pass() { printf 'PASS %s\n' "$1"; }
fail() { printf 'FAIL %s: %s\n' "$1" "$2" >&2; failures=$((failures + 1)); }

# shellcheck source=/dev/null
source "$ROOT/scripts/apt-archive-identity.sh"

# --- The packaging, run for real against a fixture root --------------------------------
# Same substitution as scripts/tests/linux-package-version.Tests.sh: the compiler, the bundled
# mpv runtime and its verifier are stubbed, and everything this suite asserts on — the carried
# files, the key gate, the maintainer scripts — is the production code path.
FIXTURE="$WORK/repo"
STUB_BIN="$WORK/stub-bin"
mkdir -p \
  "$FIXTURE/scripts" \
  "$FIXTURE/rust/packaging/linux/icons/hicolor" \
  "$FIXTURE/rust/target/release" \
  "$FIXTURE/mpv-runtime" \
  "$STUB_BIN"
cp "$ROOT/scripts/package-linux-deb.sh" "$ROOT/scripts/apt-archive-identity.sh" \
  "$ROOT/scripts/linux-package-version.sh" "$FIXTURE/scripts/"
mkdir -p "$FIXTURE/$(dirname "$OKP_APT_PUBLIC_KEY_RELATIVE")"
cp "$ROOT/$OKP_APT_PUBLIC_KEY_RELATIVE" "$FIXTURE/$OKP_APT_PUBLIC_KEY_RELATIVE"
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
  # package_deb <build version> [env assignment ...] — echoes the .deb path, or fails.
  local build="$1"
  shift
  env -u CARGO_TARGET_DIR -u OKP_DEB_APT_SUITE PATH="$STUB_BIN:$PATH" \
    OKP_TEST_MPV_RUNTIME="$FIXTURE/mpv-runtime" "$@" \
    bash "$FIXTURE/scripts/package-linux-deb.sh" "$build" >/dev/null 2>"$WORK/package.err" || return 1
  printf '%s/artifacts/linux/deb/ok-player_%s_amd64.deb' "$FIXTURE" "$build"
}

# --- 1. The package carries the repository, so install time needs no network ------------
STABLE_DEB="$(package_deb 0.11.0 OKP_DEB_APT_SUITE=stable)" || {
  fail 'packaging run' "the stable package could not be built: $(cat "$WORK/package.err")"
  exit 1
}
CANDIDATE_DEB="$(package_deb 0.11.0-beta.0.210 OKP_DEB_APT_SUITE=candidate)" || {
  fail 'packaging run' "the candidate package could not be built: $(cat "$WORK/package.err")"
  exit 1
}
DEFAULT_DEB="$(package_deb 0.11.0-beta.0.211)" || {
  fail 'packaging run' "the undeclared package could not be built: $(cat "$WORK/package.err")"
  exit 1
}

CONTENTS="$(dpkg-deb -c "$STABLE_DEB")"
if grep -q "\\.$OKP_APT_CARRIED_DIR/$OKP_APT_KEYRING_BASENAME\\.gpg\$" <<<"$CONTENTS" &&
  grep -q "\\.$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME\$" <<<"$CONTENTS"; then
  pass 'the package carries the archive key and the source stanza'
else
  fail 'carried files' "the .deb ships neither under $OKP_APT_CARRIED_DIR"
fi

# --- 2. The key it carries is the key the archive is signed with -------------------------
EXTRACT="$WORK/extract"
mkdir -p "$EXTRACT"
dpkg-deb -x "$STABLE_DEB" "$EXTRACT"
SHIPPED_FINGERPRINT="$(
  GNUPGHOME="$WORK" okp_apt_key_fingerprint \
    "$EXTRACT$OKP_APT_CARRIED_DIR/$OKP_APT_KEYRING_BASENAME.gpg"
)"
if [[ "$SHIPPED_FINGERPRINT" == "$OKP_APT_SIGNING_FINGERPRINT" ]]; then
  pass "the shipped keyring is $OKP_APT_SIGNING_FINGERPRINT"
else
  fail 'shipped key' "the package ships ${SHIPPED_FINGERPRINT:-no OpenPGP key}"
fi

# The build-time gate, exercised rather than read: a key that is not the archive's must abort
# the packaging outright. A package that shipped the wrong key would install a keyring that
# cannot verify the archive its own stanza points at — a hard `apt-get update` failure on the
# user's machine, worse than the silence #726 is fixing.
OTHER_HOME="$WORK/other-key"
mkdir -p "$OTHER_HOME"
chmod 700 "$OTHER_HOME"
GNUPGHOME="$OTHER_HOME" gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
  --quick-generate-key 'Somebody Else <other@example.invalid>' ed25519 sign never
GNUPGHOME="$OTHER_HOME" gpg --batch --quiet --armor --export \
  --output "$WORK/other-public.asc"
if package_deb 0.11.0-beta.0.212 OKP_DEB_APT_PUBLIC_KEY="$WORK/other-public.asc" >/dev/null; then
  fail 'key fingerprint gate' 'the packaging shipped a key the archive is not signed with'
else
  pass 'the packaging refuses to ship a key the archive is not signed with'
fi

# --- 3. The stanza it installs is the stanza the archive publishes -----------------------
# Not a comparison of two hand-written strings: the archive generator is sourced and asked to
# write the same suite's stanza at the published base URL, and the two files are compared.
# shellcheck source=/dev/null
source "$ROOT/scripts/build-apt-repo.sh"
expect_stanza_matches() {
  # expect_stanza_matches <deb> <suite>
  local deb="$1" suite="$2"
  local extracted="$WORK/stanza-$suite"
  rm -rf -- "$extracted"
  mkdir -p "$extracted"
  dpkg-deb -x "$deb" "$extracted"
  okp_apt_write_sources_stanza "$WORK/archive-$suite.sources" \
    "$OKP_APT_BASE_URL_DEFAULT" "$suite"
  if cmp -s "$extracted$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME" \
    "$WORK/archive-$suite.sources"; then
    pass "the $suite package installs the stanza the archive publishes for $suite"
  else
    fail "$suite stanza" \
      "$(diff -u "$WORK/archive-$suite.sources" \
        "$extracted$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME" || true)"
  fi
}
expect_stanza_matches "$STABLE_DEB" "$OKP_APT_STABLE_SUITE"
expect_stanza_matches "$CANDIDATE_DEB" "$OKP_APT_CANDIDATE_SUITE"

# An undeclared build is a rolling build. Defaulting the other way would silently move a
# tester who installed a candidate .deb onto stable, which is the failure #726 names.
DEFAULT_EXTRACT="$WORK/default-extract"
mkdir -p "$DEFAULT_EXTRACT"
dpkg-deb -x "$DEFAULT_DEB" "$DEFAULT_EXTRACT"
if grep -qx "Suites: $OKP_APT_CANDIDATE_SUITE" \
  "$DEFAULT_EXTRACT$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME"; then
  pass 'a build that does not declare itself a release provisions the candidate suite'
else
  fail 'default suite' 'an undeclared build did not provision the candidate suite'
fi

# ...which only holds because the release lane declares itself. A release that stopped saying
# so would publish `linux-v*` packages subscribing every user to the QA channel.
if grep -q 'OKP_DEB_APT_SUITE: stable' "$ROOT/.github/workflows/release-linux.yml"; then
  pass 'the versioned release lane declares the stable suite'
else
  fail 'release lane' \
    'release-linux.yml does not set OKP_DEB_APT_SUITE: stable, so its .deb would provision candidate'
fi

# --- 4. What the maintainer scripts do ----------------------------------------------------
# dpkg hands postinst/postrm DPKG_ROOT when configuring into a root, so running them against a
# temporary one asks them the same question dpkg does.
maintainer_root() {
  # maintainer_root <deb> — a root with the package's files unpacked, ready for postinst.
  local deb="$1" root
  root="$(mktemp -d "$WORK/root.XXXXXX")"
  dpkg-deb -x "$deb" "$root"
  printf '%s' "$root"
}
run_script() {
  # run_script <deb> <root> <postinst|postrm> [args...]
  local deb="$1" root="$2" script="$3"
  shift 3
  local control="$WORK/control-$script.$$"
  rm -rf -- "$control"
  dpkg-deb --control "$deb" "$control"
  DPKG_ROOT="$root" sh "$control/$script" "$@"
}

FRESH="$(maintainer_root "$STABLE_DEB")"
run_script "$STABLE_DEB" "$FRESH" postinst configure
if [[ -f "$FRESH$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]] &&
  [[ -f "$FRESH$OKP_APT_KEYRING_DIR/$OKP_APT_KEYRING_BASENAME.gpg" ]] &&
  grep -qx "Suites: $OKP_APT_STABLE_SUITE" "$FRESH$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME"; then
  pass 'postinst configures the repository on a machine that has no OK Player source'
else
  fail 'postinst' 'the keyring and the stanza were not both installed'
fi

# A plain remove leaves the repository configured — that is how every apt-repository-shipping
# package behaves, and it is what lets a reinstall not have to re-add the source.
run_script "$STABLE_DEB" "$FRESH" postrm remove
if [[ -f "$FRESH$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]] &&
  [[ -f "$FRESH$OKP_APT_KEYRING_DIR/$OKP_APT_KEYRING_BASENAME.gpg" ]]; then
  pass 'a plain remove leaves the repository configured'
else
  fail 'postrm remove' 'remove took the repository with it'
fi

run_script "$STABLE_DEB" "$FRESH" postrm purge
if [[ ! -e "$FRESH$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]] &&
  [[ ! -e "$FRESH$OKP_APT_KEYRING_DIR/$OKP_APT_KEYRING_BASENAME.gpg" ]]; then
  pass 'purge removes both files'
else
  fail 'postrm purge' 'purge left OK Player files under /etc/apt or /usr/share/keyrings'
fi

# An existing subscription is the user's. This is the case dpkg's conffile machinery cannot
# express: it compares against the md5 of what the *previous package* shipped, so an untouched
# stanza is silently replaced — which would move a candidate subscriber back to stable on the
# next stable install, the one thing #726 forbids.
SUBSCRIBED="$(maintainer_root "$STABLE_DEB")"
mkdir -p "$SUBSCRIBED$OKP_APT_SOURCES_DIR"
okp_apt_write_sources_stanza \
  "$SUBSCRIBED$OKP_APT_SOURCES_DIR/ok-player-candidate.sources" \
  "$OKP_APT_BASE_URL_DEFAULT" "$OKP_APT_CANDIDATE_SUITE"
BEFORE="$(sha256sum "$SUBSCRIBED$OKP_APT_SOURCES_DIR/ok-player-candidate.sources" | cut -d' ' -f1)"
run_script "$STABLE_DEB" "$SUBSCRIBED" postinst configure
AFTER="$(sha256sum "$SUBSCRIBED$OKP_APT_SOURCES_DIR/ok-player-candidate.sources" | cut -d' ' -f1)"
if [[ "$BEFORE" == "$AFTER" ]] &&
  [[ ! -e "$SUBSCRIBED$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]]; then
  pass 'a stable package leaves an existing candidate subscription alone'
else
  fail 'existing subscription' \
    'installing a stable package moved a candidate subscriber, or added a stable source beside theirs'
fi

# The same holds for a source the user wrote by hand into the one-line format, and for one
# they commented out — a commented line is not a subscription, so the package must still
# configure the repository.
COMMENTED="$(maintainer_root "$STABLE_DEB")"
mkdir -p "$COMMENTED$OKP_APT_SOURCES_DIR"
printf '# deb [signed-by=%s/%s.gpg] %s stable main\n' \
  "$OKP_APT_KEYRING_DIR" "$OKP_APT_KEYRING_BASENAME" "$OKP_APT_BASE_URL_DEFAULT" \
  >"$COMMENTED$OKP_APT_SOURCES_DIR/ok-player.list"
run_script "$STABLE_DEB" "$COMMENTED" postinst configure
if [[ -f "$COMMENTED$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]]; then
  pass 'a commented-out source is not a subscription'
else
  fail 'commented source' 'the package treated a commented-out line as a configured source'
fi

LEGACY="$(maintainer_root "$CANDIDATE_DEB")"
mkdir -p "$LEGACY$OKP_APT_SOURCES_DIR"
printf 'deb [signed-by=%s/%s.gpg] %s stable main\n' \
  "$OKP_APT_KEYRING_DIR" "$OKP_APT_KEYRING_BASENAME" "$OKP_APT_BASE_URL_DEFAULT" \
  >"$LEGACY$OKP_APT_SOURCES_DIR/ok-player.list"
run_script "$CANDIDATE_DEB" "$LEGACY" postinst configure
if [[ ! -e "$LEGACY$OKP_APT_SOURCES_DIR/$OKP_APT_SOURCES_BASENAME" ]]; then
  pass 'a hand-written one-line source counts as the subscription the user chose'
else
  fail 'one-line source' 'the package added a second source beside a hand-written one'
fi

printf '\n'
if [[ $failures -eq 0 ]]; then
  echo 'Debian APT provisioning tests: all assertions passed.'
else
  printf 'Debian APT provisioning tests: %d assertion(s) failed.\n' "$failures" >&2
fi
exit $((failures > 0))
