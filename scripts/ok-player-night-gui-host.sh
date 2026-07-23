#!/usr/bin/env bash
# Host-side Wave A/B/C dispatcher. Site-local live-desktop automation is supplied by hooks.
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage:
  ok-player-night-gui-host.sh probe <host-role>
  ok-player-night-gui-host.sh run <host-role> <host-alias> <YYYYMMDD> <suite-id>

Hooks live in OKP_QA_HOOK_DIR (default: ~/.local/lib/ok-player-night-gui-qa/hooks):
  probe-seat <host-role>
  probe-dual-head <artifact-dir> <host-role> <suite-id>
  prepare-candidate <artifact-dir> <host-role> <suite-id>
  run-action <action> <artifact-dir> <host-role> <suite-id>

Hook exit 0 is PASS, 75 is NOT RUN, and any other exit is FAIL.
EOF
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 64
}

require_token() {
  local label="$1" value="$2"
  [[ "$value" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "invalid $label"
}

HOOK_DIR="${OKP_QA_HOOK_DIR:-$HOME/.local/lib/ok-player-night-gui-qa/hooks}"

default_seat_probe() {
  command -v loginctl >/dev/null 2>&1 || return 75
  local session_id properties
  session_id="$(loginctl list-sessions --no-legend 2>/dev/null | awk -v user="$(id -un)" '$3 == user && $4 == "seat0" { print $1; exit }')"
  [[ -n "$session_id" ]] || return 1
  properties="$(loginctl show-session "$session_id" \
    -p Active -p LockedHint -p Remote -p State -p Type 2>/dev/null)" || return 1
  grep -Fxq 'Active=yes' <<<"$properties" || return 1
  grep -Fxq 'Remote=no' <<<"$properties" || return 1
  grep -Fxq 'LockedHint=yes' <<<"$properties" && return 1
  grep -Eq '^Type=(wayland|x11)$' <<<"$properties" || return 1
  printf 'seat=seat0 status=ready\n'
}

probe_seat() {
  local role="$1"
  if [[ -x "$HOOK_DIR/probe-seat" ]]; then
    "$HOOK_DIR/probe-seat" "$role"
  else
    default_seat_probe
  fi
}

if [[ "${1:-}" == "probe" ]]; then
  [[ $# -eq 2 ]] || { usage; exit 64; }
  require_token host-role "$2"
  probe_seat "$2"
  exit $?
fi

[[ "${1:-}" == "run" && $# -eq 5 ]] || { usage; exit 64; }
role="$2"
host_alias="$3"
run_date="$4"
suite_id="$5"
require_token host-role "$role"
require_token host-alias "$host_alias"
require_token suite-id "$suite_id"
[[ "$run_date" =~ ^[0-9]{8}$ ]] || fail "run date must be YYYYMMDD"

artifact_root="${OKP_QA_ARTIFACT_ROOT:-$HOME/qa}"
host_root="$artifact_root/okp-night-$run_date/$host_alias"
artifact_dir="$host_root/runs/$suite_id"
mkdir -p "$artifact_dir"

started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
{
  printf 'schema=1\n'
  printf 'suite_id=%s\n' "$suite_id"
  printf 'run_date_utc=%s\n' "$run_date"
  printf 'host_role=%s\n' "$role"
  printf 'host_alias=%s\n' "$host_alias"
  printf 'evidence_level=gnome-wayland-automation\n'
  printf 'started_at=%s\n' "$started_at"
} >"$artifact_dir/metadata.env"
: >"$artifact_dir/results.tsv"

failed=0
not_run=0
last_hook_status=""

record_result() {
  local wave="$1" action="$2" status="$3" detail="$4"
  printf '%s\t%s\t%s\t%s\n' "$wave" "$action" "$status" "$detail" \
    >>"$artifact_dir/results.tsv"
  case "$status" in
    FAIL) failed=1 ;;
    'NOT RUN') not_run=1 ;;
  esac
}

run_hook() {
  local wave="$1" action="$2" hook="$3"
  shift 3
  local log_file="$artifact_dir/${wave,,}-${action//_/-}.log" rc status detail
  if [[ ! -x "$hook" ]]; then
    last_hook_status='NOT RUN'
    record_result "$wave" "$action" 'NOT RUN' 'site hook is not installed'
    return 0
  fi
  set +e
  "$hook" "$@" >"$log_file" 2>&1
  rc=$?
  set -e
  case "$rc" in
    0) status=PASS; detail="log=$(basename "$log_file")" ;;
    75) status='NOT RUN'; detail="hook unavailable; log=$(basename "$log_file")" ;;
    *) status=FAIL; detail="exit=$rc; log=$(basename "$log_file")" ;;
  esac
  last_hook_status="$status"
  record_result "$wave" "$action" "$status" "$detail"
}

run_seat_check() {
  local log_file="$artifact_dir/wave-a-seat-ready.log" rc status detail
  set +e
  probe_seat "$role" >"$log_file" 2>&1
  rc=$?
  set -e
  case "$rc" in
    0) status=PASS; detail="log=$(basename "$log_file")" ;;
    75) status='NOT RUN'; detail="probe unavailable; log=$(basename "$log_file")" ;;
    *) status=FAIL; detail="exit=$rc; log=$(basename "$log_file")" ;;
  esac
  last_hook_status="$status"
  record_result A seat_ready "$status" "$detail"
}

run_action() {
  local wave="$1" action="$2"
  run_hook "$wave" "$action" "$HOOK_DIR/run-action" \
    "$action" "$artifact_dir" "$role" "$suite_id"
}

candidate_field() {
  local key="$1"
  awk -F= -v key="$key" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' \
    "$artifact_dir/candidate.env"
}

validate_candidate_identity() {
  local acceptance source_sha version package_name package_sha manifest_sha
  [[ -f "$artifact_dir/candidate.env" ]] || return 1
  acceptance="$(candidate_field acceptance)"
  source_sha="$(candidate_field source_sha)"
  version="$(candidate_field version)"
  package_name="$(candidate_field package_name)"
  package_sha="$(candidate_field package_sha256)"
  manifest_sha="$(candidate_field manifest_sha256)"
  [[ "$acceptance" == accepted ]] || return 1
  [[ "$source_sha" =~ ^[0-9a-f]{40}$ ]] || return 1
  [[ "$version" =~ ^[A-Za-z0-9][A-Za-z0-9._+:-]{0,127}$ ]] || return 1
  [[ "$package_name" =~ ^[A-Za-z0-9][A-Za-z0-9._+:-]{0,255}$ ]] || return 1
  [[ "$package_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
  [[ "$manifest_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
}

finish_artifacts() {
  local finished_at
  finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  printf 'finished_at=%s\n' "$finished_at" >>"$artifact_dir/metadata.env"
  (
    cd "$artifact_dir"
    shopt -s nullglob
    for evidence_file in *; do
      [[ -f "$evidence_file" && "$evidence_file" != SHA256SUMS ]] || continue
      sha256sum "$evidence_file"
    done
  ) >"$artifact_dir/SHA256SUMS"
  printf '%s\n' "$suite_id" >"$host_root/latest-suite-id.txt"
}

# The controller probes before acquiring the lease. Repeat the probe after the
# lease so a seat that became occupied cannot receive launch or input events.
run_seat_check
if [[ "$last_hook_status" != PASS ]]; then
  for action in \
    candidate_install \
    candidate_identity \
    headless_window_regressions \
    cold_launch \
    single_monitor_fit \
    play_pause_seek \
    non_osc_drag_10 \
    menus_settings_chapters \
    secondary_launch \
    clean_close; do
    record_result A "$action" 'NOT RUN' 'post-lease seat gate did not pass'
  done
  for action in dual_head_available open_each_head no_spanning edge_drag; do
    record_result B "$action" 'NOT RUN' 'post-lease seat gate did not pass'
  done
  for action in 4k_weak_host_stress rapid_open_close seek_screenshot_storm; do
    record_result C "$action" 'NOT RUN' 'post-lease seat gate did not pass'
  done
  finish_artifacts
  if (( failed != 0 )); then
    exit 1
  fi
  exit 4
fi

run_hook A candidate_install "$HOOK_DIR/prepare-candidate" "$artifact_dir" "$role" "$suite_id"
if [[ "$last_hook_status" == PASS ]]; then
  if validate_candidate_identity; then
    record_result A candidate_identity PASS 'candidate.env is complete and accepted'
  else
    last_hook_status=FAIL
    record_result A candidate_identity FAIL 'candidate.env is missing or invalid'
  fi
else
  record_result A candidate_identity 'NOT RUN' 'candidate preparation did not pass'
fi
if [[ "$last_hook_status" != PASS ]]; then
  for action in \
    headless_window_regressions \
    cold_launch \
    single_monitor_fit \
    play_pause_seek \
    non_osc_drag_10 \
    menus_settings_chapters \
    secondary_launch \
    clean_close; do
    record_result A "$action" 'NOT RUN' 'accepted candidate was not prepared'
  done
  for action in dual_head_available open_each_head no_spanning edge_drag; do
    record_result B "$action" 'NOT RUN' 'accepted candidate was not prepared'
  done
  for action in 4k_weak_host_stress rapid_open_close seek_screenshot_storm; do
    record_result C "$action" 'NOT RUN' 'accepted candidate was not prepared'
  done
  finish_artifacts
  if (( failed != 0 )); then
    exit 1
  fi
  exit 4
fi

for action in \
  headless_window_regressions \
  cold_launch \
  single_monitor_fit \
  play_pause_seek \
  non_osc_drag_10 \
  menus_settings_chapters \
  secondary_launch \
  clean_close; do
  run_action A "$action"
done

dual_head=0
dual_probe_log="$artifact_dir/wave-b-dual-head-probe.log"
if [[ -x "$HOOK_DIR/probe-dual-head" ]]; then
  set +e
  "$HOOK_DIR/probe-dual-head" "$artifact_dir" "$role" "$suite_id" >"$dual_probe_log" 2>&1
  dual_rc=$?
  set -e
  case "$dual_rc" in
    0)
      dual_head=1
      record_result B dual_head_available PASS "log=$(basename "$dual_probe_log")"
      ;;
    75)
      record_result B dual_head_available 'NOT RUN' "probe unavailable; log=$(basename "$dual_probe_log")"
      ;;
    1)
      record_result B dual_head_available PASS "single-head host; log=$(basename "$dual_probe_log")"
      ;;
    *)
      record_result B dual_head_available FAIL "exit=$dual_rc; log=$(basename "$dual_probe_log")"
      ;;
  esac
else
  record_result B dual_head_available 'NOT RUN' 'site hook is not installed'
fi

if (( dual_head == 1 )); then
  for action in open_each_head no_spanning edge_drag; do
    run_action B "$action"
  done
else
  for action in open_each_head no_spanning edge_drag; do
    if [[ -x "$HOOK_DIR/probe-dual-head" && "${dual_rc:-75}" == "1" ]]; then
      record_result B "$action" SKIP 'proved single-head host'
    else
      record_result B "$action" 'NOT RUN' 'dual-head availability was not proven'
    fi
  done
fi

if [[ "$role" == "slava" ]]; then
  for action in 4k_weak_host_stress rapid_open_close seek_screenshot_storm; do
    run_action C "$action"
  done
else
  for action in 4k_weak_host_stress rapid_open_close seek_screenshot_storm; do
    record_result C "$action" SKIP 'Wave C is reserved for the weak-host role'
  done
fi

finish_artifacts

if (( failed != 0 )); then
  printf 'Night GUI QA failed. Evidence: %s\n' "$artifact_dir" >&2
  exit 1
fi
if (( not_run != 0 )); then
  printf 'Night GUI QA is incomplete. Evidence: %s\n' "$artifact_dir" >&2
  exit 4
fi
printf 'Night GUI QA passed. Evidence: %s\n' "$artifact_dir"
