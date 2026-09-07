#!/usr/bin/env bash
# Behavioural coverage for the bundled-runtime platform policy (#670, #809).
# The policy is the single decision point that keeps host-integration
# libraries out of the bundled closure; these checks execute the actual
# predicate the collector and the portability verifier call.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/scripts/linux-bundled-mpv-runtime-policy.sh"

fail() { echo "FAIL: $1" >&2; exit 1; }

WORK="$(mktemp -d -t okp-bundled-runtime-policy.XXXXXX)"
trap 'rm -rf -- "$WORK"' EXIT

# Host-integration libraries must be excluded from the bundle. The audio
# client stack entries are the #670 regression. The VA-API entries are the #809
# regression: a builder libva cannot load a target driver that uses a newer
# __vaDriverInit ABI, even though both expose the same public SONAME.
for lib in \
  libva.so.2 libva-drm.so.2 libva-wayland.so.2 libva-x11.so.2 \
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

# A manifest is the package boundary, so exercise its verifier rather than
# merely asking the predicate. Every builder VA client must make an otherwise
# valid runtime manifest fail.
REJECTED="$WORK/rejected"
mkdir -p "$REJECTED"
for lib in libva.so.2 libva-drm.so.2 libva-wayland.so.2 libva-x11.so.2; do
  printf 'builder copy of %s\n' "$lib" >"$REJECTED/$lib"
  (
    cd "$REJECTED"
    sha256sum -- "$lib" >bundled-runtime.sha256
  )
  if "$ROOT/scripts/linux-bundled-mpv-runtime-policy.sh" \
      "$REJECTED/bundled-runtime.sha256" >"$WORK/manifest.out" 2>&1; then
    fail "a bundled runtime manifest carrying $lib must be rejected"
  fi
  rm -f -- "$REJECTED/$lib"
done

# Drive the actual collector with deterministic ldd output. The fake tool only
# supplies the builder resolution graph; enqueueing, platform filtering,
# copying, manifest generation, and final policy verification are all the
# production path. libavcodec models the media closure that must remain
# bundled, while all four VA clients model the closure that must stay on the
# target.
BUILDER="$WORK/builder"
STUB_BIN="$WORK/bin"
COLLECTED="$WORK/collected"
mkdir -p "$BUILDER" "$STUB_BIN"
for lib in \
  libmpv.so.2 libavcodec.so.61 \
  libva.so.2 libva-drm.so.2 libva-wayland.so.2 libva-x11.so.2; do
  printf 'fixture object %s\n' "$lib" >"$BUILDER/$lib"
done

cat >"$STUB_BIN/ldd" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
object="${1##*/}"
case "$object" in
  libmpv.so.2)
    printf '\tlibavcodec.so.61 => %s/libavcodec.so.61 (0x1)\n' "$OKP_TEST_BUILDER"
    for lib in libva.so.2 libva-drm.so.2 libva-wayland.so.2 libva-x11.so.2; do
      printf '\t%s => %s/%s (0x1)\n' "$lib" "$OKP_TEST_BUILDER" "$lib"
    done
    ;;
  libavcodec.so.61)
    printf '\tlibva.so.2 => %s/libva.so.2 (0x1)\n' "$OKP_TEST_BUILDER"
    ;;
esac
STUB
cat >"$STUB_BIN/patchelf" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
cat >"$STUB_BIN/readelf" <<'STUB'
#!/usr/bin/env bash
exit 0
STUB
chmod 755 "$STUB_BIN/ldd" "$STUB_BIN/patchelf" "$STUB_BIN/readelf"

PATH="$STUB_BIN:$PATH" OKP_TEST_BUILDER="$BUILDER" \
  "$ROOT/scripts/collect-linux-bundled-mpv-runtime.sh" \
  "$BUILDER/libmpv.so.2" "$COLLECTED" >"$WORK/collector.out"

[[ -f "$COLLECTED/libmpv.so.2" ]] \
  || fail "the collector must retain libmpv"
[[ -f "$COLLECTED/libavcodec.so.61" ]] \
  || fail "the collector must retain the bundled media closure"
if compgen -G "$COLLECTED/libva*.so*" >/dev/null; then
  fail "the collector must not copy builder VA-API clients"
fi
(
  cd "$COLLECTED"
  sha256sum --check bundled-runtime.sha256 >/dev/null
)
"$ROOT/scripts/linux-bundled-mpv-runtime-policy.sh" \
  "$COLLECTED/bundled-runtime.sha256" >/dev/null

echo "ok: collector and manifest keep VA-API/audio host integration out and the media closure bundled"
