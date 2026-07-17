#!/usr/bin/env bash
# Focused real-libmpv screenshot smoke for a built or extracted Linux binary.
# It proves the shared shortcut, missing-default-directory creation, non-empty
# output validation, exact saved-path reporting, and artifact hashing under
# Xvfb. Wayland compositor and clipboard behavior remain operator-only.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
FIXTURE="${2:-}"
OUT_DIR="${3:-$ROOT/artifacts/manual-ui/linux-screenshot-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo ffprobe sha256sum; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
[[ -x "$BINARY" ]] || { echo "Screenshot smoke binary is not executable: $BINARY" >&2; exit 1; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd -P)"
if [[ -z "$FIXTURE" ]]; then
  "$ROOT/scripts/generate-linux-acceptance-media.sh" "$OUT_DIR/fixtures"
  FIXTURE="$OUT_DIR/fixtures/dark-with-chapters.mkv"
fi
[[ -f "$FIXTURE" ]] || { echo "Screenshot smoke fixture is missing: $FIXTURE" >&2; exit 1; }

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$FIXTURE" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export HOME="$OUT_DIR/home"
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CACHE_HOME="$OUT_DIR/cache"
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1

mkdir -p "$HOME" "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":2,"updates":{"auto_check":false}}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
timeout 30s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
cleanup() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

window_id=""
for _ in $(seq 1 120); do
  for candidate in $(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null || true); do
    dimensions="$(xwininfo -id "$candidate" 2>/dev/null || true)"
    width="$(awk '/Width:/ {print $2; exit}' <<<"$dimensions")"
    height="$(awk '/Height:/ {print $2; exit}' <<<"$dimensions")"
    if [[ "${width:-0}" -ge 1000 && "${height:-0}" -ge 600 ]]; then
      window_id="$candidate"
      break 2
    fi
  done
  sleep 0.1
done
[[ -n "$window_id" ]] || { cat "$OUT_DIR/app.log" >&2; exit 1; }
xdotool windowactivate "$window_id" >/dev/null 2>&1 || true

# Wait for the generated source to decode before pausing it. The player window
# can map before libmpv has published loaded-media state on a busy builder.
sleep 5
xdotool key --clearmodifiers space
sleep 0.5

screenshot_dir="$HOME/Pictures/OK Player"
[[ ! -e "$screenshot_dir" ]] || { echo "screenshot destination unexpectedly exists" >&2; exit 1; }
xdotool key --clearmodifiers c

screenshot_files=()
for _ in $(seq 1 80); do
  shopt -s nullglob
  screenshot_files=("$screenshot_dir"/*)
  shopt -u nullglob
  ((${#screenshot_files[@]} > 0)) && break
  sleep 0.1
done
((${#screenshot_files[@]} == 1)) || {
  echo "screenshot action created ${#screenshot_files[@]} files, expected 1" >&2
  cat "$OUT_DIR/app.log" >&2
  exit 1
}

screenshot_path="${screenshot_files[0]}"
[[ -s "$screenshot_path" ]] || { echo "screenshot output is missing or empty" >&2; exit 1; }
ffprobe -v error -select_streams v:0 -show_entries stream=width,height \
  -of default=nw=1 "$screenshot_path" >"$OUT_DIR/image-probe.txt"
grep -Eq '^width=[1-9][0-9]*$' "$OUT_DIR/image-probe.txt"
grep -Eq '^height=[1-9][0-9]*$' "$OUT_DIR/image-probe.txt"

for _ in $(seq 1 20); do
  grep -Fq "Screenshot saved to $screenshot_path" "$OUT_DIR/app.log" && break
  sleep 0.1
done
grep -Fq "Screenshot saved to $screenshot_path" "$OUT_DIR/app.log" || {
  echo "screenshot completion did not report the exact saved path" >&2
  cat "$OUT_DIR/app.log" >&2
  exit 1
}

screenshot_sha256="$(sha256sum "$screenshot_path" | awk '{print $1}')"

# A configured file path is deterministically invalid as a destination. It
# must fail before libmpv dispatch, name the destination/cause, and never emit
# a phantom success.
kill "$app_pid" 2>/dev/null || true
wait "$app_pid" 2>/dev/null || true
app_pid=""
invalid_destination="$OUT_DIR/not-a-directory"
printf 'occupied\n' >"$invalid_destination"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<JSON
{"version":2,"screenshots":{"directory":"$invalid_destination"},"updates":{"auto_check":false}}
JSON
timeout 30s "$BINARY" "$FIXTURE" >"$OUT_DIR/invalid-app.log" 2>&1 &
app_pid=$!
invalid_window_id=""
for _ in $(seq 1 120); do
  invalid_window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | head -n1 || true)"
  [[ -n "$invalid_window_id" ]] && break
  sleep 0.1
done
[[ -n "$invalid_window_id" ]] || { cat "$OUT_DIR/invalid-app.log" >&2; exit 1; }
xdotool windowactivate "$invalid_window_id" >/dev/null 2>&1 || true
sleep 5
xdotool key --clearmodifiers c
for _ in $(seq 1 30); do
  grep -Fq "Couldn't save screenshot to $invalid_destination:" "$OUT_DIR/invalid-app.log" && break
  sleep 0.1
done
grep -Fq "Couldn't save screenshot to $invalid_destination:" "$OUT_DIR/invalid-app.log" || {
  echo "invalid screenshot destination did not produce an actionable error" >&2
  cat "$OUT_DIR/invalid-app.log" >&2
  exit 1
}
! grep -Fq 'Screenshot saved to ' "$OUT_DIR/invalid-app.log" || {
  echo "invalid screenshot destination produced a phantom success" >&2
  exit 1
}

printf '%s\n' \
  'evidence_level=xvfb-render' \
  "screenshot_path=$screenshot_path" \
  "screenshot_sha256=$screenshot_sha256" \
  'invalid_destination=pass' \
  'not_proven=GNOME Wayland, clipboard, compositor, portal, focus' \
  >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Screenshot smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Screenshot smoke passed. Results: $OUT_DIR/results.txt"
