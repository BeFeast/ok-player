#!/usr/bin/env bash
# Build a baseline repository and a two-commit update repository from prefetched
# sources, then emit portable identities for real-machine lifecycle acceptance.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST_DIR="$ROOT/rust/packaging/flatpak"
MANIFEST="$MANIFEST_DIR/com.befeast.okplayer.json"
OUT_DIR="${OKP_FLATPAK_OUT_DIR:-$ROOT/artifacts/linux/flatpak}"
BASELINE_VERSION="0.11.0-beta.0"
UPDATE_VERSION="0.11.0-beta.1"
APP_ID="com.befeast.okplayer"
BRANCH="beta"

for tool in cargo cp flatpak flatpak-builder git ostree python3 realpath; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

dirty_status="$(git -C "$ROOT" status --porcelain)"
if [[ -n "$dirty_status" ]]; then
  echo "Flatpak beta artifacts require a clean exact-head checkout" >&2
  printf '%s\n' "$dirty_status" >&2
  exit 2
fi

OUT_DIR="$(realpath -m "$OUT_DIR")"
SOURCE_COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
ARCH="$(flatpak --default-arch)"
REF="app/$APP_ID/$ARCH/$BRANCH"
BASELINE_BUILD_DIR="$OUT_DIR/build-baseline"
UPDATE_BUILD_DIR="$OUT_DIR/build-update"
STATE_DIR="$OUT_DIR/state"
BASELINE_REPO_DIR="$OUT_DIR/repo-baseline"
REPO_DIR="$OUT_DIR/repo"
BASELINE_BUNDLE="$OUT_DIR/OK-Player-$BASELINE_VERSION.flatpak"
UPDATE_BUNDLE="$OUT_DIR/OK-Player-$UPDATE_VERSION.flatpak"
ARTIFACT_MANIFEST="$OUT_DIR/flatpak-beta-artifact.json"

case "$OUT_DIR" in
  "$ROOT"/* | /tmp/*) ;;
  *)
    echo "Refusing unsafe Flatpak output directory" >&2
    exit 2
    ;;
esac

mkdir -p "$OUT_DIR"
for target in \
  "$BASELINE_BUILD_DIR" \
  "$UPDATE_BUILD_DIR" \
  "$BASELINE_REPO_DIR" \
  "$REPO_DIR" \
  "$BASELINE_BUNDLE" \
  "$UPDATE_BUNDLE" \
  "$ARTIFACT_MANIFEST"; do
  if [[ -e "$target" || -L "$target" ]]; then
    rm -rf -- "$target"
  fi
done
mkdir -p "$STATE_DIR" "$BASELINE_REPO_DIR" "$REPO_DIR"

BASELINE_MANIFEST="$(mktemp "$MANIFEST_DIR/.com.befeast.okplayer.baseline.XXXXXX.json")"
UPDATE_MANIFEST="$(mktemp "$MANIFEST_DIR/.com.befeast.okplayer.update.XXXXXX.json")"
trap 'rm -f "$BASELINE_MANIFEST" "$UPDATE_MANIFEST"' EXIT
python3 - \
  "$MANIFEST" \
  "$BASELINE_MANIFEST" \
  "$BASELINE_VERSION" \
  flatpak-beta-baseline \
  "$UPDATE_MANIFEST" \
  "$UPDATE_VERSION" \
  "$SOURCE_COMMIT" <<'PY'
import json
import sys
from pathlib import Path

source = Path(sys.argv[1])
for destination, version, build_sha in (
    (Path(sys.argv[2]), sys.argv[3], sys.argv[4]),
    (Path(sys.argv[5]), sys.argv[6], sys.argv[7]),
):
    manifest = json.loads(source.read_text())
    environment = manifest["modules"][0]["build-options"]["env"]
    environment["OKP_BUILD_VERSION"] = version
    environment["OKP_BUILD_SHA"] = build_sha
    destination.write_text(json.dumps(manifest, indent=4) + "\n")
PY

# Both manifests use the same declared sources. Prefetch once, then require both
# package builds to complete without network access.
flatpak-builder --user --download-only --force-clean --disable-rofiles-fuse \
  --state-dir="$STATE_DIR" \
  "$UPDATE_BUILD_DIR" "$MANIFEST"

flatpak-builder --user --disable-download --force-clean --disable-rofiles-fuse \
  --state-dir="$STATE_DIR" \
  --repo="$BASELINE_REPO_DIR" \
  --subject="OK Player Flatpak beta $BASELINE_VERSION" \
  --default-branch="$BRANCH" \
  "$BASELINE_BUILD_DIR" "$BASELINE_MANIFEST"

baseline_commit="$(ostree --repo="$BASELINE_REPO_DIR" rev-parse "$REF")"
flatpak build-bundle \
  "$BASELINE_REPO_DIR" "$BASELINE_BUNDLE" "$APP_ID" "$BRANCH"

# Seed the update repository from the exact baseline repository. Exporting the
# current package into this copy must create a direct child commit on the same
# ref, which is what Flatpak needs for update and rollback history.
cp -a "$BASELINE_REPO_DIR/." "$REPO_DIR/"
seed_commit="$(ostree --repo="$REPO_DIR" rev-parse "$REF")"
if [[ "$seed_commit" != "$baseline_commit" ]]; then
  echo "Flatpak update repository seed does not match the baseline commit" >&2
  exit 1
fi

flatpak-builder --user --disable-download --force-clean --disable-rofiles-fuse \
  --state-dir="$STATE_DIR" \
  --repo="$REPO_DIR" \
  --subject="OK Player Flatpak beta $UPDATE_VERSION" \
  --default-branch="$BRANCH" \
  "$UPDATE_BUILD_DIR" "$UPDATE_MANIFEST"

update_commit="$(ostree --repo="$REPO_DIR" rev-parse "$REF")"
update_parent="$(ostree --repo="$REPO_DIR" rev-parse "$update_commit^")"
if [[ "$update_commit" == "$baseline_commit" || "$update_parent" != "$baseline_commit" ]]; then
  echo "Flatpak update repository does not contain the required direct two-version history" >&2
  exit 1
fi

flatpak build-bundle "$REPO_DIR" "$UPDATE_BUNDLE" "$APP_ID" "$BRANCH"

cargo run --quiet --locked --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  flatpak-beta-artifact \
  --source-commit "$SOURCE_COMMIT" \
  --app-id "$APP_ID" \
  --arch "$ARCH" \
  --branch "$BRANCH" \
  --baseline-repository repo-baseline \
  --update-repository repo \
  --baseline-version "$BASELINE_VERSION" \
  --baseline-commit "$baseline_commit" \
  --baseline-bundle "$BASELINE_BUNDLE" \
  --update-version "$UPDATE_VERSION" \
  --update-commit "$update_commit" \
  --update-parent "$update_parent" \
  --update-bundle "$UPDATE_BUNDLE" >"$ARTIFACT_MANIFEST"

cargo run --quiet --locked --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  flatpak-beta-validate --manifest "$ARTIFACT_MANIFEST"

echo "Flatpak beta artifact ready: $BASELINE_VERSION -> $UPDATE_VERSION"
echo "Source commit: $SOURCE_COMMIT"
echo "Baseline OSTree commit: $baseline_commit"
echo "Update OSTree commit: $update_commit"
