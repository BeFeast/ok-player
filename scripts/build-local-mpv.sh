#!/usr/bin/env bash
# Build mpv and libmpv from one upstream source tree so standalone vo_gpu and
# OK Player can be measured against the exact same engine revision.
set -euo pipefail

SOURCE="${1:?usage: build-local-mpv.sh <mpv-source-tree> <output-dir>}"
OUT="${2:?usage: build-local-mpv.sh <mpv-source-tree> <output-dir>}"

for tool in meson ninja pkg-config; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$SOURCE/meson.build" ]] || { echo "Not an mpv source tree: $SOURCE" >&2; exit 2; }

SOURCE="$(realpath "$SOURCE")"
mkdir -p "$OUT"
OUT="$(realpath "$OUT")"
BUILD="$OUT/build"
PREFIX="$OUT/install"

meson setup "$BUILD" "$SOURCE" --wipe \
  --buildtype=debugoptimized \
  --prefix="$PREFIX" \
  -Db_lto=false \
  -Dcplayer=true \
  -Dlibmpv=true \
  -Dtests=true \
  -Dc_args='-fno-omit-frame-pointer' \
  -Dc_link_args='-Wl,--build-id'
meson compile -C "$BUILD"
meson install -C "$BUILD"

pkg_dir="$(find "$PREFIX" -type d -path '*/pkgconfig' -print -quit)"
[[ -n "$pkg_dir" ]] || { echo "Installed libmpv pkg-config metadata was not found" >&2; exit 1; }
mpv_binary="$(find "$PREFIX" -type f -path '*/bin/mpv' -print -quit)"
[[ -n "$mpv_binary" ]] || { echo "Installed standalone mpv binary was not found" >&2; exit 1; }

printf 'Standalone mpv: %s\n' "$mpv_binary"
printf 'Build OK Player with: PKG_CONFIG_PATH=%s CC=/usr/bin/cc cargo build -p okp-linux-gtk\n' "$pkg_dir"
