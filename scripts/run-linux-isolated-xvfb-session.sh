#!/usr/bin/env bash
# Run one command on a fresh Xvfb display and reap the disposable server exactly.
set -euo pipefail

EVIDENCE_FILE="${1:?usage: run-linux-isolated-xvfb-session.sh <evidence-file> <xvfb-log> <server-args> <command> [args...]}"
XVFB_LOG="${2:?usage: run-linux-isolated-xvfb-session.sh <evidence-file> <xvfb-log> <server-args> <command> [args...]}"
SERVER_ARGS_TEXT="${3:?usage: run-linux-isolated-xvfb-session.sh <evidence-file> <xvfb-log> <server-args> <command> [args...]}"
shift 3
(( $# > 0 )) || {
  echo "run-linux-isolated-xvfb-session.sh requires a command" >&2
  exit 2
}

for tool in Xvfb xauth xprop flock mcookie; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

mkdir -p "$(dirname "$EVIDENCE_FILE")" "$(dirname "$XVFB_LOG")"
: >"$EVIDENCE_FILE"
: >"$XVFB_LOG"
runtime_dir="$(mktemp -d -t okp-xvfb.XXXXXX)"
auth_file="$runtime_dir/Xauthority"
touch "$auth_file"
xvfb_pid=""
server_num=""

cleanup() {
  if [[ -n "$xvfb_pid" ]] && kill -0 "$xvfb_pid" 2>/dev/null; then
    kill -KILL "$xvfb_pid" 2>/dev/null || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi
  if [[ -n "$server_num" ]]; then
    rm -f "/tmp/.X${server_num}-lock" "/tmp/.X11-unix/X${server_num}"
  fi
  rm -rf "$runtime_dir"
}
trap cleanup EXIT

read -r -a server_args <<<"$SERVER_ARGS_TEXT"
exec 9>"/tmp/okp-xvfb-allocation.lock"
flock 9
for candidate in $(seq "${OKP_XVFB_FIRST_SERVER_NUM:-300}" "${OKP_XVFB_LAST_SERVER_NUM:-399}"); do
  if [[ -e "/tmp/.X${candidate}-lock" || -S "/tmp/.X11-unix/X${candidate}" ]]; then
    continue
  fi

  cookie="$(mcookie)"
  XAUTHORITY="$auth_file" xauth add ":${candidate}" . "$cookie"
  Xvfb ":${candidate}" "${server_args[@]}" -auth "$auth_file" \
    >>"$XVFB_LOG" 2>&1 &
  xvfb_pid=$!
  server_num="$candidate"

  ready=false
  for _ in $(seq 1 100); do
    if DISPLAY=":${candidate}" XAUTHORITY="$auth_file" xprop -root >/dev/null 2>&1; then
      ready=true
      break
    fi
    if ! kill -0 "$xvfb_pid" 2>/dev/null; then
      break
    fi
    sleep 0.05
  done
  if [[ "$ready" == "true" ]]; then
    break
  fi

  kill -KILL "$xvfb_pid" 2>/dev/null || true
  wait "$xvfb_pid" 2>/dev/null || true
  xvfb_pid=""
  rm -f "/tmp/.X${candidate}-lock" "/tmp/.X11-unix/X${candidate}"
  server_num=""
done
flock -u 9
exec 9>&-

if [[ -z "$xvfb_pid" || -z "$server_num" ]]; then
  printf 'xvfb_ready=false\nstatus=fail\n' >>"$EVIDENCE_FILE"
  echo "Could not start an isolated Xvfb server" >&2
  exit 1
fi

set +e
DISPLAY=":${server_num}" XAUTHORITY="$auth_file" "$@"
command_status=$?
set -e

xvfb_alive_before_teardown=false
if kill -0 "$xvfb_pid" 2>/dev/null; then
  xvfb_alive_before_teardown=true
  kill -KILL "$xvfb_pid" 2>/dev/null || true
  wait "$xvfb_pid" 2>/dev/null || true
fi
xvfb_pid=""
rm -f "/tmp/.X${server_num}-lock" "/tmp/.X11-unix/X${server_num}"

{
  printf 'xvfb_ready=true\n'
  printf 'command_status=%s\n' "$command_status"
  printf 'xvfb_alive_before_teardown=%s\n' "$xvfb_alive_before_teardown"
  printf 'xvfb_teardown=clean\n'
} >>"$EVIDENCE_FILE"

if [[ "$xvfb_alive_before_teardown" != "true" ]]; then
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  echo "Xvfb exited before explicit session teardown" >&2
  exit 1
fi

if (( command_status != 0 )); then
  printf 'status=fail\n' >>"$EVIDENCE_FILE"
  exit "$command_status"
fi

printf 'status=pass\n' >>"$EVIDENCE_FILE"
