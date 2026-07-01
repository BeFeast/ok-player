#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-0.1.0-linux-alpha.1}"
PACK_ID="com.befeast.okplayer"
TITLE="OK Player"
AUTHORS="BeFeast"
CHANNEL="${OKP_LINUX_CHANNEL:-linux}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
PACK_DIR="$ROOT/artifacts/linux/velopack-packdir"
OUTPUT_DIR="$ROOT/artifacts/linux/velopack"
ICON="$ROOT/rust/packaging/linux/com.befeast.okplayer.svg"
ABOUT_ICON="$ROOT/rust/packaging/linux/com.befeast.okplayer.about.svg"

if command -v vpk >/dev/null 2>&1; then
  VPK="${VPK:-vpk}"
elif [ -x "$HOME/.dotnet/tools/vpk" ]; then
  VPK="${VPK:-$HOME/.dotnet/tools/vpk}"
else
  echo "vpk is required. Install it with: dotnet tool install -g vpk" >&2
  exit 1
fi

export DOTNET_ROOT="${DOTNET_ROOT:-$HOME/.dotnet}"

OKP_BUILD_VERSION="$VERSION" cargo build --manifest-path "$ROOT/rust/Cargo.toml" -p okp-linux-gtk --release

rm -rf "$PACK_DIR" "$OUTPUT_DIR"
mkdir -p "$PACK_DIR" "$OUTPUT_DIR"
install -Dm755 "$TARGET_DIR/release/okp-linux-gtk" "$PACK_DIR/ok-player"
install -Dm644 "$ICON" "$PACK_DIR/com.befeast.okplayer.svg"
install -Dm644 "$ABOUT_ICON" "$PACK_DIR/com.befeast.okplayer.about.svg"

"$VPK" pack \
  --packId "$PACK_ID" \
  --packVersion "$VERSION" \
  --packDir "$PACK_DIR" \
  --mainExe ok-player \
  --outputDir "$OUTPUT_DIR" \
  --channel "$CHANNEL" \
  --packTitle "$TITLE" \
  --packAuthors "$AUTHORS" \
  --icon "$ICON" \
  --categories "AudioVideo;Player"

echo "Velopack Linux artifacts written to $OUTPUT_DIR"
