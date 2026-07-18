#!/usr/bin/env bash
# Refuse a Linux package payload that can fall back to the host's libmpv.
set -euo pipefail

BINARY="${1:?usage: verify-linux-bundled-mpv.sh <binary> <libmpv.so.2>}"
LIBRARY="${2:?usage: verify-linux-bundled-mpv.sh <binary> <libmpv.so.2>}"

for tool in ldd readelf strings; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -x "$BINARY" ]] || { echo "Packaged binary is not executable: $BINARY" >&2; exit 1; }
[[ -f "$LIBRARY" ]] || { echo "Bundled libmpv is missing: $LIBRARY" >&2; exit 1; }

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

echo "Bundled patched libmpv verified: $resolved"
