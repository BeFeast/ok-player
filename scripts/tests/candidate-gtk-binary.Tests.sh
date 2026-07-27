#!/usr/bin/env bash
# Behavioural coverage for candidate-gtk-binary.sh: the launch smokes must find
# the container-built binary on a clean candidate checkout (#662 follow-up),
# prefer it over a stale native build, and fail loudly when nothing was built.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RESOLVER="$ROOT/scripts/candidate-gtk-binary.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fail() { echo "FAIL: $1" >&2; exit 1; }

# Clean container-packaged checkout: only the portable build exists. This is
# the exact shape of a candidate runner checkout after the packaging gates -
# resolving anything else (or nothing) would abort every candidate build.
mkdir -p "$tmp/a/rust/target/portable/release"
printf '#!/bin/sh\n' >"$tmp/a/rust/target/portable/release/okp-linux-gtk"
chmod +x "$tmp/a/rust/target/portable/release/okp-linux-gtk"
got="$("$RESOLVER" "$tmp/a")"
[[ "$got" == "$tmp/a/rust/target/portable/release/okp-linux-gtk" ]] \
  || fail "clean container checkout resolved '$got'"

# Both builds present: the portable (shipped-floor) build must win over a
# stale native one.
mkdir -p "$tmp/a/rust/target/release"
printf '#!/bin/sh\n' >"$tmp/a/rust/target/release/okp-linux-gtk"
chmod +x "$tmp/a/rust/target/release/okp-linux-gtk"
got="$("$RESOLVER" "$tmp/a")"
[[ "$got" == "$tmp/a/rust/target/portable/release/okp-linux-gtk" ]] \
  || fail "portable build not preferred, resolved '$got'"

# Native-only checkout still resolves (local developer flow).
mkdir -p "$tmp/b/rust/target/release"
printf '#!/bin/sh\n' >"$tmp/b/rust/target/release/okp-linux-gtk"
chmod +x "$tmp/b/rust/target/release/okp-linux-gtk"
got="$("$RESOLVER" "$tmp/b")"
[[ "$got" == "$tmp/b/rust/target/release/okp-linux-gtk" ]] \
  || fail "native-only checkout resolved '$got'"

# Nothing built: loud failure, non-zero exit, cause on stderr.
mkdir -p "$tmp/c/rust/target"
if out="$("$RESOLVER" "$tmp/c" 2>&1)"; then
  fail "resolver succeeded on an empty checkout"
fi
grep -q "run the packaging gates first" <<<"$out" \
  || fail "missing-binary error does not name the cause: $out"

# A present but non-executable file must not be resolved.
mkdir -p "$tmp/d/rust/target/portable/release"
printf '#!/bin/sh\n' >"$tmp/d/rust/target/portable/release/okp-linux-gtk"
if "$RESOLVER" "$tmp/d" >/dev/null 2>&1; then
  fail "resolver accepted a non-executable binary"
fi

echo "ok: candidate GTK binary resolution covers container, native, empty, and non-executable checkouts"
