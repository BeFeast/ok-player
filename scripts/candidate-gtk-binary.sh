#!/usr/bin/env bash
# Resolve the okp-linux-gtk binary the candidate launch smokes must exercise.
#
# Container packaging (the default since #662) builds with
# CARGO_TARGET_DIR=rust/target/portable, so on a clean candidate checkout the
# only GTK binary is rust/target/portable/release/okp-linux-gtk - and it is
# also the closer artifact to the shipped payload (built against the pinned
# Debian-13 floor). A native build lands in rust/target/release. Prefer the
# portable build, fall back to native, and fail loudly when neither exists so
# the smoke aborts with a cause instead of a missing-file stack.
set -euo pipefail
CHECKOUT="${1:?usage: candidate-gtk-binary.sh <checkout-root>}"

for candidate in \
  "$CHECKOUT/rust/target/portable/release/okp-linux-gtk" \
  "$CHECKOUT/rust/target/release/okp-linux-gtk"; do
  if [[ -x "$candidate" ]]; then
    printf '%s\n' "$candidate"
    exit 0
  fi
done

echo "candidate-gtk-binary: no okp-linux-gtk under $CHECKOUT/rust/target" \
  "(neither portable/release nor release) - run the packaging gates first" >&2
exit 1
