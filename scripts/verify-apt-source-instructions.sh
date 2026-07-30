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
probe setup-commands >"$WORK/commands.txt"
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
if python3 "$WORK/readme-check.py" "$WORK/commands.txt" "$ROOT/README.md"; then
  pass 'the commands the app shows are the ones the README publishes'
else
  fail 'README parity' 'the app would hand the user commands the README does not document'
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
chmod -R a+rX "$WORK"

"$RUNTIME" run --rm --mount "type=bind,src=$WORK,dst=/srv,ro" \
  -e DEBIAN_FRONTEND=noninteractive "$IMAGE" \
  sh -c 'dpkg -i /srv/ok-player.deb >/dev/null 2>&1; apt-cache policy ok-player' \
  >"$WORK/no-source.policy"

echo '--- apt-cache policy on a machine with no OK Player source ---'
cat "$WORK/no-source.policy"
probe describe 0.11.0-beta.0.208 0.11.0-beta.0.210 <"$WORK/no-source.policy" \
  >"$WORK/no-source.surface"
echo '--- what the update surface says about it ---'
cat "$WORK/no-source.surface"

if grep -qx 'system_updater_offered: false' "$WORK/no-source.surface" &&
  grep -qx 'repository_setup: present' "$WORK/no-source.surface" &&
  grep -qx 'capability: SystemUnreachable' "$WORK/no-source.surface"; then
  pass 'with no source configured the surface offers the repository and never the updater'
else
  fail 'no-source surface' 'the surface still points at a delivery path this machine does not have'
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
chmod -R a+rX "$WORK"

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

printf '\n'
if [[ $failures -eq 0 ]]; then
  echo 'APT source instructions: every live check passed.'
else
  printf 'APT source instructions: %d check(s) failed.\n' "$failures" >&2
fi
exit $((failures > 0))
