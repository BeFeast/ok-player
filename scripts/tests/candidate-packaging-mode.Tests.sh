#!/usr/bin/env bash
# Behavioural coverage for the #662 packaging-mode boundary.
#
# 1. required portability mode must die loudly, before any artifact analysis,
#    when no usable container runtime exists - the silent degradation to
#    native-equivalence is how a glibc-2.43 bundle reached the public feed.
# 2. OKP_PORTABLE_PACKAGE_MODE=container must actually drive the packaging
#    script into the container runtime (verified with a fake runtime that
#    records its invocation), so the mode cannot silently fall back native.
# 3. The candidate lane must stay wired to container packaging and required
#    portability - reverting either is the exact regression of #662.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

# --- 1: required mode fails fast without a usable runtime -------------------
mkdir -p "$tmp/bin"
printf '#!/bin/sh\nexit 1\n' >"$tmp/bin/docker"    # present but unusable
printf '#!/bin/sh\nexit 1\n' >"$tmp/bin/podman"
chmod +x "$tmp/bin/docker" "$tmp/bin/podman"
touch "$tmp/fake.deb" "$tmp/fake.AppImage"

set +e
out="$(PATH="$tmp/bin:$PATH" OKP_PORTABILITY_CONTAINER_MODE=required \
  "$ROOT/scripts/verify-linux-package-portability.sh" \
  "$tmp/fake.deb" "$tmp/fake.AppImage" "$tmp/report.json" \
  0123456789012345678901234567890123456789 2>&1)"
rc=$?
set -e
[ "$rc" -eq 127 ] || fail "required mode without a runtime exited $rc, want 127: $out"
grep -q "requires a usable docker or podman runtime" <<<"$out" \
  || fail "required-mode failure does not name the cause: $out"
grep -q "dpkg-deb" <<<"$out" && fail "artifact analysis ran before the required-runtime check: $out"

# --- 2: container packaging mode reaches the container runtime --------------
cat >"$tmp/bin/docker" <<RECORDER
#!/bin/sh
echo "\$@" >>"$tmp/docker-invocations.log"
case "\$1" in
  info) exit 0 ;;
  build) exit 42 ;;   # prove the failure propagates instead of falling back
  *) exit 42 ;;
esac
RECORDER
chmod +x "$tmp/bin/docker"
rm -f "$tmp/bin/podman"

set +e
PATH="$tmp/bin:$PATH" OKP_PORTABLE_PACKAGE_MODE=container \
  "$ROOT/scripts/build-linux-portable-package.sh" deb 0.0.0-test.1 \
  >"$tmp/pack.log" 2>&1
rc=$?
set -e
[ "$rc" -ne 0 ] || fail "container packaging succeeded against a runtime whose build fails"
grep -q "^build " "$tmp/docker-invocations.log" 2>/dev/null \
  || fail "container mode never invoked the container runtime's build: $(cat "$tmp/pack.log" | tail -3)"
grep -q "ok-player-linux-builder" "$tmp/docker-invocations.log" \
  || fail "container build did not target the pinned builder image"

# --- 3: the candidate lane stays wired to the safe modes --------------------
lane="$ROOT/scripts/build-linux-candidate.sh"
c="$(grep -c "OKP_PORTABLE_PACKAGE_MODE=container" "$lane" || true)"
[ "$c" -eq 2 ] || fail "candidate lane must package both lanes in container mode (found $c of 2)"
grep -q "OKP_PORTABILITY_CONTAINER_MODE=required" "$lane" \
  || fail "candidate lane portability gate is no longer required-mode"
grep -q "OKP_PORTABLE_PACKAGE_MODE=native" "$lane" \
  && fail "candidate lane regressed to native packaging (#662)"

echo "ok: required portability fails fast, container mode drives the runtime, lane wiring pinned"
