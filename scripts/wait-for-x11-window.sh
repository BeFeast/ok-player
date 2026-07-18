#!/usr/bin/env bash
set -euo pipefail

EXPECTED_PID="${1:?usage: wait-for-x11-window.sh <pid> <ids-file> <diagnostics-file>}"
IDS_FILE="${2:?usage: wait-for-x11-window.sh <pid> <ids-file> <diagnostics-file>}"
DIAGNOSTICS_FILE="${3:?usage: wait-for-x11-window.sh <pid> <ids-file> <diagnostics-file>}"
APP_LOG="${4:-}"
ATTEMPTS="${OKP_X11_WINDOW_WAIT_ATTEMPTS:-80}"
INTERVAL="${OKP_X11_WINDOW_WAIT_INTERVAL:-0.1}"

: >"$IDS_FILE"
: >"$DIAGNOSTICS_FILE"

window_key() {
  local window_id="$1"
  if [[ "$window_id" =~ ^0[xX][0-9a-fA-F]+$ ]]; then
    printf '%u\n' "$((window_id))"
  elif [[ "$window_id" =~ ^[0-9]+$ ]]; then
    printf '%u\n' "$((10#$window_id))"
  else
    return 1
  fi
}

for attempt in $(seq 1 "$ATTEMPTS"); do
  search_status=0
  search_output="$(xdotool search --name '^OK Player$' 2>/dev/null)" || search_status=$?
  printf '%s' "$search_output" >"$IDS_FILE"
  {
    printf 'attempt=%s search_status=%s\n' "$attempt" "$search_status"
    if [[ -n "$search_output" ]]; then
      printf 'search_output:\n%s\n' "$search_output"
    else
      printf 'search_output: <empty>\n'
    fi
  } >>"$DIAGNOSTICS_FILE"

  selected_id=""
  selected_key="-1"
  while IFS= read -r candidate || [[ -n "$candidate" ]]; do
    [[ -n "$candidate" ]] || continue

    if ! candidate_key="$(window_key "$candidate")"; then
      printf 'candidate=%s rejected=malformed-id\n' "$candidate" >>"$DIAGNOSTICS_FILE"
      continue
    fi

    candidate_pid="$(xdotool getwindowpid "$candidate" 2>/dev/null || true)"
    if [[ "$candidate_pid" != "$EXPECTED_PID" ]]; then
      printf 'candidate=%s rejected=pid expected=%s actual=%s\n' \
        "$candidate" "$EXPECTED_PID" "${candidate_pid:-unavailable}" >>"$DIAGNOSTICS_FILE"
      continue
    fi

    if ! candidate_info="$(xwininfo -id "$candidate" 2>&1)"; then
      printf 'candidate=%s rejected=invalid-xid\n%s\n' \
        "$candidate" "$candidate_info" >>"$DIAGNOSTICS_FILE"
      continue
    fi

    candidate_width="$(awk '/Width:/ {print $2; exit}' <<<"$candidate_info")"
    candidate_height="$(awk '/Height:/ {print $2; exit}' <<<"$candidate_info")"
    candidate_state="$(awk -F': ' '/Map State:/ {print $2; exit}' <<<"$candidate_info")"
    if [[ "$candidate_state" != "IsViewable" ]]; then
      printf 'candidate=%s rejected=map-state state=%s width=%s height=%s\n' \
        "$candidate" "${candidate_state:-unknown}" "${candidate_width:-unknown}" \
        "${candidate_height:-unknown}" >>"$DIAGNOSTICS_FILE"
      continue
    fi
    if [[ ! "$candidate_width" =~ ^[0-9]+$ || ! "$candidate_height" =~ ^[0-9]+$ ]] || \
      (( candidate_width <= 1 || candidate_height <= 1 )); then
      printf 'candidate=%s rejected=unusable-geometry width=%s height=%s\n' \
        "$candidate" "${candidate_width:-unknown}" "${candidate_height:-unknown}" \
        >>"$DIAGNOSTICS_FILE"
      continue
    fi

    printf 'candidate=%s accepted pid=%s state=%s width=%s height=%s\n' \
      "$candidate" "$candidate_pid" "$candidate_state" "$candidate_width" \
      "$candidate_height" >>"$DIAGNOSTICS_FILE"
    if (( candidate_key > selected_key )); then
      selected_id="$candidate"
      selected_key="$candidate_key"
    fi
  done <"$IDS_FILE"

  if [[ -n "$selected_id" ]]; then
    printf 'selected=%s policy=highest-viewable-xid-for-pid\n' "$selected_id" \
      >>"$DIAGNOSTICS_FILE"
    printf '%s\n' "$selected_id"
    exit 0
  fi

  sleep "$INTERVAL"
done

echo "Timed out waiting for a viewable OK Player window for PID $EXPECTED_PID" >&2
echo "Window readiness diagnostics: $DIAGNOSTICS_FILE" >&2
cat "$DIAGNOSTICS_FILE" >&2
if [[ -n "$APP_LOG" ]]; then
  echo "Application log: $APP_LOG" >&2
  if [[ -f "$APP_LOG" ]]; then
    cat "$APP_LOG" >&2
  else
    echo "<missing>" >&2
  fi
fi
exit 1
