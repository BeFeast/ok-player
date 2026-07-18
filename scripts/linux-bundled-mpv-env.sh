#!/usr/bin/env bash
# Source this file, then call okp_use_linux_bundled_mpv to configure a build.

okp_use_linux_bundled_mpv() {
  local root prefix
  root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
  prefix="$("$root/scripts/prepare-linux-bundled-mpv.sh")" || return

  local pkg_configs=() libraries=() candidate
  for candidate in \
    "$prefix"/lib/pkgconfig/mpv.pc \
    "$prefix"/lib/*/pkgconfig/mpv.pc \
    "$prefix"/lib64/pkgconfig/mpv.pc; do
    [[ -e "$candidate" ]] && pkg_configs+=("$candidate")
  done
  for candidate in \
    "$prefix"/lib/libmpv.so.2 \
    "$prefix"/lib/*/libmpv.so.2 \
    "$prefix"/lib64/libmpv.so.2; do
    [[ -e "$candidate" ]] && libraries+=("$candidate")
  done

  (( ${#pkg_configs[@]} == 1 )) || {
    echo "Expected exactly one bundled mpv.pc under $prefix" >&2
    return 1
  }
  (( ${#libraries[@]} == 1 )) || {
    echo "Expected exactly one bundled libmpv.so.2 under $prefix" >&2
    return 1
  }

  export OKP_BUNDLED_MPV_PREFIX="$prefix"
  OKP_BUNDLED_MPV_LIB_DIR="$(dirname -- "${libraries[0]}")"
  OKP_BUNDLED_MPV_LIBRARY="$(readlink -f -- "${libraries[0]}")"
  PKG_CONFIG_PATH="$(dirname -- "${pkg_configs[0]}")${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  export OKP_BUNDLED_MPV_LIB_DIR OKP_BUNDLED_MPV_LIBRARY PKG_CONFIG_PATH
  export LD_LIBRARY_PATH="$OKP_BUNDLED_MPV_LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

  if [[ "${1:-}" == "package" ]]; then
    export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-Wl,-rpath,\$ORIGIN"
  fi
}
