#!/usr/bin/env bash
# Build and validate the SRPM/RPM inside a clean supported Fedora root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FEDORA_VERSION="${FEDORA_VERSION:-unknown}"
OUT_DIR="${1:-$ROOT/artifacts/linux/rpm/fedora-$FEDORA_VERSION}"

for tool in dnf rpmbuild rpmlint; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

SOURCE_DIR="$OUT_DIR/source"
"$ROOT/scripts/package-linux-rpm-source.sh" "$SOURCE_DIR"
SRPM="$(find "$SOURCE_DIR" -maxdepth 1 -name '*.src.rpm' -print -quit)"
[[ -n "$SRPM" ]] || { echo "SRPM was not produced" >&2; exit 2; }

dnf builddep -y "$SRPM"

mkdir -p "$OUT_DIR/previous" "$OUT_DIR/current"
rpmbuild --rebuild "$SRPM" \
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
