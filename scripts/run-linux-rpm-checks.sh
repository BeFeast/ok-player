#!/usr/bin/env bash
# Build and validate the SRPM/RPM inside a clean supported Fedora root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"
FEDORA_VERSION="${FEDORA_VERSION:-unknown}"
OUT_DIR="${1:-$ROOT/artifacts/linux/rpm/fedora-$FEDORA_VERSION}"

for tool in dnf rpm rpmbuild rpmdev-vercmp rpmlint cmp sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done

# The upstream version this run packages, pinned here rather than left to the script's default,
# so the assertion below is about a version that was actually passed in — the path that used to
# be ignored (issue #709).
export OKP_RPM_UPSTREAM_VERSION="${OKP_RPM_UPSTREAM_VERSION:-0.11.0-beta.1}"
# shellcheck source=/dev/null
source "$ROOT/scripts/linux-package-version.sh"
EXPECTED_RPM_VERSION="$(okp_rpm_version_for_build "$OKP_RPM_UPSTREAM_VERSION")"

SOURCE_DIR="$OUT_DIR/source"
"$ROOT/scripts/package-linux-rpm-source.sh" "$SOURCE_DIR"
SRPM="$(find "$SOURCE_DIR" -maxdepth 1 -name '*.src.rpm' -print -quit)"
[[ -n "$SRPM" ]] || { echo "SRPM was not produced" >&2; exit 2; }

# What the package calls itself, read out of the package. rpm forbids `-` in a version, so this
# lane has always encoded; what is asserted is that the encoding is of the upstream version it
# was given, rather than of a default nothing overrides.
SRPM_VERSION="$(rpm -qp --queryformat '%{VERSION}' "$SRPM" 2>/dev/null)"
[[ "$SRPM_VERSION" == "$EXPECTED_RPM_VERSION" ]] || {
  echo "the SRPM built from ${OKP_RPM_UPSTREAM_VERSION} calls itself ${SRPM_VERSION}, expected ${EXPECTED_RPM_VERSION}" >&2
  exit 1
}
echo "SRPM version: ${SRPM_VERSION} (from upstream ${OKP_RPM_UPSTREAM_VERSION})"

REPRO_SOURCE_DIR="$(okp_make_scratch_dir rpm-repro "$OUT_DIR")"
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
  sha256sum -- ./*
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

# The binary package the user installs, not just the source one.
BINARY_RPM_VERSION="$(rpm -qp --queryformat '%{VERSION}' "$CURRENT_RPM" 2>/dev/null)"
[[ "$BINARY_RPM_VERSION" == "$EXPECTED_RPM_VERSION" ]] || {
  echo "the binary RPM calls itself ${BINARY_RPM_VERSION}, expected ${EXPECTED_RPM_VERSION}" >&2
  exit 1
}
# And it must sort below the release it precedes, which is the whole reason rpm versions carry
# `~` — asked of rpm's own comparator rather than assumed from the string.
if [[ "$OKP_RPM_UPSTREAM_VERSION" == *-* ]]; then
  set +e
  # rpmdev-vercmp exits 11 when the first argument is newer and 12 when the second is.
  rpmdev-vercmp "0:${BINARY_RPM_VERSION}-1" "0:${OKP_RPM_UPSTREAM_VERSION%%-*}-1" >/dev/null
  VERCMP_STATUS=$?
  set -e
  ((VERCMP_STATUS == 12)) || {
    echo "the binary RPM ${BINARY_RPM_VERSION} does not sort below the release ${OKP_RPM_UPSTREAM_VERSION%%-*} (rpmdev-vercmp said ${VERCMP_STATUS})" >&2
    exit 1
  }
fi
echo "binary RPM version: ${BINARY_RPM_VERSION}"

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
