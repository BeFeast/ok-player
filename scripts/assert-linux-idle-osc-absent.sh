#!/usr/bin/env bash
set -euo pipefail

image="${1:?usage: assert-linux-idle-osc-absent.sh IMAGE [LABEL]}"
label="${2:-Idle canvas}"

if ! command -v magick >/dev/null 2>&1; then
  echo "Missing required tool: magick" >&2
  exit 127
fi
if [[ ! -f "$image" ]]; then
  echo "Missing idle-canvas capture: $image" >&2
  exit 2
fi

# The standard OSC occupies this band at the canonical 1120x680 window size.
# Compare each pixel with a locally blurred version of the same band: approved
# light and dark idle gradients have almost no high-frequency residual, while
# the OSC pill edges and control glyphs remain strong regardless of theme.
residual_mean="$(
  magick "$image" \
    -crop 1088x18+16+610 +repage \
    -colorspace gray \
    \( +clone -blur 0x6 \) \
    -compose difference -composite \
    -format '%[fx:mean]' info:
)"

awk -v value="$residual_mean" -v label="$label" 'BEGIN {
  printf "%s idle OSC residual mean: %.6f\n", label, value
  if (!(value < 0.004)) {
    printf "%s leaked standard playback OSC geometry: residual mean=%.6f\n", label, value > "/dev/stderr"
    exit 1
  }
}'
