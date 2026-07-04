#!/usr/bin/env bash
# Visual smoke guard for the PRD §14 state-matrix empty surfaces (issue #209):
# the no-chapters stream state (Chapters tab) and the single-URL / no-folder
# short-queue state (Up Next tab). Both used to render a bare dead string with
# no affordance; the panel now pins the now-playing item and exposes an
# "Add files to queue" affordance (Up Next) or a calm no-chapters message
# (Chapters). This script screenshot-tests both preview fixtures so a
# regression that re-introduces a blank/dead panel is caught.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-empty-states-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

if ! xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!

kill_app() {
  kill "$app_pid" 2>/dev/null || true
}

# Let the window manager come up before any GTK window is created — the sibling
# smoke scripts do the same. On a slower runner the app can start before xfwm4
# is ready, which makes the later `xdotool` / capture checks fail even though the
# UI rendered correctly.
sleep 1

# The side panel is anchored to the right (halign End, 344px wide, 24px inset).
# Both empty states render bright text over the near-black video, so a dark
# maximum in that band means the panel failed to draw. The Up Next short-queue
# state additionally carries the OK Player teal accent on the pinned now-playing
# row and the "Add files" affordance, so the green channel should read stronger
# than red there; the no-chapters Chapters state is a calm message row only.
panel_band_args=(300x440+772+64)

capture_state() {
  local env_value="$1" shot="$2" label="$3"
  rm -f "$OUT_DIR/app.log"
  OKP_OPEN_SIDE_PANEL_ON_STARTUP="$env_value" \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
  trap kill_app EXIT

  sleep 5
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids" || true
  window_id="$(head -n1 "$OUT_DIR/window.ids" || true)"

  if [[ -z "$window_id" ]]; then
    echo "$label: main window did not appear" >&2
    cat "$OUT_DIR/app.log" >&2 || true
    exit 1
  fi

  import -window "$window_id" "$shot"

  width="$(xwininfo -id "$window_id" | awk '/Width:/ { print $2; exit }')"
  height="$(xwininfo -id "$window_id" | awk '/Height:/ { print $2; exit }')"
  border="$(xwininfo -id "$window_id" | awk '/Border width:/ { print $3; exit }')"
  state="$(xwininfo -id "$window_id" | awk -F': ' '/Map State:/ { print $2; exit }')"
  if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
    echo "$label: unexpected window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
    exit 1
  fi

  panel_max="$(
    magick "$shot" \
      -crop "${panel_band_args[@]}" \
      -colorspace gray \
      -format '%[fx:maxima]' info:
  )"
  if ! awk -v max="$panel_max" 'BEGIN { exit !(max > 0.45) }'; then
    echo "$label: side panel looks blank: content maxima=${panel_max}" >&2
    exit 1
  fi

  echo "$label: panel maxima=${panel_max}"
  kill_app
  trap - EXIT
  wait "$app_pid" 2>/dev/null || true
}

capture_state "up-next-empty" "$OUT_DIR/up-next-empty.png" "Up Next short-queue"
capture_state "chapters-empty" "$OUT_DIR/chapters-empty.png" "Chapters no-chapters"

# The Up Next short-queue state pins the now-playing item in the OK Player teal
# accent and renders the dashed "Add files to queue" affordance, so across the
# band the green channel should read stronger than the red one — a stock grey
# panel (or the old bare dead string) would not.
up_next_red="$(magick "$OUT_DIR/up-next-empty.png" -crop "${panel_band_args[@]}" -format '%[fx:mean.r]' info:)"
up_next_green="$(magick "$OUT_DIR/up-next-empty.png" -crop "${panel_band_args[@]}" -format '%[fx:mean.g]' info:)"
if ! awk -v r="$up_next_red" -v g="$up_next_green" 'BEGIN { exit !(g - r > 0.01) }'; then
  echo "Up Next short-queue accent missing: red=${up_next_red} green=${up_next_green}" >&2
  exit 1
fi

# The no-chapters state must render its message text (dark pixels on the left of
# the band) — not a blank panel. The calm message row sits left-aligned, so the
# left third of the band should carry meaningfully more dark text than the empty
# right third.
left_dark="$(magick "$OUT_DIR/chapters-empty.png" -crop 100x440+772+64 -colorspace gray -threshold 50% -format '%[fx:(1-mean)*w*h]' info:)"
left_dark="${left_dark%.*}"
if (( left_dark < 40 )); then
  echo "Chapters no-chapters message did not render (left dark pixels: ${left_dark})" >&2
  exit 1
fi

kill "$wm_pid" 2>/dev/null || true
SMOKE
then
  echo "Empty-states smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Empty-states smoke passed. Screenshots: $OUT_DIR/up-next-empty.png $OUT_DIR/chapters-empty.png"