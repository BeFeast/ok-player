#!/usr/bin/env bash
# Build side-by-side, same-size reference/implementation sheets.
set -euo pipefail

REFERENCE_DIR="${1:?reference directory is required}"
IMPLEMENTATION_DIR="${2:?implementation directory is required}"
OUT_DIR="${3:?output directory is required}"

command -v magick >/dev/null 2>&1 || { echo "Missing required tool: magick" >&2; exit 127; }
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

made=0
for implementation in "$IMPLEMENTATION_DIR"/*.png; do
  [[ -e "$implementation" ]] || continue
  state="$(basename "$implementation")"
  reference="$REFERENCE_DIR/$state"
  [[ -f "$reference" ]] || continue

  dimensions="$(identify -format '%w %h' "$implementation")"
  read -r width height <<<"$dimensions"
  ref_width="$(identify -format '%w' "$reference")"
  ref_height="$(identify -format '%h' "$reference")"
  if [[ "$ref_width" != "$width" || "$ref_height" != "$height" ]]; then
    echo "$state: reference ${ref_width}x${ref_height} does not match implementation ${width}x${height}" >&2
    exit 1
  fi

  magick "$reference" "$implementation" +append "$OUT_DIR/${state%.png}-comparison.png"
  made=$((made + 1))
done

(( made > 0 )) || { echo "No same-name reference/implementation pairs found" >&2; exit 1; }
echo "Created $made comparison sheets in $OUT_DIR"
