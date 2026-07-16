#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMMAND="${1:-profile}"
FRAME_COUNT="${OKP_4K60_FRAMES:-600}"

usage() {
  echo "usage:" >&2
  echo "  $0 generate [output-directory]" >&2
  echo "  $0 profile [ok-player-binary] [output-directory]" >&2
  exit 2
}

case "$COMMAND" in
  generate)
    OUT_DIR="${2:-$ROOT/artifacts/manual-performance/linux-4k60}"
    ;;
  profile)
    BINARY="${2:-$ROOT/rust/target/release/okp-linux-gtk}"
    OUT_DIR="${3:-$ROOT/artifacts/manual-performance/linux-4k60}"
    ;;
  *) usage ;;
esac

for tool in ffmpeg ffprobe; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

mkdir -p "$OUT_DIR"
FIXTURE="$OUT_DIR/4k60-hevc-main10.mkv"

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=size=3840x2160:rate=60" \
  -frames:v "$FRAME_COUNT" -an \
  -c:v libx265 -preset ultrafast -pix_fmt yuv420p10le \
  -x265-params "log-level=error:keyint=60:min-keyint=60:scenecut=0" \
  -metadata title="OK Player 4K60 HEVC Main10 profile fixture" \
  "$FIXTURE"

probe="$({
  ffprobe -v error -select_streams v:0 \
    -show_entries stream=codec_name,profile,width,height,pix_fmt,avg_frame_rate \
    -of csv=p=0 "$FIXTURE"
} | tr -d '\r')"

IFS=',' read -r codec profile width height pix_fmt frame_rate <<<"$probe"
if [[ "$codec" != "hevc" || "$profile" != "Main 10" || "$width" != "3840" || \
      "$height" != "2160" || "$pix_fmt" != "yuv420p10le" || "$frame_rate" != "60/1" ]]; then
  echo "Unexpected fixture metadata: $probe" >&2
  exit 1
fi

printf '%s\n' "$probe" >"$OUT_DIR/fixture.ffprobe.csv"
echo "Generated $FRAME_COUNT-frame fixture: $FIXTURE"

if [[ "$COMMAND" == "generate" ]]; then
  exit 0
fi

for tool in mpv timeout; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
if [[ ! -x "$BINARY" ]]; then
  echo "OK Player binary is not executable: $BINARY" >&2
  exit 2
fi
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
  echo "Profiling requires a live X11 or Wayland desktop." >&2
  exit 2
fi

DURATION_SECONDS="$(( (FRAME_COUNT + 59) / 60 + 5 ))"

run_plain_mpv() {
  local mode="$1"
  local log="$OUT_DIR/plain-mpv-${mode}.log"
  set +e
  timeout --signal=TERM "${DURATION_SECONDS}s" \
    mpv --no-config --vo=gpu --gpu-api=opengl --audio=no --keep-open=no \
      --hwdec="$mode" --frames="$FRAME_COUNT" \
      --msg-level=all=no,status=status \
      --term-status-msg='hwdec=${hwdec-current} estimated_fps=${estimated-vf-fps} display_fps=${display-fps} vo_drops=${vo-drop-frame-count} decoder_drops=${decoder-frame-drop-count}' \
      "$FIXTURE" >"$log" 2>&1
  local status=$?
  set -e
  if [[ "$status" -ne 0 && "$status" -ne 124 ]]; then
    echo "Plain mpv profile failed for hwdec=$mode (status $status): $log" >&2
    exit "$status"
  fi
}

run_ok_player() {
  local mode="$1"
  local config_root="$OUT_DIR/config-${mode}"
  local profile_path="$OUT_DIR/ok-player-${mode}.json"
  local log="$OUT_DIR/ok-player-${mode}.log"
  rm -rf "$config_root"
  mkdir -p "$config_root/ok-player"
  if [[ "$mode" == "no" ]]; then
    printf '%s\n' '{"version":2,"video":{"hwdec":"no"}}' \
      >"$config_root/ok-player/settings.json"
  else
    printf '%s\n' '{"version":2}' >"$config_root/ok-player/settings.json"
  fi

  set +e
  timeout --signal=TERM "${DURATION_SECONDS}s" \
    env XDG_CONFIG_HOME="$config_root" \
      OKP_RENDER_PROFILE_PATH="$profile_path" \
      OKP_SKIP_OPEN_INSTALLER=1 \
      OKP_SKIP_DEB_SELF_INSTALL=1 \
      "$BINARY" "$FIXTURE" >"$log" 2>&1
  local status=$?
  set -e
  if [[ "$status" -ne 0 && "$status" -ne 124 ]]; then
    echo "OK Player profile failed for hwdec=$mode (status $status): $log" >&2
    exit "$status"
  fi
  if [[ ! -s "$profile_path" ]]; then
    echo "OK Player did not write a profile for hwdec=$mode: $log" >&2
    exit 1
  fi
}

for mode in no auto-safe; do
  run_plain_mpv "$mode"
  run_ok_player "$mode"
done

echo "4K60 profiles written to $OUT_DIR"
for profile_path in "$OUT_DIR"/ok-player-*.json; do
  echo "--- $(basename "$profile_path")"
  sed -n '1,240p' "$profile_path"
done
