#!/usr/bin/env bash
# Behavioural coverage for the post-build ownership handoff (#668 follow-up).
# The old unconditional chown remapped rootless-podman outputs onto a
# subordinate uid; the fix decides by observing the bind-mount owner. Both
# branches are executed here without any container runtime:
#   - the rootless-mapped case runs inside `unshare -Ur` (current user mapped
#     to uid 0), where the workspace appears root-owned and the script must
#     NOT chown;
#   - the rootful case runs unprivileged, where the workspace owner is the
#     invoking uid and the chown to that owner must run and succeed.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/container-fixup-ownership.sh"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $1" >&2; exit 1; }

mkdir -p "$tmp/ws/out"
touch "$tmp/ws/out/artifact"

# --- rootless-mapped branch: workspace owner appears as uid 0 ---------------
if ! command -v unshare >/dev/null 2>&1 || ! unshare -Ur true 2>/dev/null; then
  fail "unshare -Ur unavailable - cannot exercise the rootless branch"
fi
out="$(unshare -Ur bash -c '"$1" "$2/ws" "$2/ws/out"' _ "$SCRIPT" "$tmp" 2>&1)" \
  || fail "rootless branch exited non-zero: $out"
grep -q "skipping chown" <<<"$out" || fail "rootless branch did not skip the chown: $out"
[ -O "$tmp/ws/out/artifact" ] || fail "rootless branch changed ownership away from the invoking user"

# The regression direction: the OLD behaviour (unconditional chown to a passed
# host uid) breaks exactly here - prove the environment would catch it.
bad="$(unshare -Ur bash -c 'chown -R 1000:1000 "$1/ws/out" 2>&1 && stat -c %u "$1/ws/out/artifact"' _ "$tmp")"
if [ "$bad" = "1000" ]; then
  [ -O "$tmp/ws/out/artifact" ] && fail "environment cannot represent the subuid remap; test is not probative"
  # artifact now owned by a mapped uid the real user does not own - restore
  unshare -Ur chown -R 0:0 "$tmp/ws/out"
fi
[ -O "$tmp/ws/out/artifact" ] || fail "could not restore ownership after the negative control"

# --- rootful branch: workspace owner is a real non-zero uid ------------------
out="$("$SCRIPT" "$tmp/ws" "$tmp/ws/out" 2>&1)" || fail "rootful branch exited non-zero: $out"
grep -q "chowned to $(id -u):$(id -g)" <<<"$out" || fail "rootful branch did not chown to the workspace owner: $out"
[ -O "$tmp/ws/out/artifact" ] || fail "rootful branch left wrong ownership"

# --- guard rails -------------------------------------------------------------
"$SCRIPT" "$tmp/ws" >/dev/null 2>&1 && fail "script accepted a call without output paths"

echo "ok: ownership handoff skips under rootless mapping, chowns under rootful, negative control proves the old behaviour breaks"
