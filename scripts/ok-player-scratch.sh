#!/usr/bin/env bash
# Shared scratch naming contract for packaging, smoke, and worker sessions.

okp_validate_scratch_session() {
  local session="$1"
  [[ "$session" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || {
    echo "OKP_SCRATCH_SESSION must contain only letters, digits, dot, underscore, or hyphen" >&2
    return 2
  }
}

okp_make_scratch_dir() {
  local component="${1:?okp_make_scratch_dir requires a component name}"
  local parent="${2:-${OKP_SCRATCH_ROOT:-${TMPDIR:-/tmp}}}"
  [[ "$component" =~ ^[a-z0-9][a-z0-9-]*$ ]] || {
    echo "scratch component must contain only lowercase letters, digits, or hyphens" >&2
    return 2
  }

  local prefix="ok-player-${component}"
  if [[ -n "${OKP_SCRATCH_SESSION:-}" ]]; then
    okp_validate_scratch_session "$OKP_SCRATCH_SESSION" || return
    prefix="ok-player-${OKP_SCRATCH_SESSION}-${component}"
  fi
  mkdir -p -- "$parent"
  parent="$(cd -- "$parent" && pwd -P)"
  mktemp -d --tmpdir="$parent" "${prefix}.XXXXXX"
}
