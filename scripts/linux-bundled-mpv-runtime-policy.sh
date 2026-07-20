#!/usr/bin/env bash
# Shared policy for libraries that must come from the target desktop stack.

okp_is_linux_glibc_runtime() {
  case "$1" in
    ld*.so.* | libc.so.* | libc_malloc_debug.so.* | libBrokenLocale.so.* | \
      libSegFault.so | libanl.so.* | libcidn.so.* | libdl.so.* | libm.so.* | \
      libmemusage.so | libmvec.so.* | \
      libnsl.so.* | libnss_*.so.* | libpcprofile.so | libpthread.so.* | \
      libresolv.so.* | librt.so.* | libthread_db.so.* | libutil.so.*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

okp_is_linux_platform_runtime() {
  okp_is_linux_glibc_runtime "$1" && return 0
  case "$1" in
    libcrypt.so.* | libgcc_s.so.* | libGL.so.* | libEGL.so.* | libGLX.so.* | \
      libGLdispatch.so.* | libOpenGL.so.* | libdrm.so.* | libgbm.so.* | \
      libvulkan.so.* | libglib-2.0.so.* | libgobject-2.0.so.* | \
      libgio-2.0.so.* | libgmodule-2.0.so.* | libgthread-2.0.so.* | \
      libX*.so.* | libxcb*.so.* | libwayland-*.so.* | libcairo*.so.* | \
      libpango*.so.* | libfontconfig.so.* | libfreetype.so.* | \
      libharfbuzz.so.* | libgraphite2.so.* | libfribidi.so.* | \
      libmount.so.* | libblkid.so.* | libselinux.so.* | libpcre2-*.so.* | \
      libffi.so.* | libdbus-1.so.* | libsystemd.so.* | libudev.so.* | \
      libasound*.so* | libxkbcommon*.so.* | libdecor-*.so.* | libepoxy.so.* | \
      libgtk-*.so.* | libgdk*.so.* | libadwaita-*.so.*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

okp_verify_linux_bundled_runtime_manifest() {
  local manifest="$1" checksum file_name extra soname
  [[ -f "$manifest" ]] || {
    echo "Bundled runtime manifest is missing: $manifest" >&2
    return 1
  }

  while read -r checksum file_name extra; do
    [[ "$checksum" =~ ^[0-9a-f]{64}$ && -n "$file_name" && -z "${extra:-}" ]] || {
      echo "Bundled runtime manifest contains a malformed entry" >&2
      return 1
    }
    file_name="${file_name#\*}"
    soname="${file_name##*/}"
    if okp_is_linux_platform_runtime "$soname"; then
      echo "Bundled runtime manifest shadows target platform library: $soname" >&2
      return 1
    fi
  done <"$manifest"
}

okp_verify_no_linux_glibc_runtime_files() {
  local root="$1" object soname failures=0
  [[ -d "$root" ]] || {
    echo "Bundled runtime directory is missing: $root" >&2
    return 1
  }

  shopt -s nullglob globstar
  for object in "$root"/**; do
    [[ -f "$object" || -L "$object" ]] || continue
    soname="${object##*/}"
    if okp_is_linux_glibc_runtime "$soname"; then
      echo "Bundled runtime contains target glibc component: $soname" >&2
      failures=1
    fi
  done
  (( failures == 0 ))
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  okp_verify_linux_bundled_runtime_manifest \
    "${1:?usage: linux-bundled-mpv-runtime-policy.sh <bundled-runtime.sha256>}"
  echo "Bundled runtime manifest contains media-only dependencies."
fi
