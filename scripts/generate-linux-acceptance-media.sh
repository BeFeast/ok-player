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
  local color="$1" output="$2"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=1280x720:r=24:d=30" \
    -map 0:v:0 \
    -c:v libx264 -preset medium -tune stillimage -profile:v high -level:v 3.1 \
    -pix_fmt yuv420p -g 48 -an \
    -metadata title="OK Player acceptance fixture" \
    "$output"
}

generate_video "0x08090b" "$OUT_DIR/dark-base.mkv"
generate_video "0xf2f4f5" "$OUT_DIR/bright.mkv"

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
  -i "$OUT_DIR/dark-base.mkv" -i "$OUT_DIR/chapters.ffmeta" \
  -map 0 -map_metadata 1 -c copy "$OUT_DIR/dark-with-chapters.mkv"
rm "$OUT_DIR/dark-base.mkv"

for name in "Episode 1.mkv" "Episode 2.mkv" "Episode 10.mkv"; do
  cp "$OUT_DIR/dark-with-chapters.mkv" "$OUT_DIR/natural-queue/$name"
done
printf 'not media\n' >"$OUT_DIR/natural-queue/notes.txt"

dark_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/dark-with-chapters.mkv")"
bright_duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$OUT_DIR/bright.mkv")"

cat >"$OUT_DIR/fixtures.json" <<JSON
{
  "schema_version": 1,
  "media": [
    {"id": "dark-with-chapters", "path": "dark-with-chapters.mkv", "duration_seconds": $dark_duration, "chapters": 3},
    {"id": "bright", "path": "bright.mkv", "duration_seconds": $bright_duration, "chapters": 0}
  ],
  "natural_queue": {
    "directory": "natural-queue",
    "expected_order": ["Episode 1.mkv", "Episode 2.mkv", "Episode 10.mkv"],
    "ignored": ["notes.txt"]
  }
}
JSON

echo "Generated Linux acceptance media in $OUT_DIR"
