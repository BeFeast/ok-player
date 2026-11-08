#!/usr/bin/env bash
set -Eeuo pipefail
# Build with the existing Apple SDK and a caller-supplied app-local runtime.
#
# Optional inputs:
#   OKP_MACOS_BUILD_NUMBER  CI run number, the second part of CFBundleVersion (default 0)
#   OKP_CODESIGN_IDENTITY, OKP_CODESIGN_KEYCHAIN
#                           the identity scripts/macos-signing-identity.sh import chose;
#                           without them the app is signed ad hoc
: "${OKP_MPV_PREFIX:?Set the extracted runtime prefix}"
: "${OKP_MACOS_OUTPUT:?Set an empty output directory}"
: "${CARGO_TARGET_DIR:?Set an isolated Cargo target directory}"
root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/macos"
mkdir -p "$OKP_MACOS_OUTPUT"
output="$(cd "$OKP_MACOS_OUTPUT" && pwd)"
app="$output/OK Player.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT
# The dylibs are prepared in a directory without spaces in its path, so SwiftPM can link
# and test against them, and copied into the bundle afterwards.
frameworks="$work/Frameworks"
mkdir -p "$frameworks"
export MACOSX_DEPLOYMENT_TARGET=14.0
SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
export SDKROOT
DYLD_LIBRARY_PATH="$OKP_MPV_PREFIX/lib" cargo test --manifest-path "$root/rust/Cargo.toml" --locked --lib -p okp-ffi --features live-mpv --target aarch64-apple-darwin 2>&1 | tee "$output/tests.log"
# The session's own tests drive the pinned libmpv through the session and its C ABI.
DYLD_LIBRARY_PATH="$OKP_MPV_PREFIX/lib" cargo test --manifest-path "$root/rust/Cargo.toml" --locked -p okp-session --target aarch64-apple-darwin 2>&1 | tee -a "$output/tests.log"
cargo build --manifest-path "$root/rust/Cargo.toml" --locked --release -p okp-ffi --features live-mpv --target aarch64-apple-darwin 2>&1 | tee "$output/build.log"
cp "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/libokp_ffi.dylib" "$frameworks/"
cp "$OKP_MPV_PREFIX"/lib/*.dylib "$frameworks/"
headers=("$CARGO_TARGET_DIR"/aarch64-apple-darwin/release/build/okp-ffi-*/out/okp_core.h)
test "${#headers[@]}" -eq 1

# Rewrite install names and absolute references to @rpath and drop every rpath that
# points into a build machine. Signing comes last, because this invalidates signatures.
relocate() {
  local binary="$1" dep rpath
  while IFS= read -r dep; do
    case "$dep" in
      /System/*|/usr/lib/*|@*) ;;
      *) local name="${dep##*/}"; test -f "$frameworks/$name"
         install_name_tool -change "$dep" "@rpath/$name" "$binary" ;;
    esac
  done < <(otool -L "$binary" | tail -n +2 | awk '{print $1}')
  while IFS= read -r rpath; do
    case "$rpath" in
      @executable_path/*|@loader_path/*|/usr/lib/*|/System/*) ;;
      *) install_name_tool -delete_rpath "$rpath" "$binary" ;;
    esac
  done < <(otool -l "$binary" | awk '/cmd LC_RPATH/{p=1;next} p && /path /{print $2;p=0}')
}
install_name_tool -id @rpath/libokp_ffi.dylib "$frameworks/libokp_ffi.dylib"
for binary in "$frameworks/"*.dylib; do relocate "$binary"; done

# The package's two generated inputs: the C header of the FFI it links and the shell's
# colours, which come from the shared brand palette.
cp "${headers[0]}" "$package/Sources/COKPCore/okp_core.h"
mkdir -p "$package/Sources/OKPlayerShell/Generated"
python3 "$root/scripts/generate-macos-palette.py" "$root/brand/palette.json" "$package/Sources/OKPlayerShell/Generated/Palette.swift"
swiftpm=(--package-path "$package" --scratch-path "$work/swiftpm" -Xlinker "-L$frameworks")
swift build -c release --product ok-player "${swiftpm[@]}" \
  -Xlinker -rpath -Xlinker @executable_path/../Frameworks 2>&1 | tee "$output/swift-build.log"
# The test bundle loads the same dylibs from the staging directory.
swift test "${swiftpm[@]}" -Xlinker -rpath -Xlinker "$frameworks" 2>&1 | tee "$output/swift-test.log"
cp "$(swift build -c release --show-bin-path "${swiftpm[@]}")/ok-player" "$app/Contents/MacOS/ok-player"
relocate "$app/Contents/MacOS/ok-player"
cp "$frameworks/"*.dylib "$app/Contents/Frameworks/"

python3 "$root/scripts/generate-macos-info-plist.py" "$package/Info.plist" "$app/Contents/Info.plist" \
  --repo "$root" --build-number "${OKP_MACOS_BUILD_NUMBER:-0}" | tee "$output/version.txt"
cp "$package/Resources/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp "$root/LICENSE" "$app/Contents/Resources/"
cp "$package/README.md" "$app/Contents/Resources/README.md"
cp "$OKP_MPV_PREFIX/provenance.json" "$app/Contents/Resources/runtime-provenance.json"

# Every nested binary first, then the bundle, with one identity.
signing="$root/scripts/macos-signing-identity.sh"
for binary in "$app/Contents/Frameworks/"*.dylib "$app/Contents/MacOS/ok-player"; do
  bash "$signing" sign "$binary"
done
bash "$signing" sign "$app"
{
  if test "${OKP_CODESIGN_IDENTITY:--}" = -; then
    echo "mode=ad-hoc"
  else
    echo "mode=self-signed identity=$OKP_CODESIGN_IDENTITY"
  fi
  codesign -d -r- "$app" 2>&1 | sed -n 's/^#* *designated => /designated_requirement=/p'
} | tee "$output/signing.txt"
{
  for binary in "$app/Contents/MacOS/ok-player" "$app/Contents/Frameworks/"*.dylib; do
    file "$binary"; lipo -archs "$binary"; otool -L "$binary"; otool -l "$binary"
  done
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dv --verbose=2 "$app"
  codesign -d -r- "$app"
} > "$output/inspection.txt" 2>&1
# Archive exists before playback validation, so smoke failure never hides the app.
(cd "$output" && ditto -c -k --sequesterRsrc --keepParent 'OK Player.app' ok-player-macos-arm64.zip)
# Name the file relative to the checksum list, so `shasum -c` works wherever both land.
(cd "$output" && shasum -a 256 ok-player-macos-arm64.zip) | tee "$output/SHA256SUMS.txt"
