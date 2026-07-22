#!/usr/bin/env bash
# Clean install / upgrade / uninstall smoke for a Linux candidate .deb (issue #340).
#
# Runs in a disposable directory and never mutates the host package database.
#
# The default is a `dpkg-deb -x` extraction smoke that is host-independent: it
# proves the package layout, control metadata, an upgrade re-extraction, and a
# clean uninstall on any machine with dpkg-deb. Set OKP_SMOKE_REAL_DPKG=1 on a
# real Ubuntu builder to escalate to a dpkg install / upgrade / purge cycle in
# an unprivileged user namespace against a private `--root`. The private package
# database begins empty and dependency configuration is forced because the
# preceding package/portability gates validate the Depends contract and runtime
# closure; this gate owns dpkg lifecycle semantics and chrooted maintainer
# scripts. Both modes assert the installed binary, desktop entry, and icons land
# at the paths the updater and desktop integration expect, and that removal
# leaves nothing behind.
#
# Usage: smoke-linux-install-upgrade.sh <candidate.deb> [work-dir]
set -euo pipefail

# candidate-required-tools: awk cat chmod cp dirname dpkg dpkg-deb dpkg-query ldd mkdir mktemp rm unshare

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
  "usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
  "usr/share/icons/hicolor/16x16/apps/com.befeast.okplayer.svg"
  "usr/share/icons/hicolor/24x24/apps/com.befeast.okplayer.svg"
  "usr/share/icons/hicolor/32x32/apps/com.befeast.okplayer.svg"
  "usr/share/icons/hicolor/48x48/apps/com.befeast.okplayer.svg"
  "usr/share/icons/hicolor/64x64/apps/com.befeast.okplayer.svg"
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
  command -v dpkg-query >/dev/null 2>&1 || fail "OKP_SMOKE_REAL_DPKG=1 but dpkg-query is not available"
  command -v unshare >/dev/null 2>&1 || fail "OKP_SMOKE_REAL_DPKG=1 but unshare is not available"
  export PATH="/usr/local/sbin:/usr/sbin:/sbin:$PATH"
  INSTALL_ROOT="$WORK_DIR/root"
  ADMINDIR="$INSTALL_ROOT/var/lib/dpkg"
  DPKG_LOG="$WORK_DIR/dpkg.log"
  mkdir -p \
    "$INSTALL_ROOT/bin" \
    "$INSTALL_ROOT/dev" \
    "$ADMINDIR/updates" \
    "$ADMINDIR/info"
  : >"$ADMINDIR/status"

  # dpkg chroots maintainer scripts into INSTALL_ROOT. Seed only the shell and
  # its dynamic loader/runtime; desktop cache helpers remain absent, so package
  # scripts cannot mutate the host desktop while exercising their control flow.
  cp -L /bin/sh "$INSTALL_ROOT/bin/sh"
  : >"$INSTALL_ROOT/dev/null"
  chmod 666 "$INSTALL_ROOT/dev/null"
  while IFS= read -r library; do
    destination="$INSTALL_ROOT$library"
    mkdir -p "$(dirname -- "$destination")"
    cp -L "$library" "$destination"
  done < <(
    ldd /bin/sh \
      | awk '/=> \// { print $3 } /^[[:space:]]*\// { print $1 }'
  )

  DPKG="$(command -v dpkg)"
  DPKG_QUERY="$(command -v dpkg-query)"
  dpkg_root() {
    unshare --user --map-root-user --mount --fork "$DPKG" \
      --root="$INSTALL_ROOT" \
      --admindir="$ADMINDIR" \
      --log="$DPKG_LOG" \
      --force-depends \
      "$@"
  }
  assert_installed_status() {
    local status
    status="$($DPKG_QUERY --admindir="$ADMINDIR" -W -f='${db:Status-Status}' ok-player 2>/dev/null || true)"
    [[ "$status" == "installed" ]] || fail "dpkg status is not installed: ${status:-missing}"
  }

  echo "== install =="
  dpkg_root --install "$DEB" >"$WORK_DIR/install.log" 2>&1 \
    || { cat "$WORK_DIR/install.log" >&2; fail "dpkg install failed"; }
  assert_installed_status
  assert_layout "$INSTALL_ROOT"

  echo "== upgrade (reinstall same candidate) =="
  dpkg_root --install "$DEB" >"$WORK_DIR/upgrade.log" 2>&1 \
    || { cat "$WORK_DIR/upgrade.log" >&2; fail "dpkg upgrade failed"; }
  assert_installed_status
  assert_layout "$INSTALL_ROOT"

  echo "== uninstall (purge) =="
  dpkg_root --purge ok-player >"$WORK_DIR/remove.log" 2>&1 \
    || { cat "$WORK_DIR/remove.log" >&2; fail "dpkg purge failed"; }
  for relative in "${EXPECTED_FILES[@]}"; do
    [[ ! -e "$INSTALL_ROOT/$relative" && ! -L "$INSTALL_ROOT/$relative" ]] \
      || fail "$relative survived uninstall"
  done
  if "$DPKG_QUERY" --admindir="$ADMINDIR" -W ok-player >/dev/null 2>&1; then
    fail "ok-player survived purge in the private package database"
  fi
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
