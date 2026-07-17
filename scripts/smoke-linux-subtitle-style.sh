#!/usr/bin/env bash
# Deterministic subtitle-presentation smoke for issue #194. It renders a real embedded SRT track
# through libmpv at the 1120x680 reference viewport and proves the curated color, size, and
# position controls differ in the expected direction. The real-libmpv unit test separately pins
# the boxed-background properties because Xvfb exposes a black video plane on this host. Xvfb
# proves pixels and layout only; it is not evidence for live GNOME/Wayland compositor behavior.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-subtitle-style-smoke}"
SUBTITLE="$ROOT/tests/OkPlayer.IntegrationTests/fixtures/subtest.srt"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || { echo "Missing required tool: $tool" >&2; exit 127; }
done
[[ -f "$SUBTITLE" ]] || { echo "Missing subtitle fixture: $SUBTITLE" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

ffmpeg -loglevel error -y \
  -f lavfi -i 'color=c=0x32648A:s=1280x720:r=30:d=4' \
  -i "$SUBTITLE" \
  -map 0:v:0 -map 1:0 -c:v libx264 -pix_fmt yuv420p -c:s srt -t 4 \
  "$OUT_DIR/subtitle-style-fixture.mkv"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
FIXTURE="$OUT_DIR/subtitle-style-fixture.mkv"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

capture_state() {
  local name="$1" scale="$2" position="$3" style="$4"
  local config="$OUT_DIR/config-$name"
  local state="$OUT_DIR/state-$name"
  mkdir -p "$config/ok-player" "$state"
  printf '%s\n' \
    "{\"version\":2,\"subtitles\":{\"scale\":$scale,\"position\":$position,\"style\":\"$style\"},\"updates\":{\"auto_check\":false}}" \
    >"$config/ok-player/settings.json"

  env \
    XDG_CONFIG_HOME="$config" \
    XDG_STATE_HOME="$state" \
    OKP_FIXED_VIEWPORT_SMOKE=1 \
    OKP_DISABLE_MPRIS=1 \
    OKP_SKIP_UPDATE_CHECK=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    timeout 20s "$BINARY" "$FIXTURE" --resume 1 --sub 1 >"$OUT_DIR/$name-app.log" 2>&1 &
  app_pid=$!

  local window_id=""
  for _ in $(seq 1 100); do
    window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | tail -n1 || true)"
    [[ -n "$window_id" ]] && break
    sleep 0.1
  done
  [[ -n "$window_id" ]] || { echo "$name: main window did not appear" >&2; exit 1; }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  for _ in $(seq 1 80); do
    grep -q 'Applied explicit launch resume at 1.000s' "$OUT_DIR/$name-app.log" && break
    sleep 0.1
  done
  grep -q 'Applied explicit launch resume at 1.000s' "$OUT_DIR/$name-app.log" || {
    echo "$name: media did not finish loading" >&2
    exit 1
  }
  xdotool key --clearmodifiers space
  xdotool mousemove --window "$window_id" 560 340
  sleep 1

  xwininfo -id "$window_id" >"$OUT_DIR/$name.xwininfo"
  local width height window_x window_y
  width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/$name.xwininfo")"
  height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/$name.xwininfo")"
  window_x="$(awk -F': ' '/Absolute upper-left X:/ {print $2; exit}' "$OUT_DIR/$name.xwininfo" | xargs)"
  window_y="$(awk -F': ' '/Absolute upper-left Y:/ {print $2; exit}' "$OUT_DIR/$name.xwininfo" | xargs)"
  [[ "$width" == 1120 && "$height" == 680 ]] || {
    echo "$name: unexpected geometry ${width}x${height}" >&2
    exit 1
  }
  import -window root "$OUT_DIR/$name-root.png"
  magick "$OUT_DIR/$name-root.png" \
    -crop "${width}x${height}+${window_x}+${window_y}" +repage \
    "$OUT_DIR/$name.png"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 0.25
}

capture_state default-standard 1.0 100 Default
capture_state classic-standard 1.0 100 Classic
capture_state boxed-large-raised 1.4 90 Contrast

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  echo "Subtitle style smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

caption_geometry() {
  local image="$1"
  magick "$image" \
    -crop 800x500+160+60 +repage \
    -colorspace gray -threshold 75% -trim \
    -format '%wx%h%X%Y' info:
}

parse_geometry() {
  local geometry="$1"
  if [[ "$geometry" =~ ^([0-9]+)x([0-9]+)\+([0-9]+)\+([0-9]+)$ ]]; then
    printf '%s %s %s %s\n' \
      "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}" "${BASH_REMATCH[4]}"
  else
    echo "Could not parse caption geometry: $geometry" >&2
    exit 1
  fi
}

read -r default_w default_h default_x default_y \
  <<<"$(parse_geometry "$(caption_geometry "$OUT_DIR/default-standard.png")")"
read -r classic_w classic_h classic_x classic_y \
  <<<"$(parse_geometry "$(caption_geometry "$OUT_DIR/classic-standard.png")")"
read -r large_w large_h large_x large_y \
  <<<"$(parse_geometry "$(caption_geometry "$OUT_DIR/boxed-large-raised.png")")"

caption_blue_mean() {
  local image="$1" width="$2" height="$3" x="$4" y="$5"
  local crop_x=$((160 + x - 18))
  local crop_y=$((60 + y - 12))
  local crop_w=$((width + 36))
  local crop_h=$((height + 24))
  magick "$image" -crop "${crop_w}x${crop_h}+${crop_x}+${crop_y}" \
    -format '%[fx:mean.b]' info:
}

default_blue="$(caption_blue_mean "$OUT_DIR/default-standard.png" "$default_w" "$default_h" "$default_x" "$default_y")"
classic_blue="$(caption_blue_mean "$OUT_DIR/classic-standard.png" "$classic_w" "$classic_h" "$classic_x" "$classic_y")"

if ! awk -v plain="$default_blue" -v classic="$classic_blue" \
  'BEGIN { exit !(plain - classic > 0.08) }'; then
  echo "Classic preset did not reduce the caption blue channel: default=${default_blue}, classic=${classic_blue}" >&2
  exit 1
fi

if (( large_h * 100 < classic_h * 120 )); then
  echo "Large subtitle did not grow enough: standard=${classic_w}x${classic_h}, large=${large_w}x${large_h}" >&2
  exit 1
fi

default_global_y=$((60 + classic_y))
raised_global_y=$((60 + large_y))
if (( raised_global_y > default_global_y - 45 )); then
  echo "Raised subtitle did not move up enough: standard-y=${default_global_y}, raised-y=${raised_global_y}" >&2
  exit 1
fi
if (( raised_global_y + large_h >= 590 )); then
  echo "Raised subtitle overlaps the OSC band: y=${raised_global_y}, height=${large_h}" >&2
  exit 1
fi

printf '%s\n' \
  "default-caption=${default_w}x${default_h}+${default_x}+${default_y}" \
  "classic-caption=${classic_w}x${classic_h}+${classic_x}+${classic_y}" \
  "large-raised-caption=${large_w}x${large_h}+${large_x}+${large_y}" \
  "default-blue-mean=$default_blue" \
  "classic-blue-mean=$classic_blue" \
  >"$OUT_DIR/measurements.txt"

echo "Subtitle style smoke passed. Captures: $OUT_DIR"
