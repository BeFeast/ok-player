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
FIXED_ICONS="$ROOT/rust/packaging/linux/icons/hicolor"
METAINFO="$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

if command -v vpk >/dev/null 2>&1; then
  VPK="${VPK:-vpk}"
elif [ -x "$HOME/.dotnet/tools/vpk" ]; then
  VPK="${VPK:-$HOME/.dotnet/tools/vpk}"
else
  echo "vpk is required. Install it with: dotnet tool install -g vpk" >&2
  exit 1
fi

export DOTNET_ROOT="${DOTNET_ROOT:-$HOME/.dotnet}"
source "$ROOT/scripts/linux-bundled-mpv-env.sh"
okp_use_linux_bundled_mpv package

OKP_BUILD_VERSION="$VERSION" OKP_PACKAGE_KIND=appimage cargo build \
  --manifest-path "$ROOT/rust/Cargo.toml" \
  --release \
  -p okp-linux-gtk \
  -p okp-core \
  --bin okp-linux-gtk \
  --bin okp-candidate

rm -rf "$PACK_DIR" "$OUTPUT_DIR"
mkdir -p "$PACK_DIR" "$OUTPUT_DIR"
install -Dm755 "$TARGET_DIR/release/okp-linux-gtk" "$PACK_DIR/ok-player"
install -Dm755 "$OKP_BUNDLED_MPV_LIBRARY" "$PACK_DIR/libmpv.so.2"
install -Dm644 "$ICON" "$PACK_DIR/com.befeast.okplayer.svg"
install -Dm644 "$ICON" "$PACK_DIR/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
install -Dm644 "$METAINFO" "$PACK_DIR/usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$FIXED_ICONS/${size}x${size}/apps/com.befeast.okplayer.svg" \
    "$PACK_DIR/usr/share/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done

"$ROOT/scripts/verify-linux-bundled-mpv.sh" \
  "$PACK_DIR/ok-player" \
  "$PACK_DIR/libmpv.so.2"

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

"$TARGET_DIR/release/okp-candidate" stage-velopack \
  --output-dir "$OUTPUT_DIR" \
  --channel "$CHANNEL" \
  --package-id "$PACK_ID" \
  --version "$VERSION" \
  --versioned-appimage "OK-Player-$VERSION-x86_64.AppImage"

APPIMAGE_INSPECT="$(mktemp -d)"
trap 'rm -rf "$APPIMAGE_INSPECT"' EXIT
(
  cd "$APPIMAGE_INSPECT"
  "$OUTPUT_DIR/OK-Player-$VERSION-x86_64.AppImage" --appimage-extract >/dev/null
  "$ROOT/scripts/verify-linux-bundled-mpv.sh" \
    squashfs-root/usr/bin/ok-player \
    squashfs-root/usr/bin/libmpv.so.2
)
rm -rf "$APPIMAGE_INSPECT"
trap - EXIT

echo "Velopack Linux artifacts written to $OUTPUT_DIR"
echo "Run write-linux-acceptance-template.sh after both package lanes complete; publishing requires evidence for this exact artifact hash."
