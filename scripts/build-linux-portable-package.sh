#!/usr/bin/env bash
# Build one shipping Linux package lane against the pinned portable ABI baseline.
set -euo pipefail

LANE="${1:?usage: build-linux-portable-package.sh <deb|appimage> <version>}"
VERSION="${2:?usage: build-linux-portable-package.sh <deb|appimage> <version>}"
case "$LANE" in
  deb | appimage) ;;
  *) echo "Unknown Linux package lane: $LANE" >&2; exit 2 ;;
esac

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${OKP_PORTABLE_BUILDER_IMAGE:-ok-player-linux-builder:debian-13-v1}"
MODE="${OKP_PORTABLE_PACKAGE_MODE:-container}"

for tool in git id; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

SOURCE_SHA="$(git -C "$ROOT" rev-parse --verify 'HEAD^{commit}')"
[[ "$SOURCE_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  echo "Portable package source SHA is invalid: $SOURCE_SHA" >&2
  exit 1
}
BUILD_SHA="${SOURCE_SHA:0:7}"

case "$MODE" in
  native)
    if [[ "$LANE" == deb ]]; then
      exec env OKP_BUILD_SHA="$BUILD_SHA" \
        "$ROOT/scripts/package-linux-deb.sh" "$VERSION"
    fi
    exec env OKP_BUILD_SHA="$BUILD_SHA" \
      OKP_LINUX_CHANNEL="${OKP_LINUX_CHANNEL:-linux}" \
      "$ROOT/scripts/package-linux-velopack.sh" "$VERSION"
    ;;
  container) ;;
  *) echo "Unknown Linux package build mode: $MODE" >&2; exit 2 ;;
esac

if command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=docker
elif command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1; then
  CONTAINER_RUNTIME=podman
else
  echo "Missing usable container runtime: docker or podman" >&2
  exit 127
fi

"$CONTAINER_RUNTIME" build \
  --tag "$IMAGE" \
  --target "$LANE" \
  --file "$ROOT/scripts/linux-portable-builder.Dockerfile" \
  "$ROOT/scripts"

"$CONTAINER_RUNTIME" run --rm \
  --mount "type=bind,src=$ROOT,dst=/workspace" \
  --mount "type=volume,src=ok-player-cargo-registry,dst=/root/.cargo/registry" \
  --mount "type=volume,src=ok-player-cargo-git,dst=/root/.cargo/git" \
  --workdir /workspace \
  -e CC=/usr/bin/cc \
  -e CARGO_TARGET_DIR=/workspace/rust/target/portable \
  -e OKP_BUNDLED_MPV_ROOT=/workspace/rust/target/portable/okp-bundled-mpv \
  -e OKP_LINUX_CHANNEL="${OKP_LINUX_CHANNEL:-linux}" \
  -e OKP_BUILD_SHA="$BUILD_SHA" \
  -e LANE="$LANE" \
  -e VERSION="$VERSION" \
  "$IMAGE" bash -ceu '
    if [[ "$LANE" == deb ]]; then
      ./scripts/package-linux-deb.sh "$VERSION"
    else
      ./scripts/package-linux-velopack.sh "$VERSION"
    fi
    # Hand the outputs back to the host user. The decision is observed from
    # the bind mount owner, which is correct for rootful and rootless docker
    # AND podman alike - see the script header.
    ./scripts/container-fixup-ownership.sh /workspace artifacts/linux rust/target/portable
  '
