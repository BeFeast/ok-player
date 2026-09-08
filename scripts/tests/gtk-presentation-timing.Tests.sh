#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d -t okp-gtk-timing.XXXXXX)"
trap 'rm -rf -- "$WORK"' EXIT
PROTOCOLS="$(pkg-config --variable=pkgdatadir wayland-protocols)"
for protocol in presentation-time viewporter; do
  wayland-scanner client-header "$PROTOCOLS/stable/$protocol/$protocol.xml" \
    "$WORK/$protocol-client-protocol.h"
  wayland-scanner private-code "$PROTOCOLS/stable/$protocol/$protocol.xml" \
    "$WORK/$protocol-protocol.c"
done
"${CC:-cc}" -std=c11 -D_GNU_SOURCE -ffunction-sections -fdata-sections \
  -Wl,--gc-sections -I"$WORK" "$ROOT/scripts/tests/gtk-presentation-timing.c" \
  "$WORK/presentation-time-protocol.c" "$WORK/viewporter-protocol.c" \
  -lwayland-client -lwayland-egl -lEGL -lGL -lm -o "$WORK/test"
"$WORK/test"
