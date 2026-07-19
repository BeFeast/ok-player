#!/usr/bin/env bash
# Bind a completed portability report to the exact candidate artifacts.
set -euo pipefail

REPORT="${1:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage>}"
DEB="${2:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage>}"
APPIMAGE="${3:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage>}"
EXPECTED_TARGET_IMAGE="${OKP_PORTABILITY_IMAGE:-debian:testing-slim}"

for tool in jq sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
for file in "$REPORT" "$DEB" "$APPIMAGE"; do
  [[ -f "$file" ]] || { echo "Portability input is missing: $file" >&2; exit 1; }
done

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
  '.schema_version == 1
   and .status == "pass"
   and .target_image == $target_image
   and (.target_image_id | test("^sha256:[0-9a-f]{64}$"))
   and .checks == ["all-bundled-elf-ldd", "appimage-installed-launch", "debian-installed-launch"]
   and .artifacts.debian == {file_name:$deb_name, sha256:$deb_sha256}
   and .artifacts.appimage == {file_name:$appimage_name, sha256:$appimage_sha256}' \
  "$REPORT" >/dev/null || {
    echo "Portability report does not attest the exact candidate artifacts" >&2
    exit 1
  }

echo "Portability report matches the exact Debian and AppImage artifacts."
