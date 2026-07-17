#!/usr/bin/env bash
# Acceptance for issue #342 against a built Debian package: prove that
# installing the .deb adds the AppStream MetaInfo, that the exact installed file
# validates, that the app is then discoverable by name and desktop id in a local
# AppStream metadata query, and that removing the package removes the file again.
#
# Runtime dependencies are not resolved here (dpkg --force-depends): this checks
# packaging metadata, not that the GTK app launches. Requires root (dpkg) and
# the `appstream` package.
set -euo pipefail

DEB="${1:?usage: smoke-linux-appstream-deb.sh <path-to.deb>}"
[[ -f "$DEB" ]] || { echo "Debian package not found: $DEB" >&2; exit 1; }

APP_ID="com.befeast.okplayer"
PACKAGE="ok-player"
METAINFO_PATH="/usr/share/metainfo/${APP_ID}.metainfo.xml"

SUDO=""
if [[ "$(id -u)" -ne 0 ]]; then
  SUDO="sudo"
fi

for tool in dpkg appstreamcli; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

fail() { printf 'AppStream deb acceptance failed: %b\n' "$1" >&2; exit 1; }

cleanup() {
  # Best-effort removal so a mid-run failure does not leave the package behind.
  $SUDO dpkg -r "$PACKAGE" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "== Install $DEB =="
$SUDO dpkg -i --force-depends "$DEB"

echo "== MetaInfo present after install =="
[[ -f "$METAINFO_PATH" ]] || fail "install did not place $METAINFO_PATH"

echo "== Installed MetaInfo validates (pedantic) =="
appstreamcli validate --pedantic --no-color "$METAINFO_PATH"

echo "== Discoverable by id and name in a local AppStream query =="
appstreamcli refresh-cache --force >/dev/null 2>&1 || true
got="$(appstreamcli get --no-cache "$APP_ID" 2>&1 || true)"
grep -q "Identifier: $APP_ID" <<<"$got" || fail "appstreamcli get did not find $APP_ID:\n$got"
grep -q "Name: OK Player" <<<"$got" || fail "component name is not 'OK Player':\n$got"
found="$(appstreamcli search --no-cache 'OK Player' 2>&1 || true)"
grep -q "Identifier: $APP_ID" <<<"$found" || fail "name search did not surface $APP_ID"

echo "== Remove package removes MetaInfo =="
$SUDO dpkg -r "$PACKAGE"
[[ ! -f "$METAINFO_PATH" ]] || fail "uninstall left $METAINFO_PATH behind"

trap - EXIT
echo "AppStream deb acceptance passed: install adds, validates, is discoverable, and uninstall removes $METAINFO_PATH"
