#!/usr/bin/env bash
# Copy libmpv's non-platform runtime closure into one origin-relative directory.
set -euo pipefail

LIBMPV="${1:?usage: collect-linux-bundled-mpv-runtime.sh <libmpv.so.2> <output-dir>}"
OUTPUT="${2:?usage: collect-linux-bundled-mpv-runtime.sh <libmpv.so.2> <output-dir>}"

for tool in awk basename cmp cp ldd mkdir patchelf readelf rm sha256sum sort; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$LIBMPV" ]] || { echo "Bundled libmpv is missing: $LIBMPV" >&2; exit 1; }

# These libraries are part of the target kernel/libc or graphics-driver ABI.
# Bundling them can make Mesa, proprietary GPU drivers, NSS, or the dynamic
# loader select an incompatible build. The Debian package declares the
# corresponding platform dependencies; the portability gate verifies that the
# target supplies them. Everything else in libmpv's resolved closure is copied.
is_platform_runtime() {
  case "$1" in
    ld-linux*.so.* | libc.so.* | libdl.so.* | libm.so.* | libpthread.so.* | \
      libresolv.so.* | librt.so.* | libutil.so.* | libanl.so.* | libnsl.so.* | \
      libcrypt.so.* | libgcc_s.so.* | libGL.so.* | libEGL.so.* | libGLX.so.* | \
      libGLdispatch.so.* | libOpenGL.so.* | libdrm.so.* | libgbm.so.* | \
      libvulkan.so.* | libglib-2.0.so.* | libgobject-2.0.so.* | \
      libgio-2.0.so.* | libgmodule-2.0.so.* | libgthread-2.0.so.*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"

declare -a queue=()
declare -A queued=()

enqueue() {
  local soname="$1" source="$2" target
  target="$OUTPUT/$soname"
  [[ -f "$source" ]] || { echo "Resolved runtime object is missing: $source" >&2; exit 1; }
  if [[ -e "$target" ]]; then
    cmp -s -- "$source" "$target" || {
      echo "Runtime closure contains conflicting objects named $soname" >&2
      exit 1
    }
  else
    cp -L --preserve=mode,timestamps -- "$source" "$target"
  fi
  if [[ -z "${queued[$soname]+present}" ]]; then
    queued[$soname]=1
    queue+=("$target")
  fi
}

enqueue libmpv.so.2 "$LIBMPV"

for ((index = 0; index < ${#queue[@]}; index++)); do
  object="${queue[$index]}"
  while IFS='|' read -r soname source; do
    [[ -n "$soname" && -n "$source" ]] || continue
    is_platform_runtime "$soname" && continue
    enqueue "$soname" "$source"
  done < <(
    ldd "$object" | awk '
      $2 == "=>" && $3 ~ /^\// { print $1 "|" $3 }
      $1 ~ /^\// { name = $1; sub(/^.*\//, "", name); print name "|" $1 }
    '
  )
done

for object in "$OUTPUT"/*.so*; do
  readelf -h "$object" >/dev/null 2>&1 || continue
  patchelf --set-rpath '$ORIGIN' "$object"
done

(
  cd "$OUTPUT"
  sha256sum -- *.so* | sort -k2
) >"$OUTPUT/bundled-runtime.sha256"

printf '%s\n' "$OUTPUT"
