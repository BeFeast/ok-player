#!/usr/bin/env bash
# Build and validate the SRPM/RPM inside a clean supported Fedora root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FEDORA_VERSION="${FEDORA_VERSION:-unknown}"
OUT_DIR="${1:-$ROOT/artifacts/linux/rpm/fedora-$FEDORA_VERSION}"

for tool in dnf rpmbuild rpmlint cmp sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

SOURCE_DIR="$OUT_DIR/source"
"$ROOT/scripts/package-linux-rpm-source.sh" "$SOURCE_DIR"
SRPM="$(find "$SOURCE_DIR" -maxdepth 1 -name '*.src.rpm' -print -quit)"
[[ -n "$SRPM" ]] || { echo "SRPM was not produced" >&2; exit 2; }

REPRO_SOURCE_DIR="$(mktemp -d)"
trap 'rm -rf "$REPRO_SOURCE_DIR"' EXIT
"$ROOT/scripts/package-linux-rpm-source.sh" "$REPRO_SOURCE_DIR"
for artifact in "$SOURCE_DIR"/*; do
  counterpart="$REPRO_SOURCE_DIR/$(basename "$artifact")"
  [[ -f "$counterpart" ]] || { echo "Reproducibility build omitted $(basename "$artifact")" >&2; exit 1; }
  cmp "$artifact" "$counterpart" || {
    echo "Fedora source artifact is not reproducible: $(basename "$artifact")" >&2
    exit 1
  }
done
(
  cd "$SOURCE_DIR"
  sha256sum *
) > "$OUT_DIR/source-reproducibility.txt"
echo "Fedora source artifacts are byte-identical across two clean builds" >> "$OUT_DIR/source-reproducibility.txt"

dnf builddep -y "$SRPM"

rm -rf "$OUT_DIR/previous" "$OUT_DIR/current"
mkdir -p "$OUT_DIR/previous" "$OUT_DIR/current"
rpmbuild --rebuild "$SRPM" \
  --nocheck \
  --define "_rpmdir $OUT_DIR/previous" \
  --define "rpm_release 0.1"
rpmbuild --rebuild "$SRPM" \
  --define "_rpmdir $OUT_DIR/current"

PREVIOUS_RPM="$(find "$OUT_DIR/previous" -type f -name 'ok-player-[0-9]*.x86_64.rpm' -print -quit)"
CURRENT_RPM="$(find "$OUT_DIR/current" -type f -name 'ok-player-[0-9]*.x86_64.rpm' -print -quit)"
[[ -n "$PREVIOUS_RPM" && -n "$CURRENT_RPM" ]] || { echo "Binary RPMs were not produced" >&2; exit 2; }

RPMLINT_LOG="$OUT_DIR/rpmlint.txt"
set +e
rpmlint "$ROOT/rust/packaging/fedora/ok-player.spec" "$SRPM" "$CURRENT_RPM" \
  >"$RPMLINT_LOG" 2>&1
RPMLINT_STATUS=$?
set -e
cat "$RPMLINT_LOG"
if grep -q ': E:' "$RPMLINT_LOG"; then
  echo "rpmlint reported an error" >&2
  exit 1
fi
if [[ "$RPMLINT_STATUS" -ne 0 ]]; then
  echo "rpmlint warnings were recorded; the Fedora beta gate rejects errors and accounts for warnings in the PR." >&2
fi

"$ROOT/scripts/smoke-linux-rpm-install-upgrade.sh" "$CURRENT_RPM" "$PREVIOUS_RPM"

rpm -qpl "$CURRENT_RPM" | sort > "$OUT_DIR/installed-files.txt"
rpm -qpR "$CURRENT_RPM" | sort > "$OUT_DIR/declared-requires.txt"
sha256sum "$SRPM" "$PREVIOUS_RPM" "$CURRENT_RPM" > "$OUT_DIR/SHA256SUMS"

echo "Fedora $FEDORA_VERSION RPM checks passed: $CURRENT_RPM"
