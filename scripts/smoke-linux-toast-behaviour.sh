#!/usr/bin/env bash
# Run the display-backed toast regression tests on a virtual display.
#
# These tests construct the real GTK status toast, so they are `#[ignore]`d in the plain
# workspace run (which has no display server) and driven here under Xvfb instead.
# `--test-threads=1` is required: GTK may only be initialized from one thread per process.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Xvfb must not load a hardware EGL vendor: on hosts with a proprietary driver the GLX
# extension setup crashes the virtual server. Software rendering is all these tests need.
MESA_EGL_VENDOR=/usr/share/glvnd/egl_vendor.d/50_mesa.json
if [[ -f "$MESA_EGL_VENDOR" ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES="$MESA_EGL_VENDOR"
fi

cd "$ROOT/rust"
xvfb-run --auto-servernum -- \
  cargo test -p okp-linux-gtk -- --ignored --test-threads=1 --nocapture

echo "Toast behaviour smokes passed."
