#!/usr/bin/env bash
# Run one command on a fresh session bus and do not return until that bus is gone.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/ok-player-scratch.sh"

SESSION_INFRA_EXIT_CODE="${OKP_SESSION_INFRA_EXIT_CODE:-75}"
if [[ ! "$SESSION_INFRA_EXIT_CODE" =~ ^[1-9][0-9]{0,2}$ ]] \
  || (( SESSION_INFRA_EXIT_CODE > 255 )); then
    echo "OKP_SESSION_INFRA_EXIT_CODE must be an integer from 1 through 255" >&2
    exit 2
fi

EVIDENCE_FILE="${1:?usage: run-linux-isolated-dbus-session.sh <evidence-file> <command> [args...]}"
shift
(( $# > 0 )) || {
  echo "run-linux-isolated-dbus-session.sh requires a command" >&2
  exit 2
}

for tool in dbus-run-session gdbus python3 tr; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
SUBREAPER="$SCRIPT_DIR/run-linux-child-subreaper.py"
if [[ ! -f "$SUBREAPER" ]]; then
  echo "Missing required helper: run-linux-child-subreaper.py" >&2
  exit 127
fi

mkdir -p "$(dirname "$EVIDENCE_FILE")"
address_file="${EVIDENCE_FILE}.address"
runtime_dir="$(okp_make_scratch_dir dbus-runtime)"
chmod 700 "$runtime_dir"
rm -f "$address_file"
: >"$EVIDENCE_FILE"

cleanup() {
  rm -f "$address_file"
  rm -rf "$runtime_dir"
}
trap cleanup EXIT

export XDG_RUNTIME_DIR="$runtime_dir"

set +e
python3 "$SUBREAPER" dbus-run-session -- bash -c '
set -euo pipefail
address_file="$1"
shift
printf "%s\n" "$DBUS_SESSION_BUS_ADDRESS" >"$address_file"
gdbus call --address "$DBUS_SESSION_BUS_ADDRESS" \
  --dest org.freedesktop.DBus \
  --object-path /org/freedesktop/DBus \
  --method org.freedesktop.DBus.GetId >/dev/null
exec "$@"
' bash "$address_file" "$@"
command_status=$?
set -e

if [[ ! -s "$address_file" ]]; then
  printf 'xdg_runtime_dir_private=true\nsession_bus_ready=false\ncommand_status=%s\nfailure_kind=session-infra\nstatus=fail\n' "$command_status" \
    >>"$EVIDENCE_FILE"
  echo "Isolated session did not record a ready D-Bus address" >&2
  exit "$SESSION_INFRA_EXIT_CODE"
fi

address="$(cat "$address_file")"
bus_reachable=true
for _ in $(seq 1 "${OKP_DBUS_TEARDOWN_ATTEMPTS:-40}"); do
  if ! gdbus call --address "$address" \
    --dest org.freedesktop.DBus \
    --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.GetId >/dev/null 2>&1; then
    bus_reachable=false
    break
  fi
  sleep "${OKP_DBUS_TEARDOWN_INTERVAL:-0.05}"
done

session_pids() {
  local session_address="$1" environ pid entry matches
  for environ in /proc/[0-9]*/environ; do
    matches=false
    while IFS= read -r entry; do
      if [[ "$entry" == "DBUS_SESSION_BUS_ADDRESS=$session_address" ]]; then
        matches=true
        break
      fi
    done < <(tr '\0' '\n' 2>/dev/null <"$environ" || true)
    if [[ "$matches" == "true" ]]; then
      pid="${environ#/proc/}"
      printf '%s\n' "${pid%/environ}"
    fi
  done
}

mapfile -t residual_pids < <(session_pids "$address")
for pid in "${residual_pids[@]}"; do
  kill "$pid" 2>/dev/null || true
done
for _ in $(seq 1 "${OKP_DBUS_PROCESS_TEARDOWN_ATTEMPTS:-40}"); do
  mapfile -t residual_pids < <(session_pids "$address")
  (( ${#residual_pids[@]} == 0 )) && break
  sleep "${OKP_DBUS_PROCESS_TEARDOWN_INTERVAL:-0.05}"
done
for pid in "${residual_pids[@]}"; do
  kill -KILL "$pid" 2>/dev/null || true
done
mapfile -t residual_pids < <(session_pids "$address")
process_teardown=clean
if (( ${#residual_pids[@]} > 0 )); then
  process_teardown=failed
fi

{
  printf 'xdg_runtime_dir_private=true\n'
  printf 'session_bus_ready=true\n'
  printf 'command_status=%s\n' "$command_status"
  if [[ "$bus_reachable" == "false" ]]; then
    printf 'session_bus_teardown=clean\n'
  else
    printf 'session_bus_teardown=failed\n'
  fi
  printf 'session_process_teardown=%s\n' "$process_teardown"
} >>"$EVIDENCE_FILE"

if [[ "$bus_reachable" == "true" ]]; then
  printf 'failure_kind=session-infra\nstatus=fail\n' >>"$EVIDENCE_FILE"
  echo "Isolated session bus remained reachable after its command exited" >&2
  exit "$SESSION_INFRA_EXIT_CODE"
fi

if [[ "$process_teardown" != "clean" ]]; then
  printf 'failure_kind=session-infra\nstatus=fail\n' >>"$EVIDENCE_FILE"
  echo "Processes from the isolated session remained after teardown: ${residual_pids[*]}" >&2
  exit "$SESSION_INFRA_EXIT_CODE"
fi

if (( command_status != 0 )); then
  if (( command_status == SESSION_INFRA_EXIT_CODE )); then
    printf 'failure_kind=session-infra\n' >>"$EVIDENCE_FILE"
  else
    printf 'failure_kind=command\n' >>"$EVIDENCE_FILE"
  fi
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  exit "$command_status"
fi

printf 'status=pass\n' >>"$EVIDENCE_FILE"
