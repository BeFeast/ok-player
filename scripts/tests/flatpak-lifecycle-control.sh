#!/usr/bin/env bash
# Behavioural self-test for the lifecycle lane's negative control.
#
# The negative control only means something if a caller can distinguish "the
# controlled transition's own assertion fired" from "the script died before it
# got there". That is a two-sided property, and both sides are checked here:
# status 3 must appear when the controlled step fails, and must NOT appear when
# some other step fails while a control is set. Without the second half, routing
# every failure to status 3 would satisfy the workflow's check.
#
# This test drives scripts/smoke-linux-flatpak-lifecycle.sh against a scripted
# stand-in for flatpak and checks the outcomes that matter:
#
#   1. an unmodified run reports success,
#   2. a control on a transition step fails with status 3 and names that step,
#   3. a control on a launch step also fails with status 3,
#   4. a failure at a step other than the controlled one keeps status 1,
#   5. a misspelled control id is rejected instead of quietly passing,
#   6. a missing artifact manifest keeps its own status rather than 3, and
#   7. the emitted evidence records the caller's source commit rather than
#      echoing the artifact's own, which is what makes the pull_request_head
#      assertion in okp-core falsifiable at all.
#
# The stand-ins fake Flatpak's deployment bookkeeping and the X launch probe.
# They intentionally prove nothing about real Flatpak behaviour - that is the
# job of the packaged lane in CI. What they prove is that the control wiring is
# not vacuous, which no amount of running the real lane can show.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LANE="$ROOT/scripts/smoke-linux-flatpak-lifecycle.sh"

for tool in bash git python3 sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

WORK="$(mktemp -d "${TMPDIR:-/tmp}/okp-flatpak-control-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

STUB_BIN="$WORK/bin"
STATE="$WORK/state"
OUT_DIR="$WORK/out"
PICTURES="$WORK/pictures"
mkdir -p "$STUB_BIN" "$STATE" "$OUT_DIR" "$PICTURES"

BASELINE_COMMIT="$(printf 'a%.0s' {1..64})"
UPDATE_COMMIT="$(printf 'c%.0s' {1..64})"
SOURCE_COMMIT="$(printf '1%.0s' {1..40})"
UPDATE_BUNDLE="OK-Player-0.11.0-beta.1.flatpak"

cat >"$STUB_BIN/flatpak" <<'STUB'
#!/usr/bin/env bash
# Deployment bookkeeping stand-in: remembers which commit is deployed and which
# remotes exist, so the lane's assertions have something real to read back.
set -euo pipefail
command="${1:-}"
shift || true
positional=()
for argument in "$@"; do
  case "$argument" in
    -*) ;;
    *) positional+=("$argument") ;;
  esac
done
deployed_file="$OKP_STUB_STATE/deployed"
remotes_file="$OKP_STUB_STATE/remotes"
touch "$remotes_file"

case "$command" in
  info)
    [[ -s "$deployed_file" ]] || exit 1
    for argument in "$@"; do
      if [[ "$argument" == "--show-commit" ]]; then
        cat "$deployed_file"
      fi
    done
    ;;
  install)
    printf '%s\n' "$OKP_STUB_BASELINE_COMMIT" >"$deployed_file"
    ;;
  update)
    [[ -s "$deployed_file" ]] || { echo "not installed" >&2; exit 1; }
    target="$OKP_STUB_UPDATE_COMMIT"
    for argument in "$@"; do
      case "$argument" in
        --commit=*) target="${argument#--commit=}" ;;
      esac
    done
    # Report success without moving the deployment. That is the failure mode the
    # lane's deployed-commit assertions exist to catch, and it is how this test
    # forces a failure at a step other than the controlled one.
    if [[ -n "${OKP_STUB_REFUSE_COMMIT:-}" && "$target" == "$OKP_STUB_REFUSE_COMMIT" ]]; then
      exit 0
    fi
    printf '%s\n' "$target" >"$deployed_file"
    ;;
  uninstall)
    rm -f "$deployed_file"
    ;;
  remote-add)
    printf '%s\n' "${positional[0]}" >>"$remotes_file"
    ;;
  remote-modify) ;;
  remote-delete)
    grep -vxF "${positional[0]}" "$remotes_file" >"$remotes_file.next" || true
    mv "$remotes_file.next" "$remotes_file"
    ;;
  remotes)
    cat "$remotes_file"
    ;;
  kill) ;;
  *) ;;
esac
STUB

cat >"$STUB_BIN/xvfb-run" <<'STUB'
#!/usr/bin/env bash
# The launch probe reads its body from stdin; drain it and report a mapped
# window. Real window mapping is proven by the packaged lane, not here.
set -euo pipefail
cat >/dev/null
exit 0
STUB

cat >"$STUB_BIN/ffmpeg" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
fixture="${*: -1}"
: >"$fixture"
STUB

cat >"$STUB_BIN/cargo" <<'STUB'
#!/usr/bin/env bash
# Evidence-schema validation is covered by the okp-core unit tests; this test is
# about the control wiring around it.
exit 0
STUB

for name in dbus-run-session xdotool xwininfo; do
  printf '#!/usr/bin/env bash\nexit 0\n' >"$STUB_BIN/$name"
done

chmod +x "$STUB_BIN"/*

write_artifact_manifest() {
  python3 - "$OUT_DIR/flatpak-beta-artifact.json" \
    "$SOURCE_COMMIT" "$BASELINE_COMMIT" "$UPDATE_COMMIT" "$UPDATE_BUNDLE" <<'PY'
import json
import sys
from pathlib import Path

destination, source_commit, baseline_commit, update_commit, update_bundle = sys.argv[1:6]
Path(destination).write_text(
    json.dumps(
        {
            "schema_version": 1,
            "source_commit": source_commit,
            "app_id": "com.befeast.okplayer",
            "arch": "x86_64",
            "branch": "beta",
            "baseline_repository": "repo-baseline",
            "update_repository": "repo",
            "baseline": {
                "version": "0.11.0-beta.0",
                "ostree_commit": baseline_commit,
                "bundle": {
                    "file_name": "OK-Player-0.11.0-beta.0.flatpak",
                    "sha256": "b" * 64,
                },
            },
            "update": {
                "version": "0.11.0-beta.1",
                "ostree_commit": update_commit,
                "bundle": {"file_name": update_bundle, "sha256": "d" * 64},
            },
            "update_parent_commit": baseline_commit,
        },
        indent=2,
    )
    + "\n"
)
PY
}

write_artifact_manifest
mkdir -p "$OUT_DIR/repo-baseline" "$OUT_DIR/repo"
: >"$OUT_DIR/$UPDATE_BUNDLE"

run_lane() {
  local control="$1" log="$2" head="${3:-$SOURCE_COMMIT}" refuse="${4:-}" status=0
  rm -f "$STATE/deployed" "$STATE/remotes"
  # Every OKP_FLATPAK_* input is set explicitly. The workflow exports some of
  # them job-wide for the real lane, and inheriting those here would point this
  # test at the real artifact directory instead of its own fixture.
  env \
    PATH="$STUB_BIN:$PATH" \
    HOME="$WORK" \
    XDG_PICTURES_DIR="$PICTURES" \
    OKP_STUB_STATE="$STATE" \
    OKP_STUB_BASELINE_COMMIT="$BASELINE_COMMIT" \
    OKP_STUB_UPDATE_COMMIT="$UPDATE_COMMIT" \
    OKP_STUB_REFUSE_COMMIT="$refuse" \
    OKP_ACCEPTANCE_SOURCE_COMMIT="$head" \
    OKP_FLATPAK_OUT_DIR="$OUT_DIR" \
    OKP_FLATPAK_ARTIFACT_MANIFEST="$OUT_DIR/flatpak-beta-artifact.json" \
    OKP_FLATPAK_LIFECYCLE_EVIDENCE="$OUT_DIR/evidence.json" \
    OKP_FLATPAK_LIFECYCLE_LOG_DIR="$OUT_DIR/logs" \
    OKP_FLATPAK_LAUNCH_TIMEOUT_SECONDS=30 \
    OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL="$control" \
    "$LANE" >"$log" 2>&1 || status=$?
  printf '%s\n' "$status"
}

failures=0

expect() {
  local label="$1" expected_status="$2" actual_status="$3" log="$4" expected_text="${5:-}"
  if [[ "$actual_status" != "$expected_status" ]]; then
    echo "FAIL $label: expected status $expected_status, got $actual_status" >&2
    sed -e 's/^/    /' "$log" >&2
    failures=$((failures + 1))
    return
  fi
  if [[ -n "$expected_text" ]] && ! grep -qF "$expected_text" "$log"; then
    echo "FAIL $label: expected output to contain: $expected_text" >&2
    sed -e 's/^/    /' "$log" >&2
    failures=$((failures + 1))
    return
  fi
  echo "ok $label"
}

status="$(run_lane "" "$WORK/clean.log")"
expect "an uncontrolled run completes the lifecycle" 0 "$status" "$WORK/clean.log" \
  "Flatpak lifecycle passed"

status="$(run_lane "update-current" "$WORK/update.log")"
expect "a controlled update fails with the dedicated status" 3 "$status" "$WORK/update.log" \
  "Flatpak lifecycle step update-current failed: deployed"

status="$(run_lane "launch-current" "$WORK/launch.log")"
expect "a controlled launch fails with the dedicated status" 3 "$status" "$WORK/launch.log" \
  "Flatpak lifecycle step launch-current failed"

# The discriminating half of the contract. Status 3 must mean "the controlled
# step's own assertion fired" and nothing else, so a different step failing
# while a control is set must keep the ordinary status 1. Without this check,
# routing every failure to status 3 passes every other check in this file and
# the workflow's status-3 requirement stops carrying any information.
status="$(run_lane "uninstall" "$WORK/other-step.log" "$SOURCE_COMMIT" "$BASELINE_COMMIT")"
expect "a failure at a step other than the controlled one is not status 3" 1 "$status" \
  "$WORK/other-step.log" "Flatpak lifecycle step rollback-baseline failed: deployed"

status="$(run_lane "update-currrent" "$WORK/typo.log")"
expect "a misspelled control id is rejected" 2 "$status" "$WORK/typo.log" \
  "Unknown OKP_FLATPAK_LIFECYCLE_NEGATIVE_CONTROL"

mv "$OUT_DIR/flatpak-beta-artifact.json" "$OUT_DIR/flatpak-beta-artifact.json.hidden"
status="$(run_lane "update-current" "$WORK/no-artifact.log")"
expect "a missing artifact manifest does not look like a fired control" 2 "$status" \
  "$WORK/no-artifact.log" "Missing Flatpak artifact manifest"
mv "$OUT_DIR/flatpak-beta-artifact.json.hidden" "$OUT_DIR/flatpak-beta-artifact.json"

# The lane must record the commit its caller checked out. If it copied the
# artifact's own source_commit into the evidence, the okp-core assertion that
# the two agree could never fail and the lifecycle evidence would be unbound.
FOREIGN_HEAD="$(printf '9%.0s' {1..40})"
status="$(run_lane "" "$WORK/binding.log" "$FOREIGN_HEAD")"
recorded_head="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["pull_request_head"])' \
  "$OUT_DIR/evidence.json")"
if [[ "$status" != "0" || "$recorded_head" != "$FOREIGN_HEAD" ]]; then
  echo "FAIL the evidence binds to the caller's source commit: status $status, recorded '$recorded_head', expected '$FOREIGN_HEAD'" >&2
  failures=$((failures + 1))
else
  echo "ok the evidence binds to the caller's source commit"
fi

if (( failures > 0 )); then
  echo "$failures lifecycle negative-control checks failed" >&2
  exit 1
fi

echo "Flatpak lifecycle negative control self-test passed"
