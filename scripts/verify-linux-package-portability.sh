#!/usr/bin/env bash
# Verify package runtime closure without requiring a container on native builders.
set -euo pipefail

# candidate-required-tools: awk basename chmod cp dirname dpkg-deb dpkg-query ldd mkdir mktemp objdump readlink rm sed sha256sum strings

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"
DEB="${1:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
APPIMAGE="${2:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
REPORT="${3:-$ROOT/artifacts/linux/portability-report.json}"
EXPECTED_SOURCE_SHA="${4:?usage: verify-linux-package-portability.sh <deb> <appimage> <report.json> <source-sha>}"
TARGET_IMAGE="${OKP_PORTABILITY_IMAGE:-debian:testing-slim}"
CONTAINER_MODE="${OKP_PORTABILITY_CONTAINER_MODE:-auto}"

for tool in awk basename chmod cp dirname dpkg-deb dpkg-query ldd mkdir mktemp objdump readlink rm sed sha256sum strings; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$DEB" ]] || { echo "Debian package is missing: $DEB" >&2; exit 1; }
[[ -f "$APPIMAGE" ]] || { echo "AppImage is missing: $APPIMAGE" >&2; exit 1; }
[[ "$EXPECTED_SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "Expected package source SHA is invalid: $EXPECTED_SOURCE_SHA" >&2
  exit 1
}
EXPECTED_BUILD_MARKER="${EXPECTED_SOURCE_SHA:0:7}"

case "$CONTAINER_MODE" in
  auto | required | skip) ;;
  *) echo "Unknown portability container mode: $CONTAINER_MODE" >&2; exit 2 ;;
esac

DEB="$(readlink -f -- "$DEB")"
APPIMAGE="$(readlink -f -- "$APPIMAGE")"
ARTIFACT_DIR="$(dirname -- "$DEB")"
APPIMAGE_DIR="$(dirname -- "$APPIMAGE")"
DEB_NAME="$(basename -- "$DEB")"
APPIMAGE_NAME="$(basename -- "$APPIMAGE")"
REPORT_DIR="$(dirname -- "$REPORT")"
WORK="$(okp_make_scratch_dir portability "$REPORT_DIR")"
trap 'rm -rf "$WORK"' EXIT
APPIMAGE_EXEC="$WORK/ok-player.AppImage"
cp "$APPIMAGE" "$APPIMAGE_EXEC"
chmod 755 "$APPIMAGE_EXEC"

declare -A DECLARED_PACKAGES=()
depends="$(dpkg-deb -f "$DEB" Depends)"
while IFS= read -r package; do
  package="${package#"${package%%[![:space:]]*}"}"
  package="${package%"${package##*[![:space:]]}"}"
  if [[ "$package" =~ ^(.+):(any|native|[[:alnum:]_-]+)$ ]]; then
    package="${BASH_REMATCH[1]}"
  fi
  [[ -n "$package" ]] && DECLARED_PACKAGES["$package"]=1
done < <(
  printf '%s\n' "$depends" \
    | sed -E 's/\([^)]*\)//g; s/[,|]/\n/g'
)

(( ${#DECLARED_PACKAGES[@]} > 0 )) || {
  echo "Debian package has no declared runtime dependencies" >&2
  exit 1
}

package_owner() {
  local path="$1" canonical owner
  canonical="$(readlink -f -- "$path")"
  owner="$(dpkg-query -S "$canonical" 2>/dev/null | sed -n '1{s/: .*//;p;q}' || true)"
  if [[ -z "$owner" && "$canonical" == /usr/* ]]; then
    owner="$(dpkg-query -S "${canonical#/usr}" 2>/dev/null | sed -n '1{s/: .*//;p;q}' || true)"
  fi
  printf '%s\n' "${owner%%:*}"
}

check_elf_tree() {
  local root="$1" private_lib="$2" label="$3"
  local object output soname resolved canonical owner checked=0 dependency_failures=0
  local private_canonical
  private_canonical="$(readlink -f -- "$private_lib")"

  shopt -s nullglob globstar
  for object in "$root"/**; do
    [[ -f "$object" ]] || continue
    declare -A needed=()
    while IFS= read -r soname; do
      needed["$soname"]=1
    done < <(objdump -p "$object" 2>/dev/null | awk '$1 == "NEEDED" { print $2 }')
    (( ${#needed[@]} > 0 )) || continue
    ((checked += 1))
    output="$(LD_LIBRARY_PATH="$private_lib" ldd "$object" 2>&1)" || {
      echo "portability ldd failed: $label/${object#$root/}" >&2
      printf '%s\n' "$output" >&2
      return 1
    }
    if awk '/not found/ { missing = 1 } END { exit !missing }' <<<"$output"; then
      echo "portability ldd found an unresolved dependency: $label/${object#$root/}" >&2
      printf '%s\n' "$output" >&2
      return 1
    fi

    while IFS='|' read -r soname resolved; do
      [[ -n "$resolved" ]] || continue
      [[ -n "${needed[$soname]+present}" ]] || continue
      canonical="$(readlink -f -- "$resolved")"
      if [[ "$canonical" == "$private_canonical"/* ]]; then
        continue
      fi
      owner="$(package_owner "$canonical")"
      if [[ -z "$owner" ]]; then
        echo "portability dependency has no package owner: $label/${object#$root/}: $soname => $resolved" >&2
        dependency_failures=1
        continue
      fi
      if [[ -z "${DECLARED_PACKAGES[$owner]+present}" ]]; then
        echo "portability dependency is not declared by the Debian package: $label/${object#$root/}: $soname => $resolved ($owner)" >&2
        dependency_failures=1
      fi
    done < <(
      awk '
        $2 == "=>" && $3 ~ /^\// { print $1 "|" $3 }
        $1 ~ /^\// { name = $1; sub(/^.*\//, "", name); print name "|" $1 }
      ' <<<"$output"
    )
  done

  (( checked > 0 )) || {
    echo "portability check found no dynamic ELF objects under $label" >&2
    return 1
  }
  (( dependency_failures == 0 )) || return 1
  echo "portability dependency equivalence: $checked dynamic ELF objects under $label PASS"
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

DEB_ROOT="$WORK/deb"
APP_ROOT="$WORK/appimage/squashfs-root"
mkdir -p "$DEB_ROOT" "$WORK/appimage"
dpkg-deb -x "$DEB" "$DEB_ROOT"
(
  cd "$WORK/appimage"
  "$APPIMAGE_EXEC" --appimage-extract >/dev/null
)

check_elf_tree "$DEB_ROOT/usr/lib/ok-player" "$DEB_ROOT/usr/lib/ok-player" debian
check_elf_tree "$APP_ROOT/usr/bin" "$APP_ROOT/usr/bin" appimage
check_build_marker "$APP_ROOT/usr/bin/ok-player" appimage
check_build_marker "$DEB_ROOT/usr/lib/ok-player/ok-player" debian

CONTAINER_RUNTIME=""
if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=docker
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=podman
fi

if [[ "$CONTAINER_MODE" == required && -z "$CONTAINER_RUNTIME" ]]; then
  echo "Strict portability verification requires a usable docker or podman runtime" >&2
  exit 127
fi

verification_mode=native-equivalence
target_image_json=null
target_image_id_json=null
checks_json='["all-bundled-elf-dependency-equivalence", "appimage-package-build-marker", "debian-package-build-marker"]'

if [[ "$CONTAINER_MODE" != skip && -n "$CONTAINER_RUNTIME" ]]; then
  "$CONTAINER_RUNTIME" pull "$TARGET_IMAGE" >/dev/null
  IMAGE_ID="$("$CONTAINER_RUNTIME" image inspect --format '{{.Id}}' "$TARGET_IMAGE")"
  [[ "$IMAGE_ID" == sha256:* ]] || IMAGE_ID="sha256:$IMAGE_ID"

  "$CONTAINER_RUNTIME" run --rm -i \
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

scratch="$(mktemp -d -t ok-player-portability.XXXXXX)"
trap 'rm -rf -- "$scratch"' EXIT
appimage_exec="$scratch/ok-player.AppImage"
appimage_root="$scratch/appimage"
cp "/artifacts/appimage/$APPIMAGE_NAME" "$appimage_exec"
chmod 755 "$appimage_exec"
mkdir -p "$appimage_root"
(
  cd "$appimage_root"
  "$appimage_exec" --appimage-extract >/dev/null
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
    "$binary" "$scratch/${label}-narrow-width"
  echo "portability media render: $label PASS"
}

APP_ROOT="$appimage_root/squashfs-root"
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

  verification_mode=foreign-container
  target_image_json="\"$TARGET_IMAGE\""
  target_image_id_json="\"$IMAGE_ID\""
  checks_json='["all-bundled-elf-dependency-equivalence", "all-bundled-elf-ldd", "appimage-package-build-marker", "appimage-media-render", "debian-package-build-marker", "debian-media-render"]'
fi

mkdir -p "$(dirname -- "$REPORT")"
deb_sha256="$(sha256sum -- "$DEB" | awk '{print $1}')"
appimage_sha256="$(sha256sum -- "$APPIMAGE" | awk '{print $1}')"
cat >"$REPORT" <<JSON
{
  "schema_version": 2,
  "verification_mode": "$verification_mode",
  "target_image": $target_image_json,
  "target_image_id": $target_image_id_json,
  "source_sha": "$EXPECTED_SOURCE_SHA",
  "build_marker": "$EXPECTED_BUILD_MARKER",
  "status": "pass",
  "checks": $checks_json,
  "artifacts": {
    "debian": {"file_name": "$DEB_NAME", "sha256": "$deb_sha256"},
    "appimage": {"file_name": "$APPIMAGE_NAME", "sha256": "$appimage_sha256"}
  }
}
JSON

echo "Linux portability report ($verification_mode): $REPORT"
