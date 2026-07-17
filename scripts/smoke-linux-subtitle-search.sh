#!/usr/bin/env bash
# X11/Xvfb smoke for the subtitle-search query and result states.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-subtitle-search-smoke}"
FIXTURE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

[[ -f "$FIXTURE" ]] || { echo "Missing media fixture: $FIXTURE" >&2; exit 127; }
if [[ ! -x "$BINARY" ]] && ! command -v "$BINARY" >/dev/null 2>&1; then
  echo "Player binary is not executable: $BINARY" >&2
  exit 127
fi

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi
export LIBGL_ALWAYS_SOFTWARE=1

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$FIXTURE" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
FIXTURE="$2"
OUT_DIR="$3"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_DISABLE_MPRIS=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_OPEN_SUBTITLE_SEARCH_ON_STARTUP=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

timeout 25s "$BINARY" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
sleep 6

dialog_id="$(xdotool search --onlyvisible --name 'Search Subtitles' | head -n1)"
[[ -n "$dialog_id" ]] || {
  echo "Subtitle search dialog did not open" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
}
xdotool windowactivate "$dialog_id" >/dev/null 2>&1 || true
import -window "$dialog_id" "$OUT_DIR/subtitle-search-empty-query.png"

xdotool type --clearmodifiers 'matching'
xdotool key --clearmodifiers Return
sleep 1
import -window "$dialog_id" "$OUT_DIR/subtitle-search-results.png"

dialog_width="$(xwininfo -id "$dialog_id" | awk '/Width:/ {print $2; exit}')"
dialog_height="$(xwininfo -id "$dialog_id" | awk '/Height:/ {print $2; exit}')"
edge_mean="$(magick "$OUT_DIR/subtitle-search-results.png" -colorspace gray -edge 1 -format '%[fx:mean]' info:)"
if ! awk -v edge="$edge_mean" 'BEGIN {exit !(edge > 0.004)}'; then
  echo "Subtitle search result surface did not render enough structure: edge=$edge_mean" >&2
  exit 1
fi

printf '%s\n' \
  "dialog_geometry=${dialog_width}x${dialog_height}" \
  "result_edge_mean=$edge_mean" \
  'empty_query_state=pass' \
  'query_result_state=pass' >"$OUT_DIR/results.txt"
SMOKE
then
  echo "Subtitle-search smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

echo "Linux subtitle-search smoke passed. Results: $OUT_DIR/results.txt"
