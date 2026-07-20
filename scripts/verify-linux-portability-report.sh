#!/usr/bin/env bash
# Bind a completed portability report to the exact candidate artifacts.
set -euo pipefail

REPORT="${1:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
DEB="${2:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
APPIMAGE="${3:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
EXPECTED_SOURCE_SHA="${4:?usage: verify-linux-portability-report.sh <report.json> <deb> <appimage> <source-sha>}"
REQUIRED_MODE="${OKP_PORTABILITY_REQUIRED_MODE:-any}"

if [[ -n "${OKP_PORTABILITY_IMAGES:-}" ]]; then
  EXPECTED_TARGET_IMAGES_TEXT="$OKP_PORTABILITY_IMAGES"
elif [[ -n "${OKP_PORTABILITY_IMAGE:-}" ]]; then
  EXPECTED_TARGET_IMAGES_TEXT="$OKP_PORTABILITY_IMAGE"
else
  EXPECTED_TARGET_IMAGES_TEXT="debian:testing-slim ubuntu:26.04"
fi
read -r -a EXPECTED_TARGET_IMAGES <<<"$EXPECTED_TARGET_IMAGES_TEXT"

for tool in jq sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
case "$REQUIRED_MODE" in
  any | foreign-container) ;;
  *) echo "Unknown required portability mode: $REQUIRED_MODE" >&2; exit 2 ;;
esac
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
expected_target_images_json="$(
  printf '%s\n' "${EXPECTED_TARGET_IMAGES[@]}" \
    | jq -Rsc 'split("\n") | map(select(length > 0))'
)"

jq -e \
  --arg deb_name "$deb_name" \
  --arg deb_sha256 "$deb_sha256" \
  --arg appimage_name "$appimage_name" \
  --arg appimage_sha256 "$appimage_sha256" \
  --argjson target_images "$expected_target_images_json" \
  --arg source_sha "$EXPECTED_SOURCE_SHA" \
  --arg build_marker "$EXPECTED_BUILD_MARKER" \
  --arg required_mode "$REQUIRED_MODE" \
  '.schema_version == 3
   and .status == "pass"
   and .source_sha == $source_sha
   and .build_marker == $build_marker
   and ($required_mode == "any" or .verification_mode == $required_mode)
   and (
     (.verification_mode == "native-equivalence"
      and .targets == []
      and .checks == ["no-bundled-glibc-runtime", "all-bundled-elf-dependency-equivalence", "appimage-package-build-marker", "debian-package-build-marker"])
     or
     (.verification_mode == "foreign-container"
      and ([.targets[].image] == $target_images)
      and ((.targets | length) == ($target_images | length))
      and all(.targets[]; .image_id | test("^sha256:[0-9a-f]{64}$"))
      and .checks == ["no-bundled-glibc-runtime", "all-bundled-elf-dependency-equivalence", "all-bundled-elf-ldd", "appimage-package-build-marker", "appimage-media-narrow-width", "appimage-media-fullscreen", "appimage-media-compact-transition", "debian-package-build-marker", "debian-media-narrow-width", "debian-media-fullscreen", "debian-media-compact-transition"])
   )
   and .artifacts.debian == {file_name:$deb_name, sha256:$deb_sha256}
   and .artifacts.appimage == {file_name:$appimage_name, sha256:$appimage_sha256}' \
  "$REPORT" >/dev/null || {
    echo "Portability report does not attest the exact candidate artifacts" >&2
    exit 1
  }

echo "Portability report matches the exact Debian and AppImage artifacts."
