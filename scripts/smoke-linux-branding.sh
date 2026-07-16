#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-branding-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export OKP_SKIP_UPDATE_CHECK=1
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

capture() {
  local theme="$1"
  local label="$2"
  local state_dir="$OUT_DIR/state-$label"
  local config_dir="$OUT_DIR/config-$label"
  mkdir -p "$state_dir/ok-player" "$config_dir/ok-player"
  printf '%s\n' '{"version":2,"updates":{"auto_check":false}}' \
    > "$config_dir/ok-player/settings.json"

  XDG_STATE_HOME="$state_dir" \
  XDG_CONFIG_HOME="$config_dir" \
  OKP_GTK_THEME_PREVIEW="$theme" \
  OKP_WELCOME_STATE=empty \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
    "$BINARY" >"$OUT_DIR/app-$label.log" 2>&1 &
  app_pid=$!
  sleep 5

  window_id="$(xdotool search --name 'OK Player' | head -n1 || true)"
  if [[ -z "$window_id" ]]; then
    echo "$label: main window did not appear" >&2
    exit 1
  fi
  import -window "$window_id" "$OUT_DIR/first-run-$label.png"
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  if xdotool search --name 'OK Player' >/dev/null 2>&1; then
    echo "$label: app window remained after its process exited" >&2
    exit 1
  fi
}

capture light light
capture dark dark
capture light light-after-dark
SMOKE
then
  echo "Branding smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

for capture in light:light dark:dark light-after-dark:light; do
  label="${capture%%:*}"
  theme="${capture##*:}"
  shot="$OUT_DIR/first-run-$label.png"
  dimensions="$(magick identify -format '%wx%h' "$shot")"
  if [[ "$dimensions" != "1120x680" ]]; then
    echo "$label: unexpected screenshot geometry $dimensions" >&2
    exit 1
  fi

  tile="$OUT_DIR/tile-$label.png"
  titlebar="$OUT_DIR/titlebar-mark-$label.png"
  magick "$shot" -crop 48x48+536+252 +repage "$tile"
  magick "$shot" -crop 24x16+12+5 +repage "$titlebar"

  tile_green="$(magick "$tile" -format '%[fx:mean.g]' info:)"
  tile_red="$(magick "$tile" -format '%[fx:mean.r]' info:)"
  tile_white="$(magick "$tile" -colorspace Gray -threshold 90% -format '%[fx:mean]' info:)"
  canvas_luma="$(magick "$shot" -crop 80x80+100+100 +repage -colorspace Gray -format '%[fx:mean]' info:)"
  titlebar_green="$(magick "$titlebar" -format '%[fx:mean.g]' info:)"
  titlebar_red="$(magick "$titlebar" -format '%[fx:mean.r]' info:)"
  if ! awk -v r="$tile_red" -v g="$tile_green" 'BEGIN { exit !(g-r > 0.08) }'; then
    echo "$label: canonical 48 px gradient tile is missing" >&2
    exit 1
  fi
  if ! awk -v white="$tile_white" 'BEGIN { exit !(white > 0.04) }'; then
    echo "$label: canonical white full mark is missing: white fraction=$tile_white" >&2
    exit 1
  fi
  if [[ "$theme" == "light" ]]; then
    awk -v luma="$canvas_luma" 'BEGIN { exit !(luma > 0.75) }' || {
      echo "light: GTK light preference did not resolve: canvas luma=$canvas_luma" >&2
      exit 1
    }
  else
    awk -v luma="$canvas_luma" 'BEGIN { exit !(luma < 0.30) }' || {
      echo "dark: GTK dark preference did not resolve: canvas luma=$canvas_luma" >&2
      exit 1
    }
  fi
  if ! awk -v r="$titlebar_red" -v g="$titlebar_green" 'BEGIN { exit !(g-r > 0.015) }'; then
    echo "$label: canonical full titlebar mark is missing" >&2
    exit 1
  fi
done

echo "Linux branding smoke passed: $OUT_DIR/first-run-light.png $OUT_DIR/first-run-dark.png $OUT_DIR/first-run-light-after-dark.png"
