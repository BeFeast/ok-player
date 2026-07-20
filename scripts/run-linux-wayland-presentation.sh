#!/usr/bin/env bash
# Live native/compact/A-B harness for issues #312 and #470. This must run inside
# the target GNOME Wayland session; Xvfb, X11, and callback counters are
# deliberately rejected.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:?usage: run-linux-wayland-presentation.sh <ok-player-binary> <4k60-fixture> [output-dir]}"
FIXTURE="${2:?usage: run-linux-wayland-presentation.sh <ok-player-binary> <4k60-fixture> [output-dir]}"
OUT_DIR="${3:-$ROOT/artifacts/linux-wayland-presentation}"

for tool in ffprobe jq rg timeout cargo; do
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
  (.[0].avg_frame_rate == "60/1" or .[0].avg_frame_rate == "60000/1001")
' <<<"$probe" >/dev/null || {
  echo "Fixture must be 3840x2160 HEVC Main10 yuv420p10le at 60/1 or 60000/1001" >&2
  jq . <<<"$probe" >&2
  exit 2
}
fixture_rate="$(jq -r '.streams[0].avg_frame_rate' <<<"$probe")"

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

run_backend() {
  local backend="$1"
  local mode="${2:-standard}"
  local state="$OUT_DIR/${backend}-${mode}"
  local timeout_seconds=51
  local run_env=(OKP_PRESENT_EXERCISE=1)
  if [[ "$mode" == compact ]]; then
    timeout_seconds=30
    run_env=(OKP_START_COMPACT=1 OKP_DEBUG_INTERACTIONS=1)
  fi
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
    OKP_SKIP_UPDATE_CHECK=1 \
    "${run_env[@]}" \
    timeout --signal=TERM "${timeout_seconds}s" "$BINARY" "$FIXTURE" >"$state/app.log" 2>&1
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
  if [[ "$mode" == compact ]]; then
    rg -q 'interaction: compact-mode-enter size=480x270 floor=284x160' "$state/app.log" || {
      echo "$backend backend did not enter the canonical Mini-player geometry" >&2
      tail -100 "$state/app.log" >&2 || true
      exit 1
    }
  fi
}

run_backend native standard
run_backend native compact
run_backend gtk standard

(
  cd "$ROOT/rust"
  CC=/usr/bin/cc cargo run -q -p okp-core --bin okp-acceptance-evidence -- \
    presentation --log "$OUT_DIR/native-standard/presentation.jsonl" --warmup-seconds 3 \
    >"$OUT_DIR/native-standard/summary.json"
  CC=/usr/bin/cc cargo run -q -p okp-core --bin okp-acceptance-evidence -- \
    presentation --log "$OUT_DIR/native-compact/presentation.jsonl" --warmup-seconds 3 \
    >"$OUT_DIR/native-compact/summary.json"
  CC=/usr/bin/cc cargo run -q -p okp-core --bin okp-acceptance-evidence -- \
    presentation --log "$OUT_DIR/gtk-standard/presentation.jsonl" --warmup-seconds 3 --report-only \
    >"$OUT_DIR/gtk-standard/summary.json"
)

jq -e '
  (.backend == "native-wayland-egl" or .backend == "native-wayland-dmabuf") and
  .compositor_presented > 0 and
  .compositor_presented > .compositor_discarded and
  .presents_per_second >= 55
' "$OUT_DIR/native-compact/summary.json" >/dev/null || {
  echo "Native Mini-player did not retain compositor-presented video" >&2
  jq . "$OUT_DIR/native-compact/summary.json" >&2
  exit 1
}

jq -n \
  --arg fixture_rate "$fixture_rate" \
  --slurpfile native_standard "$OUT_DIR/native-standard/summary.json" \
  --slurpfile native_compact "$OUT_DIR/native-compact/summary.json" \
  --slurpfile gtk "$OUT_DIR/gtk-standard/summary.json" \
  '{fixture:("3840x2160 HEVC Main10 " + $fixture_rate),native_standard:$native_standard[0],native_mini_player:$native_compact[0],gtk_glarea:$gtk[0]}' \
  >"$OUT_DIR/comparison.json"

echo "Live Wayland native and Mini-player presentation passed. Evidence: $OUT_DIR/comparison.json"
