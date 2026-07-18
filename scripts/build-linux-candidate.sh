#!/usr/bin/env bash
# Coalesced native Ubuntu candidate builder (issue #340).
#
# A self-hosted x86_64 Ubuntu builder invokes this on a schedule. It polls
# origin/main, skips an unchanged SHA, holds a single-run lock so overlapping
# schedules cannot race, builds a clean checkout of HEAD (coalescing every merge
# since the last build), runs the bounded gates, and emits a stable artifact
# bundle the candidate channel (#339) can promote without rebuilding.
#
# It deliberately does NOT publish, tag, create a GitHub Release, or move any
# updater feed. Promotion is a separate step (scripts/promote-linux-candidate.sh).
# A gate failure exits non-zero and leaves the last-built marker and feed
# untouched. Progress heartbeats let an external watchdog tell an active build
# from a stalled build from an idle unchanged main.
#
# Configuration (all optional; no host-specific values are baked in):
#   OKP_CANDIDATE_STATE_DIR   persistent state/lock/heartbeats
#                             (default: ${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate)
#   OKP_CANDIDATE_REPO_URL    clone source (default: public GitHub repo)
#   OKP_CANDIDATE_BRANCH      branch to track (default: main)
#   OKP_CANDIDATE_VERSION_BASE  candidate version base (default: 0.11.0-beta.0)
#   OKP_CANDIDATE_NATIVE_SMOKE  optional command; when set its evidence is required
#   OKP_CANDIDATE_STALL_SECONDS watchdog stall threshold recorded for reference
set -euo pipefail

STATE_DIR="${OKP_CANDIDATE_STATE_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/ok-player-candidate}"
SCRIPT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_URL="${OKP_CANDIDATE_REPO_URL:-https://github.com/BeFeast/ok-player.git}"
BRANCH="${OKP_CANDIDATE_BRANCH:-main}"
VERSION_BASE="${OKP_CANDIDATE_VERSION_BASE:-0.11.0-beta.0}"
STALL_SECONDS="${OKP_CANDIDATE_STALL_SECONDS:-900}"

MIRROR="$STATE_DIR/mirror.git"
CHECKOUT="$STATE_DIR/checkout"
OUT_ROOT="$STATE_DIR/out"
LOCK="$STATE_DIR/build.lock"
LOCK_OWNER="$STATE_DIR/build.lock.owner.json"
LAST_BUILT="$STATE_DIR/last-built.sha"
BUILD_NUMBER_FILE="$STATE_DIR/build-number"
HEARTBEAT="$STATE_DIR/heartbeat.jsonl"

mkdir -p "$STATE_DIR"

# Direct invocations acquire through the Rust coordinator. Its descriptor is
# close-on-exec, so a delayed package/headless child cannot retain the lock.
# The repository workflow sets OKP_CANDIDATE_LOCK_HELD while it owns one
# build-and-publish critical section around this script and the publisher.
if [[ "${OKP_CANDIDATE_LOCK_HELD:-}" != "1" ]]; then
  if [[ -n "${OKP_CANDIDATE_LOCK_CLI:-}" ]]; then
    LOCK_CLI="$OKP_CANDIDATE_LOCK_CLI"
  else
    CC="${CC:-/usr/bin/cc}" cargo build --quiet \
      --manifest-path "$SCRIPT_ROOT/rust/Cargo.toml" \
      -p okp-core --bin okp-candidate
    LOCK_CLI="$SCRIPT_ROOT/rust/target/debug/okp-candidate"
  fi
  [[ -x "$LOCK_CLI" ]] || { echo "candidate lock coordinator not found: $LOCK_CLI" >&2; exit 1; }
  exec "$LOCK_CLI" lock-run \
    --lock "$LOCK" \
    --owner "$LOCK_OWNER" \
    --phase build \
    --coalesce \
    -- "$0" "$@"
fi

# Publish the stall threshold the external watchdog should use with
# `okp-candidate classify --stall-after`. Kept beside the heartbeats so the
# builder and watchdog agree without a private out-of-band value.
printf '%s\n' "$STALL_SECONDS" >"$STATE_DIR/stall-after-seconds"

now_utc() { date -u +%Y-%m-%dT%H:%M:%SZ; }
now_unix() { date -u +%s; }

# Append a heartbeat line a watchdog can classify with `okp-candidate classify`.
heartbeat() {
  local phase="$1" note="${2:-}" sha="${3:-}"
  printf '{"phase":"%s","unix_seconds":%s,"note":"%s","source_sha":"%s"}\n' \
    "$phase" "$(now_unix)" "$note" "$sha" >>"$HEARTBEAT"
  echo "[$(now_utc)] $phase: $note"
}

# --- Resolve origin/main and decide whether to build -------------------------
if [[ ! -d "$MIRROR" ]]; then
  git clone --mirror "$REPO_URL" "$MIRROR" >/dev/null 2>&1 \
    || { echo "failed to mirror $REPO_URL" >&2; exit 1; }
fi
git -C "$MIRROR" remote set-url origin "$REPO_URL"
git -C "$MIRROR" fetch --prune origin "+refs/heads/${BRANCH}:refs/heads/${BRANCH}" >/dev/null 2>&1 \
  || { echo "failed to fetch ${BRANCH} from $REPO_URL" >&2; exit 1; }

HEAD_SHA="$(git -C "$MIRROR" rev-parse "refs/heads/${BRANCH}")"
LAST_SHA="$(cat "$LAST_BUILT" 2>/dev/null || true)"

# Skip an unchanged SHA. A blank marker means "never built" and always builds;
# building HEAD coalesces every merge landed since the last build.
if [[ -n "$LAST_SHA" && "$LAST_SHA" == "$HEAD_SHA" ]]; then
  heartbeat idle "main unchanged at ${HEAD_SHA}; nothing to build" "$HEAD_SHA"
  exit 0
fi

# --- Clean checkout of exactly HEAD ------------------------------------------
heartbeat building "clean checkout of ${HEAD_SHA}" "$HEAD_SHA"
rm -rf "$CHECKOUT"
git clone --shared --branch "$BRANCH" "$MIRROR" "$CHECKOUT" >/dev/null 2>&1 \
  || { echo "failed to clone checkout" >&2; exit 1; }
git -C "$CHECKOUT" checkout --quiet --detach "$HEAD_SHA"
BUILD_SHA="$(git -C "$CHECKOUT" rev-parse HEAD)"
[[ "$BUILD_SHA" == "$HEAD_SHA" ]] || { echo "checkout SHA drift" >&2; exit 1; }

# --- Version and monotonic build number --------------------------------------
BUILD_NUMBER="$(( $(cat "$BUILD_NUMBER_FILE" 2>/dev/null || echo 0) + 1 ))"
echo "$BUILD_NUMBER" >"$BUILD_NUMBER_FILE"
VERSION="$(CC=/usr/bin/cc cargo run --quiet \
  --manifest-path "$CHECKOUT/rust/Cargo.toml" \
  -p okp-core --bin okp-candidate -- \
  version --base "$VERSION_BASE" --build "$BUILD_NUMBER")"
STARTED_AT="$(now_utc)"

OUT_DIR="$OUT_ROOT/${BUILD_NUMBER}"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

CANDIDATE_CLI="$CHECKOUT/rust/target/release/okp-candidate"

# Gate results accumulate here as `--gate name:status` arguments.
GATE_ARGS=()
record_gate() { GATE_ARGS+=(--gate "$1:$2"); }

# Run a gate; on failure, record it, emit a final failing record, and abort with
# the last-built marker untouched so the feed never moves.
abort_failed() {
  local gate="$1"
  record_gate "$gate" failed
  heartbeat building "gate ${gate} FAILED; aborting without promotion" "$BUILD_SHA"
  echo "candidate build ${VERSION} failed at gate ${gate}" >&2
  exit 1
}

run_gate() {
  local gate="$1"; shift
  heartbeat building "gate ${gate}" "$BUILD_SHA"
  if "$@"; then
    record_gate "$gate" passed
  else
    abort_failed "$gate"
  fi
}

export CC="${CC:-/usr/bin/cc}"
export OKP_SKIP_UPDATE_CHECK=1
export OKP_RUN_VELOPACK_PACK_TEST=1

source "$CHECKOUT/scripts/linux-bundled-mpv-env.sh"
run_gate bundled-mpv okp_use_linux_bundled_mpv

# --- Bounded gates -----------------------------------------------------------
run_gate fmt \
  cargo fmt --manifest-path "$CHECKOUT/rust/Cargo.toml" --all -- --check
run_gate clippy \
  cargo clippy --manifest-path "$CHECKOUT/rust/Cargo.toml" --workspace --all-targets -- -D warnings
run_gate workspace-tests \
  cargo test --manifest-path "$CHECKOUT/rust/Cargo.toml" --workspace

# Packaging lanes produce the installable artifacts under $CHECKOUT/artifacts/linux.
run_gate deb-package \
  "$CHECKOUT/scripts/package-linux-deb.sh" "$VERSION"
run_gate appimage-package \
  env OKP_LINUX_CHANNEL=linux-candidate \
  "$CHECKOUT/scripts/package-linux-velopack.sh" "$VERSION"

DEB="$CHECKOUT/artifacts/linux/deb/ok-player_${VERSION}_amd64.deb"
APPIMAGE="$CHECKOUT/artifacts/linux/velopack/OK-Player-${VERSION}-x86_64.AppImage"

# Package identity + SHA-256 verification: a checksum manifest over the
# user-facing installables plus package-identity.json bound to these exact bytes.
package_identity_gate() {
  ( cd "$CHECKOUT/artifacts/linux/deb" && sha256sum -- *.deb
    cd "$CHECKOUT/artifacts/linux/velopack" && sha256sum -- "OK-Player-${VERSION}-x86_64.AppImage"
  ) >"$CHECKOUT/artifacts/linux/SHA256SUMS"
  "$CHECKOUT/scripts/write-linux-acceptance-template.sh" "$VERSION" "$BUILD_SHA"
}
run_gate package-identity package_identity_gate

# Clean install / upgrade / uninstall in a disposable environment.
run_gate install-upgrade-uninstall-smoke \
  "$CHECKOUT/scripts/smoke-linux-install-upgrade.sh" "$DEB" "$OUT_DIR/install-smoke"

# Exercise the screenshot shortcut from the exact Debian payload, not merely
# the build-tree binary. The resulting image and hash travel with the verified
# candidate bundle; compositor and clipboard claims remain operator-only.
deb_screenshot_smoke() {
  local smoke_root="$OUT_DIR/deb-screenshot-root"
  local smoke_output="$OUT_DIR/deb-screenshot-smoke"
  local fixture_dir="$OUT_DIR/deb-screenshot-fixtures"
  mkdir -p "$smoke_root" "$CHECKOUT/artifacts/linux/acceptance"
  dpkg-deb -x "$DEB" "$smoke_root"
  "$CHECKOUT/scripts/generate-linux-acceptance-media.sh" "$fixture_dir"
  "$CHECKOUT/scripts/smoke-linux-screenshot.sh" \
    "$smoke_root/usr/lib/ok-player/ok-player" \
    "$fixture_dir/dark-with-chapters.mkv" \
    "$smoke_output"

  local saved_path saved_sha256 artifact
  saved_path="$(sed -n 's/^screenshot_path=//p' "$smoke_output/results.txt")"
  saved_sha256="$(sed -n 's/^screenshot_sha256=//p' "$smoke_output/results.txt")"
  [[ -n "$saved_path" && -s "$saved_path" && -n "$saved_sha256" ]]
  artifact="$CHECKOUT/artifacts/linux/acceptance/deb-screenshot.png"
  cp "$saved_path" "$artifact"
  [[ "$(sha256sum "$artifact" | awk '{print $1}')" == "$saved_sha256" ]]
  printf '%s\n' \
    'evidence_level=xvfb-render' \
    'package=debian-payload' \
    'artifact=acceptance/deb-screenshot.png' \
    "sha256=$saved_sha256" \
    'not_proven=GNOME Wayland, clipboard, compositor, portal, focus' \
    >"$CHECKOUT/artifacts/linux/acceptance/deb-screenshot.txt"
}
run_gate deb-screenshot-smoke deb_screenshot_smoke

# Headless launch smoke: prove the idle surface once, then require the complete
# fit-only lifecycle three consecutive times with no retry inside the gate.
headless_launch_smoke() {
  OKP_MAIN_WINDOW_IDLE_ONLY=1 \
    "$CHECKOUT/scripts/smoke-linux-main-window.sh" \
    "$CHECKOUT/rust/target/release/okp-linux-gtk" "$OUT_DIR/headless-launch/idle"
  OKP_WINDOW_FIT_SOURCE_SHA="$BUILD_SHA" \
    "$CHECKOUT/scripts/run-linux-window-fit-series.sh" \
    "$CHECKOUT/rust/target/release/okp-linux-gtk" "$OUT_DIR/headless-launch/fit-series"
}
run_gate headless-launch-smoke headless_launch_smoke

# Optional native-hardware smoke hook whose evidence the operator may require.
REQUIRE_NATIVE=()
if [[ -n "${OKP_CANDIDATE_NATIVE_SMOKE:-}" ]]; then
  REQUIRE_NATIVE=(--require-native-hardware)
  run_gate native-hardware-smoke bash -c "$OKP_CANDIDATE_NATIVE_SMOKE"
fi

# --- Stage the artifact bundle -----------------------------------------------
heartbeat building "staging artifact bundle" "$BUILD_SHA"
mkdir -p "$OUT_DIR/artifacts"
cp -a "$CHECKOUT/artifacts/linux/." "$OUT_DIR/artifacts/"

# The decision/record binary is built from this exact checkout.
cargo build --manifest-path "$CHECKOUT/rust/Cargo.toml" --release -p okp-core --bin okp-candidate

FINISHED_AT="$(now_utc)"
STAGED_DEB="$OUT_DIR/artifacts/deb/$(basename -- "$DEB")"
STAGED_APPIMAGE="$OUT_DIR/artifacts/velopack/$(basename -- "$APPIMAGE")"

"$CANDIDATE_CLI" record \
  --source-sha "$BUILD_SHA" \
  --build-number "$BUILD_NUMBER" \
  --version "$VERSION" \
  --started-at "$STARTED_AT" \
  --finished-at "$FINISHED_AT" \
  "${REQUIRE_NATIVE[@]}" \
  --deb "$STAGED_DEB" \
  --appimage "$STAGED_APPIMAGE" \
  "${GATE_ARGS[@]}" \
  >"$OUT_DIR/candidate-build.json"

# --- Promotability check (build only decides; it never moves the feed) -------
if ! "$CANDIDATE_CLI" promotable --record "$OUT_DIR/candidate-build.json"; then
  heartbeat building "build ${VERSION} is not promotable; marker untouched" "$BUILD_SHA"
  echo "candidate build ${VERSION} is not promotable" >&2
  exit 1
fi
if ! "$CANDIDATE_CLI" verify-bundle --bundle "$OUT_DIR"; then
  heartbeat building "bundle verification failed; marker untouched" "$BUILD_SHA"
  exit 1
fi

# Record this SHA as successfully built so the next schedule skips it. This is
# the builder's own idempotency marker, NOT feed promotion.
echo "$BUILD_SHA" >"$LAST_BUILT"
echo "$OUT_DIR" >"$STATE_DIR/last-bundle.path"

heartbeat idle "candidate ${VERSION} (source ${BUILD_SHA}) built; bundle at ${OUT_DIR}" "$BUILD_SHA"
echo "Candidate bundle: $OUT_DIR"
echo "  candidate-build.json, SHA256SUMS, package-identity.json, deb + AppImage under artifacts/"
echo "Promote separately with scripts/promote-linux-candidate.sh (#339 consumes the bundle)."
