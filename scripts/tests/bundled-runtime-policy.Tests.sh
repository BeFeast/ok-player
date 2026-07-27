#!/usr/bin/env bash
# Behavioural coverage for the bundled-runtime platform policy (#670).
# The policy is the single decision point that keeps host-integration
# libraries out of the bundled closure; these checks execute the actual
# predicate the collector and the portability verifier call.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/scripts/linux-bundled-mpv-runtime-policy.sh"

fail() { echo "FAIL: $1" >&2; exit 1; }

# Host-integration libraries must be excluded from the bundle. The audio
# client stack entries are the #670 regression: bundling Debian libpipewire
# onto Ubuntu hosts mixed client and SPA-plugin ABIs (garbled audio, stalled
# AV clock).
for lib in \
  libpipewire-0.3.so.0 libpulse.so.0 libpulsecommon-17.0.so libjack.so.0 \
  libasound.so.2 libc.so.6 libGL.so.1 libwayland-client.so.0 libgtk-4.so.1; do
  okp_is_linux_platform_runtime "$lib" \
    || fail "$lib must be treated as host platform runtime (excluded from the bundle)"
done

# Media libraries the bundle exists FOR must stay bundled - an over-broad
# pattern here would hollow out the closure and resurrect the #423-class
# distro-libmpv fallback.
for lib in libmpv.so.2 libavcodec.so.61 libplacebo.so.349 libx264.so.164 libass.so.9; do
  okp_is_linux_platform_runtime "$lib" \
    && fail "$lib must stay in the bundled closure, not be delegated to the host"
done

echo "ok: platform-runtime policy excludes host integration (incl. the audio client stack) and keeps the media closure bundled"
