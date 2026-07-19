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
IMAGE="${OKP_PORTABLE_BUILDER_IMAGE:-ok-player-linux-builder:ubuntu-24.04-v1}"
HOST_UID="${SUDO_UID:-$(id -u)}"
HOST_GID="${SUDO_GID:-$(id -g)}"

for tool in docker id; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

docker build \
  --tag "$IMAGE" \
  --file "$ROOT/scripts/linux-portable-builder.Dockerfile" \
  "$ROOT/scripts"

docker run --rm \
  --mount "type=bind,src=$ROOT,dst=/workspace" \
  --mount "type=volume,src=ok-player-cargo-registry,dst=/root/.cargo/registry" \
  --mount "type=volume,src=ok-player-cargo-git,dst=/root/.cargo/git" \
  --workdir /workspace \
  -e CC=/usr/bin/cc \
  -e CARGO_TARGET_DIR=/workspace/rust/target/portable \
  -e OKP_BUNDLED_MPV_ROOT=/workspace/rust/target/portable/okp-bundled-mpv \
  -e OKP_LINUX_CHANNEL="${OKP_LINUX_CHANNEL:-linux}" \
  -e LANE="$LANE" \
  -e VERSION="$VERSION" \
  -e HOST_UID="$HOST_UID" \
  -e HOST_GID="$HOST_GID" \
  "$IMAGE" bash -ceu '
    if [[ "$LANE" == deb ]]; then
      ./scripts/package-linux-deb.sh "$VERSION"
    else
      ./scripts/package-linux-velopack.sh "$VERSION"
    fi
    chown -R "$HOST_UID:$HOST_GID" artifacts/linux rust/target/portable
  '
