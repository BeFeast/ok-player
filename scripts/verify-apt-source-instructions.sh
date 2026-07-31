#!/usr/bin/env bash
# The two live halves of #725, measured in clean containers rather than described.
#
# The defect was that the update surface named a version apt could not deliver and offered
# "Open software updater", which then correctly answered that the machine was up to date. The
# app's claim and the tool it sent the user to disagreed, and pressing every button in the
# product changed nothing. So both halves are checked against what a real machine reports:
#
#   1. **A machine with no OK Player source.** The operator's state, reproduced: a package
#      installed from a file, so `apt-cache policy ok-player` knows it only through dpkg's own
#      status. The surface must offer the repository instructions and must not offer the
#      system updater.
#   2. **The instructions the app shows.** They are the README's, verbatim, and following them
#      in a clean container end to end must leave a machine that apt can carry — the archive as
#      a source, and `ok-player` installed from it.
#   3. **The upgrade command the app shows (#759).** A subscribed machine is handed one command
#      and told it upgrades OK Player and nothing else. That is a claim about a transaction, so
#      it is measured as one: the command is run verbatim on a machine that has an unrelated
#      package waiting to be upgraded, and only `ok-player` may move. The offer this replaced
#      was "Open software updater", which started a transaction over everything the machine
#      could upgrade — on the reporting machine, `tzdata` and a debconf prompt that never
#      returned.
#
# The lifecycle is asked through `cargo run --example update-surface-probe`, which is the same
# chain the GTK shell uses: run `apt-cache policy`, hand the output to okp_core::apt_policy,
# let the lifecycle decide. Only the command runs in the container; the interpretation runs
# here, exactly as it does in the shell.
#
# The second half reaches the network on purpose. "The instructions work" cannot be established
# against a fixture: it is a claim about the published archive, its signature and its suite, and
# the whole point of #725 is that nobody had checked it end to end from the text the user is
# given.
#
# Requires: docker or podman, cargo, dpkg-deb, python3. A missing container runtime is a hard
# failure (127), not a skip.

set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${OKP_APT_INSTRUCTIONS_IMAGE:-debian:13-slim}"

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT

for tool in cargo dpkg-deb python3; do
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

probe() {
  cargo run --quiet --manifest-path "$ROOT/rust/Cargo.toml" -p okp-core \
    --example update-surface-probe -- "$@"
}

# --- The commands the app shows are the README's -----------------------------------------
# Per suite, because the app no longer has one block: the commands name a channel, and handing
# a candidate tester the stable block would move them off the channel they installed for
# (#726). Both have to be the README's.
# --- The packaging, run for real against a fixture root ----------------------------------
# The same substitution scripts/tests/deb-apt-provisioning.Tests.sh uses: the compiler, the
# bundled mpv runtime and its verifier are stubbed, so what runs is the real control-file
# emission, the real maintainer scripts and the real dpkg-deb. What is under test here is what
# postinst leaves on the machine, not what the player does once it starts.
PKG_FIXTURE="$WORK/packaging"
STUB_BIN="$WORK/stub-bin"
build_packaging_fixture() {
  mkdir -p \
    "$PKG_FIXTURE/scripts" \
    "$PKG_FIXTURE/rust/packaging/linux/icons/hicolor" \
    "$PKG_FIXTURE/rust/target/release" \
    "$PKG_FIXTURE/mpv-runtime" \
    "$STUB_BIN"
  cp "$ROOT/scripts/package-linux-deb.sh" "$ROOT/scripts/apt-archive-identity.sh" \
    "$ROOT/scripts/linux-package-version.sh" "$ROOT/scripts/stage-license-documents.sh" \
    "$PKG_FIXTURE/scripts/"
  cp "$ROOT/LICENSE" "$ROOT/LICENSE.LGPL-3.0" "$ROOT/THIRD-PARTY-NOTICES.md" "$PKG_FIXTURE/"
  cp "$ROOT/rust/packaging/linux/copyright" "$PKG_FIXTURE/rust/packaging/linux/copyright"
  cp "$ROOT/rust/packaging/linux/ok-player-archive-keyring.asc" \
    "$PKG_FIXTURE/rust/packaging/linux/"
  cat >"$PKG_FIXTURE/scripts/linux-bundled-mpv-env.sh" <<'STUB'
okp_use_linux_bundled_mpv() { export OKP_BUNDLED_MPV_RUNTIME_DIR="$OKP_TEST_MPV_RUNTIME"; }
STUB
  printf '#!/usr/bin/env bash\nexit 0\n' >"$PKG_FIXTURE/scripts/verify-linux-bundled-mpv.sh"
  printf '#!/bin/sh\nexit 0\n' >"$STUB_BIN/cargo"
  chmod 755 "$PKG_FIXTURE/scripts/verify-linux-bundled-mpv.sh" "$STUB_BIN/cargo"
  printf '[Desktop Entry]\nName=OK Player\n' \
    >"$PKG_FIXTURE/rust/packaging/linux/com.befeast.okplayer.desktop"
  printf '<component type="desktop-application"/>\n' \
    >"$PKG_FIXTURE/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"
  printf '<svg/>\n' >"$PKG_FIXTURE/rust/packaging/linux/com.befeast.okplayer.svg"
  local size
  for size in 16 24 32 48 64; do
    mkdir -p "$PKG_FIXTURE/rust/packaging/linux/icons/hicolor/${size}x${size}/apps"
    printf '<svg/>\n' \
      >"$PKG_FIXTURE/rust/packaging/linux/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
  done
  printf '#!/bin/sh\nexit 0\n' >"$PKG_FIXTURE/rust/target/release/okp-linux-gtk"
  chmod 755 "$PKG_FIXTURE/rust/target/release/okp-linux-gtk"
  printf 'bundled runtime\n' >"$PKG_FIXTURE/mpv-runtime/libmpv.so.2"
}

probe setup-commands stable >"$WORK/commands.txt"
probe setup-commands candidate >"$WORK/commands-candidate.txt"
cat >"$WORK/readme-check.py" <<'PYTHON'
import pathlib
import sys

commands = pathlib.Path(sys.argv[1]).read_text()
readme = pathlib.Path(sys.argv[2]).read_text()
if commands.strip() and commands in readme:
    sys.exit(0)
print("the app's setup commands do not appear verbatim in the README:", file=sys.stderr)
print(commands, file=sys.stderr)
sys.exit(1)
PYTHON
if python3 "$WORK/readme-check.py" "$WORK/commands.txt" "$ROOT/README.md" &&
  python3 "$WORK/readme-check.py" "$WORK/commands-candidate.txt" "$ROOT/README.md"; then
  pass 'the commands the app shows, for both channels, are the ones the README publishes'
else
  fail 'README parity' 'the app would hand the user commands the README does not document'
fi

if grep -q 'ok-player-candidate.sources' "$WORK/commands-candidate.txt" &&
  ! grep -q 'ok-player-candidate.sources' "$WORK/commands.txt"; then
  pass 'each channel gets its own stanza, so the instructions cannot move a subscriber'
else
  fail 'channel separation' 'the two channels produced the same setup commands'
fi

# --- 1. A machine with no OK Player source ------------------------------------------------
# The operator's machine, reproduced: OK Player installed from a downloaded file and no source
# anywhere. A synthetic package is enough — what is being measured is what apt says about a
# package it knows only through dpkg, which is a property of the machine, not of the payload.
FAKE="$WORK/fake/ok-player"
mkdir -p "$FAKE/DEBIAN" "$FAKE/usr/bin"
printf '#!/bin/sh\nexit 0\n' >"$FAKE/usr/bin/ok-player"
chmod 755 "$FAKE/usr/bin/ok-player"
{
  printf 'Package: ok-player\n'
  printf 'Version: 1:0.11.0~beta.0.208\n'
  printf 'Architecture: amd64\n'
  printf 'Maintainer: OK Player <noreply@example.invalid>\n'
  printf 'Description: stand-in for a package installed from a downloaded file\n'
} >"$FAKE/DEBIAN/control"
dpkg-deb --build --root-owner-group "$FAKE" "$WORK/ok-player.deb" >/dev/null
# apt drops privileges to `_apt` to fetch, and mktemp -d hands back a 0700 directory.
# Readings the containers wrote back are owned by root and are already world-readable;
# they are not what apt reads, so failing to widen them is not a failure.
chmod -R a+rX "$WORK" 2>/dev/null || true

"$RUNTIME" run --rm --mount "type=bind,src=$WORK,dst=/srv,ro" \
  -e DEBIAN_FRONTEND=noninteractive "$IMAGE" \
  sh -c 'dpkg -i /srv/ok-player.deb >/dev/null 2>&1; apt-cache policy ok-player' \
  >"$WORK/no-source.policy"

echo '--- apt-cache policy on a machine with no OK Player source ---'
cat "$WORK/no-source.policy"
# A build that records the channel it came from is told how to subscribe to *that* channel.
probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 --packaged-suite candidate \
  <"$WORK/no-source.policy" >"$WORK/no-source.surface"
echo '--- what the update surface says about it ---'
cat "$WORK/no-source.surface"

if grep -qx 'upgrade_command: absent' "$WORK/no-source.surface" &&
  grep -qx 'repository_setup: present (candidate)' "$WORK/no-source.surface" &&
  grep -qx 'capability: SystemUnreachable' "$WORK/no-source.surface"; then
  pass 'with no source configured the surface offers the repository and no upgrade command'
else
  fail 'no-source surface' 'the surface still points at a delivery path this machine does not have'
fi

# ...and a build that does not — anything published before #726 carries no stanza — is told
# that, rather than handed a guess. A guessed suite here is a silent channel change.
probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 \
  <"$WORK/no-source.policy" >"$WORK/no-channel.surface"
echo '--- and the same machine, for a build that does not record its channel ---'
cat "$WORK/no-channel.surface"
if grep -qx 'repository_setup: absent' "$WORK/no-channel.surface" &&
  grep -q 'cannot tell which channel' "$WORK/no-channel.surface"; then
  pass 'a build with no recorded channel is told so rather than handed a guess'
else
  fail 'unknown channel' 'the surface guessed a channel it could not establish'
fi

# --- 1b. The state a .deb install is in on first launch (#725 x #726) ---------------------
# The package writes /etc/apt/sources.list.d/ok-player.sources in its postinst and stops there;
# running `apt update` is not a maintainer script's business. So the very first launch after
# installing a .deb has the stanza on disk and apt still knowing the package only through
# dpkg. Reading that as "no repository is configured" made the app deny a file its own
# packaging had just written, and offer setup commands for a channel that might not be the
# user's — which for a candidate tester means being moved to stable by the app's own advice.
FIRST_LAUNCH="$WORK/first-launch"
mkdir -p "$FIRST_LAUNCH"
cat >"$WORK/provisioned.sh" <<'PROVISIONED'
set -eu
# Both of these happen before the package adds its source, so what follows is genuinely "the
# source exists and apt has never read it" rather than "apt could not reach it". debian:13-slim
# ships no CA bundle, which any real Debian desktop has.
apt-get update -qq
apt-get install -y -qq --no-install-recommends ca-certificates >/dev/null

dpkg -i --force-depends /srv/ok-player.deb >/dev/null 2>&1 || true
dpkg --configure --force-depends ok-player >/dev/null 2>&1 || true
mkdir -p /srv-out
cp /etc/apt/sources.list.d/ok-player.sources /srv-out/ok-player.sources
cp /usr/share/ok-player/apt/ok-player.sources /srv-out/carried.sources

# First launch: the stanza is on disk and nothing has run `apt update` since it appeared.
apt-cache policy ok-player >/srv-out/policy.before
apt-get update -qq
apt-cache policy ok-player >/srv-out/policy.after
PROVISIONED
# apt drops privileges to `_apt` to fetch, and mktemp -d hands back a 0700 directory.
# Readings the containers wrote back are owned by root and are already world-readable;
# they are not what apt reads, so failing to widen them is not a failure.
chmod -R a+rX "$WORK" 2>/dev/null || true

# The package under test is built by the packaging that ships it, pointed at the archive built
# above so the whole thing stays offline and deterministic.
package_provisioning_deb() {
  env -u CARGO_TARGET_DIR PATH="$STUB_BIN:$PATH" \
    OKP_TEST_MPV_RUNTIME="$PKG_FIXTURE/mpv-runtime" \
    OKP_DEB_APT_SUITE=candidate \
    bash "$PKG_FIXTURE/scripts/package-linux-deb.sh" "$1" >"$WORK/package.log" 2>&1
}

if [[ -n "${OKP_SKIP_COMPOSED_CASE:-}" ]]; then
  printf 'skipping the composed case on request\n'
else
  build_packaging_fixture
  if package_provisioning_deb 0.11.0-beta.0.208; then
    cp "$PKG_FIXTURE/artifacts/linux/deb/ok-player_0.11.0-beta.0.208_amd64.deb" \
      "$WORK/ok-player.deb"
    # apt drops privileges to `_apt` to fetch, and mktemp -d hands back a 0700 directory.
# Readings the containers wrote back are owned by root and are already world-readable;
# they are not what apt reads, so failing to widen them is not a failure.
chmod -R a+rX "$WORK" 2>/dev/null || true
    "$RUNTIME" run --rm \
      --mount "type=bind,src=$WORK,dst=/srv,ro" \
      --mount "type=bind,src=$FIRST_LAUNCH,dst=/srv-out" \
      -e DEBIAN_FRONTEND=noninteractive "$IMAGE" \
      bash /srv/provisioned.sh >"$WORK/provisioned.log" 2>&1 ||
      fail 'provisioned install' "$(cat "$WORK/provisioned.log")"

    echo '--- what the package provisioned, and what apt knew before any update ---'
    cat "$FIRST_LAUNCH/ok-player.sources"
    cat "$FIRST_LAUNCH/policy.before"

    CARRIED_SUITE="$(awk -F': *' '/^[Ss]uites:/ { print $2; exit }' "$FIRST_LAUNCH/carried.sources")"
    probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 \
      --packaged-suite "$CARRIED_SUITE" \
      --sources "$FIRST_LAUNCH/ok-player.sources" \
      <"$FIRST_LAUNCH/policy.before" >"$FIRST_LAUNCH/surface.before"
    echo '--- what the update surface says on that first launch ---'
    cat "$FIRST_LAUNCH/surface.before"

    if grep -qx 'repository_setup: absent' "$FIRST_LAUNCH/surface.before" &&
      grep -qx 'refresh_command: sudo apt update' "$FIRST_LAUNCH/surface.before" &&
      grep -qx 'action: Some(CopyRefreshCommand)' "$FIRST_LAUNCH/surface.before" &&
      ! grep -q 'No OK Player repository' "$FIRST_LAUNCH/surface.before"; then
      pass 'first launch after a provisioned install offers a refresh, not repository setup'
    else
      fail 'first launch' \
        'the app denied the repository its own postinst wrote, or offered to add another one'
    fi

    # ...and once apt has read it, the machine moves to the configured-and-readable state.
    echo '--- and after apt-get update ---'
    cat "$FIRST_LAUNCH/policy.after"
    probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 \
      --packaged-suite "$CARRIED_SUITE" \
      --sources "$FIRST_LAUNCH/ok-player.sources" \
      <"$FIRST_LAUNCH/policy.after" >"$FIRST_LAUNCH/surface.after"
    cat "$FIRST_LAUNCH/surface.after"
    if grep -qx 'refresh_command: absent' "$FIRST_LAUNCH/surface.after" &&
      grep -qx 'repository_setup: absent' "$FIRST_LAUNCH/surface.after" &&
      grep -qx 'capability: SystemManaged' "$FIRST_LAUNCH/surface.after"; then
      pass 'reading the lists moves the machine to the configured-and-readable state'
    else
      fail 'after refresh' 'the surface did not settle once apt had read the source'
    fi

    # Negative control: the reading this change replaced. A policy with no origins was taken
    # to mean no source, whatever the configuration said.
    probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 \
      --packaged-suite "$CARRIED_SUITE" \
      <"$FIRST_LAUNCH/policy.before" >"$FIRST_LAUNCH/surface.control"
    echo '--- negative control: the same machine, read without its configuration ---'
    cat "$FIRST_LAUNCH/surface.control"
    if grep -q 'No OK Player repository is configured' "$FIRST_LAUNCH/surface.control" &&
      grep -q '^repository_setup: present' "$FIRST_LAUNCH/surface.control"; then
      pass 'negative control: ignoring the configuration reproduces the false denial'
    else
      fail 'negative control' \
        'the old reading no longer produces the defect, so this check proves nothing'
    fi
  else
    fail 'provisioned install' "the package could not be built: $(cat "$WORK/package.log")"
  fi
fi

# --- 2. Following the instructions, end to end --------------------------------------------
# curl and sudo are prerequisites of the instructions rather than part of them, so they are
# installed first and the block below is then run exactly as the README prints it.
cat >"$WORK/follow.sh" <<'FOLLOW'
set -eu
apt-get update -qq
apt-get install -y -qq --no-install-recommends ca-certificates curl sudo >/dev/null
sh /srv/commands.txt
echo '--- apt-cache policy after following the instructions ---'
apt-cache policy ok-player
FOLLOW
# The block ends with `sudo apt install ok-player`, which needs a terminal-free apt.
sed -i 's/^sudo apt install ok-player$/sudo apt-get install -y -qq ok-player/' "$WORK/commands.txt"
grep -q 'apt-get install -y -qq ok-player' "$WORK/commands.txt" || {
  echo "The instructions no longer end in 'sudo apt install ok-player'; this script has to be" >&2
  echo "taught how to run the new last step without a terminal before it can follow them." >&2
  exit 1
}
# apt drops privileges to `_apt` to fetch, and mktemp -d hands back a 0700 directory.
# Readings the containers wrote back are owned by root and are already world-readable;
# they are not what apt reads, so failing to widen them is not a failure.
chmod -R a+rX "$WORK" 2>/dev/null || true

if "$RUNTIME" run --rm --mount "type=bind,src=$WORK,dst=/srv,ro" \
  -e DEBIAN_FRONTEND=noninteractive "$IMAGE" \
  sh -c 'sh /srv/follow.sh; dpkg-query -W -f="installed: \${Version}\n" ok-player' \
  >"$WORK/followed.policy" 2>&1; then
  echo '--- following the README block in a clean container ---'
  cat "$WORK/followed.policy"
  if grep -q 'befeast.github.io/ok-player/apt stable/main' "$WORK/followed.policy" &&
    grep -q '^installed: ' "$WORK/followed.policy"; then
    pass 'following the instructions leaves a machine apt can carry OK Player to'
  else
    fail 'instructions' 'the archive did not become a source, or the package did not install'
  fi
else
  cat "$WORK/followed.policy" >&2
  fail 'instructions' 'the README block did not run to completion in a clean container'
fi

# --- 3. The same machine, subscribed to `stable`, when a candidate is published -------------
# #725's second defect, live. The check reads the rolling candidate feed, but `stable`
# deliberately never carries a candidate build (#689), so a correctly subscribed user could
# still be told about a version apt would refuse. The container above is on `stable`; the
# version below is a rolling build that suite does not carry.
sed -n '/^ok-player:/,$p' "$WORK/followed.policy" >"$WORK/subscribed.policy"
INSTALLED_VERSION="$(awk '/^  Installed:/ { print $2; exit }' "$WORK/subscribed.policy")"
probe describe "${INSTALLED_VERSION:?apt reported no installed version}" 0.11.0-beta.0.210 \
  <"$WORK/subscribed.policy" >"$WORK/subscribed.surface"
echo '--- what the update surface says to a stable subscriber about a candidate build ---'
cat "$WORK/subscribed.surface"
if grep -qx 'repository_setup: absent' "$WORK/subscribed.surface" &&
  grep -qx 'capability: SystemManaged' "$WORK/subscribed.surface"; then
  pass 'a subscribed machine is no longer told to add a repository it already has'
else
  fail 'subscribed surface' 'the surface still treats a configured machine as having no source'
fi

if grep -q '^message: .*stable' "$WORK/subscribed.surface" &&
  ! grep -q '^message: .*0\.11\.0-beta\.0\.210 is available' "$WORK/subscribed.surface"; then
  pass 'a stable subscriber is told which channel it is current on, not about a build it cannot get'
else
  fail 'suite honesty' 'the surface announced a build this machine cannot install'
fi

# --- 4. The upgrade command the app prints, run verbatim (#759) ----------------------------
# The property is "this command upgrades OK Player and nothing else", so the machine it runs on
# must have something else to upgrade. A two-package fixture archive supplies that: `ok-player`
# with a newer version waiting, and `okp-unrelated` with one waiting too. `okp-unrelated` stands
# in for the `tzdata` on the reporting machine — something apt could upgrade that is none of
# this app's business. A whole-machine action takes it; a package-scoped one leaves it alone,
# and the difference is visible in dpkg's own log.
#
# The machine is built by one script run twice, because the app's verdict is computed on this
# host and the command has to be executed on that machine: the first run reports what apt says,
# the second rebuilds the identical machine and runs what the app decided to print. The setup is
# byte-identical and offline after `apt-get update`, so the two machines are the same machine.
cat >"$WORK/pending-upgrade.sh" <<'PENDING'
set -eu
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends apt-utils sudo >/dev/null

build_fixture_package() {
  root="/tmp/build/$1-$2"
  mkdir -p "$root/DEBIAN" "$root/usr/share/$1"
  printf '%s\n' "$2" >"$root/usr/share/$1/version"
  {
    printf 'Package: %s\n' "$1"
    printf 'Version: %s\n' "$2"
    printf 'Architecture: all\n'
    printf 'Maintainer: OK Player <noreply@example.invalid>\n'
    printf 'Description: fixture for the package-scoped upgrade check\n'
  } >"$root/DEBIAN/control"
  dpkg-deb --build --root-owner-group "$root" /opt/repo/stable >/dev/null
}

mkdir -p /opt/repo/stable
build_fixture_package ok-player 1.0.0
build_fixture_package ok-player 2.0.0
build_fixture_package okp-unrelated 1.0.0
build_fixture_package okp-unrelated 2.0.0
( cd /opt/repo && apt-ftparchive packages stable >stable/Packages )
echo 'deb [trusted=yes] file:/opt/repo stable/' >/etc/apt/sources.list.d/okp-fixture.list
apt-get update -qq
apt-get install -y -qq ok-player=1.0.0 okp-unrelated=1.0.0 >/dev/null
PENDING

cat >"$WORK/pending-report.sh" <<'REPORT'
set -eu
. /srv/pending-upgrade.sh
mkdir -p /srv-out
apt-cache policy ok-player >/srv-out/policy
apt list --upgradable 2>/dev/null >/srv-out/upgradable
REPORT

PENDING_OUT="$WORK/pending"
mkdir -p "$PENDING_OUT"
chmod -R a+rX "$WORK" 2>/dev/null || true

if "$RUNTIME" run --rm \
  --mount "type=bind,src=$WORK,dst=/srv,ro" \
  --mount "type=bind,src=$PENDING_OUT,dst=/srv-out" \
  -e DEBIAN_FRONTEND=noninteractive "$IMAGE" \
  bash /srv/pending-report.sh >"$WORK/pending-setup.log" 2>&1; then

  echo '--- a subscribed machine with an unrelated upgrade also pending ---'
  cat "$PENDING_OUT/upgradable"
  cat "$PENDING_OUT/policy"

  probe describe 1.0.0 2.0.0 --packaged-suite stable \
    <"$PENDING_OUT/policy" >"$PENDING_OUT/surface"
  echo '--- what the update surface offers it ---'
  cat "$PENDING_OUT/surface"

  UPGRADE_COMMAND="$(sed -n 's/^upgrade_command: //p' "$PENDING_OUT/surface")"
  if grep -qx 'capability: SystemManaged' "$PENDING_OUT/surface" &&
    grep -qx 'action: Some(CopyUpgradeCommand)' "$PENDING_OUT/surface" &&
    [[ -n "$UPGRADE_COMMAND" && "$UPGRADE_COMMAND" != absent ]]; then
    pass 'a subscribed machine whose source carries the build is given a command to run'
  else
    fail 'upgrade offer' 'the surface named a version and offered no way to install it'
  fi

  # The machine must really have had something else to upgrade, or the check below proves
  # nothing: a command cannot be shown to leave other packages alone on a machine that had none.
  if grep -q '^okp-unrelated/' "$PENDING_OUT/upgradable"; then
    pass 'the machine under test has an unrelated upgrade pending, as the reporting one did'
  else
    fail 'pending upgrade' 'nothing unrelated was waiting, so the command had nothing to spare'
  fi

  printf '%s\n' "$UPGRADE_COMMAND" >"$WORK/upgrade-command.txt"
  cat >"$WORK/pending-run.sh" <<'RUN'
set -eu
. /srv/pending-upgrade.sh
mkdir -p /srv-out
echo '--- what the app told the user to run ---'
cat /srv/upgrade-command.txt
: >/var/log/dpkg.log
# Verbatim, as printed. `yes` only answers a prompt the app cannot; it changes no word of it.
yes | sh /srv/upgrade-command.txt
dpkg-query -W -f='${Package} ${Version}\n' ok-player okp-unrelated >/srv-out/versions
awk '$3 == "upgrade" || $3 == "install" || $3 == "remove" || $3 == "purge" { sub(/:.*/, "", $4); print $4 }' \
  /var/log/dpkg.log | sort -u >/srv-out/touched
RUN
  chmod -R a+rX "$WORK" 2>/dev/null || true

  if "$RUNTIME" run --rm \
    --mount "type=bind,src=$WORK,dst=/srv,ro" \
    --mount "type=bind,src=$PENDING_OUT,dst=/srv-out" \
    "$IMAGE" bash /srv/pending-run.sh >"$WORK/pending-run.log" 2>&1; then
    cat "$WORK/pending-run.log"
    echo '--- versions after running it ---'
    cat "$PENDING_OUT/versions"
    echo '--- every package dpkg touched while it ran ---'
    cat "$PENDING_OUT/touched"

    if grep -qx 'ok-player 2.0.0' "$PENDING_OUT/versions"; then
      pass 'running the command the app printed upgrades OK Player'
    else
      fail 'upgrade command' 'the command the app prints does not upgrade OK Player'
    fi

    if grep -qx 'okp-unrelated 1.0.0' "$PENDING_OUT/versions" &&
      [[ "$(cat "$PENDING_OUT/touched")" == 'ok-player' ]]; then
      pass 'and touches nothing else: the unrelated upgrade is still pending afterwards'
    else
      fail 'package scope' \
        "the command reached past OK Player: dpkg touched $(tr '\n' ' ' <"$PENDING_OUT/touched")"
    fi
  else
    cat "$WORK/pending-run.log" >&2
    fail 'upgrade command' 'the command the app prints did not run to completion'
  fi
else
  cat "$WORK/pending-setup.log" >&2
  fail 'pending upgrade fixture' 'the machine with a pending unrelated upgrade could not be built'
fi

printf '\n'
if [[ $failures -eq 0 ]]; then
  echo 'APT source instructions: every live check passed.'
else
  printf 'APT source instructions: %d check(s) failed.\n' "$failures" >&2
fi
exit $((failures > 0))
