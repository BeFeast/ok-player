#!/usr/bin/env bash
# Run inside a disposable D-Bus session with X11 or headless Sway.
set -euo pipefail

image="$(realpath "${1:?usage: smoke-linux-appimage.sh <AppImage> <video> <evidence-dir>}")"
media="$(realpath "${2:?Expected a video file}")"
evidence="$(realpath -m "${3:?Expected an evidence directory}")"
mkdir -p "$evidence"
work="$(mktemp -d)"
app_pid=
trap 'if [[ -n "$app_pid" ]]; then kill "$app_pid" 2>/dev/null || true; wait "$app_pid" 2>/dev/null || true; fi; rm -rf -- "$work"' EXIT
export TMPDIR="$work"
export XDG_CONFIG_HOME="$work/config" XDG_DATA_HOME="$work/data"
export XDG_CACHE_HOME="$work/cache" XDG_STATE_HOME="$work/state"
export APPIMAGE_EXTRACT_AND_RUN=1 GDK_BACKEND="${GDK_BACKEND:-x11}" LIBGL_ALWAYS_SOFTWARE=1
export OKP_SKIP_UPDATE_CHECK=1 OKP_SKIP_OPEN_INSTALLER=1 OKP_VIDEO_BACKEND=gtk
unset OKP_DISABLE_MPRIS
"$image" "$media" >"$evidence/app.log" 2>&1 &
app_pid=$!

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
if [[ "$GDK_BACKEND" == wayland ]]; then
  geometry="$(swaymsg -t get_tree -r | jq -er '
    first(.. | objects | select(.app_id? == "com.befeast.okplayer") | .rect)
    | "\(.x),\(.y) \(.width)x\(.height)"')"
  grim -g "$geometry" "$evidence/playback.png"
else
  window="$(xdotool search --onlyvisible --name 'OK Player' | tail -n 1)"
  import -window "$window" "$evidence/playback.png"
fi
