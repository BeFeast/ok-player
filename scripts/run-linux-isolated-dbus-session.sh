#!/usr/bin/env bash
# Run one command on a fresh session bus and do not return until that bus is gone.
set -euo pipefail

EVIDENCE_FILE="${1:?usage: run-linux-isolated-dbus-session.sh <evidence-file> <command> [args...]}"
shift
(( $# > 0 )) || {
  echo "run-linux-isolated-dbus-session.sh requires a command" >&2
  exit 2
}

for tool in dbus-run-session gdbus tr; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

mkdir -p "$(dirname "$EVIDENCE_FILE")"
address_file="${EVIDENCE_FILE}.address"
rm -f "$address_file"
: >"$EVIDENCE_FILE"

cleanup() {
  rm -f "$address_file"
}
trap cleanup EXIT

set +e
dbus-run-session -- bash -c '
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
  printf 'session_bus_ready=false\ncommand_status=%s\nstatus=fail\n' "$command_status" \
    >>"$EVIDENCE_FILE"
  echo "Isolated session did not record a ready D-Bus address" >&2
  exit 1
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
    done < <(tr '\0' '\n' <"$environ" 2>/dev/null || true)
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
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  echo "Isolated session bus remained reachable after its command exited" >&2
  exit 1
fi

if [[ "$process_teardown" != "clean" ]]; then
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  echo "Processes from the isolated session remained after teardown: ${residual_pids[*]}" >&2
  exit 1
fi

if (( command_status != 0 )); then
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  exit "$command_status"
fi

printf 'status=pass\n' >>"$EVIDENCE_FILE"
