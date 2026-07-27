#!/usr/bin/env bash
# Exercise the exported Flatpak beta repository through its full delivery
# lifecycle and assert every transition.
#
# Building a repository only proves that bytes were produced. This lane proves
# the bytes behave as a delivery channel: a fresh install lands on the baseline
# commit, an update moves the deployment to the child commit, a rollback returns
# to the parent commit, a restore moves forward again, and uninstall plus remote
# removal leave nothing behind. Every step compares the deployed OSTree commit
# reported by Flatpak against the identity recorded in the artifact manifest, so
# a command that reports success without changing the deployment fails the lane.
#
# Set OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL=<step-id> to skip one transition on
# purpose. The lane must then fail on that step, and it exits with the dedicated
# status 3 only when that step's own assertion is what failed. Any other failure
# - a missing tool, a missing artifact manifest, an unrelated broken transition -
# keeps its ordinary non-zero status, so a caller can tell "the control worked"
# apart from "something else went wrong before the control was reached". An
# unknown step id is rejected outright rather than silently disabling the
# control.
#
# Exit statuses: 0 success, 1 a lifecycle assertion failed, 2 bad invocation or
# missing inputs, 3 the negative control's own step failed as designed,
# 127 a required tool is missing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OKP_FLATPAK_OUT_DIR:-$ROOT/artifacts/linux/flatpak}"
ARTIFACT_MANIFEST="${OKP_FLATPAK_ARTIFACT_MANIFEST:-$OUT_DIR/flatpak-beta-artifact.json}"
EVIDENCE="${OKP_FLATPAK_LIFECYCLE_EVIDENCE:-$OUT_DIR/flatpak-lifecycle-ci.json}"
NEGATIVE_CONTROL="${OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL:-}"
NEGATIVE_CONTROL_EXIT=3
LAUNCH_TIMEOUT_SECONDS="${OKP_FLATPAK_LAUNCH_TIMEOUT_SECONDS:-180}"
APP_ID="com.befeast.okplayer"
BRANCH="beta"
REMOTE="ok-player-beta-ci"

# Transitions this lane knows how to skip. install-baseline is deliberately
# absent: nothing downstream could run without it, so naming it would not be a
# control.
CONTROLLABLE_STEPS=(
  launch-baseline
  update-current
  launch-current
  rollback-baseline
  launch-rollback
  restore-current
  uninstall
  remote-cleanup
)

if [[ -n "$NEGATIVE_CONTROL" ]]; then
  control_is_known=0
  for step in "${CONTROLLABLE_STEPS[@]}"; do
    if [[ "$step" == "$NEGATIVE_CONTROL" ]]; then
      control_is_known=1
    fi
  done
  if (( control_is_known == 0 )); then
    echo "Unknown OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL '$NEGATIVE_CONTROL'." >&2
    echo "A renamed or misspelled step id would skip nothing and pass, so it is rejected here." >&2
    echo "Known controllable steps: ${CONTROLLABLE_STEPS[*]}" >&2
    exit 2
  fi
fi

for tool in cargo cut dbus-run-session ffmpeg flatpak git python3 sha256sum timeout xdotool xvfb-run xwininfo; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

[[ -f "$ARTIFACT_MANIFEST" ]] || {
  echo "Missing Flatpak artifact manifest: run scripts/build-flatpak-beta.sh first" >&2
  exit 2
}

# The evidence must bind to the commit the workflow checked out, not to a value
# copied out of the artifact it is meant to police. Reading it back from the
# artifact would make the pull_request_head assertion unfalsifiable.
SOURCE_COMMIT="${OKP_ACCEPTANCE_SOURCE_COMMIT:-$(git -C "$ROOT" rev-parse HEAD)}"
[[ -n "$SOURCE_COMMIT" ]] || {
  echo "Could not determine the acceptance source commit" >&2
  exit 2
}

read -r BASELINE_COMMIT UPDATE_COMMIT UPDATE_BUNDLE <<<"$(
  python3 - "$ARTIFACT_MANIFEST" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text())
print(
    manifest["baseline"]["ostree_commit"],
    manifest["update"]["ostree_commit"],
    manifest["update"]["bundle"]["file_name"],
)
PY
)"

BASELINE_REPO_URL="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' "$OUT_DIR/repo-baseline")"
UPDATE_REPO_URL="$(python3 -c 'import pathlib,sys; print(pathlib.Path(sys.argv[1]).resolve().as_uri())' "$OUT_DIR/repo")"
ARTIFACT_SHA256="$(sha256sum "$OUT_DIR/$UPDATE_BUNDLE" | cut -d' ' -f1)"

LOG_DIR="${OKP_FLATPAK_LIFECYCLE_LOG_DIR:-$OUT_DIR/lifecycle-logs}"
rm -rf "$LOG_DIR"
mkdir -p "$LOG_DIR"
STEP_LOG="$LOG_DIR/steps.jsonl"
: >"$STEP_LOG"

# The sandbox only sees the Pictures grant, so the launch fixture lives there.
PICTURES_ROOT="${XDG_PICTURES_DIR:-$HOME/Pictures}"
mkdir -p "$PICTURES_ROOT"
FIXTURE_DIR="$(mktemp -d "$PICTURES_ROOT/.ok-player-lifecycle.XXXXXX")"
FIXTURE="$FIXTURE_DIR/lifecycle-red.mkv"

cleanup() {
  local status=$?
  rm -rf "$FIXTURE_DIR"
  flatpak uninstall --user -y "$APP_ID" >/dev/null 2>&1 || true
  flatpak remote-delete --user --force "$REMOTE" >/dev/null 2>&1 || true
  exit "$status"
}
trap cleanup EXIT

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'color=c=0x601010:s=320x180:r=24' \
  -f lavfi -i 'color=c=0xff3030:s=80x80:r=24' \
  -filter_complex "[0:v][1:v]overlay=x='mod(t*80,240)':y=50:shortest=1" \
  -t 30 -an -c:v ffv1 -level 3 -pix_fmt yuv420p "$FIXTURE"

deployed_commit() {
  flatpak info --user --show-commit "$APP_ID" "$BRANCH" 2>/dev/null || true
}

skip_transition() {
  [[ -n "$NEGATIVE_CONTROL" && "$NEGATIVE_CONTROL" == "$1" ]]
}

record_step() {
  local id="$1" commit="$2" status="$3" commit_json="null"
  if [[ -n "$commit" ]]; then
    commit_json="\"$commit\""
  fi
  printf '{"id": "%s", "deployed_commit": %s, "status": "%s"}\n' \
    "$id" "$commit_json" "$status" >>"$STEP_LOG"
}

write_evidence() {
  python3 - "$ARTIFACT_MANIFEST" "$STEP_LOG" "$EVIDENCE" "$ARTIFACT_SHA256" "$SOURCE_COMMIT" <<'PY'
import json
import sys
from pathlib import Path

artifact = json.loads(Path(sys.argv[1]).read_text())
steps = [
    json.loads(line)
    for line in Path(sys.argv[2]).read_text().splitlines()
    if line.strip()
]
Path(sys.argv[3]).write_text(
    json.dumps(
        {
            "schema_version": 2,
            "pull_request_head": sys.argv[5],
            "downloaded_artifact_sha256": sys.argv[4],
            "desktop": "headless",
            "session": "headless-ci",
            "artifact": artifact,
            "steps": steps,
        },
        indent=2,
    )
    + "\n"
)
PY
}

fail_step() {
  local id="$1" commit="$2" message="$3" status=1
  record_step "$id" "$commit" "fail"
  write_evidence
  echo "Flatpak lifecycle step $id failed: $message" >&2
  # Only the controlled step's own assertion earns the dedicated status. A
  # caller that requires exactly this status therefore cannot be satisfied by a
  # preflight abort, a missing artifact, or an unrelated broken transition.
  if [[ -n "$NEGATIVE_CONTROL" && "$id" == "$NEGATIVE_CONTROL" ]]; then
    status="$NEGATIVE_CONTROL_EXIT"
  fi
  exit "$status"
}

# Assert the deployment identity a transition claims to have produced.
assert_deployed() {
  local id="$1" expected="$2" observed
  observed="$(deployed_commit)"
  if [[ "$observed" != "$expected" ]]; then
    fail_step "$id" "$observed" "deployed '${observed:-<none>}', expected '$expected'"
  fi
  record_step "$id" "$observed" "pass"
  echo "Flatpak lifecycle step $id: deployed $observed"
}

assert_absent() {
  local id="$1"
  if flatpak info --user "$APP_ID" "$BRANCH" >/dev/null 2>&1; then
    fail_step "$id" "$(deployed_commit)" "$APP_ID is still installed"
  fi
  record_step "$id" "" "pass"
  echo "Flatpak lifecycle step $id: $APP_ID is no longer installed"
}

# Launch the deployed revision under a throwaway X server and require a mapped
# top-level window owned by the packaged application.
launch_probe() {
  local id="$1" expected="$2"
  local log="$LOG_DIR/$id.log"
  if skip_transition "$id"; then
    fail_step "$id" "$(deployed_commit)" "the negative control skipped this launch"
  fi
  if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp +extension GLX +render -noreset' \
    dbus-run-session -- bash -s -- "$FIXTURE" "$LAUNCH_TIMEOUT_SECONDS" "$APP_ID" \
    >"$log" 2>&1 <<'PROBE'
set -euo pipefail
FIXTURE="$1"
LAUNCH_TIMEOUT_SECONDS="$2"
APP_ID="$3"

export GDK_BACKEND=x11
export NO_AT_BRIDGE=1

app_pid=""
cleanup_app() {
  if [[ -n "$app_pid" ]]; then
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
  flatpak kill "$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup_app EXIT

timeout "$LAUNCH_TIMEOUT_SECONDS" flatpak run --user \
  --nodevice=dri --nosocket=wayland --socket=x11 \
  --env=GDK_BACKEND=x11 \
  --env=OKP_SKIP_UPDATE_CHECK=1 --env=OKP_DISABLE_MPRIS=1 \
  "$APP_ID" "$FIXTURE" &
app_pid=$!

window_id=""
for _ in $(seq 1 200); do
  mapfile -t windows < <(xdotool search --onlyvisible --name '^OK Player$' 2>/dev/null || true)
  if [[ "${#windows[@]}" -eq 1 ]]; then
    window_id="${windows[0]}"
    break
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then
    echo "The packaged application exited before mapping a window" >&2
    exit 1
  fi
  sleep 0.5
done

[[ -n "$window_id" ]] || {
  echo "No mapped OK Player top-level window appeared" >&2
  exit 1
}

window_info="$(xwininfo -id "$window_id")"
printf '%s\n' "$window_info"
map_state="$(printf '%s\n' "$window_info" | sed -n 's/^[[:space:]]*Map State:[[:space:]]*//p')"
[[ "$map_state" == "IsViewable" ]] || {
  echo "OK Player window map state was '$map_state', expected IsViewable" >&2
  exit 1
}
PROBE
  then
    fail_step "$id" "$(deployed_commit)" "the deployed revision did not map a window (see $log)"
  fi
  assert_deployed "$id" "$expected"
}

echo "Flatpak lifecycle: baseline $BASELINE_COMMIT -> update $UPDATE_COMMIT"

flatpak uninstall --user -y "$APP_ID" >/dev/null 2>&1 || true
flatpak remote-delete --user --force "$REMOTE" >/dev/null 2>&1 || true

# install-baseline: a fresh machine must land on version N, not on the newest
# commit that happens to exist in the repository.
flatpak remote-add --user --no-gpg-verify "$REMOTE" "$BASELINE_REPO_URL"
flatpak install --user -y "$REMOTE" "$APP_ID" "$BRANCH"
assert_deployed "install-baseline" "$BASELINE_COMMIT"

launch_probe "launch-baseline" "$BASELINE_COMMIT"

# update-current: pointing the same remote at the two-commit repository must
# move the deployment to the child commit.
flatpak remote-modify --user --url="$UPDATE_REPO_URL" "$REMOTE"
if skip_transition "update-current"; then
  echo "Negative control: skipping the update-current transition"
else
  flatpak update --user -y "$APP_ID"
fi
assert_deployed "update-current" "$UPDATE_COMMIT"

launch_probe "launch-current" "$UPDATE_COMMIT"

# rollback-baseline: history must still carry the parent commit and the
# deployment must return to it.
if skip_transition "rollback-baseline"; then
  echo "Negative control: skipping the rollback-baseline transition"
else
  flatpak update --user -y --commit="$BASELINE_COMMIT" "$APP_ID"
fi
assert_deployed "rollback-baseline" "$BASELINE_COMMIT"

launch_probe "launch-rollback" "$BASELINE_COMMIT"

# restore-current: a rollback must not strand the installation on the old commit.
if skip_transition "restore-current"; then
  echo "Negative control: skipping the restore-current transition"
else
  flatpak update --user -y --commit="$UPDATE_COMMIT" "$APP_ID"
fi
assert_deployed "restore-current" "$UPDATE_COMMIT"

if skip_transition "uninstall"; then
  echo "Negative control: skipping the uninstall transition"
else
  flatpak uninstall --user -y "$APP_ID"
fi
assert_absent "uninstall"

if skip_transition "remote-cleanup"; then
  echo "Negative control: skipping the remote-cleanup transition"
else
  flatpak remote-delete --user "$REMOTE"
fi
if flatpak remotes --user --columns=name | tr -d '[:blank:]' | grep -qx "$REMOTE"; then
  fail_step "remote-cleanup" "" "the beta remote is still configured"
fi
record_step "remote-cleanup" "" "pass"
echo "Flatpak lifecycle step remote-cleanup: the beta remote is gone"

write_evidence

cargo run --quiet --locked --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  flatpak-lifecycle-validate --manifest "$EVIDENCE" --transitions-only

if [[ -n "$NEGATIVE_CONTROL" ]]; then
  echo "Negative control '$NEGATIVE_CONTROL' did not fail the lifecycle lane" >&2
  exit 1
fi

echo "Flatpak lifecycle passed: install, update, rollback, restore, uninstall, remote cleanup"
