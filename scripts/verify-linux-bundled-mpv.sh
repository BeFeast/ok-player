#!/usr/bin/env bash
# Refuse a Linux package payload with an incomplete or host-resolved mpv runtime.
set -euo pipefail

BINARY="${1:?usage: verify-linux-bundled-mpv.sh <binary> <runtime-dir>}"
RUNTIME_DIR="${2:?usage: verify-linux-bundled-mpv.sh <binary> <runtime-dir>}"
LIBRARY="$RUNTIME_DIR/libmpv.so.2"
MANIFEST="$RUNTIME_DIR/bundled-runtime.sha256"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

source "$SCRIPT_DIR/linux-bundled-mpv-runtime-policy.sh"

for tool in ldd readelf strings; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -x "$BINARY" ]] || { echo "Packaged binary is not executable: $BINARY" >&2; exit 1; }
[[ -f "$LIBRARY" ]] || { echo "Bundled libmpv is missing: $LIBRARY" >&2; exit 1; }
[[ -f "$MANIFEST" ]] || { echo "Bundled runtime manifest is missing: $MANIFEST" >&2; exit 1; }

(cd "$RUNTIME_DIR" && sha256sum --check bundled-runtime.sha256) >/dev/null || {
  echo "Bundled runtime files do not match their manifest" >&2
  exit 1
}
okp_verify_linux_bundled_runtime_manifest "$MANIFEST"

readelf -d "$BINARY" | awk '/\((RUNPATH|RPATH)\)/ && /\[\$ORIGIN\]/ { found = 1 } END { exit !found }' || {
  echo "Packaged binary does not carry an origin-relative libmpv lookup" >&2
  exit 1
}
readelf -d "$LIBRARY" | awk '/Library soname: \[libmpv\.so\.2\]/ { found = 1 } END { exit !found }' || {
  echo "Bundled library does not expose the expected libmpv.so.2 SONAME" >&2
  exit 1
}
strings "$LIBRARY" | awk '$0 == "wayland-embed-display" { found = 1 } END { exit !found }' || {
  echo "Bundled libmpv does not contain the embedded Wayland backend options" >&2
  exit 1
}

resolved="$(env -u LD_LIBRARY_PATH ldd "$BINARY" | sed -n 's/^[[:space:]]*libmpv\.so\.2 => \([^ ]*\).*/\1/p')"
[[ -n "$resolved" ]] || { echo "Packaged binary did not resolve libmpv.so.2" >&2; exit 1; }
[[ "$(readlink -f -- "$resolved")" == "$(readlink -f -- "$LIBRARY")" ]] || {
  echo "Packaged binary resolved libmpv outside its payload: $resolved" >&2
  exit 1
}

for object in "$BINARY" "$RUNTIME_DIR"/*.so*; do
  readelf -h "$object" >/dev/null 2>&1 || continue
  readelf -d "$object" | awk '/\((RUNPATH|RPATH)\)/ && /\[\$ORIGIN\]/ { found = 1 } END { exit !found }' || {
    echo "Bundled object does not carry an origin-relative runtime path: $object" >&2
    exit 1
  }
  if LD_LIBRARY_PATH="$RUNTIME_DIR" ldd "$object" | awk '/not found/ { missing = 1 } END { exit !missing }'; then
    echo "Bundled object has unresolved runtime dependencies: $object" >&2
    LD_LIBRARY_PATH="$RUNTIME_DIR" ldd "$object" >&2
    exit 1
  fi
done

for required in \
  libavcodec.so libavdevice.so libavformat.so libavfilter.so libavutil.so \
  libswscale.so libswresample.so libplacebo.so libass.so libbluray.so \
  librubberband.so; do
  compgen -G "$RUNTIME_DIR/$required*" >/dev/null || {
    echo "Bundled mpv runtime is missing required dependency family: $required" >&2
    exit 1
  }
done

echo "Self-contained patched libmpv runtime verified: $resolved"
