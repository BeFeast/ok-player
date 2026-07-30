#!/usr/bin/env bash
# Negative controls for scripts/check-packaging-licenses.py (issue #743).
#
# A compliance gate nobody has watched fail is a decoration. Each case below
# builds a copy of the tree, takes one licence document away from exactly one
# packaging lane, and requires the checker to go red and to name that lane. If
# the checker ever stops noticing, these tests fail here rather than in a
# release nobody audits.
#
# The tree copy is deliberately the same file set the checker reads, so a case
# cannot pass by mutating a file the checker never opens.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECK="$ROOT/scripts/check-packaging-licenses.py"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
fail() { echo "FAIL: $1" >&2; failures=$((failures + 1)); }

TRACKED=(
  LICENSE
  LICENSE.LGPL-3.0
  THIRD-PARTY-NOTICES.md
  rust/packaging/linux/copyright
  rust/packaging/fedora/ok-player.spec
  rust/packaging/flatpak/com.befeast.okplayer.json
  installer/build-velopack.ps1
  scripts/stage-license-documents.sh
  scripts/package-linux-deb.sh
  scripts/package-linux-velopack.sh
  scripts/smoke-linux-install-upgrade.sh
  scripts/smoke-linux-rpm-install-upgrade.sh
  scripts/assert-windows-installed-tree.ps1
)

make_tree() {
  local name="$1" dest="$tmp/$1" relative
  rm -rf "$dest"
  for relative in "${TRACKED[@]}"; do
    mkdir -p "$dest/$(dirname "$relative")"
    cp "$ROOT/$relative" "$dest/$relative"
  done
  printf '%s' "$dest"
}

# Drop every line matching a pattern from one file in a mutated tree.
drop_line() {
  local tree="$1" relative="$2" pattern="$3"
  local before after
  before="$(wc -l <"$tree/$relative")"
  grep -v -- "$pattern" "$tree/$relative" >"$tree/$relative.mutated"
  mv "$tree/$relative.mutated" "$tree/$relative"
  after="$(wc -l <"$tree/$relative")"
  [ "$before" -gt "$after" ] || fail "mutation matched nothing: $pattern in $relative"
}

run_check() {
  python3 "$CHECK" --root "$1" 2>&1
}

expect_green() {
  local label="$1" tree="$2" out rc
  out="$(run_check "$tree")"
  rc=$?
  [ "$rc" -eq 0 ] || { fail "$label: checker exited $rc on an intact tree: $out"; return; }
  echo "ok: $label"
}

# The negative control proper: mutate, then require red AND require the report
# to name the lane, so a checker that fails for an unrelated reason cannot pass
# this test.
expect_red() {
  local label="$1" tree="$2" lane="$3" needle="$4" out rc
  out="$(run_check "$tree")"
  rc=$?
  if [ "$rc" -eq 0 ]; then
    fail "$label: the checker stayed green after the document was removed"
    return
  fi
  grep -qE "^FAIL $lane\b" <<<"$out" \
    || { fail "$label: report did not mark the $lane lane FAIL: $out"; return; }
  grep -qF -- "$needle" <<<"$out" \
    || { fail "$label: report never mentions '$needle': $out"; return; }
  echo "ok: $label"
}

# --- 0: the tree as committed is compliant ----------------------------------
expect_green "the repository as committed ships every licence document" "$(make_tree baseline)"

# --- 1: repository root ------------------------------------------------------
tree="$(make_tree no-lgpl-text)"
rm "$tree/LICENSE.LGPL-3.0"
expect_red "removing the LGPL-3 text from the tree is caught" \
  "$tree" repository "LICENSE.LGPL-3.0 is missing"

# A present-but-hollow document is not a licence. This is the case a pure
# existence check waves through.
tree="$(make_tree hollow-lgpl-text)"
: >"$tree/LICENSE.LGPL-3.0"
expect_red "an empty LGPL-3 file is not accepted as the licence" \
  "$tree" repository "GNU LESSER GENERAL PUBLIC LICENSE"

# --- 2: deb ------------------------------------------------------------------
tree="$(make_tree deb-no-staging)"
drop_line "$tree" scripts/package-linux-deb.sh stage-license-documents.sh
expect_red "a .deb lane that stops staging the documents is caught" \
  "$tree" deb "no longer calls stage-license-documents.sh"

# The behavioural leg: the lane still calls the staging script, but the staging
# script stops producing the LGPL text. Only running it catches this.
tree="$(make_tree deb-staging-drops-lgpl)"
drop_line "$tree" scripts/stage-license-documents.sh 'LICENSE.LGPL-3.0'
expect_red "staging that silently stops producing the LGPL text is caught" \
  "$tree" deb "staging produced no LICENSE.LGPL-3.0"

# The install smoke is the only thing that proves the documents survive a real
# dpkg transaction, so gutting it is also a compliance regression.
tree="$(make_tree deb-smoke-gutted)"
drop_line "$tree" scripts/smoke-linux-install-upgrade.sh 'usr/share/doc/ok-player/copyright'
expect_red "a .deb install smoke that stops asserting the copyright file is caught" \
  "$tree" deb "does not assert usr/share/doc/ok-player/copyright"

# --- 3: appimage -------------------------------------------------------------
tree="$(make_tree appimage-no-staging)"
drop_line "$tree" scripts/package-linux-velopack.sh stage-license-documents.sh
expect_red "an AppImage lane that stops staging the documents is caught" \
  "$tree" appimage "no longer calls stage-license-documents.sh"

# --- 4: rpm ------------------------------------------------------------------
tree="$(make_tree rpm-no-lgpl-install)"
drop_line "$tree" rust/packaging/fedora/ok-player.spec 'LICENSE.LGPL-3.0'
expect_red "an rpm spec that stops installing the LGPL text is caught" \
  "$tree" rpm "%install no longer puts LICENSE.LGPL-3.0"

tree="$(make_tree rpm-no-license-marker)"
drop_line "$tree" rust/packaging/fedora/ok-player.spec '^%license'
expect_red "an rpm spec that stops marking the texts %license is caught" \
  "$tree" rpm "%files no longer marks LICENSE"

# --- 5: flatpak --------------------------------------------------------------
tree="$(make_tree flatpak-no-lgpl)"
drop_line "$tree" rust/packaging/flatpak/com.befeast.okplayer.json 'LICENSE.LGPL-3.0'
expect_red "a Flatpak manifest that stops installing the LGPL text is caught" \
  "$tree" flatpak "no longer installs LICENSE.LGPL-3.0"

tree="$(make_tree flatpak-no-notices)"
drop_line "$tree" rust/packaging/flatpak/com.befeast.okplayer.json 'THIRD-PARTY-NOTICES.md /app/share'
expect_red "a Flatpak manifest that stops installing the notices is caught" \
  "$tree" flatpak "no longer installs THIRD-PARTY-NOTICES.md"

# --- 6: windows --------------------------------------------------------------
tree="$(make_tree windows-no-lgpl)"
drop_line "$tree" installer/build-velopack.ps1 'LICENSE.LGPL-3.0'
expect_red "a Windows installer that stops staging the LGPL text is caught" \
  "$tree" windows "no longer stages LICENSE.LGPL-3.0"

tree="$(make_tree windows-tree-unchecked)"
drop_line "$tree" scripts/assert-windows-installed-tree.ps1 'LICENSE.LGPL-3.0.txt'
expect_red "a Windows installed-tree assertion that stops naming the LGPL text is caught" \
  "$tree" windows "does not assert LICENSE.LGPL-3.0.txt"

if [ "$failures" -gt 0 ]; then
  echo "$failures packaging licence policy test(s) failed." >&2
  exit 1
fi
echo "ok: every packaging lane's licence documents are gated, and each gate was proven to go red"
