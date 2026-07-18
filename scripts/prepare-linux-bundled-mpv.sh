#!/usr/bin/env bash
# Prepare the exact patched mpv used by Debian and AppImage packages.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
UPSTREAM_URL="https://github.com/mpv-player/mpv.git"
UPSTREAM_TAG="v0.40.0"
UPSTREAM_COMMIT="e48ac7ce08462f5e33af6ef9deeac6fa87eef01e"
EMBED_PATCH="$ROOT/rust/patches/mpv-v0.40.0-wayland-embed.patch"
FFMPEG_PATCH="$ROOT/rust/patches/mpv-v0.40.0-ffmpeg-8.patch"

for tool in git sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

patch_key="$(sha256sum "$EMBED_PATCH" "$FFMPEG_PATCH" | sha256sum | cut -c1-16)"
WORK_ROOT="${OKP_BUNDLED_MPV_ROOT:-$TARGET_DIR/okp-bundled-mpv}"
WORK="$WORK_ROOT/${UPSTREAM_TAG}-${UPSTREAM_COMMIT:0:12}-$patch_key"
SOURCE="$WORK/source"
OUTPUT="$WORK/output"
PREFIX="$OUTPUT/install"

installed_libraries=()
for library in \
  "$PREFIX"/lib/libmpv.so.2 \
  "$PREFIX"/lib/*/libmpv.so.2 \
  "$PREFIX"/lib64/libmpv.so.2; do
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
  mkdir -p "$WORK"
  if [[ ! -d "$SOURCE/.git" ]]; then
    git clone --quiet --depth 1 --branch "$UPSTREAM_TAG" "$UPSTREAM_URL" "$SOURCE"
  fi
  actual_commit="$(git -C "$SOURCE" rev-parse HEAD)"
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

printf '%s\n' "$PREFIX"
