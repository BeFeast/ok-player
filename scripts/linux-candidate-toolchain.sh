#!/usr/bin/env bash
# Preflight and command contract for the pinned Linux candidate mpv toolchain.

set -euo pipefail

OKP_CANDIDATE_TOOLCHAIN_SCRIPT_DIR="$(cd -- "${BASH_SOURCE[0]%/*}" && pwd)"
OKP_CANDIDATE_TOOLCHAIN_ROOT="$(cd -- "$OKP_CANDIDATE_TOOLCHAIN_SCRIPT_DIR/.." && pwd)"
OKP_CANDIDATE_TOOLCHAIN_MANIFEST="${OKP_CANDIDATE_TOOLCHAIN_MANIFEST:-$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/linux-candidate-toolchain.manifest}"
OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT="${OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT:-$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/build-local-mpv.sh}"
OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="${OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS-$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/package-linux-deb.sh
$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/package-linux-velopack.sh
$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/collect-linux-bundled-mpv-runtime.sh
$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/verify-linux-bundled-mpv.sh
$OKP_CANDIDATE_TOOLCHAIN_ROOT/scripts/verify-linux-package-portability.sh}"

okp_candidate_manifest_rows() {
  local kind name probe package extra
  [[ -r "$OKP_CANDIDATE_TOOLCHAIN_MANIFEST" ]] || {
    echo "candidate toolchain manifest is unreadable: $OKP_CANDIDATE_TOOLCHAIN_MANIFEST" >&2
    return 2
  }
  while IFS='|' read -r kind name probe package extra; do
    [[ -z "$kind" || "$kind" == \#* ]] && continue
    if [[ -n "${extra:-}" || -z "$name" || -z "$probe" || -z "$package" ]] \
        || [[ "$kind" != command && "$kind" != command-or-dotnet-tool && "$kind" != pkg-config ]]; then
      echo "candidate toolchain manifest has an invalid row for ${name:-unknown}" >&2
      return 2
    fi
    printf '%s|%s|%s|%s\n' "$kind" "$name" "$probe" "$package"
  done <"$OKP_CANDIDATE_TOOLCHAIN_MANIFEST"
}

okp_candidate_tool_is_declared() {
  local requested="$1" rows kind name probe package
  rows="$(okp_candidate_manifest_rows)" || return
  while IFS='|' read -r kind name probe package; do
    [[ "$kind" == command* && "$name" == "$requested" ]] && return 0
  done <<<"$rows"
  return 1
}

okp_candidate_tool() {
  local requested="${1:?tool name is required}"
  shift
  okp_candidate_tool_is_declared "$requested" || {
    echo "candidate build tool is not declared in linux-candidate-toolchain.manifest: $requested" >&2
    return 2
  }
  if [[ "$requested" == cc ]]; then
    "${CC:-cc}" "$@"
  else
    command "$requested" "$@"
  fi
}

okp_candidate_verify_tool_references() {
  local script="${1:?build script path is required}"
  local line remainder referenced
  [[ -r "$script" ]] || {
    echo "candidate build script is unreadable: $script" >&2
    return 2
  }
  while IFS= read -r line; do
    remainder="$line"
    while [[ "$remainder" =~ okp_candidate_tool[[:space:]]+([[:alnum:]_.+-]+) ]]; do
      referenced="${BASH_REMATCH[1]}"
      okp_candidate_tool_is_declared "$referenced" || {
        echo "candidate build tool is not declared in linux-candidate-toolchain.manifest: $referenced" >&2
        return 1
      }
      remainder="${remainder#*"${BASH_REMATCH[0]}"}"
    done
  done <"$script"
}

okp_candidate_verify_gate_script_requirements() {
  local script="${1:?gate script path is required}"
  local line requirements referenced found=0
  [[ -r "$script" ]] || {
    echo "candidate gate script is unreadable: $script" >&2
    return 2
  }
  while IFS= read -r line; do
    if [[ "$line" =~ ^#[[:space:]]candidate-required-tools:[[:space:]](.*)$ ]]; then
      found=1
      requirements="${BASH_REMATCH[1]}"
      for referenced in $requirements; do
        [[ "$referenced" =~ ^[[:alnum:]_.+-]+$ ]] || {
          echo "candidate gate script has an invalid tool requirement: $referenced" >&2
          return 2
        }
        okp_candidate_tool_is_declared "$referenced" || {
          echo "candidate gate tool is not declared in linux-candidate-toolchain.manifest: $referenced" >&2
          return 1
        }
      done
    fi
  done <"$script"
  (( found == 1 )) || {
    echo "candidate gate script has no candidate-required-tools declaration: $script" >&2
    return 2
  }
}

okp_candidate_toolchain_preflight() {
  local -a missing=()
  local rows kind name probe package script resolved_pkg_config=""
  rows="$(okp_candidate_manifest_rows)" || return
  okp_candidate_verify_tool_references "$OKP_CANDIDATE_TOOLCHAIN_BUILD_SCRIPT" || return
  while IFS= read -r script; do
    [[ -n "$script" ]] || continue
    okp_candidate_verify_gate_script_requirements "$script" || return
  done <<<"$OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS"
  while IFS='|' read -r kind name probe package; do
    if [[ "$kind" == command ]]; then
      if [[ "$name" == cc ]]; then
        command -v "${CC:-cc}" >/dev/null 2>&1 || missing+=("$name [$package]")
      elif ! command -v "$probe" >/dev/null 2>&1; then
        missing+=("$name [$package]")
      fi
      [[ "$name" != pkg-config ]] || resolved_pkg_config="$(command -v "$probe" 2>/dev/null || true)"
    elif [[ "$kind" == command-or-dotnet-tool ]]; then
      if [[ "${OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS:-true}" == true ]] \
          && ! command -v "$probe" >/dev/null 2>&1 \
          && [[ ! -x "$HOME/.dotnet/tools/$probe" ]]; then
        missing+=("$name [$package]")
      fi
    elif [[ -n "$resolved_pkg_config" ]] && ! "$resolved_pkg_config" --exists "$probe"; then
      missing+=("pkg-config:$name [$package]")
    fi
  done <<<"$rows"

  if (( ${#missing[@]} > 0 )); then
    local joined
    printf -v joined '%s, ' "${missing[@]}"
    echo "candidate build failed at gate bundled-mpv; missing dependencies: ${joined%, }" >&2
    return 1
  fi
}

okp_candidate_portable_packages() {
  local include_dotnet_tools="${1:-true}"
  local rows kind name probe package
  local -A seen=()
  rows="$(okp_candidate_manifest_rows)" || return
  while IFS='|' read -r kind name probe package; do
    if [[ "$include_dotnet_tools" != true && "$kind" == command-or-dotnet-tool ]]; then
      continue
    fi
    if [[ -z "${seen[$package]+present}" ]]; then
      seen[$package]=1
      printf '%s\n' "$package"
    fi
  done <<<"$rows"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  case "${1:-}" in
    "")
      okp_candidate_toolchain_preflight
      ;;
    --print-ubuntu-packages)
      okp_candidate_portable_packages
      ;;
    --print-portable-debian-packages)
      okp_candidate_portable_packages false
      ;;
    --check-build-script)
      [[ $# -eq 2 ]] || {
        echo "usage: $0 --check-build-script PATH" >&2
        exit 2
      }
      okp_candidate_verify_tool_references "$2"
      ;;
    --check-gate-script)
      [[ $# -eq 2 ]] || {
        echo "usage: $0 --check-gate-script PATH" >&2
        exit 2
      }
      okp_candidate_verify_gate_script_requirements "$2"
      ;;
    *)
      echo "usage: $0 [--print-ubuntu-packages | --print-portable-debian-packages | --check-build-script PATH | --check-gate-script PATH]" >&2
      exit 2
      ;;
  esac
fi
