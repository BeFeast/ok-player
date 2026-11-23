#!/usr/bin/env bash
# Launch the packaged AppImage in its own compositor and D-Bus session.
set -euo pipefail

image="$(realpath "${1:?usage: smoke-linux-appimage.sh <AppImage> <video> <evidence-dir>}")"
media="$(realpath "${2:?Expected a video file}")"
evidence="$(realpath -m "${3:?Expected an evidence directory}")"
mkdir -p "$evidence"
work="$(mktemp -d)"
sway_pid=
cleanup() {
  if [[ -n "$sway_pid" ]]; then
    kill "$sway_pid" 2>/dev/null || true
    wait "$sway_pid" 2>/dev/null || true
  fi
  if command -v fusermount3 >/dev/null; then
    for mount in "$work/runtime/doc" "$work/runtime/gvfs"; do
      fusermount3 -uz "$mount" 2>/dev/null || true
    done
  fi
  rm -rf -- "$work"
}
trap cleanup EXIT
export XDG_RUNTIME_DIR="$work/runtime" WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_RENDERER=pixman
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export XDG_CACHE_HOME="$work/cache" XDG_STATE_HOME="$work/state"
export TMPDIR="$work" APPIMAGE_EXTRACT_AND_RUN=1 GDK_BACKEND=wayland LIBGL_ALWAYS_SOFTWARE=1
export GSK_RENDERER=gl OKP_PRESENT_LOG="$work/presentation.jsonl" GTK_USE_PORTAL=0 GIO_USE_VFS=local
export OKP_SKIP_UPDATE_CHECK=1 OKP_SKIP_OPEN_INSTALLER=1 OKP_VIDEO_BACKEND=gtk
unset WAYLAND_DISPLAY DISPLAY SWAYSOCK HYPRLAND_INSTANCE_SIGNATURE OKP_DISABLE_MPRIS
install -d -m700 "$XDG_RUNTIME_DIR"
printf 'xwayland disable\noutput HEADLESS-1 mode 1280x800\n' >"$work/sway.conf"
sway -c "$work/sway.conf" >"$evidence/sway.log" 2>&1 &
sway_pid=$!
for (( attempt=1; attempt<=40; attempt++ )); do
  wayland=("$XDG_RUNTIME_DIR"/wayland-[0-9]*)
  sway_socket=("$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
  if [[ -S "${wayland[0]}" && -S "${sway_socket[0]}" ]]; then break; fi
  sleep 0.2
done
[[ -S "${wayland[0]}" && -S "${sway_socket[0]}" ]]
export WAYLAND_DISPLAY="${wayland[0]##*/}" SWAYSOCK="${sway_socket[0]}"

dbus-run-session -- bash -s -- "$image" "$media" "$evidence" <<'SMOKE'
set -euo pipefail
image=$1 media=$2 evidence=$3
"$image" "$media" >"$evidence/app.log" 2>&1 &
app_pid=$!
trap 'swaymsg -q "[app_id=com.befeast.okplayer] kill" 2>/dev/null || true; kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true' EXIT

property() {
  gdbus call --session --timeout 2 --dest org.mpris.MediaPlayer2.okplayer \
    --object-path /org/mpris/MediaPlayer2 --method org.freedesktop.DBus.Properties.Get \
    org.mpris.MediaPlayer2.Player "$1"
}

position() {
  property Position | sed -n 's/^(<int64 \([0-9][0-9]*\)>,)$/\1/p'
}

first=
for (( attempt=1; attempt<=60; attempt++ )); do
  kill -0 "$app_pid"
  first="$(position 2>/dev/null || true)"
  if [[ -n "$first" && "$first" -gt 0 ]]; then break; fi
  sleep 0.5
done
[[ -n "$first" && "$first" -gt 0 ]]
[[ "$(property PlaybackStatus)" == "(<'Playing'>,)" ]]
sleep 2
second="$(position)"
printf 'position_us_start=%s\nposition_us_end=%s\n' "$first" "$second" | tee "$evidence/playback.txt"
(( second - first >= 500000 ))
cp "$OKP_PRESENT_LOG" "$evidence/presentation.jsonl"
python3 - "$evidence/presentation.jsonl" <<'PY'
import json
from pathlib import Path
import sys

lines = Path(sys.argv[1]).read_bytes().splitlines(keepends=True)
presented = [json.loads(line) for line in lines if line.endswith(b"\n")]
frames = {event["sequence"] for event in presented
          if event.get("boundary") == "gtk-glarea-render" and event["width"] > 0 and event["height"] > 0}
if len(frames) < 3:
    raise SystemExit(f"Expected video frames on the GTK surface, observed {len(frames)}")
print(f"Observed {len(frames)} rendered video frames")
PY
geometry="$(swaymsg -t get_tree -r | jq -er '
  first(.. | objects | select(.app_id? == "com.befeast.okplayer") | .rect)
  | "\(.x),\(.y) \(.width)x\(.height)"')"
grim -g "$geometry" "$evidence/playback.png"
SMOKE
