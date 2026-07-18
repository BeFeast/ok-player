#!/usr/bin/env bash
set -euo pipefail

EXPECTED_PID="${1:?usage: wait-for-x11-app-clear.sh <pid|none> <diagnostics-file>}"
DIAGNOSTICS_FILE="${2:?usage: wait-for-x11-app-clear.sh <pid|none> <diagnostics-file>}"
ATTEMPTS="${OKP_X11_APP_CLEAR_ATTEMPTS:-40}"
INTERVAL="${OKP_X11_APP_CLEAR_INTERVAL:-0.1}"

: >"$DIAGNOSTICS_FILE"

for attempt in $(seq 1 "$ATTEMPTS"); do
  process_alive=false
  if [[ "$EXPECTED_PID" != "none" ]] && kill -0 "$EXPECTED_PID" 2>/dev/null; then
    process_alive=true
  fi

  search_status=0
  search_output="$(xdotool search --name '^OK Player$' 2>/dev/null)" || search_status=$?
  windows_present=false
  {
    printf 'attempt=%s expected_pid=%s process_alive=%s search_status=%s\n' \
      "$attempt" "$EXPECTED_PID" "$process_alive" "$search_status"
    if [[ -n "$search_output" ]]; then
      printf 'search_output:\n%s\n' "$search_output"
    else
      printf 'search_output: <empty>\n'
    fi
  } >>"$DIAGNOSTICS_FILE"

  while IFS= read -r candidate || [[ -n "$candidate" ]]; do
    [[ -n "$candidate" ]] || continue
    windows_present=true
    candidate_pid="$(xdotool getwindowpid "$candidate" 2>/dev/null || true)"
    if candidate_info="$(xwininfo -id "$candidate" 2>&1)"; then
      candidate_width="$(awk '/Width:/ {print $2; exit}' <<<"$candidate_info")"
      candidate_height="$(awk '/Height:/ {print $2; exit}' <<<"$candidate_info")"
      candidate_state="$(awk -F': ' '/Map State:/ {print $2; exit}' <<<"$candidate_info")"
      printf 'candidate=%s pid=%s state=%s width=%s height=%s\n' \
        "$candidate" "${candidate_pid:-unavailable}" "${candidate_state:-unknown}" \
        "${candidate_width:-unknown}" "${candidate_height:-unknown}" \
        >>"$DIAGNOSTICS_FILE"
    else
      printf 'candidate=%s pid=%s state=invalid-xid\n%s\n' \
        "$candidate" "${candidate_pid:-unavailable}" "$candidate_info" \
        >>"$DIAGNOSTICS_FILE"
    fi
  done <<<"$search_output"

  if [[ "$process_alive" == "false" && "$windows_present" == "false" ]]; then
    printf 'clear=true\n' >>"$DIAGNOSTICS_FILE"
    exit 0
  fi

  sleep "$INTERVAL"
done

echo "Timed out waiting for the previous OK Player process/window lifecycle to clear" >&2
echo "X11 lifecycle diagnostics: $DIAGNOSTICS_FILE" >&2
cat "$DIAGNOSTICS_FILE" >&2
exit 1
