#!/usr/bin/env bash
# Stage the licence documents every user-facing artifact has to carry.
#
# OK Player is GPL-3.0-or-later and its shipped chrome icons are Adwaita
# artwork taken under the LGPL-3 option, so GPLv3 §4 and LGPLv3 §4(b) both
# apply to every package handed to a user: the combined work must be
# accompanied by the GPL *and* the LGPL license documents. This script is the
# single place the Linux archive lanes get them from, so a lane cannot drift
# out of compliance by editing its own copy of the list.
#
# Usage: stage-license-documents.sh <deb|appimage> <doc-dir>
#
# <doc-dir> is the directory the documents land in - conventionally
# `<root>/usr/share/doc/ok-player` for both lanes. The Debian lane also gets a
# DEP-5 `copyright`, which Debian policy §12.5 requires and which no other
# ecosystem expects.
set -euo pipefail

LANE="${1:?usage: stage-license-documents.sh <deb|appimage> <doc-dir>}"
DOC_DIR="${2:?usage: stage-license-documents.sh <deb|appimage> <doc-dir>}"
case "$LANE" in
  deb | appimage) ;;
  *) echo "Unknown licence staging lane: $LANE" >&2; exit 2 ;;
esac

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

# The project's own terms, the LGPL-3 the Adwaita artwork is taken under, and
# the notices for everything bundled alongside. All three, in every lane.
install -Dm644 "$ROOT/LICENSE" "$DOC_DIR/LICENSE"
install -Dm644 "$ROOT/LICENSE.LGPL-3.0" "$DOC_DIR/LICENSE.LGPL-3.0"
install -Dm644 "$ROOT/THIRD-PARTY-NOTICES.md" "$DOC_DIR/THIRD-PARTY-NOTICES.md"

if [[ "$LANE" == deb ]]; then
  install -Dm644 "$ROOT/rust/packaging/linux/copyright" "$DOC_DIR/copyright"
fi

echo "Licence documents staged for the $LANE lane in $DOC_DIR"
