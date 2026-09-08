#!/usr/bin/env bash
set -Eeuo pipefail
# Build with the existing Apple SDK and a caller-supplied app-local runtime.
: "${OKP_MPV_PREFIX:?Set the extracted runtime prefix}"
: "${OKP_MACOS_OUTPUT:?Set an empty output directory}"
: "${CARGO_TARGET_DIR:?Set an isolated Cargo target directory}"
root="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$OKP_MACOS_OUTPUT"
output="$(cd "$OKP_MACOS_OUTPUT" && pwd)"
app="$output/OK Player.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Frameworks" "$app/Contents/Resources"
export MACOSX_DEPLOYMENT_TARGET=13.0
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
DYLD_LIBRARY_PATH="$OKP_MPV_PREFIX/lib" cargo test --manifest-path "$root/rust/Cargo.toml" --locked --lib -p okp-ffi --features live-mpv --target aarch64-apple-darwin 2>&1 | tee "$output/tests.log"
cargo build --manifest-path "$root/rust/Cargo.toml" --locked --release -p okp-ffi --features live-mpv --target aarch64-apple-darwin 2>&1 | tee "$output/build.log"
cp "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/libokp_ffi.dylib" "$app/Contents/Frameworks/"
cp "$OKP_MPV_PREFIX"/lib/*.dylib "$app/Contents/Frameworks/"
cp "$root/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/LICENSE" "$app/Contents/Resources/"
cp "$root/macos/README.md" "$app/Contents/Resources/README.md"
cp "$OKP_MPV_PREFIX/provenance.json" "$app/Contents/Resources/runtime-provenance.json"
headers=("$CARGO_TARGET_DIR"/aarch64-apple-darwin/release/build/okp-ffi-*/out/okp_core.h)
test "${#headers[@]}" -eq 1
xcrun swiftc -swift-version 5 -O -target arm64-apple-macosx13.0 -sdk "$SDKROOT" \
  -I "$(dirname "${headers[0]}")" -import-objc-header "$root/macos/Bridge.h" "$root/macos/Player.swift" \
  -L "$app/Contents/Frameworks" -lokp_ffi -framework AppKit -framework OpenGL \
  -Xlinker -rpath -Xlinker @executable_path/../Frameworks \
  -o "$app/Contents/MacOS/ok-player" 2>&1 | tee "$output/swift-build.log"
# Rewrite the Rust dylib's install name and any absolute references before signing.
install_name_tool -id @rpath/libokp_ffi.dylib "$app/Contents/Frameworks/libokp_ffi.dylib"
for binary in "$app/Contents/MacOS/ok-player" "$app/Contents/Frameworks/"*.dylib; do
  while IFS= read -r dep; do
    case "$dep" in
      /System/*|/usr/lib/*|@*) ;;
      *) name="${dep##*/}"; test -f "$app/Contents/Frameworks/$name"
         install_name_tool -change "$dep" "@rpath/$name" "$binary" ;;
    esac
  done < <(otool -L "$binary" | tail -n +2 | awk '{print $1}')
  while IFS= read -r rpath; do
    case "$rpath" in /nix/*|"$CARGO_TARGET_DIR"*|"$OKP_MPV_PREFIX"*) install_name_tool -delete_rpath "$rpath" "$binary" ;; esac
  done < <(otool -l "$binary" | awk '/cmd LC_RPATH/{p=1;next} p && /path /{print $2;p=0}')
  codesign --force --sign - "$binary"
done
codesign --force --sign - "$app"
{
  for binary in "$app/Contents/MacOS/ok-player" "$app/Contents/Frameworks/"*.dylib; do
    file "$binary"; lipo -archs "$binary"; otool -L "$binary"; otool -l "$binary"
  done
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign -dv --verbose=2 "$app"
} > "$output/inspection.txt" 2>&1
# Archive exists before playback validation, so smoke failure never hides the app.
(cd "$output" && ditto -c -k --sequesterRsrc --keepParent 'OK Player.app' ok-player-macos-arm64.zip)
shasum -a 256 "$output/ok-player-macos-arm64.zip" | tee "$output/SHA256SUMS.txt"
