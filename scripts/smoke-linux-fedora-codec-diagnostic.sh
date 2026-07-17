#!/usr/bin/env bash
# Deterministic X11 capture for the Fedora missing-codec diagnostic. This proves
# the existing non-modal error-card composition and copy at 1120x680; it does
# not prove live GNOME/KDE, portals, or an actual RPM Fusion codec transition.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/fedora-codec-diagnostic}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
test -x "$BINARY" || { echo "Binary not found: $BINARY" >&2; exit 127; }

mkdir -p "$OUT_DIR"
runner=(dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR")
if [[ -z "${DISPLAY:-}" ]]; then
  runner=(xvfb-run -a --server-args=-screen\ 0\ 1280x900x24\ -nolisten\ tcp "${runner[@]}")
fi
"${runner[@]}" <<'SMOKE'
set -euo pipefail
BINARY="$1"
OUT_DIR="$2"
export GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_USE_PORTAL=0 NO_AT_BRIDGE=1
export XDG_CONFIG_HOME="$OUT_DIR/config" XDG_STATE_HOME="$OUT_DIR/state"
mkdir -p "$XDG_CONFIG_HOME/ok-player"
printf '%s\n' '{"version":1,"updates":{"auto_check":false}}' \
  > "$XDG_CONFIG_HOME/ok-player/settings.json"
xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
trap 'kill "$wm_pid" "$app_pid" 2>/dev/null || true' EXIT
OKP_FIXED_VIEWPORT_SMOKE=1 OKP_PLAYBACK_FAILURE_PREVIEW=fedora-codec \
  timeout 30s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
sleep 4
window_id="$(xdotool search --name 'OK Player' | tail -n1)"
xwininfo -id "$window_id" > "$OUT_DIR/window.xwininfo"
test "$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/window.xwininfo")" = 1120
test "$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/window.xwininfo")" = 680
import -window "$window_id" "$OUT_DIR/fedora-codec-unavailable.png"
grep -q 'playback-failure-title=Codec unavailable' "$OUT_DIR/app.log"
grep -q 'Optional RPM Fusion' "$OUT_DIR/app.log"
SMOKE

echo "Fedora codec diagnostic smoke passed: $OUT_DIR/fedora-codec-unavailable.png"
