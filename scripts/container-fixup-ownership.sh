#!/usr/bin/env bash
# Runs INSIDE the packaging container, after the build, to hand the outputs
# back to the host user.
#
# The right decision is observable from the bind mount itself, independent of
# which runtime (docker/podman, rootful/rootless) is driving:
#   - rootful runtime: the mounted workspace keeps its host owner (non-zero
#     uid) while we run as real root -> chown the outputs to that owner.
#   - rootless runtime: the invoking host user is mapped to in-container root,
#     so the workspace appears owned by uid 0 and the outputs are already
#     correctly owned on the host. A chown to any other uid would remap them
#     onto a subordinate uid the host user cannot write (the mktemp
#     Permission denied that killed the first container candidate).
set -euo pipefail
WORKSPACE="${1:?usage: container-fixup-ownership.sh <workspace> <path>...}"
shift
(( $# > 0 )) || { echo "container-fixup-ownership: no output paths given" >&2; exit 2; }

owner="$(stat -c %u -- "$WORKSPACE")"
group="$(stat -c %g -- "$WORKSPACE")"

if [[ "$owner" == 0 ]]; then
  echo "container-fixup-ownership: workspace appears root-owned (rootless mapping) - outputs already belong to the host user, skipping chown"
  exit 0
fi

chown -R "$owner:$group" -- "$@"
echo "container-fixup-ownership: outputs chowned to $owner:$group (rootful runtime)"
