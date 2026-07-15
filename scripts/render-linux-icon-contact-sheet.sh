#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ICON="${1:-$ROOT/rust/packaging/linux/com.befeast.okplayer.svg}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-icon-contact-sheet}"
SIZES=(16 24 32 48 64 128 256)

for tool in rsvg-convert magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

if [[ ! -f "$ICON" ]]; then
  echo "Missing icon: $ICON" >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/tiles"

tiles=()
for size in "${SIZES[@]}"; do
  png="$OUT_DIR/ok-player-${size}.png"
  rsvg-convert --width "$size" --height "$size" "$ICON" --output "$png"

  dark="$OUT_DIR/tiles/dark-${size}.png"
  light="$OUT_DIR/tiles/light-${size}.png"
  label="$OUT_DIR/tiles/label-${size}.png"
  tile="$OUT_DIR/tiles/tile-${size}.png"
  magick -size 300x280 canvas:'#080b0e' "$png" \
    -gravity center -compose over -composite "$dark"
  magick -size 300x280 canvas:'#eef4f9' "$png" \
    -gravity center -compose over -composite "$light"
  magick -size 300x32 canvas:'#171a1f' -fill '#f4f8f7' \
    -gravity center -pointsize 14 -annotate 0 "${size} px" "$label"
  magick "$dark" "$light" "$label" -append "$tile"
  tiles+=("$tile")
done

magick "${tiles[@]}" +append "$OUT_DIR/icon-contact-sheet.png"
echo "Icon contact sheet written to $OUT_DIR/icon-contact-sheet.png"
