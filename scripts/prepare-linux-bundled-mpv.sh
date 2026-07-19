#!/usr/bin/env bash
# Prepare the exact patched mpv used by Debian and AppImage packages.
set -euo pipefail

SCRIPT_DIR="$(cd -- "${BASH_SOURCE[0]%/*}" && pwd)"
ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
UPSTREAM_URL="https://github.com/mpv-player/mpv.git"
UPSTREAM_TAG="v0.40.0"
UPSTREAM_COMMIT="e48ac7ce08462f5e33af6ef9deeac6fa87eef01e"
EMBED_PATCH="$ROOT/rust/patches/mpv-v0.40.0-wayland-embed.patch"
FFMPEG_PATCH="$ROOT/rust/patches/mpv-v0.40.0-ffmpeg-8.patch"

source "$ROOT/scripts/linux-candidate-toolchain.sh"
okp_candidate_toolchain_preflight

patch_key="$(okp_candidate_tool sha256sum "$EMBED_PATCH" "$FFMPEG_PATCH" \
  | okp_candidate_tool sha256sum \
  | okp_candidate_tool cut -c1-16)"
WORK_ROOT="${OKP_BUNDLED_MPV_ROOT:-$TARGET_DIR/okp-bundled-mpv}"
WORK="$WORK_ROOT/${UPSTREAM_TAG}-${UPSTREAM_COMMIT:0:12}-$patch_key"
SOURCE="$WORK/source"
OUTPUT="$WORK/output"
PREFIX="$OUTPUT/install"
RUNTIME="$PREFIX/lib/ok-player"

installed_libraries=()
for library in \
  "$PREFIX"/lib/libmpv.so.2 \
  "$PREFIX"/lib/*/libmpv.so.2 \
  "$PREFIX"/lib64/libmpv.so.2; do
  [[ "$(dirname -- "$library")" == "$RUNTIME" ]] && continue
  [[ -e "$library" ]] && installed_libraries+=("$library")
done
installed_pkg_configs=()
for pkg_config in \
  "$PREFIX"/lib/pkgconfig/mpv.pc \
  "$PREFIX"/lib/*/pkgconfig/mpv.pc \
  "$PREFIX"/lib64/pkgconfig/mpv.pc; do
  [[ -e "$pkg_config" ]] && installed_pkg_configs+=("$pkg_config")
done
if (( ${#installed_libraries[@]} != 1 || ${#installed_pkg_configs[@]} != 1 )); then
  okp_candidate_tool mkdir -p "$WORK"
  if [[ ! -d "$SOURCE/.git" ]]; then
    okp_candidate_tool git clone --quiet --depth 1 --branch "$UPSTREAM_TAG" "$UPSTREAM_URL" "$SOURCE"
  fi
  actual_commit="$(okp_candidate_tool git -C "$SOURCE" rev-parse HEAD)"
  [[ "$actual_commit" == "$UPSTREAM_COMMIT" ]] || {
    echo "mpv $UPSTREAM_TAG resolved to $actual_commit, expected $UPSTREAM_COMMIT" >&2
    exit 1
  }
  "$ROOT/scripts/build-local-mpv.sh" "$SOURCE" "$OUTPUT" >&2
  installed_libraries=()
  for library in \
    "$PREFIX"/lib/libmpv.so.2 \
    "$PREFIX"/lib/*/libmpv.so.2 \
    "$PREFIX"/lib64/libmpv.so.2; do
    [[ "$(dirname -- "$library")" == "$RUNTIME" ]] && continue
    [[ -e "$library" ]] && installed_libraries+=("$library")
  done
  installed_pkg_configs=()
  for pkg_config in \
    "$PREFIX"/lib/pkgconfig/mpv.pc \
    "$PREFIX"/lib/*/pkgconfig/mpv.pc \
    "$PREFIX"/lib64/pkgconfig/mpv.pc; do
    [[ -e "$pkg_config" ]] && installed_pkg_configs+=("$pkg_config")
  done
fi

(( ${#installed_libraries[@]} == 1 )) || {
  echo "Expected exactly one bundled libmpv.so.2 under $PREFIX" >&2
  exit 1
}
(( ${#installed_pkg_configs[@]} == 1 )) || {
  echo "Expected exactly one bundled mpv.pc under $PREFIX" >&2
  exit 1
}

"$ROOT/scripts/collect-linux-bundled-mpv-runtime.sh" \
  "${installed_libraries[0]}" "$RUNTIME" >&2

printf '%s\n' "$PREFIX"
