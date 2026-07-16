#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT/artifacts/manual-ui/linux-icon-theme-smoke}"
ICON_NAME="com.befeast.okplayer.svg"
SOURCE_ROOT="$ROOT/rust/packaging/linux/icons/hicolor"
SCALABLE_ICON="$ROOT/rust/packaging/linux/com.befeast.okplayer.svg"

for tool in gtk-update-icon-cache rsvg-convert magick rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/hicolor/scalable/apps"
install -Dm644 "$SCALABLE_ICON" "$OUT_DIR/hicolor/scalable/apps/$ICON_NAME"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$SOURCE_ROOT/${size}x${size}/apps/$ICON_NAME" \
    "$OUT_DIR/hicolor/${size}x${size}/apps/$ICON_NAME"
done

cat > "$OUT_DIR/hicolor/index.theme" <<'THEME'
[Icon Theme]
Name=OK Player smoke
Comment=Temporary icon-cache validation theme
Directories=16x16/apps,24x24/apps,32x32/apps,48x48/apps,64x64/apps,scalable/apps

[16x16/apps]
Size=16
Type=Fixed
Context=Applications

[24x24/apps]
Size=24
Type=Fixed
Context=Applications

[32x32/apps]
Size=32
Type=Fixed
Context=Applications

[48x48/apps]
Size=48
Type=Fixed
Context=Applications

[64x64/apps]
Size=64
Type=Fixed
Context=Applications

[scalable/apps]
Size=128
MinSize=65
MaxSize=512
Type=Scalable
Context=Applications
THEME

gtk-update-icon-cache --force "$OUT_DIR/hicolor"
[[ -s "$OUT_DIR/hicolor/icon-theme.cache" ]]

for size in 16 24 32 48 64; do
  icon="$OUT_DIR/hicolor/${size}x${size}/apps/$ICON_NAME"
  if rg -q '<text' "$icon"; then
    echo "$size px icon contains font-dependent text" >&2
    exit 1
  fi
  rsvg-convert --width "$size" --height "$size" "$icon" --output "$OUT_DIR/icon-$size.png"
  dimensions="$(magick identify -format '%wx%h' "$OUT_DIR/icon-$size.png")"
  if [[ "$dimensions" != "${size}x${size}" ]]; then
    echo "$size px icon rendered at $dimensions" >&2
    exit 1
  fi
done

echo "Linux icon-theme smoke passed: $OUT_DIR/hicolor/icon-theme.cache"
