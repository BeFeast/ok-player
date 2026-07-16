#!/usr/bin/env bash
# Generate small, deterministic media fixtures for Linux acceptance runs.
set -euo pipefail

OUT_DIR="${1:-artifacts/linux-acceptance/fixtures}"

for tool in ffmpeg ffprobe; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/natural-queue"

generate_video() {
  local color="$1" output="$2" duration="${3:-30}"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=1280x720:r=24:d=${duration}" \
    -map 0:v:0 \
    -c:v libx264 -preset medium -tune stillimage -profile:v high -level:v 3.1 \
    -pix_fmt yuv420p -g 48 -an \
    -metadata title="OK Player acceptance fixture" \
    "$output"
}

generate_video "0x08090b" "$OUT_DIR/dark.mkv"
# Long enough to produce useful 30-second interval markers, with no chapter metadata.
generate_video "0x08090b" "$OUT_DIR/dark-no-chapters-long.mkv" 90

# Native-size and workarea-clamp fixtures for the main-window fit smoke. A low
# frame rate keeps the 4K fixture quick to generate while preserving real video
# dimensions through libmpv's file-loaded/video-reconfig lifecycle.
generate_window_fit_video() {
  local size="$1" color="$2" title="$3" output="$4"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=${size}:r=2:d=12" \
    -map 0:v:0 \
    -c:v libx264 -preset ultrafast -tune stillimage -crf 35 \
    -pix_fmt yuv420p -g 4 -an \
    -metadata title="$title" \
    "$output"
}

generate_window_fit_hevc_main10_video() {
  local size="$1" color="$2" title="$3" output="$4"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=${size}:r=2:d=12" \
    -map 0:v:0 \
    -c:v libx265 -preset ultrafast -crf 35 -profile:v main10 \
    -pix_fmt yuv420p10le \
    -x265-params 'log-level=error:keyint=4:min-keyint=4:scenecut=0' \
    -an -metadata title="$title" \
    "$output"
}

generate_window_fit_video \
  "320x180" "0x17313a" "OK Player small window-fit fixture" "$OUT_DIR/fit-small.mkv"
generate_window_fit_hevc_main10_video \
  "3840x2160" "0x241b35" "OK Player 4K window-fit fixture" "$OUT_DIR/fit-4k.mkv"

fit_4k_codec="$(ffprobe -v error -select_streams v:0 -show_entries stream=codec_name -of default=nw=1:nk=1 "$OUT_DIR/fit-4k.mkv")"
fit_4k_pixel_format="$(ffprobe -v error -select_streams v:0 -show_entries stream=pix_fmt -of default=nw=1:nk=1 "$OUT_DIR/fit-4k.mkv")"
if [[ "$fit_4k_codec" != "hevc" || "$fit_4k_pixel_format" != "yuv420p10le" ]]; then
  echo "4K fit fixture is not HEVC Main10: codec=${fit_4k_codec} pix_fmt=${fit_4k_pixel_format}" >&2
  exit 1
fi

ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "color=c=0xf2f4f5:s=1280x720:r=24:d=30" \
  -map 0:v:0 -vf "noise=alls=3:allf=t" \
  -c:v libx264 -preset medium -profile:v high -level:v 3.1 \
  -pix_fmt yuv420p -g 48 -an \
  -metadata title="OK Player bright acceptance fixture" \
  "$OUT_DIR/bright.mkv"

# A longer, non-uniform source gives the throttled localhost acceptance server
# enough payload to build a real partial demuxer cache instead of downloading
# the entire solid-color fixture before the first screenshot.
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=s=640x360:r=24:d=60" \
  -map 0:v:0 \
  -c:v libx264 -preset veryfast -profile:v high -level:v 3.1 \
  -pix_fmt yuv420p -g 48 -b:v 900k -maxrate 900k -bufsize 1800k -an \
  -metadata title="OK Player buffered acceptance fixture" \
  "$OUT_DIR/buffered.mkv"

cat >"$OUT_DIR/chapters.ffmeta" <<'META'
;FFMETADATA1
title=OK Player acceptance fixture with chapters
[CHAPTER]
TIMEBASE=1/1000
START=0
END=10000
title=Cold Open
[CHAPTER]
TIMEBASE=1/1000
START=10000
END=20000
title=Main Title
[CHAPTER]
TIMEBASE=1/1000
START=20000
END=30000
title=Final Scene
META

ffmpeg -hide_banner -loglevel error -y \
  -i "$OUT_DIR/dark.mkv" -i "$OUT_DIR/chapters.ffmeta" \
  -map 0 -map_metadata 1 -c copy "$OUT_DIR/dark-with-chapters.mkv"

for name in "Episode 1.mkv" "Episode 2.mkv" "Episode 10.mkv"; do
  cp "$OUT_DIR/dark-with-chapters.mkv" "$OUT_DIR/natural-queue/$name"
done
printf 'not media\n' >"$OUT_DIR/natural-queue/notes.txt"

dark_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/dark-with-chapters.mkv")"
bright_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/bright.mkv")"
buffered_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/buffered.mkv")"
interval_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/dark-no-chapters-long.mkv")"
fit_small_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/fit-small.mkv")"
fit_4k_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/fit-4k.mkv")"

cat >"$OUT_DIR/fixtures.json" <<JSON
{
  "schema_version": 1,
  "media": [
    {"id": "dark", "path": "dark.mkv", "duration_seconds": $dark_duration, "chapters": 0},
    {"id": "dark-with-chapters", "path": "dark-with-chapters.mkv", "duration_seconds": $dark_duration, "chapters": 3},
    {"id": "dark-no-chapters-long", "path": "dark-no-chapters-long.mkv", "duration_seconds": $interval_duration, "chapters": 0},
    {"id": "bright", "path": "bright.mkv", "duration_seconds": $bright_duration, "chapters": 0},
    {"id": "buffered", "path": "buffered.mkv", "duration_seconds": $buffered_duration, "chapters": 0},
    {"id": "fit-small", "path": "fit-small.mkv", "duration_seconds": $fit_small_duration, "chapters": 0},
    {"id": "fit-4k", "path": "fit-4k.mkv", "duration_seconds": $fit_4k_duration, "chapters": 0, "video_codec": "$fit_4k_codec", "pixel_format": "$fit_4k_pixel_format"}
  ],
  "natural_queue": {
    "directory": "natural-queue",
    "expected_order": ["Episode 1.mkv", "Episode 2.mkv", "Episode 10.mkv"],
    "ignored": ["notes.txt"]
  }
}
JSON

echo "Generated Linux acceptance media in $OUT_DIR"
