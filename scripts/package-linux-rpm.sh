#!/usr/bin/env bash
# Reproducible Fedora SRPM source builder. The source and Cargo vendor archives
# are normalized to the source commit timestamp, and the spec builds offline
# against Fedora's system GTK/libmpv development packages.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UPSTREAM_VERSION="0.11.0-beta.1"
RPM_VERSION=""
RPM_RELEASE="0.1.beta.1"
OUT_DIR="$ROOT/artifacts/linux/rpm"

usage() {
  cat >&2 <<'USAGE'
usage: package-linux-rpm.sh [--version VERSION] [--rpm-release RELEASE] [--out DIR]

Builds a normalized source archive, a locked Cargo vendor archive, and one SRPM.
The RPM Version is derived from the part before the first '-' unless overridden
by the package version format.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) UPSTREAM_VERSION="${2:?}"; shift 2 ;;
    --rpm-release) RPM_RELEASE="${2:?}"; shift 2 ;;
    --out) OUT_DIR="${2:?}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

RPM_VERSION="${UPSTREAM_VERSION%%-*}"
if [[ ! "$UPSTREAM_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)*$ ]]; then
  echo "invalid upstream version: $UPSTREAM_VERSION" >&2
  exit 2
fi
if [[ ! "$RPM_RELEASE" =~ ^[0-9][0-9A-Za-z.]*$ ]]; then
  echo "invalid RPM release: $RPM_RELEASE" >&2
  exit 2
fi

for tool in cargo git rpmbuild tar xz; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct)"
BUILD_ROOT="$(mktemp -d)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

TOPDIR="$BUILD_ROOT/rpmbuild"
SOURCE_ROOT="$BUILD_ROOT/ok-player-$UPSTREAM_VERSION"
mkdir -p "$TOPDIR"/{BUILD,BUILDROOT,RPMS,SOURCES,SPECS,SRPMS} "$SOURCE_ROOT" "$OUT_DIR"

git -C "$ROOT" ls-files --cached --others --exclude-standard -z \
  | tar -C "$ROOT" --null -T - -cf - \
  | tar -xf - -C "$SOURCE_ROOT"

SOURCE_ARCHIVE="$TOPDIR/SOURCES/ok-player-$UPSTREAM_VERSION.tar.xz"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --clamp-mtime \
  --owner=0 --group=0 --numeric-owner \
  -C "$BUILD_ROOT" -cJf "$SOURCE_ARCHIVE" "ok-player-$UPSTREAM_VERSION"

VENDOR_ROOT="$BUILD_ROOT/vendor"
(cd "$SOURCE_ROOT/rust" && cargo vendor --quiet --locked --versioned-dirs "$VENDOR_ROOT" >/dev/null)
VENDOR_ARCHIVE="$TOPDIR/SOURCES/ok-player-$UPSTREAM_VERSION-vendor.tar.xz"
tar --sort=name --mtime="@$SOURCE_DATE_EPOCH" --clamp-mtime \
  --owner=0 --group=0 --numeric-owner \
  -C "$BUILD_ROOT" -cJf "$VENDOR_ARCHIVE" vendor

cp "$ROOT/rust/packaging/fedora/ok-player.spec" "$TOPDIR/SPECS/ok-player.spec"

export SOURCE_DATE_EPOCH
rpmbuild -bs "$TOPDIR/SPECS/ok-player.spec" \
  --define "_topdir $TOPDIR" \
  --define "_buildhost localhost" \
  --define "use_source_date_epoch_as_buildtime 1" \
  --define "clamp_mtime_to_source_date_epoch 1" \
  --define "okp_upstream_version $UPSTREAM_VERSION" \
  --define "okp_rpm_version $RPM_VERSION" \
  --define "okp_rpm_release $RPM_RELEASE"

SRPM="$(find "$TOPDIR/SRPMS" -maxdepth 1 -type f -name '*.src.rpm' -print -quit)"
if [[ -z "$SRPM" ]]; then
  echo "rpmbuild did not produce an SRPM" >&2
  exit 1
fi
install -Dm0644 "$SRPM" "$OUT_DIR/$(basename "$SRPM")"
sha256sum "$OUT_DIR/$(basename "$SRPM")" > "$OUT_DIR/$(basename "$SRPM").sha256"
echo "Fedora SRPM written to $OUT_DIR/$(basename "$SRPM")"
