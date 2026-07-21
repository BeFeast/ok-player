#!/usr/bin/env bash
# Copy libmpv's non-platform runtime closure into one origin-relative directory.
set -euo pipefail

# candidate-required-tools: awk basename cmp cp ldd mkdir patchelf readelf rm sha256sum sort

LIBMPV="${1:?usage: collect-linux-bundled-mpv-runtime.sh <libmpv.so.2> <output-dir>}"
OUTPUT="${2:?usage: collect-linux-bundled-mpv-runtime.sh <libmpv.so.2> <output-dir>}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

source "$SCRIPT_DIR/linux-bundled-mpv-runtime-policy.sh"

for tool in awk basename cmp cp ldd mkdir patchelf readelf rm sha256sum sort; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$LIBMPV" ]] || { echo "Bundled libmpv is missing: $LIBMPV" >&2; exit 1; }

# The target desktop owns libc, graphics, GTK, X11/Wayland, Cairo/Pango, font,
# audio, image-codec, and session libraries. JPEG is the exception because mpv
# links it directly and Debian/Ubuntu expose incompatible SONAMEs. The collector
# carries that exact builder ABI under a private OK Player SONAME so it cannot
# shadow the JPEG ABI used by target TIFF/GDK modules.

rm -rf "$OUTPUT"
mkdir -p "$OUTPUT"

# okp_use_linux_bundled_mpv exports this directory for later compiler and
# runtime invocations. A repeated collection must not let host tools resolve
# against the directory they are rebuilding; patchelf is linked to libstdc++
# and can otherwise load the bundled copy before rewriting that same file.
if [[ -v LD_LIBRARY_PATH ]]; then
  declare -a search_paths=() host_search_paths=()
  IFS=: read -r -a search_paths <<<"$LD_LIBRARY_PATH"
  for search_path in "${search_paths[@]}"; do
    [[ "$search_path" == "$OUTPUT" ]] || host_search_paths+=("$search_path")
  done
  if (( ${#host_search_paths[@]} > 0 )); then
    LD_LIBRARY_PATH="$(IFS=:; printf '%s' "${host_search_paths[*]}")"
    export LD_LIBRARY_PATH
  else
    unset LD_LIBRARY_PATH
  fi
fi

declare -a queue=()
declare -A queued=()
declare -A namespaced_sources=()

enqueue() {
  local soname="$1" source="$2" target
  if okp_is_linux_platform_runtime "$soname"; then
    echo "Refusing to queue target platform library in bundled runtime: $soname" >&2
    exit 1
  fi
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
    if okp_is_linux_namespaced_media_source "$soname"; then
      private_soname="$(okp_linux_namespaced_media_soname "$soname")"
      enqueue "$private_soname" "$source"
      namespaced_sources["$private_soname"]="$soname"
      patchelf --replace-needed "$soname" "$private_soname" "$object"
      continue
    fi
    okp_is_linux_platform_runtime "$soname" && continue
    enqueue "$soname" "$source"
  done < <(
    ldd "$object" | awk '
      $2 == "=>" && $3 ~ /^\// { print $1 "|" $3 }
      $1 ~ /^\// { name = $1; sub(/^.*\//, "", name); print name "|" $1 }
    '
  )
done

for private_soname in "${!namespaced_sources[@]}"; do
  printf 'Setting bundled runtime private SONAME: %s\n' "$private_soname" >&2
  patchelf --set-soname "$private_soname" "$OUTPUT/$private_soname"
done

for object in "$OUTPUT"/*.so*; do
  readelf -h "$object" >/dev/null 2>&1 || continue
  printf 'Setting bundled runtime rpath: %s\n' "$object" >&2
  patchelf --set-rpath '$ORIGIN' "$object"
done

(
  cd "$OUTPUT"
  sha256sum -- *.so* | sort -k2
) >"$OUTPUT/bundled-runtime.sha256"

okp_verify_linux_bundled_runtime_manifest "$OUTPUT/bundled-runtime.sha256"
okp_verify_no_linux_glibc_runtime_files "$OUTPUT"

printf '%s\n' "$OUTPUT"
