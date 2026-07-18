#!/usr/bin/env bash
# Thin entrypoint for the tested release-preparation logic in okp-core.
# Every command is non-mutating with respect to GitHub Releases, tags, workflows,
# and update feeds; generated JSON is written only to the requested local path.

set -euo pipefail

repo_root="$(cd -- "$(dirname -- "$0")/.." && pwd)"
export CC="${CC:-/usr/bin/cc}"
exec cargo run --quiet --manifest-path "$repo_root/rust/Cargo.toml" \
  -p okp-core --bin okp-release-preparation -- "$@"
