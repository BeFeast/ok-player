#!/usr/bin/env bash
# Live A/B harness for issue #312. This must run inside the target GNOME Wayland
# session; Xvfb, X11, and callback counters are deliberately rejected.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-wayland-presentation.sh <ok-player-binary> <4k60-fixture> [output-dir]}"
FIXTURE="${2:?usage: run-linux-wayland-presentation.sh <ok-player-binary> <4k60-fixture> [output-dir]}"
OUT_DIR="${3:-$ROOT/artifacts/linux-wayland-presentation}"

for tool in ffprobe jq timeout cargo; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -n "${WAYLAND_DISPLAY:-}" ]] || { echo "WAYLAND_DISPLAY is required" >&2; exit 2; }
[[ "${XDG_SESSION_TYPE:-}" == "wayland" ]] || { echo "XDG_SESSION_TYPE=wayland is required" >&2; exit 2; }
[[ -f "$FIXTURE" ]] || { echo "Fixture does not exist: $FIXTURE" >&2; exit 2; }

probe="$(ffprobe -v error -select_streams v:0 \
  -show_entries stream=codec_name,profile,pix_fmt,width,height,avg_frame_rate \
  -of json "$FIXTURE")"
jq -e '
  .streams | length == 1 and
  .[0].codec_name == "hevc" and
  .[0].profile == "Main 10" and
  .[0].pix_fmt == "yuv420p10le" and
  .[0].width == 3840 and
  .[0].height == 2160 and
  .[0].avg_frame_rate == "60/1"
' <<<"$probe" >/dev/null || {
  echo "Fixture must be exactly 3840x2160 HEVC Main10 yuv420p10le 60/1" >&2
  jq . <<<"$probe" >&2
  exit 2
}

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

run_backend() {
  local backend="$1"
  local state="$OUT_DIR/$backend"
  mkdir -p "$state/config/ok-player" "$state/data" "$state/state" "$state/cache" "$state/home"
  cat >"$state/config/ok-player/settings.json" <<'JSON'
{"version":2,"video":{"hwdec":"auto-safe"},"updates":{"auto_check":false}}
JSON

  set +e
  env \
    XDG_CONFIG_HOME="$state/config" \
    XDG_DATA_HOME="$state/data" \
    XDG_STATE_HOME="$state/state" \
    XDG_CACHE_HOME="$state/cache" \
    HOME="$state/home" \
    GDK_BACKEND=wayland \
    GSK_RENDERER=gl \
    OKP_VIDEO_BACKEND="$backend" \
    OKP_PRESENT_LOG="$state/presentation.jsonl" \
    OKP_PRESENT_EXERCISE=1 \
    OKP_SKIP_UPDATE_CHECK=1 \
    timeout --signal=TERM 51s "$BINARY" "$FIXTURE" >"$state/app.log" 2>&1
  status=$?
  set -e
  if [[ "$status" -ne 0 && "$status" -ne 124 && "$status" -ne 143 ]]; then
    echo "$backend backend exited with status $status" >&2
    tail -100 "$state/app.log" >&2 || true
    exit "$status"
  fi
  [[ -s "$state/presentation.jsonl" ]] || {
    echo "$backend backend produced no presentation evidence" >&2
    tail -100 "$state/app.log" >&2 || true
    exit 1
  }
}

run_backend native
run_backend gtk

(
  cd "$ROOT/rust"
  CC=/usr/bin/cc cargo run -q -p okp-core --bin okp-acceptance-evidence -- \
    presentation --log "$OUT_DIR/native/presentation.jsonl" --warmup-seconds 3 \
    >"$OUT_DIR/native/summary.json"
  CC=/usr/bin/cc cargo run -q -p okp-core --bin okp-acceptance-evidence -- \
    presentation --log "$OUT_DIR/gtk/presentation.jsonl" --warmup-seconds 3 --report-only \
    >"$OUT_DIR/gtk/summary.json"
)

jq -n \
  --slurpfile native "$OUT_DIR/native/summary.json" \
  --slurpfile gtk "$OUT_DIR/gtk/summary.json" \
  '{fixture:"3840x2160 HEVC Main10 60/1",native:$native[0],gtk_glarea:$gtk[0]}' \
  >"$OUT_DIR/comparison.json"

echo "Live Wayland native presentation passed. Evidence: $OUT_DIR/comparison.json"
