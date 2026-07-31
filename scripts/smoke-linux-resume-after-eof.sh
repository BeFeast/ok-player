#!/usr/bin/env bash
# A file watched to its end must reopen at the beginning (#766).
#
# Both phases are behavioural. The first plays a fixture to end of file and reads what
# the history entry says afterwards. The second reopens the same file and times how long
# it takes to reach end of file again: a run that starts at zero needs the whole
# duration, a run that resumes near the end needs only the tail. Timing the playback is
# what makes this a check on where playback started rather than a check on the same
# history write the first phase already read.
#
# The fixture is deliberately longer than the 10 s progress-persistence interval, so a
# periodic position sample lands before the end and the entry has something to resume
# from. That is the operator's reported shape.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-resume-after-eof-smoke}"
if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi
OUT_DIR="$(realpath -m "$OUT_DIR")"

for tool in xvfb-run dbus-run-session ffmpeg python3 realpath; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

DURATION_SECONDS=14
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "color=c=0x2277cc:s=640x360:r=24:d=$DURATION_SECONDS" \
  -map 0:v:0 -c:v libx264 -preset ultrafast -tune stillimage \
  -pix_fmt yuv420p -g 24 -an "$OUT_DIR/watched.mkv"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$DURATION_SECONDS" \
  >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
DURATION_SECONDS="$3"
FIXTURE="$OUT_DIR/watched.mkv"

export GDK_BACKEND=x11
export GDK_DEBUG=no-portals
export GSK_RENDERER=cairo
export GIO_USE_PORTALS=0
export GTK_USE_PORTAL=0
export GTK_A11Y=none
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_DEBUG_IDLE_RETURN_SMOKE=1
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CACHE_HOME="$OUT_DIR/cache"
mkdir -p "$XDG_CONFIG_HOME/ok-player" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
# Auto-advance off keeps end of file a return to idle rather than a jump to a sibling,
# so the run under measurement is the fixture and nothing else.
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{
  "version": 2,
  "playback": { "auto_advance": false },
  "updates": { "auto_check": false }
}
JSON

HISTORY="$XDG_STATE_HOME/ok-player/history.json"
RESULTS="$OUT_DIR/results.txt"
FAILURES="$OUT_DIR/failures.txt"
: >"$RESULTS"
: >"$FAILURES"

fail() {
  printf '%s\n' "$1" >>"$FAILURES"
  echo "FAIL: $1" >&2
}

app_pid=""
stop_app() {
  [[ -n "$app_pid" ]] || return 0
  kill -TERM "$app_pid" 2>/dev/null || true
  for _ in $(seq 1 50); do
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 0.1
  done
  kill -KILL "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}
trap stop_app EXIT

# Wall-clock seconds at which `marker` first appears in `log`, or empty on timeout.
wait_for_marker() {
  local log="$1" marker="$2" timeout_seconds="$3"
  local deadline=$((SECONDS + timeout_seconds))
  while ((SECONDS < deadline)); do
    if grep -qF "$marker" "$log" 2>/dev/null; then
      python3 -c 'import time; print(f"{time.monotonic():.3f}")'
      return 0
    fi
    sleep 0.1
  done
  return 1
}

run_fixture() {
  local log="$1"
  "$BINARY" "$FIXTURE" >"$log" 2>&1 &
  app_pid=$!
}

# ---------------------------------------------------------------- phase one --
# Watch the file to its end.
run_fixture "$OUT_DIR/first-run.log"
if ! wait_for_marker "$OUT_DIR/first-run.log" 'idle-return-smoke: eof-idle' 90 >/dev/null; then
  fail "the fixture never reached end of file on the first run"
  stop_app
else
  # The end-of-file write is applied while draining the lifecycle event; give the
  # save a moment to land before reading the file back.
  sleep 2
  stop_app
  python3 - "$HISTORY" "$FIXTURE" "$RESULTS" <<'PY' || fail "the entry was not marked watched to the end"
import json, sys
history_path, fixture, results = sys.argv[1:4]
with open(history_path, encoding="utf-8") as stream:
    entry = json.load(stream)["files"][fixture]
with open(results, "a", encoding="utf-8") as stream:
    stream.write(f"after_eof_position={entry['position']}\n")
    stream.write(f"after_eof_finished={entry['finished']}\n")
if not entry["finished"]:
    print(f"entry is not finished after playing to end of file: {entry}", file=sys.stderr)
    raise SystemExit(1)
if entry["position"] != 0.0:
    print(f"entry kept a resume position after end of file: {entry}", file=sys.stderr)
    raise SystemExit(1)
PY
fi

# ---------------------------------------------------------------- phase two --
# Reopen it. Starting at zero means the whole duration has to play again.
run_fixture "$OUT_DIR/second-run.log"
loaded_at="$(wait_for_marker "$OUT_DIR/second-run.log" 'idle-return-smoke: file-loaded' 60 || true)"
ended_at="$(wait_for_marker "$OUT_DIR/second-run.log" 'idle-return-smoke: eof-idle' 90 || true)"
stop_app

if [[ -z "$loaded_at" || -z "$ended_at" ]]; then
  fail "the reopened file never played through to end of file"
else
  played="$(python3 -c "print(f'{float('$ended_at') - float('$loaded_at'):.3f}')")"
  printf 'reopened_playback_seconds=%s\n' "$played" >>"$RESULTS"
  # 80% of the duration: generous against decode and startup jitter, and far below the
  # ~40% of the duration a run resuming from the last periodic sample would take.
  floor="$(python3 -c "print(f'{$DURATION_SECONDS * 0.8:.3f}')")"
  printf 'reopened_playback_floor_seconds=%s\n' "$floor" >>"$RESULTS"
  if ! python3 -c "raise SystemExit(0 if float('$played') >= float('$floor') else 1)"; then
    fail "the reopened file reached end of file after ${played}s, under the ${floor}s a run starting at zero needs: it resumed instead of restarting"
  fi
fi

cat "$RESULTS"
if [[ -s "$FAILURES" ]]; then
  echo "--- failures ---" >&2
  cat "$FAILURES" >&2
  exit 1
fi
echo "resume-after-eof smoke: pass"
SMOKE
then
  echo "--- session log ---" >&2
  tail -60 "$OUT_DIR/session.log" >&2
  exit 1
fi

tail -20 "$OUT_DIR/session.log"
