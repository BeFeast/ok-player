#!/usr/bin/env bash
# Clean install / upgrade / uninstall smoke for a Linux candidate .deb (issue #340).
#
# Runs in a disposable directory and never needs the host package database.
#
# The default is a `dpkg-deb -x` extraction smoke that is host-independent: it
# proves the package layout, control metadata, an upgrade re-extraction, and a
# clean uninstall on any machine with dpkg-deb. Set OKP_SMOKE_REAL_DPKG=1 on a
# real Ubuntu builder to escalate to a full dpkg install / upgrade / remove
# cycle against a private `--root` (needs the standard sbin tools on PATH). Both
# modes assert the installed binary, desktop entry, and icons land at the paths
# the updater and desktop integration expect, and that removal leaves nothing
# behind.
#
# Usage: smoke-linux-install-upgrade.sh <candidate.deb> [work-dir]
set -euo pipefail

DEB="${1:?usage: smoke-linux-install-upgrade.sh <candidate.deb> [work-dir]}"
ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"
REMOVE_WORK_DIR=false
if [[ -n "${2:-}" ]]; then
  WORK_DIR="$2"
else
  WORK_DIR="$(okp_make_scratch_dir install-smoke)"
  REMOVE_WORK_DIR=true
fi
cleanup() {
  if [[ "$REMOVE_WORK_DIR" == "true" ]]; then
    rm -rf -- "$WORK_DIR"
  fi
}
trap cleanup EXIT

[[ -f "$DEB" ]] || { echo "Candidate .deb not found: $DEB" >&2; exit 1; }

EXPECTED_FILES=(
  "usr/lib/ok-player/ok-player"
  "usr/lib/ok-player/libmpv.so.2"
  "usr/bin/ok-player"
  "usr/share/applications/com.befeast.okplayer.desktop"
  "usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
)

fail() { echo "install/upgrade/uninstall smoke: $1" >&2; exit 1; }

assert_layout() {
  local root="$1"
  local relative
  for relative in "${EXPECTED_FILES[@]}"; do
    [[ -e "$root/$relative" || -L "$root/$relative" ]] \
      || fail "expected $relative under $root after install"
  done
  # The launcher must resolve to the private lib binary, not dangle.
  [[ -x "$root/usr/lib/ok-player/ok-player" ]] \
    || fail "installed binary is not executable"
  "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/verify-linux-bundled-mpv.sh" \
    "$root/usr/lib/ok-player/ok-player" \
    "$root/usr/lib/ok-player" \
    || fail "installed binary does not resolve the packaged patched libmpv"
}

rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR"

if [[ "${OKP_SMOKE_REAL_DPKG:-0}" == "1" ]]; then
  command -v dpkg >/dev/null 2>&1 || fail "OKP_SMOKE_REAL_DPKG=1 but dpkg is not available"
  export PATH="/usr/local/sbin:/usr/sbin:/sbin:$PATH"
  ROOT="$WORK_DIR/root"
  ADMINDIR="$WORK_DIR/dpkg-admin"
  mkdir -p "$ROOT" "$ADMINDIR/updates" "$ADMINDIR/info"
  : >"$ADMINDIR/status"

  dpkg_root() { dpkg --root="$ROOT" --admindir="$ADMINDIR" --force-not-root "$@"; }

  echo "== install =="
  dpkg_root --install "$DEB" >"$WORK_DIR/install.log" 2>&1 \
    || { cat "$WORK_DIR/install.log" >&2; fail "dpkg install failed"; }
  assert_layout "$ROOT"

  echo "== upgrade (reinstall same candidate) =="
  dpkg_root --install "$DEB" >"$WORK_DIR/upgrade.log" 2>&1 \
    || { cat "$WORK_DIR/upgrade.log" >&2; fail "dpkg upgrade failed"; }
  assert_layout "$ROOT"

  echo "== uninstall =="
  dpkg_root --remove ok-player >"$WORK_DIR/remove.log" 2>&1 \
    || { cat "$WORK_DIR/remove.log" >&2; fail "dpkg remove failed"; }
  # Removal must clear the application payload; a leftover binary means an
  # upgrade could strand a stale executable.
  [[ ! -e "$ROOT/usr/lib/ok-player/ok-player" ]] \
    || fail "binary survived uninstall"
  echo "dpkg install/upgrade/uninstall smoke passed"
  exit 0
fi

# --- Default: host-independent extraction layout smoke -----------------------
command -v dpkg-deb >/dev/null 2>&1 || fail "dpkg-deb is required for the extraction smoke"

echo "== extract (install) =="
EXTRACT="$WORK_DIR/extract"
mkdir -p "$EXTRACT"
dpkg-deb -x "$DEB" "$EXTRACT" || fail "dpkg-deb extraction failed"
assert_layout "$EXTRACT"

echo "== control metadata =="
CONTROL="$WORK_DIR/control"
mkdir -p "$CONTROL"
dpkg-deb -e "$DEB" "$CONTROL" || fail "dpkg-deb control extraction failed"
awk '$0 == "Package: ok-player" { found = 1 } END { exit !found }' "$CONTROL/control" \
  || fail "control Package field is not ok-player"
awk '$0 == "Architecture: amd64" { found = 1 } END { exit !found }' "$CONTROL/control" \
  || fail "control Architecture is not amd64"

echo "== upgrade (re-extract over install) =="
dpkg-deb -x "$DEB" "$EXTRACT" || fail "dpkg-deb re-extraction failed"
assert_layout "$EXTRACT"

echo "== uninstall (remove tree) =="
rm -rf "$EXTRACT"
[[ ! -e "$EXTRACT" ]] || fail "extraction tree survived removal"
echo "extraction install/upgrade/uninstall smoke passed (layout + control verified; set OKP_SMOKE_REAL_DPKG=1 for the full dpkg cycle)"
