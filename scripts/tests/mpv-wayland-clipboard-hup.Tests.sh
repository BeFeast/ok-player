#!/usr/bin/env bash
# Behavioural regression for the pinned mpv Wayland clipboard source (#826).
# This compiles the actual patched clipboard-wayland.c from the candidate source
# tree; the harness does not parse source text or reimplement its dispatch path.
set -euo pipefail

SOURCE="${1:?usage: mpv-wayland-clipboard-hup.Tests.sh <mpv-source> <meson-build>}"
BUILD="${2:?usage: mpv-wayland-clipboard-hup.Tests.sh <mpv-source> <meson-build>}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS="$ROOT/scripts/tests/mpv-wayland-clipboard-hup.c"

source "$ROOT/scripts/linux-candidate-toolchain.sh"

[[ -f "$SOURCE/player/clipboard/clipboard-wayland.c" ]] || {
  echo "Missing mpv Wayland clipboard source: $SOURCE" >&2
  exit 2
}
[[ -f "$BUILD/config.h" ]] || {
  echo "Missing configured mpv build tree: $BUILD" >&2
  exit 2
}

WORK="$(okp_candidate_tool mktemp -d -t okp-mpv-clipboard-test.XXXXXX)"
trap 'okp_candidate_tool rm -rf -- "$WORK"' EXIT

okp_candidate_tool ninja -C "$BUILD" video/out/ext-data-control-v1.h >/dev/null
okp_candidate_tool cc \
  -std=c11 -D_GNU_SOURCE -DNDEBUG \
  -ffunction-sections -fdata-sections -Wl,--gc-sections \
  -I"$BUILD" -I"$BUILD/video/out" -I"$SOURCE" \
  -pthread "$HARNESS" -lwayland-client \
  -o "$WORK/mpv-wayland-clipboard-hup"

"$WORK/mpv-wayland-clipboard-hup"
