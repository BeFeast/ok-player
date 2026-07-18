#!/usr/bin/env bash
set -euo pipefail

DIAGNOSTICS_FILE="${1:?usage: wait-for-dbus-names-clear.sh <diagnostics-file> <name> [name...]}"
shift
(( $# > 0 )) || {
  echo "wait-for-dbus-names-clear.sh requires at least one bus name" >&2
  exit 2
}
NAMES=("$@")
ATTEMPTS="${OKP_DBUS_NAME_CLEAR_ATTEMPTS:-40}"
INTERVAL="${OKP_DBUS_NAME_CLEAR_INTERVAL:-0.1}"
GDBUS="${OKP_DBUS_NAME_CLEAR_GDBUS:-gdbus}"

: >"$DIAGNOSTICS_FILE"

for attempt in $(seq 1 "$ATTEMPTS"); do
  list_status=0
  list_output="$(OKP_DBUS_NAME_CLEAR_ATTEMPT="$attempt" "$GDBUS" call --session \
    --dest org.freedesktop.DBus \
    --object-path /org/freedesktop/DBus \
    --method org.freedesktop.DBus.ListNames 2>&1)" || list_status=$?
  names_present=false
  printf 'attempt=%s list_status=%s\n' "$attempt" "$list_status" \
    >>"$DIAGNOSTICS_FILE"

  if (( list_status == 0 )); then
    for name in "${NAMES[@]}"; do
      # gdbus renders every ListNames entry as a single-quoted GVariant
      # string. Include those delimiters so a longer, unrelated bus name
      # cannot keep the requested name falsely "present".
      if [[ "$list_output" == *"'$name'"* ]]; then
        names_present=true
        printf 'name=%s present=true\n' "$name" >>"$DIAGNOSTICS_FILE"
      else
        printf 'name=%s present=false\n' "$name" >>"$DIAGNOSTICS_FILE"
      fi
    done
  else
    printf 'list_error=%s\n' "$list_output" >>"$DIAGNOSTICS_FILE"
  fi

  if (( list_status == 0 )) && [[ "$names_present" == "false" ]]; then
    printf 'clear=true\n' >>"$DIAGNOSTICS_FILE"
    exit 0
  fi

  sleep "$INTERVAL"
done

echo "Timed out waiting for OK Player D-Bus registrations to clear" >&2
echo "D-Bus lifecycle diagnostics: $DIAGNOSTICS_FILE" >&2
cat "$DIAGNOSTICS_FILE" >&2
exit 1
