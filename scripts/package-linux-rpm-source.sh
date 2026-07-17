#!/usr/bin/env bash
# Reproducible Fedora source bundle and SRPM builder for issue #346.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPEC="$ROOT/rust/packaging/fedora/ok-player.spec"
OUT_DIR="${1:-$ROOT/artifacts/linux/rpm/source}"

for tool in cargo git rpmbuild tar gzip zstd; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

if [[ "${OKP_RPM_ALLOW_DIRTY:-0}" != "1" ]] &&
  { ! git -C "$ROOT" diff --quiet || ! git -C "$ROOT" diff --cached --quiet; }; then
  echo "RPM source builds require a clean committed tree (set OKP_RPM_ALLOW_DIRTY=1 only for local experiments)." >&2
  exit 2
fi

UPSTREAM_VERSION="${OKP_RPM_UPSTREAM_VERSION:-0.11.0-beta.1}"
SOURCE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct HEAD)}"
SOURCE_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
PREFIX="ok-player-$UPSTREAM_VERSION"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$OUT_DIR" "$TMP_DIR/rpmbuild/SOURCES" "$TMP_DIR/rpmbuild/SRPMS"

git -C "$ROOT" archive --format=tar --prefix="$PREFIX/" HEAD \
  | gzip -n -9 > "$OUT_DIR/$PREFIX.tar.gz"

cargo vendor \
  --manifest-path "$ROOT/rust/Cargo.toml" \
  --locked \
  --versioned-dirs \
  "$TMP_DIR/vendor" >/dev/null
tar \
  --sort=name \
  --mtime="@$SOURCE_EPOCH" \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C "$TMP_DIR" \
  -cf - vendor \
  | zstd -19 -T1 -q -o "$OUT_DIR/$PREFIX-vendor.tar.zst"
printf '%s\n' "$SOURCE_COMMIT" > "$OUT_DIR/$PREFIX-source-commit"

cp "$OUT_DIR/$PREFIX.tar.gz" "$TMP_DIR/rpmbuild/SOURCES/"
cp "$OUT_DIR/$PREFIX-vendor.tar.zst" "$TMP_DIR/rpmbuild/SOURCES/"
cp "$OUT_DIR/$PREFIX-source-commit" "$TMP_DIR/rpmbuild/SOURCES/"
rpmbuild -bs "$SPEC" \
  --define "_topdir $TMP_DIR/rpmbuild" \
  --define "upstream_version $UPSTREAM_VERSION" \
  --define "_source_filedigest_algorithm 8" \
  --define "_binary_filedigest_algorithm 8"
cp "$TMP_DIR"/rpmbuild/SRPMS/*.src.rpm "$OUT_DIR/"

sha256sum \
  "$OUT_DIR/$PREFIX.tar.gz" \
  "$OUT_DIR/$PREFIX-vendor.tar.zst" \
  "$OUT_DIR/$PREFIX-source-commit" \
  "$OUT_DIR"/*.src.rpm > "$OUT_DIR/SHA256SUMS"

echo "Fedora source artifacts written to $OUT_DIR"
