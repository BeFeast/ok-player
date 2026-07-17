#!/usr/bin/env bash
# Install, upgrade, remove, and config-preservation smoke for a Fedora RPM.
set -euo pipefail

CURRENT_RPM="${1:?usage: smoke-linux-rpm-install-upgrade.sh <current.rpm> [previous.rpm]}"
PREVIOUS_RPM="${2:-$CURRENT_RPM}"

[[ -f "$CURRENT_RPM" ]] || { echo "Current RPM not found: $CURRENT_RPM" >&2; exit 2; }
[[ -f "$PREVIOUS_RPM" ]] || { echo "Previous RPM not found: $PREVIOUS_RPM" >&2; exit 2; }
[[ "$(id -u)" == "0" ]] || { echo "RPM transaction smoke must run as root in a disposable Fedora root." >&2; exit 2; }
command -v dnf >/dev/null 2>&1 || { echo "dnf is required" >&2; exit 127; }

CONFIG_DIR="$(mktemp -d)"
trap 'rm -rf "$CONFIG_DIR"' EXIT
mkdir -p "$CONFIG_DIR/ok-player"
printf '{"preserve":true}\n' > "$CONFIG_DIR/ok-player/settings.json"

assert_installed() {
  test -x /usr/bin/ok-player
  test -f /usr/share/applications/com.befeast.okplayer.desktop
  test -f /usr/share/metainfo/com.befeast.okplayer.metainfo.xml
  test -f /usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg
  test -f /usr/share/licenses/ok-player/LICENSE
  test -f /usr/share/doc/ok-player/THIRD-PARTY-NOTICES.md
  rpm -q --requires ok-player | grep -q '^mpv-libs'
  ldd /usr/bin/ok-player | grep -q 'libmpv\.so'
}

dnf install -y "$PREVIOUS_RPM"
assert_installed

dnf upgrade -y "$CURRENT_RPM"
assert_installed

dnf remove -y ok-player
test ! -e /usr/bin/ok-player
test -f "$CONFIG_DIR/ok-player/settings.json"
grep -q '"preserve":true' "$CONFIG_DIR/ok-player/settings.json"

echo "RPM install/upgrade/removal and config-preservation smoke passed"
