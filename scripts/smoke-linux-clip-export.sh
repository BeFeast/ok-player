#!/usr/bin/env bash
# Deterministic X11/Xvfb rendering smoke for the issue #228 A-B export states.
# It proves native popover composition and state copy only; it does not exercise
# an encoder or establish live GNOME/Wayland compositor behavior.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-clip-export-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
if [[ ! -x "$BINARY" ]] && ! command -v "$BINARY" >/dev/null 2>&1; then
  echo "Player binary is not executable: $BINARY" >&2
  exit 127
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" \
    >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export OKP_FIXED_VIEWPORT_SMOKE=1
export OKP_SKIP_UPDATE_CHECK=1
export OKP_SKIP_OPEN_INSTALLER=1
export OKP_SKIP_DEB_SELF_INSTALL=1
export OKP_OPEN_MORE_POPOVER_ON_STARTUP=1
export OKP_PLAYBACK_FRAME_PREVIEW=bright

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

popup_window() {
  local tree="$1"
  local best_area=0 best=""
  while read -r id geometry; do
    if [[ "$geometry" =~ ^([0-9]+)x([0-9]+)\+(-?[0-9]+)\+(-?[0-9]+)$ ]]; then
      local width="${BASH_REMATCH[1]}" height="${BASH_REMATCH[2]}"
      local x="${BASH_REMATCH[3]}" y="${BASH_REMATCH[4]}"
      local area=$((width * height))
      if (( width < 600 && height > 80 && area > best_area )); then
        best_area=$area
        best="$id $width $height $x $y"
      fi
    fi
  done < <(awk '/^     0x.*"okp-linux-gtk"/ {print $1, $5}' "$tree")
  [[ -n "$best" ]] || return 1
  printf '%s\n' "$best"
}

capture() {
  local preview_state="$1" shot="$2"
  local state_dir="$OUT_DIR/state-$shot"
  local config_dir="$OUT_DIR/config-$shot"
  mkdir -p "$state_dir" "$config_dir/ok-player"
  printf '%s\n' '{"version":2,"updates":{"auto_check":false}}' \
    >"$config_dir/ok-player/settings.json"

  env \
    XDG_STATE_HOME="$state_dir" \
    XDG_CONFIG_HOME="$config_dir" \
    OKP_CLIP_EXPORT_PREVIEW_STATE="$preview_state" \
    timeout 20s "$BINARY" >"$OUT_DIR/$shot-app.log" 2>&1 &
  app_pid=$!
  sleep 7

  local window_id
  window_id="$(xdotool search --onlyvisible --name 'OK Player' | tail -n1)"
  [[ -n "$window_id" ]] || { cat "$OUT_DIR/$shot-app.log" >&2; exit 1; }
  xwininfo -id "$window_id" >"$OUT_DIR/$shot-window.xwininfo"
  local width height
  width="$(awk '/Width:/ {print $2; exit}' "$OUT_DIR/$shot-window.xwininfo")"
  height="$(awk '/Height:/ {print $2; exit}' "$OUT_DIR/$shot-window.xwininfo")"
  [[ "$width" == 1120 && "$height" == 680 ]] || {
    echo "$shot: unexpected player geometry ${width}x${height}" >&2
    exit 1
  }

  xwininfo -root -tree >"$OUT_DIR/$shot-tree.txt"
  local popup_id outer_width outer_height outer_x outer_y
  read -r popup_id outer_width outer_height outer_x outer_y \
    <<<"$(popup_window "$OUT_DIR/$shot-tree.txt")"
  [[ -n "${popup_id:-}" ]] || { echo "$shot: More popover did not open" >&2; exit 1; }

  local visible_width=$((outer_width - 96))
  local visible_height=$((outer_height - 96))
  [[ "$visible_width" == 212 ]] || {
    echo "$shot: More width is $visible_width, expected 212" >&2
    exit 1
  }
  if [[ "$preview_state" == hidden ]]; then
    (( visible_height >= 250 && visible_height < 450 )) || {
      echo "$shot: reference More geometry is unexpected; height=$visible_height" >&2
      exit 1
    }
  else
    (( visible_height >= 450 )) || {
      echo "$shot: export row/reason is missing; popover height=$visible_height" >&2
      exit 1
    }
  fi

  import -window root "$OUT_DIR/$shot-window.png"

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture hidden reference-more
capture missing-tooling export-missing-tooling
capture ready export-ready

pixel_delta="$(
  magick compare -metric AE \
    "$OUT_DIR/export-missing-tooling-window.png" \
    "$OUT_DIR/export-ready-window.png" null: 2>&1 || true
)"
pixel_delta_count="${pixel_delta%% *}"
if ! awk -v delta="$pixel_delta_count" 'BEGIN {exit !(delta > 20)}'; then
  echo "Export state copy did not visibly change: pixel delta=$pixel_delta" >&2
  exit 1
fi
printf 'state-pixel-delta=%s\n' "$pixel_delta_count" >"$OUT_DIR/result.txt"

reference_delta="$(
  magick compare -metric AE \
    "$OUT_DIR/reference-more-window.png" \
    "$OUT_DIR/export-missing-tooling-window.png" null: 2>&1 || true
)"
reference_delta_count="${reference_delta%% *}"
if ! awk -v delta="$reference_delta_count" 'BEGIN {exit !(delta > 20)}'; then
  echo "Reference and implementation captures did not differ: pixel delta=$reference_delta" >&2
  exit 1
fi
printf 'reference-pixel-delta=%s\n' "$reference_delta_count" >>"$OUT_DIR/result.txt"

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  echo "Clip-export smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

echo "Linux clip-export smoke passed. Captures: $OUT_DIR"
