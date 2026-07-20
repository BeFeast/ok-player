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
      libasound*.so* | libjpeg*.so* | libturbojpeg*.so* | libtiff*.so* | \
      libwebp*.so* | libpng*.so* | libxkbcommon*.so.* | libdecor-*.so.* | \
      libepoxy.so.* | \
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

# Run one UI smoke in a disposable output directory. Exit 75 is reserved for
# D-Bus/X/Xfwm session infrastructure, so only that status receives one retry;
# product assertions and every other command failure return immediately.
okp_run_linux_smoke_with_infra_retry() {
  local label="${1:?okp_run_linux_smoke_with_infra_retry requires a label}"
  local output_dir="${2:?okp_run_linux_smoke_with_infra_retry requires an output directory}"
  local evidence_root="${3:?okp_run_linux_smoke_with_infra_retry requires an evidence root}"
  shift 3
  (( $# > 0 )) || {
    echo "okp_run_linux_smoke_with_infra_retry requires a command" >&2
    return 2
  }

  local infra_exit_code="${OKP_SESSION_INFRA_EXIT_CODE:-75}"
  local attempt attempt_dir evidence_dir status
  if [[ ! "$infra_exit_code" =~ ^[1-9][0-9]{0,2}$ ]] || (( infra_exit_code > 255 )); then
    echo "OKP_SESSION_INFRA_EXIT_CODE must be an integer from 1 through 255" >&2
    return 2
  fi

  for attempt in 1 2; do
    attempt_dir="${output_dir}-attempt-${attempt}"
    rm -rf -- "$attempt_dir"
    mkdir -p -- "$attempt_dir"

    set +e
    OKP_SMOKE_OUTPUT_DIR="$attempt_dir" OKP_SMOKE_ATTEMPT="$attempt" "$@"
    status=$?
    set -e

    if (( status == 0 )); then
      rm -rf -- "$output_dir"
      mv -- "$attempt_dir" "$output_dir"
      return 0
    fi

    evidence_dir="$evidence_root/$label/attempt-$attempt"
    rm -rf -- "$evidence_dir"
    mkdir -p -- "$evidence_dir"
    cp -a -- "$attempt_dir"/. "$evidence_dir"/
    printf 'label=%s\nattempt=%s\nexit_status=%s\n' "$label" "$attempt" "$status" \
      >"$evidence_dir/retry-evidence.txt"

    if (( status != infra_exit_code )); then
      printf 'failure_kind=command\nretried=false\n' >>"$evidence_dir/retry-evidence.txt"
      return "$status"
    fi

    printf 'failure_kind=session-infra\n' >>"$evidence_dir/retry-evidence.txt"
    if (( attempt == 1 )); then
      printf 'retried=true\n' >>"$evidence_dir/retry-evidence.txt"
      echo "Retrying $label once after isolated session infrastructure failed" >&2
      continue
    fi
    printf 'retried=false\n' >>"$evidence_dir/retry-evidence.txt"
    return "$status"
  done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  okp_verify_linux_bundled_runtime_manifest \
    "${1:?usage: linux-bundled-mpv-runtime-policy.sh <bundled-runtime.sha256>}"
  echo "Bundled runtime manifest contains media-only dependencies."
fi
