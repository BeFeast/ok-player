#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT/rust"
CC="${CC:-/usr/bin/cc}" cargo test -p okp-linux-gtk native_file_dialog_result -- --nocapture

echo "Native file dialog result smoke passed."
