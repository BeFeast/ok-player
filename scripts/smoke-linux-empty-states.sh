#!/usr/bin/env bash
# Visual smoke guard for the PRD §14 state-matrix surfaces: the no-chapters and
# short-queue side-panel states, plus the Continue Watching and private-session
# welcome states from issue #191. The welcome fixtures use an isolated history
# document and capture both default and narrow geometry so cards, labels, and
# placeholders cannot silently overlap or leak through private mode.
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

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_STATE_HOME="$OUT_DIR/state"

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

mkdir -p "$XDG_STATE_HOME/ok-player"
now="$(date +%s)"
cat >"$XDG_STATE_HOME/ok-player/history.json" <<JSON
{
  "version": 2,
  "files": {
    "/media/films/Arrival.mkv": {
      "position": 1320.0,
      "duration": 6960.0,
      "finished": false,
      "updated_at_unix": $((now - 1800)),
      "title": "Arrival"
    },
    "/media/shows/Severance/Season 02/Episode 04.mkv": {
      "position": 1180.0,
      "duration": 3180.0,
      "finished": false,
      "updated_at_unix": $((now - 86000)),
      "title": "Woe's Hollow"
    },
    "/media/documentaries/Free Solo.mp4": {
      "position": 880.0,
      "duration": 6000.0,
      "finished": false,
      "updated_at_unix": $((now - 260000))
    },
    "/media/finished/Old Film.mkv": {
      "position": 0.0,
      "duration": 5400.0,
      "finished": true,
      "updated_at_unix": $((now - 600))
    }
  }
}
JSON

capture_welcome() {
  local mode="$1" shot="$2" width="$3" height="$4" label="$5"
  rm -f "$OUT_DIR/app.log"
  if [[ "$mode" == "private" ]]; then
    OKP_PRIVATE_SESSION_ON_STARTUP=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  else
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  fi
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

  if [[ "$width" != "1120" || "$height" != "680" ]]; then
    xdotool windowsize "$window_id" "$width" "$height"
    sleep 1
  fi
  import -window "$window_id" "$shot"

  actual_width="$(xwininfo -id "$window_id" | awk '/Width:/ { print $2; exit }')"
  actual_height="$(xwininfo -id "$window_id" | awk '/Height:/ { print $2; exit }')"
  if (( actual_width > width + 8 || actual_width < width - 8 || actual_height > height + 8 || actual_height < height - 8 )); then
    echo "$label: unexpected geometry ${actual_width}x${actual_height}, expected ${width}x${height}" >&2
    exit 1
  fi

  # Welcome copy, placeholders, and controls all carry bright pixels. A blank or
  # clipped canvas stays near black across the center band.
  center_max="$(magick "$shot" -crop "$((actual_width * 3 / 4))x$((actual_height * 3 / 4))+$((actual_width / 8))+$((actual_height / 8))" -colorspace gray -format '%[fx:maxima]' info:)"
  if ! awk -v max="$center_max" 'BEGIN { exit !(max > 0.55) }'; then
    echo "$label: welcome content looks blank or clipped: maxima=${center_max}" >&2
    exit 1
  fi

  echo "$label: geometry=${actual_width}x${actual_height} maxima=${center_max}"
  kill_app
  trap - EXIT
  wait "$app_pid" 2>/dev/null || true
}

capture_welcome "recents" "$OUT_DIR/continue-watching.png" 1120 680 "Continue Watching"
capture_welcome "recents" "$OUT_DIR/continue-watching-narrow.png" 480 540 "Continue Watching narrow"
capture_welcome "private" "$OUT_DIR/private-session.png" 1120 680 "Private session"

rm -f "$OUT_DIR/app.log"
OKP_OPEN_HISTORY_ON_STARTUP=1 \
OKP_SKIP_OPEN_INSTALLER=1 \
OKP_SKIP_DEB_SELF_INSTALL=1 \
timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
app_pid=$!
trap kill_app EXIT
sleep 5
xdotool search --name "History" >"$OUT_DIR/history-window.ids" || true
history_window_id="$(tail -n1 "$OUT_DIR/history-window.ids" || true)"
if [[ -z "$history_window_id" ]]; then
  echo "History: window did not appear" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi
import -window "$history_window_id" "$OUT_DIR/history.png"
history_max="$(magick "$OUT_DIR/history.png" -colorspace gray -format '%[fx:maxima]' info:)"
if ! awk -v max="$history_max" 'BEGIN { exit !(max > 0.65) }'; then
  echo "History: surface looks blank: maxima=${history_max}" >&2
  exit 1
fi
echo "History: maxima=${history_max}"
kill_app
trap - EXIT
wait "$app_pid" 2>/dev/null || true

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

echo "Empty-states smoke passed. Screenshots: $OUT_DIR/up-next-empty.png $OUT_DIR/chapters-empty.png $OUT_DIR/continue-watching.png $OUT_DIR/continue-watching-narrow.png $OUT_DIR/private-session.png $OUT_DIR/history.png"
