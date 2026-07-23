#!/usr/bin/env bash
# Lease-gated controller for the live Linux night GUI QA fleet.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
LEASE_SCRIPT="$ROOT/scripts/ok-player-qa-lease.sh"
HOST_SCRIPT="$ROOT/scripts/ok-player-night-gui-host.sh"

usage() {
  cat >&2 <<'EOF'
usage: ok-player-night-gui-qa.sh [--host HOST] [--force-window]

Default automatic order is slava, mimir, then baldr. Set OKP_QA_HOSTS to a
whitespace-separated list of sanitized logical host aliases to override that
order. Sindri is never accepted in the automatic list. An explicit --host
sindri run also requires OKP_QA_ALLOW_SINDRI=1 and OKP_QA_OPERATOR_GO=1.
EOF
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 64
}

explicit_host=""
force_window=0
while (( $# > 0 )); do
  case "$1" in
    --host)
      [[ $# -ge 2 ]] || fail "--host requires a value"
      explicit_host="$2"
      shift 2
      ;;
    --force-window)
      force_window=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

if [[ -n "$explicit_host" ]] &&
  [[ ! "$explicit_host" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]]; then
  fail "invalid host alias: $explicit_host"
fi

if [[ "$explicit_host" == "sindri" ]] &&
  [[ "${OKP_QA_ALLOW_SINDRI:-0}" != "1" || "${OKP_QA_OPERATOR_GO:-0}" != "1" ]]; then
  fail "sindri requires explicit operator authorization"
fi

utc_hour="${OKP_QA_UTC_HOUR:-$(date -u +%H)}"
[[ "$utc_hour" =~ ^[0-9]{1,2}$ ]] || fail "UTC hour must be numeric"
(( 10#$utc_hour <= 23 )) || fail "UTC hour must be between 0 and 23"
if (( force_window == 0 )) && [[ "${OKP_QA_FORCE:-0}" != "1" ]] &&
  (( 10#$utc_hour < 0 || 10#$utc_hour > 5 )); then
  printf 'Outside the scheduled UTC 00:05-05:05 window; no hosts were touched.\n'
  exit 0
fi

run_date="${OKP_QA_RUN_DATE:-$(date -u +%Y%m%d)}"
[[ "$run_date" =~ ^[0-9]{8}$ ]] || fail "OKP_QA_RUN_DATE must be YYYYMMDD"
suite_id="${OKP_QA_SUITE_ID:-night-$(date -u +%Y%m%dT%H%M%SZ)-$$}"
[[ "$suite_id" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "invalid suite id"
lease_ttl="${OKP_QA_LEASE_TTL_MINUTES:-45}"
[[ "$lease_ttl" =~ ^[0-9]+$ ]] || fail "lease TTL must be numeric"
(( lease_ttl >= 1 && lease_ttl <= 180 )) || fail "lease TTL must be between 1 and 180 minutes"
host_timeout_seconds=$((lease_ttl * 60 - 30))

if [[ -n "$explicit_host" ]]; then
  hosts=("$explicit_host")
elif [[ -v OKP_QA_HOSTS ]]; then
  read -r -a hosts <<<"$OKP_QA_HOSTS"
  (( ${#hosts[@]} > 0 )) || fail "OKP_QA_HOSTS must contain at least one host alias"
else
  hosts=(slava mimir baldr)
fi

declare -A seen_hosts=()
for host in "${hosts[@]}"; do
  [[ "$host" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] ||
    fail "invalid host alias in automatic list: $host"
  [[ "$host" != sindri || -n "$explicit_host" ]] ||
    fail "sindri is not allowed in OKP_QA_HOSTS"
  [[ ! -v "seen_hosts[$host]" ]] || fail "duplicate host alias: $host"
  seen_hosts["$host"]=1
done

if [[ -n "${OKP_QA_SSH_COMMAND:-}" ]]; then
  ssh_command=("$OKP_QA_SSH_COMMAND")
else
  ssh_command=(ssh -o BatchMode=yes -o ConnectTimeout=8)
fi

remote_script() {
  local host="$1" script="$2"
  shift 2
  "${ssh_command[@]}" "$host" \
    env \
      "OKP_QA_ALLOW_SINDRI=${OKP_QA_ALLOW_SINDRI:-0}" \
      "OKP_QA_OPERATOR_GO=${OKP_QA_OPERATOR_GO:-0}" \
    bash -s -- "$@" <"$script"
}

remote_host_run() {
  local host="$1"
  shift
  "${ssh_command[@]}" "$host" \
    env \
      "OKP_QA_ALLOW_SINDRI=${OKP_QA_ALLOW_SINDRI:-0}" \
      "OKP_QA_OPERATOR_GO=${OKP_QA_OPERATOR_GO:-0}" \
    timeout --signal=TERM --kill-after=10s "${host_timeout_seconds}s" \
    bash -s -- "$@" <"$HOST_SCRIPT"
}

active_host=""
lease_held=0
release_active_lease() {
  if (( lease_held == 1 )) && [[ -n "$active_host" ]]; then
    remote_script "$active_host" "$LEASE_SCRIPT" release "$suite_id" || true
    lease_held=0
  fi
}
trap release_active_lease EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

ran_hosts=0
failed_hosts=0
incomplete_hosts=0

for host in "${hosts[@]}"; do
  printf '%s\n' "-- probe host=$host"
  if ! remote_script "$host" "$HOST_SCRIPT" probe "$host"; then
    printf '%s\n' "SKIP host=$host reason=seat-unavailable"
    continue
  fi

  active_host="$host"
  if ! remote_script "$host" "$LEASE_SCRIPT" acquire "$host" "$suite_id" "$lease_ttl" "$$"; then
    printf '%s\n' "SKIP host=$host reason=lease-unavailable"
    active_host=""
    continue
  fi
  lease_held=1
  ran_hosts=$((ran_hosts + 1))

  set +e
  remote_host_run "$host" run "$host" "$host" "$run_date" "$suite_id"
  host_rc=$?
  set -e
  case "$host_rc" in
    0) printf '%s\n' "PASS host=$host suite=$suite_id" ;;
    4)
      incomplete_hosts=$((incomplete_hosts + 1))
      printf '%s\n' "INCOMPLETE host=$host suite=$suite_id"
      ;;
    *)
      failed_hosts=$((failed_hosts + 1))
      printf '%s\n' "FAIL host=$host suite=$suite_id exit=$host_rc" >&2
      ;;
  esac

  release_active_lease
  active_host=""
done

if (( failed_hosts > 0 )); then
  printf 'Night GUI QA failed on %s host(s); suite=%s\n' "$failed_hosts" "$suite_id" >&2
  exit 1
fi
if (( incomplete_hosts > 0 )); then
  printf 'Night GUI QA incomplete on %s host(s); suite=%s\n' "$incomplete_hosts" "$suite_id" >&2
  exit 4
fi
if (( ran_hosts == 0 )); then
  printf 'No eligible host was leased; suite=%s\n' "$suite_id"
  exit 0
fi
printf 'Night GUI QA passed on %s host(s); suite=%s\n' "$ran_hosts" "$suite_id"
