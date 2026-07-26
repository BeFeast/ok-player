#!/usr/bin/env bash
# Re-pin the Flatpak application source and regenerate the integration patch.
#
# The manifest ships a Flathub-shaped git source: a permanent upstream commit
# plus a checked-in patch that carries the Flatpak integration files which are
# not on that commit yet. Both halves are frozen inputs, so they never track the
# working tree automatically. This script is the only supported way to move
# them, and CI runs it on a schedule to open a re-pin pull request instead of
# letting the pair rot.
#
# Usage: scripts/flatpak-repin.sh [<commit-ish>]   (default: origin/main)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
PATCH="$ROOT/rust/packaging/flatpak/ok-player-flatpak.patch"
PATCHED_PATHS="$ROOT/rust/packaging/flatpak/patched-paths.txt"
TARGET="${1:-origin/main}"

for tool in git python3 sed; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

pin="$(git -C "$ROOT" rev-parse --verify "${TARGET}^{commit}")"

if ! git -C "$ROOT" merge-base --is-ancestor "$pin" HEAD; then
  echo "Refusing to pin $pin: it is not an ancestor of HEAD, so the patch would revert work" >&2
  exit 2
fi

# The manifest clones the public GitHub URL. A pin that only exists locally
# would make the offline build unreproducible for everyone else.
if [[ "${OKP_FLATPAK_ALLOW_UNPUBLISHED_PIN:-0}" != "1" ]]; then
  if ! git -C "$ROOT" merge-base --is-ancestor "$pin" origin/main; then
    echo "Refusing to pin $pin: it is not published on origin/main" >&2
    exit 2
  fi
fi

mapfile -t paths < <(sed -e 's/#.*//' -e 's/[[:space:]]*$//' "$PATCHED_PATHS" | grep -v '^$')
[[ "${#paths[@]}" -gt 0 ]] || {
  echo "No patched paths declared in $PATCHED_PATHS" >&2
  exit 2
}

git -C "$ROOT" diff --full-index --binary --no-ext-diff "$pin" -- "${paths[@]}" >"$PATCH"
# Git emits a bare space for empty context lines; several patch consumers strip
# trailing whitespace in transit, so normalise it here once and for all.
sed -i 's/^ $//' "$PATCH"

if [[ -s "$PATCH" ]]; then
  patch_state=present
else
  rm -f "$PATCH"
  patch_state=absent
fi

python3 - "$MANIFEST" "$pin" "$patch_state" "$(basename "$PATCH")" <<'PY'
import json
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
pin = sys.argv[2]
patch_state = sys.argv[3]
patch_name = sys.argv[4]

manifest = json.loads(manifest_path.read_text())
app = manifest["modules"][0]
sources = app["sources"]
if not sources or sources[0].get("type") != "git":
    raise SystemExit("manifest module 0 must start with the git application source")
sources[0]["commit"] = pin
patch_source = {"type": "patch", "path": patch_name}
sources = [source for source in sources if source != patch_source]
if patch_state == "present":
    sources.insert(1, patch_source)
app["sources"] = sources
manifest_path.write_text(json.dumps(manifest, indent=4) + "\n")
PY

echo "Flatpak application source pinned to $pin (integration patch $patch_state)"
