#!/usr/bin/env bash
# Verify Debian and AppImage payloads on a distro that did not build them.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
DEB="${1:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
APPIMAGE="${2:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
REPORT="${3:-$ROOT/artifacts/linux/portability-report.json}"
EXPECTED_SOURCE_SHA="${4:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
TARGET_IMAGE="${OKP_PORTABILITY_IMAGE:-debian:testing-slim}"

for tool in docker readlink sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$DEB" ]] || { echo "Debian package is missing: $DEB" >&2; exit 1; }
[[ -f "$APPIMAGE" ]] || { echo "AppImage is missing: $APPIMAGE" >&2; exit 1; }
[[ "$EXPECTED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "Expected package source SHA is invalid: $EXPECTED_SOURCE_SHA" >&2
  exit 1
}
EXPECTED_BUILD_MARKER="${EXPECTED_SOURCE_SHA:0:7}"

DEB="$(readlink -f -- "$DEB")"
APPIMAGE="$(readlink -f -- "$APPIMAGE")"
ARTIFACT_DIR="$(dirname -- "$DEB")"
APPIMAGE_DIR="$(dirname -- "$APPIMAGE")"
DEB_NAME="$(basename -- "$DEB")"
APPIMAGE_NAME="$(basename -- "$APPIMAGE")"

docker pull "$TARGET_IMAGE" >/dev/null
IMAGE_ID="$(docker image inspect --format '{{.Id}}' "$TARGET_IMAGE")"

docker run --rm -i \
  --mount "type=bind,src=$ROOT,dst=/workspace,readonly" \
  --mount "type=bind,src=$ARTIFACT_DIR,dst=/artifacts/deb,readonly" \
  --mount "type=bind,src=$APPIMAGE_DIR,dst=/artifacts/appimage,readonly" \
  -e DEB_NAME="$DEB_NAME" \
  -e APPIMAGE_NAME="$APPIMAGE_NAME" \
  -e EXPECTED_BUILD_MARKER="$EXPECTED_BUILD_MARKER" \
  "$TARGET_IMAGE" bash -s <<'CONTAINER'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

apt-get update -qq
apt-get install -y --no-install-recommends \
  binutils ca-certificates dbus-x11 file libegl1 libgbm1 libgl1 libglx0 \
  libdecor-0-0 libgtk-4-1 libva2 libvulkan1 libwayland-client0 \
  libwayland-egl1 libxss1 \
  imagemagick procps squashfs-tools x11-utils xauth xdotool xfwm4 xvfb >/dev/null

cp "/artifacts/appimage/$APPIMAGE_NAME" /tmp/ok-player.AppImage
chmod 755 /tmp/ok-player.AppImage
mkdir -p /tmp/appimage
(
  cd /tmp/appimage
  /tmp/ok-player.AppImage --appimage-extract >/dev/null
)

check_elf_tree() {
  local root="$1" object output checked=0
  while IFS= read -r -d '' object; do
    readelf -h "$object" >/dev/null 2>&1 || continue
    readelf -d "$object" 2>/dev/null | awk '/\(NEEDED\)/ { needed = 1 } END { exit !needed }' || continue
    ((checked += 1))
    output="$(ldd "$object" 2>&1)" || {
      echo "portability ldd failed: ${object#$root/}" >&2
      printf '%s\n' "$output" >&2
      return 1
    }
    if awk '/not found/ { missing = 1 } END { exit !missing }' <<<"$output"; then
      echo "portability ldd found an unresolved dependency: ${object#$root/}" >&2
      printf '%s\n' "$output" >&2
      return 1
    fi
  done < <(find "$root" -type f -print0)
  echo "portability ldd: $checked dynamic ELF objects under $root PASS"
}

check_build_marker() {
  local binary="$1" label="$2"
  strings "$binary" | awk -v marker="$EXPECTED_BUILD_MARKER" \
    'index($0, marker) { found = 1 } END { exit !found }' || {
      echo "packaged build marker mismatch: $label expected $EXPECTED_BUILD_MARKER" >&2
      return 1
    }
  echo "portability build marker: $label PASS"
}

media_render_smoke() {
  local binary="$1" label="$2"
  "/workspace/scripts/smoke-linux-narrow-width.sh" \
    "$binary" "/tmp/${label}-narrow-width"
  echo "portability media render: $label PASS"
}

APP_ROOT=/tmp/appimage/squashfs-root
check_elf_tree "$APP_ROOT"
check_build_marker "$APP_ROOT/usr/bin/ok-player" appimage
media_render_smoke "$APP_ROOT/usr/bin/ok-player" appimage

depends="$(dpkg-deb -f "/artifacts/deb/$DEB_NAME" Depends)"
apt-get satisfy -y --no-install-recommends "$depends" >/dev/null
dpkg -i "/artifacts/deb/$DEB_NAME" >/dev/null
check_elf_tree /usr/lib/ok-player
check_build_marker /usr/bin/ok-player debian
media_render_smoke /usr/bin/ok-player debian
CONTAINER

mkdir -p "$(dirname -- "$REPORT")"
deb_sha256="$(sha256sum -- "$DEB" | awk '{print $1}')"
appimage_sha256="$(sha256sum -- "$APPIMAGE" | awk '{print $1}')"
cat >"$REPORT" <<JSON
{
  "schema_version": 1,
  "target_image": "$TARGET_IMAGE",
  "target_image_id": "$IMAGE_ID",
  "source_sha": "$EXPECTED_SOURCE_SHA",
  "build_marker": "$EXPECTED_BUILD_MARKER",
  "status": "pass",
  "checks": ["all-bundled-elf-ldd", "appimage-package-build-marker", "appimage-media-render", "debian-package-build-marker", "debian-media-render"],
  "artifacts": {
    "debian": {"file_name": "$DEB_NAME", "sha256": "$deb_sha256"},
    "appimage": {"file_name": "$APPIMAGE_NAME", "sha256": "$appimage_sha256"}
  }
}
JSON

echo "Linux portability report: $REPORT"
