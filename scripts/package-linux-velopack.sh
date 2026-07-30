#!/usr/bin/env bash
set -euo pipefail

# candidate-required-tools: cargo cp install mkdir mktemp rm vpk

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"
export OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="$ROOT/scripts/package-linux-velopack.sh
$ROOT/scripts/collect-linux-bundled-mpv-runtime.sh
$ROOT/scripts/verify-linux-bundled-mpv.sh"
export OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS=true
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
cp -a "$OKP_BUNDLED_MPV_RUNTIME_DIR/." "$PACK_DIR/"
install -Dm644 "$ICON" "$PACK_DIR/com.befeast.okplayer.svg"
install -Dm644 "$ICON" "$PACK_DIR/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
install -Dm644 "$METAINFO" "$PACK_DIR/usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$FIXED_ICONS/${size}x${size}/apps/com.befeast.okplayer.svg" \
    "$PACK_DIR/usr/share/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done

# The AppImage carries its whole payload, so the licence documents have to be
# inside it. Velopack maps this pack directory onto the AppDir's usr/bin, so
# these land at usr/bin/usr/share/doc/ok-player inside the extracted image -
# one level deeper than the path below reads, and the same nesting this lane
# already gives its metainfo and icons. The extraction check further down
# asserts them at that real path.
"$ROOT/scripts/stage-license-documents.sh" appimage "$PACK_DIR/usr/share/doc/ok-player"

"$ROOT/scripts/verify-linux-bundled-mpv.sh" \
  "$PACK_DIR/ok-player" \
  "$PACK_DIR"

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

APPIMAGE_INSPECT="$(okp_make_scratch_dir appimage-inspect "$OUTPUT_DIR")"
trap 'rm -rf "$APPIMAGE_INSPECT"' EXIT
(
  cd "$APPIMAGE_INSPECT"
  "$OUTPUT_DIR/OK-Player-$VERSION-x86_64.AppImage" --appimage-extract >/dev/null
  "$ROOT/scripts/verify-linux-bundled-mpv.sh" \
    squashfs-root/usr/bin/ok-player \
    squashfs-root/usr/bin
  # Asked of the image that was produced, not of the directory it was packed
  # from: this is the only step that proves a user unpacking the AppImage finds
  # the licence documents in it (issue #743).
  for document in LICENSE LICENSE.LGPL-3.0 THIRD-PARTY-NOTICES.md; do
    [[ -s "squashfs-root/usr/bin/usr/share/doc/ok-player/$document" ]] || {
      echo "AppImage payload carries no $document" >&2
      exit 1
    }
  done
  echo "Licence documents verified inside the AppImage payload"
)
rm -rf "$APPIMAGE_INSPECT"
trap - EXIT

echo "Velopack Linux artifacts written to $OUTPUT_DIR"
echo "Run write-linux-acceptance-template.sh after both package lanes complete; publishing requires evidence for this exact artifact hash."
