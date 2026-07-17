#!/usr/bin/env bash
# Thin CLI wrapper for the rolling Linux candidate manifest (issue #339).
# Schema, identity, monotonicity, history, and retention decisions live in
# okp-core::candidate_build; this script only resolves the repository CLI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ -n "${OKP_CANDIDATE_CLI:-}" ]]; then
  exec "$OKP_CANDIDATE_CLI" feed "$@"
fi

exec env CC=/usr/bin/cc cargo run --quiet \
  --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-candidate -- feed "$@"
