#!/usr/bin/env bash
# Validate the canonical AppStream MetaInfo and prove it is consistent with the
# desktop entry and icon that the Linux packages ship. This is the CI gate for
# issue #342: it fails on invalid or incomplete metadata before any package is
# built, and it is deliberately root-free so it runs on every pull request.
#
# What it checks:
#   1. `appstreamcli validate --pedantic` on the source MetaInfo.
#   2. `appstreamcli validate-tree --pedantic` on an assembled installed layout.
#   3. `appstreamcli compose` on that layout, which fails when the desktop-id or
#      icon referenced by the MetaInfo is missing or mismatched.
#   4. Explicit field assertions (id, launchable, icon, license, project URL)
#      so Flathub-required fields that the spec validator treats as optional are
#      still enforced.
#
# Package install/uninstall and system-pool discoverability are exercised
# separately against the built .deb in the Linux release workflow, because they
# require a real system prefix.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG="$ROOT/rust/packaging/linux"
METAINFO="$PKG/com.befeast.okplayer.metainfo.xml"
DESKTOP="$PKG/com.befeast.okplayer.desktop"
SCALABLE_ICON="$PKG/com.befeast.okplayer.svg"
FIXED_ICONS="$PKG/icons/hicolor"

APP_ID="com.befeast.okplayer"
DESKTOP_ID="com.befeast.okplayer.desktop"
EXPECTED_LICENSE="GPL-3.0-or-later"
EXPECTED_HOMEPAGE="https://github.com/BeFeast/ok-player"

for tool in appstreamcli; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool (install the 'appstream' package)" >&2
    exit 127
  fi
done

fail() {
  echo "AppStream smoke failed: $1" >&2
  exit 1
}

for path in "$METAINFO" "$DESKTOP" "$SCALABLE_ICON"; do
  [[ -f "$path" ]] || fail "missing packaging input $path"
done

echo "== Validate source MetaInfo (pedantic) =="
appstreamcli validate --pedantic --no-color "$METAINFO"

# Assemble the exact file tree the packages install, so the validator and the
# metadata composer see what a user would actually get.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
install -Dm644 "$METAINFO" "$STAGE/usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
install -Dm644 "$DESKTOP" "$STAGE/usr/share/applications/$DESKTOP_ID"
install -Dm644 "$SCALABLE_ICON" \
  "$STAGE/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$FIXED_ICONS/${size}x${size}/apps/$APP_ID.svg" \
    "$STAGE/usr/share/icons/hicolor/${size}x${size}/apps/$APP_ID.svg"
done

echo "== Validate installed file tree (pedantic) =="
appstreamcli validate-tree --pedantic --no-color "$STAGE"

# `compose` turns the installed tree into catalog metadata the way a distro
# software centre does. It resolves the launchable desktop entry and the icon,
# so a MetaInfo that names a desktop-id or icon the tree does not contain fails
# here even though plain validation passes.
if command -v appstreamcli >/dev/null 2>&1 && \
   [[ -x /usr/libexec/appstreamcli-compose || -x /usr/lib/appstreamcli-compose ]]; then
  echo "== Compose catalog metadata (desktop-id + icon consistency) =="
  COMPOSE_OUT="$STAGE/compose-out"
  COMPOSE_DATA="$STAGE/compose-data"
  # --media-baseurl is only a throwaway prefix for the composer's local icon
  # cache; it is never written into the shipped MetaInfo.
  appstreamcli compose \
    --result-root="$COMPOSE_OUT" \
    --data-dir="$COMPOSE_DATA" \
    --prefix=/usr \
    --origin=okplayer-smoke \
    --media-baseurl="https://localhost/okplayer-smoke-media" \
    "$STAGE"
  # compose names the catalog after --origin, so the path is deterministic.
  catalog="$COMPOSE_DATA/okplayer-smoke.xml.gz"
  [[ -f "$catalog" ]] || fail "compose produced no catalog metadata at $catalog"
  catalog_xml="$(zcat "$catalog")"
  grep -q "<id>$APP_ID</id>" <<<"$catalog_xml" \
    || fail "composed catalog does not carry id $APP_ID"
  grep -q "<name>OK Player</name>" <<<"$catalog_xml" \
    || fail "composed catalog does not carry the application name"
else
  echo "== Compose skipped (appstreamcli-compose addon not installed) =="
fi

echo "== Field and cross-file consistency =="
# MetaInfo id must match the reverse-DNS application id.
grep -q "<id>$APP_ID</id>" "$METAINFO" \
  || fail "MetaInfo <id> is not $APP_ID"
# Launchable must point at the desktop entry the packages install.
grep -q "<launchable type=\"desktop-id\">$DESKTOP_ID</launchable>" "$METAINFO" \
  || fail "MetaInfo launchable is not $DESKTOP_ID"
# Project license is a Flathub requirement the spec validator treats as optional.
grep -q "<project_license>$EXPECTED_LICENSE</project_license>" "$METAINFO" \
  || fail "MetaInfo project_license is not $EXPECTED_LICENSE"
# Homepage URL must match the repository.
grep -q "<url type=\"homepage\">$EXPECTED_HOMEPAGE</url>" "$METAINFO" \
  || fail "MetaInfo homepage is not $EXPECTED_HOMEPAGE"
# The desktop entry must reference the same application id as its icon.
grep -q "^Icon=$APP_ID$" "$DESKTOP" \
  || fail "desktop entry Icon= is not $APP_ID"

echo "AppStream smoke passed: MetaInfo, desktop entry, and icon are consistent for $APP_ID"
