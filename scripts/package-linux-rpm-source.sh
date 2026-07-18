#!/usr/bin/env bash
# Reproducible Fedora source bundle and SRPM builder for issue #346.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SPEC="$ROOT/rust/packaging/fedora/ok-player.spec"
OUT_DIR="${1:-$ROOT/artifacts/linux/rpm/source}"

for tool in cargo git rpmbuild tar gzip zstd sha256sum touch flock; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

# Run all Git commands from the resolved repository root. In GitHub Actions
# container jobs the checkout may be a fresh shallow clone with detached HEAD and
# a safe.directory config stored in a temporary HOME directory. Explicit
# --git-dir/--work-tree can fail in that environment, so let git discover the
# repository from the workspace itself and preserve the strict dirty-tree guard
# for real worktrees.
cd "$ROOT"

if ! git rev-parse --git-dir >/dev/null 2>&1; then
  echo "RPM source builds require a committed Git checkout at $ROOT." >&2
  exit 2
fi

if ! git rev-parse --verify HEAD >/dev/null 2>&1; then
  echo "RPM source builds require a committed Git checkout at $ROOT." >&2
  exit 2
fi

if [[ "${OKP_RPM_ALLOW_DIRTY:-0}" != "1" ]] &&
  { ! git diff --quiet || ! git diff --cached --quiet; }; then
  echo "RPM source builds require a clean committed tree (set OKP_RPM_ALLOW_DIRTY=1 only for local experiments)." >&2
  exit 2
fi

UPSTREAM_VERSION="${OKP_RPM_UPSTREAM_VERSION:-0.11.0-beta.1}"
SOURCE_EPOCH="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct HEAD)}"
SOURCE_COMMIT="$(git rev-parse HEAD)"
PREFIX="ok-player-$UPSTREAM_VERSION"
TMP_DIR="$(mktemp -d)"
RPM_TOPDIR="/tmp/ok-player-rpmbuild"
RPM_LOCK="/tmp/ok-player-rpmbuild.lock"
cleanup() {
  if [[ -L "$RPM_TOPDIR" ]]; then
    rm -f "$RPM_TOPDIR"
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT
export SOURCE_DATE_EPOCH="$SOURCE_EPOCH"

mkdir -p "$OUT_DIR" "$TMP_DIR/rpmbuild/SOURCES" "$TMP_DIR/rpmbuild/SRPMS"
rm -f \
  "$OUT_DIR/$PREFIX.tar.gz" \
  "$OUT_DIR/$PREFIX-vendor.tar.zst" \
  "$OUT_DIR/$PREFIX-source-commit" \
  "$OUT_DIR/SHA256SUMS" \
  "$OUT_DIR"/*.src.rpm
exec 9>"$RPM_LOCK"
flock 9
if [[ -e "$RPM_TOPDIR" && ! -L "$RPM_TOPDIR" ]]; then
  echo "Refusing to replace non-symlink RPM build path: $RPM_TOPDIR" >&2
  exit 2
fi
rm -f "$RPM_TOPDIR"
ln -s "$TMP_DIR/rpmbuild" "$RPM_TOPDIR"

git archive --format=tar --prefix="$PREFIX/" HEAD \
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
  | zstd -10 -T1 -q -o "$OUT_DIR/$PREFIX-vendor.tar.zst"
printf '%s\n' "$SOURCE_COMMIT" > "$OUT_DIR/$PREFIX-source-commit"
touch --date="@$SOURCE_EPOCH" \
  "$OUT_DIR/$PREFIX.tar.gz" \
  "$OUT_DIR/$PREFIX-vendor.tar.zst" \
  "$OUT_DIR/$PREFIX-source-commit"

cp "$OUT_DIR/$PREFIX.tar.gz" "$TMP_DIR/rpmbuild/SOURCES/"
cp "$OUT_DIR/$PREFIX-vendor.tar.zst" "$TMP_DIR/rpmbuild/SOURCES/"
cp "$OUT_DIR/$PREFIX-source-commit" "$TMP_DIR/rpmbuild/SOURCES/"
rpmbuild -bs "$SPEC" \
  --define "_topdir $RPM_TOPDIR" \
  --define "upstream_version $UPSTREAM_VERSION" \
  --define "_buildhost reproducible.invalid" \
  --define "use_source_date_epoch_as_buildtime 1" \
  --define "clamp_mtime_to_source_date_epoch 1" \
  --define "_source_filedigest_algorithm 8" \
  --define "_binary_filedigest_algorithm 8"
cp "$RPM_TOPDIR"/SRPMS/*.src.rpm "$OUT_DIR/"

(
  cd "$OUT_DIR"
  sha256sum \
    "$PREFIX.tar.gz" \
    "$PREFIX-vendor.tar.zst" \
    "$PREFIX-source-commit" \
    ./*.src.rpm > SHA256SUMS
)

echo "Fedora source artifacts written to $OUT_DIR"
