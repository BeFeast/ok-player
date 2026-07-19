#!/usr/bin/env bash
# Bind a completed portability report to the exact candidate artifacts.
set -euo pipefail

REPORT="${1:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
DEB="${2:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
APPIMAGE="${3:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
EXPECTED_SOURCE_SHA="${4:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
EXPECTED_TARGET_IMAGE="${OKP_PORTABILITY_IMAGE:-debian:testing-slim}"

for tool in jq sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
for file in "$REPORT" "$DEB" "$APPIMAGE"; do
  [[ -f "$file" ]] || { echo "Portability input is missing: $file" >&2; exit 1; }
done
[[ "$EXPECTED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "Expected package source SHA is invalid: $EXPECTED_SOURCE_SHA" >&2
  exit 1
}
EXPECTED_BUILD_MARKER="${EXPECTED_SOURCE_SHA:0:7}"

deb_name="$(basename -- "$DEB")"
appimage_name="$(basename -- "$APPIMAGE")"
deb_sha256="$(sha256sum -- "$DEB" | awk '{print $1}')"
appimage_sha256="$(sha256sum -- "$APPIMAGE" | awk '{print $1}')"

jq -e \
  --arg deb_name "$deb_name" \
  --arg deb_sha256 "$deb_sha256" \
  --arg appimage_name "$appimage_name" \
  --arg appimage_sha256 "$appimage_sha256" \
  --arg target_image "$EXPECTED_TARGET_IMAGE" \
  --arg source_sha "$EXPECTED_SOURCE_SHA" \
  --arg build_marker "$EXPECTED_BUILD_MARKER" \
  '.schema_version == 1
   and .status == "pass"
   and .target_image == $target_image
   and (.target_image_id | test("^sha256:[0-9a-f]{64}$"))
   and .source_sha == $source_sha
   and .build_marker == $build_marker
   and .checks == ["all-bundled-elf-ldd", "appimage-package-build-marker", "appimage-media-render", "debian-package-build-marker", "debian-media-render"]
   and .artifacts.debian == {file_name:$deb_name, sha256:$deb_sha256}
   and .artifacts.appimage == {file_name:$appimage_name, sha256:$appimage_sha256}' \
  "$REPORT" >/dev/null || {
    echo "Portability report does not attest the exact candidate artifacts" >&2
    exit 1
  }

echo "Portability report matches the exact Debian and AppImage artifacts."
