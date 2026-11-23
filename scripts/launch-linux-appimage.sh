#!/bin/sh
set -eu

root="${XDG_DATA_HOME:-$HOME/.local/share}/ok-player"
if [ "${1:-}" = '--update' ]; then
  shift
  exec python3 "$root/linux-appimage.py" update "$@"
fi

if [ ! -c /dev/fuse ]; then
  export APPIMAGE_EXTRACT_AND_RUN=1
fi
export OKP_MANAGED_APPIMAGE=1 OKP_SKIP_UPDATE_CHECK=1
exec flock --shared "$root/.appimage.lock" "$root/OK-Player.AppImage" "$@"
