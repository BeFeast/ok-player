#!/usr/bin/env bash
# Build the local Flatpak beta repository with every source prefetched first,
# then prove the build itself succeeds with downloads disabled.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
OUT_DIR="${OKP_FLATPAK_OUT_DIR:-$ROOT/artifacts/linux/flatpak}"
BUILD_DIR="$OUT_DIR/build"
STATE_DIR="$OUT_DIR/state"
REPO_DIR="$OUT_DIR/repo"
BUNDLE="$OUT_DIR/OK-Player-0.11.0-beta.1.flatpak"

for tool in flatpak flatpak-builder; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

mkdir -p "$OUT_DIR" "$STATE_DIR" "$REPO_DIR"

flatpak-builder --user --download-only --force-clean --disable-rofiles-fuse \
  --state-dir="$STATE_DIR" \
  "$BUILD_DIR" "$MANIFEST"

flatpak-builder --user --disable-download --force-clean --disable-rofiles-fuse \
  --state-dir="$STATE_DIR" \
  --repo="$REPO_DIR" \
  --default-branch=beta \
  "$BUILD_DIR" "$MANIFEST"

flatpak build-bundle "$REPO_DIR" "$BUNDLE" com.befeast.okplayer beta

echo "Flatpak beta repository: $REPO_DIR"
echo "Flatpak beta bundle: $BUNDLE"
