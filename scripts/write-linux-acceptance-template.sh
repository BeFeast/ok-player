#!/usr/bin/env bash
# Bind release evidence to the exact Linux candidate artifacts.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:?package version is required}"
COMMIT_SHA="${2:?full commit SHA is required}"
OUT_DIR="${3:-$ROOT/artifacts/linux}"

deb="$OUT_DIR/deb/ok-player_${VERSION}_amd64.deb"
appimage="$OUT_DIR/velopack/OK-Player-${VERSION}-x86_64.AppImage"
for artifact in "$deb" "$appimage"; do
  [[ -f "$artifact" ]] || { echo "Missing candidate artifact: $artifact" >&2; exit 1; }
done

mkdir -p "$OUT_DIR"
CC=/usr/bin/cc cargo run --quiet \
  --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  identity --version "$VERSION" --commit "$COMMIT_SHA" \
  --deb "$deb" --appimage "$appimage" >"$OUT_DIR/package-identity.json"

CC=/usr/bin/cc cargo run --quiet \
  --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  template --version "$VERSION" --commit "$COMMIT_SHA" \
  --deb "$deb" --appimage "$appimage" >"$OUT_DIR/acceptance-template.json"

echo "Package identity: $OUT_DIR/package-identity.json"
echo "Acceptance template: $OUT_DIR/acceptance-template.json"
