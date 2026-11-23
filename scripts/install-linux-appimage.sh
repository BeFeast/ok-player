#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
channel="${1:?usage: install-linux-appimage.sh <beta|stable>}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
launcher="$HOME/.local/bin/ok-player"
if [[ -e "$launcher" || -L "$launcher" ]]; then
  cmp -s "$launcher" "$root/scripts/launch-linux-appimage.sh" || {
    echo "Move the existing launcher aside before installing: $launcher" >&2
    exit 1
  }
fi

python3 "$root/scripts/linux-appimage.py" update "$channel"
install -Dm755 "$root/scripts/linux-appimage.py" "$data/ok-player/linux-appimage.py"
install -Dm644 "$root/rust/packaging/linux/ok-player-archive-keyring.asc" "$data/ok-player/ok-player-archive-keyring.asc"
install -Dm755 "$root/scripts/launch-linux-appimage.sh" "$launcher"
install -Dm644 "$root/rust/packaging/linux/com.befeast.okplayer.svg" \
  "$data/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
install -Dm644 "$root/rust/packaging/linux/com.befeast.okplayer.desktop" \
  "$data/applications/com.befeast.okplayer.desktop"
if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$data/applications"
fi
xdg-mime default com.befeast.okplayer.desktop x-scheme-handler/ok-player
