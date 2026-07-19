#!/usr/bin/env bash
# Build mpv and libmpv from one upstream source tree so standalone vo_gpu and
# OK Player can be measured against the exact same engine revision.
set -euo pipefail

SOURCE="${1:?usage: build-local-mpv.sh <mpv-source-tree> <output-dir>}"
OUT="${2:?usage: build-local-mpv.sh <mpv-source-tree> <output-dir>}"
SCRIPT_DIR="$(cd -- "${BASH_SOURCE[0]%/*}" && pwd)"
EMBED_PATCH="$SCRIPT_DIR/../rust/patches/mpv-v0.40.0-wayland-embed.patch"
FFMPEG_PATCH="$SCRIPT_DIR/../rust/patches/mpv-v0.40.0-ffmpeg-8.patch"

source "$SCRIPT_DIR/linux-candidate-toolchain.sh"
okp_candidate_toolchain_preflight
[[ -f "$SOURCE/meson.build" ]] || { echo "Not an mpv source tree: $SOURCE" >&2; exit 2; }
[[ -f "$EMBED_PATCH" ]] || { echo "Missing Wayland embed patch" >&2; exit 2; }
[[ -f "$FFMPEG_PATCH" ]] || { echo "Missing FFmpeg compatibility patch" >&2; exit 2; }

SOURCE="$(okp_candidate_tool realpath "$SOURCE")"
okp_candidate_tool mkdir -p "$OUT"
OUT="$(okp_candidate_tool realpath "$OUT")"
BUILD="$OUT/build"
PREFIX="$OUT/install"

apply_patch_once() {
  local patch="$1" description="$2"
  if okp_candidate_tool git -C "$SOURCE" apply --reverse --check "$patch" >/dev/null 2>&1; then
    printf '%s: already applied\n' "$description"
  elif okp_candidate_tool git -C "$SOURCE" apply --check "$patch"; then
    okp_candidate_tool git -C "$SOURCE" apply "$patch"
    printf '%s: applied\n' "$description"
  else
    echo "$description requires the pinned mpv v0.40.0 source tree." >&2
    exit 2
  fi
}

apply_patch_once "$EMBED_PATCH" "Wayland embed patch"
apply_patch_once "$FFMPEG_PATCH" "FFmpeg 8 compatibility patch"

okp_candidate_tool meson setup "$BUILD" "$SOURCE" --wipe \
  --buildtype=debugoptimized \
  --prefix="$PREFIX" \
  -Db_lto=false \
  -Dcplayer=true \
  -Dlibmpv=true \
  -Dtests=true \
  -Dc_args='-fno-omit-frame-pointer' \
  -Dc_link_args='-Wl,--build-id'
okp_candidate_tool meson compile -C "$BUILD"
okp_candidate_tool meson install -C "$BUILD"

pkg_configs=()
for candidate in \
  "$PREFIX"/lib/pkgconfig/mpv.pc \
  "$PREFIX"/lib/*/pkgconfig/mpv.pc \
  "$PREFIX"/lib64/pkgconfig/mpv.pc; do
  [[ -e "$candidate" ]] && pkg_configs+=("$candidate")
done
(( ${#pkg_configs[@]} == 1 )) || { echo "Installed libmpv pkg-config metadata was not found" >&2; exit 1; }
pkg_dir="$(okp_candidate_tool dirname -- "${pkg_configs[0]}")"
mpv_binary="$PREFIX/bin/mpv"
[[ -x "$mpv_binary" ]] || { echo "Installed standalone mpv binary was not found" >&2; exit 1; }

printf 'Standalone mpv: %s\n' "$mpv_binary"
printf 'Build OK Player with: PKG_CONFIG_PATH=%s CC=/usr/bin/cc cargo build -p okp-linux-gtk\n' "$pkg_dir"
