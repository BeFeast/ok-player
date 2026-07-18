#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-0.1.0-linux-alpha.1}"
ARCH="${OKP_DEB_ARCH:-amd64}"
PACKAGE="ok-player"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
DEB_DIR="$ROOT/artifacts/linux/deb"
BUILD_ROOT="$DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}"
ICON="$ROOT/rust/packaging/linux/com.befeast.okplayer.svg"
FIXED_ICONS="$ROOT/rust/packaging/linux/icons/hicolor"
DESKTOP="$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
METAINFO="$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

source "$ROOT/scripts/linux-bundled-mpv-env.sh"
okp_use_linux_bundled_mpv package

OKP_BUILD_VERSION="$VERSION" OKP_PACKAGE_KIND=deb \
  cargo build --manifest-path "$ROOT/rust/Cargo.toml" -p okp-linux-gtk --release

rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT/DEBIAN"
mkdir -p "$BUILD_ROOT/usr/lib/ok-player"
mkdir -p "$BUILD_ROOT/usr/bin"
mkdir -p "$BUILD_ROOT/usr/share/applications"
mkdir -p "$BUILD_ROOT/usr/share/metainfo"
mkdir -p "$BUILD_ROOT/usr/share/icons/hicolor/scalable/apps"

install -Dm755 "$TARGET_DIR/release/okp-linux-gtk" "$BUILD_ROOT/usr/lib/ok-player/ok-player"
install -Dm755 "$OKP_BUNDLED_MPV_LIBRARY" "$BUILD_ROOT/usr/lib/ok-player/libmpv.so.2"
ln -s ../lib/ok-player/ok-player "$BUILD_ROOT/usr/bin/ok-player"
install -Dm644 "$DESKTOP" "$BUILD_ROOT/usr/share/applications/com.befeast.okplayer.desktop"
install -Dm644 "$METAINFO" "$BUILD_ROOT/usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
install -Dm644 "$ICON" "$BUILD_ROOT/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$FIXED_ICONS/${size}x${size}/apps/com.befeast.okplayer.svg" \
    "$BUILD_ROOT/usr/share/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done

"$ROOT/scripts/verify-linux-bundled-mpv.sh" \
  "$BUILD_ROOT/usr/lib/ok-player/ok-player" \
  "$BUILD_ROOT/usr/lib/ok-player/libmpv.so.2"

cat > "$BUILD_ROOT/DEBIAN/control" <<CONTROL
Package: $PACKAGE
Version: $VERSION
Section: video
Priority: optional
Architecture: $ARCH
Maintainer: BeFeast <noreply@github.com>
Depends: libgtk-4-1, libmpv2, libgl1, libegl1, libglx0, libwayland-client0, libwayland-egl1, ffmpeg
Homepage: https://github.com/BeFeast/ok-player
Description: Elegant mpv-based media player
 OK Player is a native desktop media player built over its packaged libmpv.
 This Linux package is an early GTK4/Rust alpha.
CONTROL

cat > "$BUILD_ROOT/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi

exit 0
POSTINST

cat > "$BUILD_ROOT/DEBIAN/postrm" <<'POSTRM'
#!/bin/sh
set -e

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi

exit 0
POSTRM

chmod 755 "$BUILD_ROOT/DEBIAN/postinst" "$BUILD_ROOT/DEBIAN/postrm"
chmod -R u+rwX,go+rX,go-w "$BUILD_ROOT"
dpkg-deb --root-owner-group --build "$BUILD_ROOT" "$DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}.deb"

echo "Debian package written to $DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}.deb"
echo "Run write-linux-acceptance-template.sh after both package lanes complete; publishing requires evidence for this exact artifact hash."
