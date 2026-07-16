#!/usr/bin/env bash
# Real-mpv acceptance capture for distinct playback, buffering, failure, panel,
# screenshot, and idle states. No playback-state image is copied or synthesized.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
FIXTURE_DIR="$(realpath -m "${2:-$ROOT/artifacts/linux-acceptance/fixtures}")"
OUT_DIR="$(realpath -m "${3:-$ROOT/artifacts/linux-acceptance/playback}")"
if [[ "$BINARY" == */* ]]; then
  BINARY="$(realpath -m "$BINARY")"
fi

DARK="$FIXTURE_DIR/dark.mkv"
CHAPTERS="$FIXTURE_DIR/dark-with-chapters.mkv"
INTERVALS="$FIXTURE_DIR/dark-no-chapters-long.mkv"
BRIGHT="$FIXTURE_DIR/bright.mkv"
BUFFERED="$FIXTURE_DIR/buffered.mkv"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick ffprobe python3 sha256sum rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
for fixture in "$DARK" "$CHAPTERS" "$INTERVALS" "$BRIGHT" "$BUFFERED"; do
  if [[ ! -f "$fixture" ]]; then
    echo "Missing generated fixture: $fixture" >&2
    exit 127
  fi
done

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp' \
  dbus-run-session -- bash -s -- "$BINARY" "$DARK" "$CHAPTERS" "$INTERVALS" "$BRIGHT" "$BUFFERED" "$OUT_DIR" \
  >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
DARK="$2"
CHAPTERS="$3"
INTERVALS="$4"
BRIGHT="$5"
BUFFERED="$6"
OUT_DIR="$7"
FIXTURE_DIR="$(dirname "$DARK")"

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export LIBGL_ALWAYS_SOFTWARE=1
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_CACHE_HOME="$OUT_DIR/cache"
export HOME="$OUT_DIR/home"
mkdir -p "$HOME/Pictures/OK Player" "$XDG_CONFIG_HOME/ok-player"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":2,"updates":{"auto_check":false}}
JSON

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
server_pid=""

cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

python3 - "$FIXTURE_DIR" "$OUT_DIR/http-port" >"$OUT_DIR/http-server.log" 2>&1 <<'PY' &
import http.server
import os
import pathlib
import re
import socketserver
import sys
import time
import urllib.parse

root = pathlib.Path(sys.argv[1])
port_file = pathlib.Path(sys.argv[2])

class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, format, *args):
        print(format % args, flush=True)

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        if path == "/missing.mkv":
            self.send_error(404, "Acceptance failure fixture")
            return
        if path == "/loading.mkv":
            time.sleep(20)
            self.send_file(root / "dark.mkv", path, 0.0)
            return
        if path == "/buffered.mkv":
            self.send_file(root / "buffered.mkv", path, 0.08)
            return
        self.send_error(404)

    def send_file(self, file_path, request_path, throttle):
        size = file_path.stat().st_size
        start = 0
        end = size - 1
        range_header = self.headers.get("Range")
        if range_header:
            match = re.fullmatch(r"bytes=(\d+)-(\d*)", range_header.strip())
            if not match:
                self.send_error(416)
                return
            start = int(match.group(1))
            if match.group(2):
                end = min(int(match.group(2)), end)
        if start > end or start >= size:
            self.send_error(416)
            return

        length = end - start + 1
        self.send_response(206 if range_header else 200)
        self.send_header("Content-Type", "video/x-matroska")
        self.send_header("Content-Length", str(length))
        self.send_header("Accept-Ranges", "bytes")
        if range_header:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()

        sent = 0
        try:
            with file_path.open("rb") as source:
                source.seek(start)
                remaining = length
                while remaining:
                    chunk = source.read(min(16384, remaining))
                    if not chunk:
                        break
                    self.wfile.write(chunk)
                    self.wfile.flush()
                    sent += len(chunk)
                    remaining -= len(chunk)
                    if throttle:
                        time.sleep(throttle)
        except (BrokenPipeError, ConnectionResetError):
            pass
        finally:
            print(f"served path={request_path} bytes={sent} range={start}-{end}", flush=True)

class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True

with Server(("127.0.0.1", 0), Handler) as server:
    port_file.write_text(str(server.server_address[1]), encoding="ascii")
    server.serve_forever()
PY
server_pid=$!

for _ in $(seq 1 100); do
  [[ -s "$OUT_DIR/http-port" ]] && break
  sleep 0.05
done
[[ -s "$OUT_DIR/http-port" ]] || { echo "acceptance HTTP server did not start" >&2; exit 1; }
port="$(cat "$OUT_DIR/http-port")"
BUFFERED_URL="http://127.0.0.1:${port}/buffered.mkv"
LOADING_URL="http://127.0.0.1:${port}/loading.mkv"
MISSING_URL="http://127.0.0.1:${port}/missing.mkv"

launch() {
  local source="$1" name="$2" settle="$3"
  shift 3
  local state_home="$OUT_DIR/state/$name"
  mkdir -p "$state_home"
  env \
    XDG_STATE_HOME="$state_home" \
    OKP_DISABLE_MPRIS=1 \
    OKP_FIXED_VIEWPORT_SMOKE=1 \
    OKP_SKIP_UPDATE_CHECK=1 \
    OKP_SKIP_OPEN_INSTALLER=1 \
    OKP_SKIP_DEB_SELF_INSTALL=1 \
    timeout 45s "$BINARY" "$source" "$@" >"$OUT_DIR/$name-app.log" 2>&1 &
  app_pid=$!

  window_id=""
  for _ in $(seq 1 120); do
    for candidate in $(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null || true); do
      candidate_width="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Width:/ { print $2; exit }')"
      candidate_height="$(xwininfo -id "$candidate" 2>/dev/null | awk '/Height:/ { print $2; exit }')"
      if [[ "${candidate_width:-0}" -ge 1000 && "${candidate_height:-0}" -ge 600 ]]; then
        window_id="$candidate"
      fi
    done
    [[ -n "$window_id" ]] && break
    sleep 0.1
  done
  if [[ -z "$window_id" ]]; then
    echo "$name: main window did not appear" >&2
    cat "$OUT_DIR/$name-app.log" >&2 || true
    exit 1
  fi
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true
  sleep "$settle"

  xwininfo -id "$window_id" >"$OUT_DIR/$name-window.xwininfo"
  width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/$name-window.xwininfo")"
  height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/$name-window.xwininfo")"
  [[ "$width" == 1120 && "$height" == 680 ]] || {
    echo "$name: unexpected geometry ${width}x${height}" >&2
    exit 1
  }
}

stop_app() {
  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  sleep 0.5
}

capture() {
  local name="$1"
  import -window "$window_id" "$OUT_DIR/$name.png"
  local image_width image_height
  image_width="$(identify -format '%w' "$OUT_DIR/$name.png")"
  image_height="$(identify -format '%h' "$OUT_DIR/$name.png")"
  [[ "$image_width" == 1120 && "$image_height" == 680 ]] || {
    echo "$name: captured ${image_width}x${image_height}, expected 1120x680" >&2
    exit 1
  }
}

capture_sized() {
  local name="$1" expected_width="$2" expected_height="$3"
  import -window "$window_id" "$OUT_DIR/$name.png"
  local image_width image_height
  image_width="$(identify -format '%w' "$OUT_DIR/$name.png")"
  image_height="$(identify -format '%h' "$OUT_DIR/$name.png")"
  [[ "$image_width" == "$expected_width" && "$image_height" == "$expected_height" ]] || {
    echo "$name: captured ${image_width}x${image_height}, expected ${expected_width}x${expected_height}" >&2
    exit 1
  }
}

wake_chrome() {
  xdotool mousemove --window "$window_id" 420 300
  sleep 0.1
  xdotool mousemove --window "$window_id" 560 340
}

capture_dark_active_chrome() {
  local name="$1" bottom_max=""
  for _ in 1 2 3 4; do
    wake_chrome
    sleep 0.5
    capture "$name"
    bottom_max="$(magick "$OUT_DIR/$name.png" -crop 1088x70+16+600 \
      -colorspace gray -format '%[fx:maxima]' info:)"
    if awk -v value="$bottom_max" 'BEGIN { exit !(value > 0.45) }'; then
      return
    fi
  done
  echo "$name: active OSC did not become visible (bottom max=$bottom_max)" >&2
  exit 1
}

# Paused on a separately loaded real dark frame with no chapter metadata.
launch "$DARK" paused 4
xdotool key --clearmodifiers space
wake_chrome
sleep 1
capture paused
stop_app

# The companion launch contract is ordinary process invocation. This row proves the packaged GTK
# shell parsed the explicit target and libmpv accepted the one-shot absolute seek; core unit tests
# separately cover precedence over remembered state, zero, near-end, and private reporting.
launch "$DARK" explicit-launch-resume 4 --resume 12
grep -q 'Applied explicit launch resume at 12.000s' "$OUT_DIR/explicit-launch-resume-app.log" || {
  echo "explicit launch resume was not applied" >&2
  cat "$OUT_DIR/explicit-launch-resume-app.log" >&2 || true
  exit 1
}
stop_app

# A throttled real HTTP source produces an observed demuxer cache ahead of the
# playhead. Pause only after FileLoaded so this is buffered playback, not the
# loading shimmer state.
launch "$BUFFERED_URL" buffered-timeline 7
xdotool key --clearmodifiers space
wake_chrome
sleep 1
capture buffered-timeline
xdotool windowsize --sync "$window_id" 1240 760
wake_chrome
sleep 1
capture_sized buffered-timeline-wide 1240 760
stop_app

# Seek into chapter two, wait for the seek toast to clear, then capture the
# production titlebar context appended from observed chapter metadata.
launch "$CHAPTERS" chapter-context 4
xdotool key --clearmodifiers space
xdotool key --clearmodifiers Right Right
sleep 2
capture chapter-context

# A fresh seek creates the real one-slot OSD readout.
xdotool key --clearmodifiers Right
sleep 0.4
capture osd

# Open the shared Chapters/Up Next panel through its real OSC action.
panel_visible=0
for _ in 1 2 3; do
  wake_chrome
  sleep 0.5
  xdotool mousemove --window "$window_id" 914 638 click 1
  sleep 1
  capture chapters-loaded
  panel_mean="$(magick "$OUT_DIR/chapters-loaded.png" -crop 316x500+780+24 -colorspace gray -format '%[fx:mean]' info:)"
  if awk -v value="$panel_mean" 'BEGIN { exit !(value > 0.04) }'; then
    panel_visible=1
    break
  fi
done
(( panel_visible == 1 )) || { echo "chapters panel action did not reveal panel content" >&2; exit 1; }
xdotool mousemove --window "$window_id" 1097 75 click 1
sleep 1

# Save a frame through the real screenshot action.
before_count="$(find "$HOME/Pictures/OK Player" -maxdepth 1 -type f | wc -l)"
xdotool key --clearmodifiers c
sleep 3
after_count="$(find "$HOME/Pictures/OK Player" -maxdepth 1 -type f | wc -l)"
(( after_count > before_count )) || { echo "screenshot action did not create a file" >&2; exit 1; }

# Resume and wait past the canonical idle timeout.
xdotool key --clearmodifiers space
sleep 4
capture playing-idle
stop_app

# A real local file with known duration but no embedded chapter metadata opens on
# the interval fallback surface. The explicit Detect chapters action remains at
# the initial scroll position and resolves honestly while no engine is wired.
launch "$INTERVALS" interval-chapters 4 OKP_DEBUG_INTERACTIONS=1
xdotool key --clearmodifiers space
panel_visible=0
for _ in 1 2 3; do
  wake_chrome
  sleep 0.5
  xdotool mousemove --window "$window_id" 914 638 click 1
  sleep 1
  capture intervals-loaded
  panel_mean="$(magick "$OUT_DIR/intervals-loaded.png" -crop 300x480+812+52 -colorspace gray -format '%[fx:mean]' info:)"
  if awk -v value="$panel_mean" 'BEGIN { exit !(value > 0.50) }'; then
    panel_visible=1
    break
  fi
done
(( panel_visible == 1 )) || { echo "interval fallback panel did not render for metadata-less media" >&2; exit 1; }
xdotool mousemove --window "$window_id" 950 145 click 1
sleep 1
rg -q '^interaction: chapter-detection=unavailable$' "$OUT_DIR/interval-chapters-app.log" || {
  echo "Detect chapters did not report the honest no-engine state" >&2
  cat "$OUT_DIR/interval-chapters-app.log" >&2 || true
  exit 1
}
capture intervals-unavailable
stop_app

# Loading is a real URL whose response headers are deliberately delayed.
launch "$LOADING_URL" buffering-loading 1
capture buffering-loading
stop_app

# Failure is a real mpv 404. Retry must re-enter the load path without the
# RefCell borrow panic reported on the original PR.
launch "$MISSING_URL" playback-error 3
capture playback-error
xdotool mousemove --window "$window_id" 437 388 click 1
sleep 3
kill -0 "$app_pid" 2>/dev/null || { echo "Retry crashed the player" >&2; exit 1; }
error_count="$(grep -c '"GET /missing.mkv' "$OUT_DIR/http-server.log" || true)"
(( error_count >= 2 )) || {
  echo "Retry did not issue a second real failed load (requests=$error_count)" >&2
  cat "$OUT_DIR/playback-error-app.log" >&2
  exit 1
}
stop_app

# Bright evidence first captures a real playing frame with active chrome, then
# pauses that same loaded source for the loaded/paused OSC state. Presenting the
# moving frame before pausing avoids a cold software-renderer clear frame.
launch "$BRIGHT" bright-video-background 6
wake_chrome
sleep 1
capture bright-video-background
xdotool key --clearmodifiers space
sleep 1
capture loaded-paused-osc
stop_app

# Dark evidence is a separately loaded real frame while playing with active
# chrome. Neither bright nor dark uses the preview substrate hook.
launch "$DARK" dark-video-background 4
capture_dark_active_chrome dark-video-background
stop_app

printf '%s\n' \
  "default=1120x680" \
  "screenshot_files=$after_count" \
  "retry_real_404_count=$error_count" >"$OUT_DIR/functional-results.txt"
SMOKE
then
  echo "Playback acceptance smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

assert_distinct() {
  local baseline="$1" candidate="$2" crop="$3" minimum="$4" label="$5"
  local left="$OUT_DIR/.compare-left.png" right="$OUT_DIR/.compare-right.png" metric normalized
  if [[ "$crop" == full ]]; then
    metric="$(magick compare -metric RMSE "$baseline" "$candidate" null: 2>&1 || true)"
  else
    magick "$baseline" -crop "$crop" +repage "$left"
    magick "$candidate" -crop "$crop" +repage "$right"
    metric="$(magick compare -metric RMSE "$left" "$right" null: 2>&1 || true)"
  fi
  normalized="$(sed -n 's/.*(\([^()]\+\)).*/\1/p' <<<"$metric")"
  if [[ -z "$normalized" ]] || ! awk -v value="$normalized" -v minimum="$minimum" 'BEGIN { exit !(value > minimum) }'; then
    echo "$label: screenshots are not meaningfully distinct (RMSE=$metric)" >&2
    exit 1
  fi
  printf '%s rmse=%s threshold=%s\n' "$label" "$normalized" "$minimum" >>"$OUT_DIR/image-deltas.txt"
}

rail_center_for_segment() {
  local image="$1" x="$2" segment_width="$3" y_start="$4" sample_rows="$5"
  local samples=""
  for offset in $(seq 0 $((sample_rows - 1))); do
    local y=$((y_start + offset)) mean
    mean="$(magick "$image" -crop "${segment_width}x1+${x}+${y}" \
      -colorspace gray -format '%[fx:mean]' info:)"
    samples+="$y $mean"$'\n'
  done
  awk '
    { y[NR]=$1; value[NR]=$2 }
    END {
      best=-1
      best_start=0
      for (i=1; i<=NR-3; i++) {
        sum=value[i]+value[i+1]+value[i+2]+value[i+3]
        if (sum > best) { best=sum; best_start=i }
      }
      if (best_start == 0) exit 1
      printf "%.1f", y[best_start] + 1.5
    }
  ' <<<"$samples"
}

assert_single_rail_alignment() {
  local image="$1" label="$2" played_x="$3" buffered_x="$4" trough_x="$5" y_start="$6"
  local played_center buffered_center trough_center spread
  played_center="$(rail_center_for_segment "$image" "$played_x" 16 "$y_start" 18)"
  buffered_center="$(rail_center_for_segment "$image" "$buffered_x" 14 "$y_start" 18)"
  trough_center="$(rail_center_for_segment "$image" "$trough_x" 36 "$y_start" 18)"
  spread="$(awk -v a="$played_center" -v b="$buffered_center" -v c="$trough_center" '
    BEGIN {
      min=a; max=a
      if (b<min) min=b; if (b>max) max=b
      if (c<min) min=c; if (c>max) max=c
      print max-min
    }
  ')"
  awk -v spread="$spread" 'BEGIN { exit !(spread <= 0.1) }' || {
    echo "$label: timeline layers do not share one rail (played=$played_center buffered=$buffered_center trough=$trough_center)" >&2
    exit 1
  }
  printf '%s played-center=%s buffered-center=%s trough-center=%s spread=%s\n' \
    "$label" "$played_center" "$buffered_center" "$trough_center" "$spread" \
    >>"$OUT_DIR/timeline-alignment.txt"
}

rm -f "$OUT_DIR/image-deltas.txt"
assert_distinct "$OUT_DIR/loaded-paused-osc.png" "$OUT_DIR/paused.png" full 0.20 "bright-paused-vs-dark-paused"
assert_distinct "$OUT_DIR/paused.png" "$OUT_DIR/buffered-timeline.png" 300x18+245+625 0.02 "paused-vs-real-buffered-rail"
assert_distinct "$OUT_DIR/paused.png" "$OUT_DIR/chapter-context.png" 520x46+0+0 0.005 "paused-vs-chapter-title-context"
assert_distinct "$OUT_DIR/chapter-context.png" "$OUT_DIR/osd.png" 360x80+380+42 0.01 "chapter-context-vs-seek-osd"
assert_distinct "$OUT_DIR/paused.png" "$OUT_DIR/buffering-loading.png" 220x140+450+245 0.01 "paused-vs-real-loading"
assert_distinct "$OUT_DIR/paused.png" "$OUT_DIR/playback-error.png" 420x240+350+210 0.01 "paused-vs-real-load-failure"
assert_distinct "$OUT_DIR/loaded-paused-osc.png" "$OUT_DIR/bright-video-background.png" 1088x100+16+280 0.005 "bright-paused-vs-bright-playing"
assert_distinct "$OUT_DIR/playing-idle.png" "$OUT_DIR/dark-video-background.png" 1088x90+16+582 0.02 "dark-playing-idle-vs-dark-active"

rm -f "$OUT_DIR/timeline-alignment.txt"
assert_single_rail_alignment "$OUT_DIR/buffered-timeline.png" canonical-1120x680 263 302 470 624
assert_single_rail_alignment "$OUT_DIR/buffered-timeline-wide.png" wide-1240x760 263 315 590 704
canonical_alignment="$(sed -n '1s/.*spread=//p' "$OUT_DIR/timeline-alignment.txt")"
wide_alignment="$(sed -n '2s/.*spread=//p' "$OUT_DIR/timeline-alignment.txt")"

state_images=(
  loaded-paused-osc paused buffered-timeline buffered-timeline-wide chapter-context osd chapters-loaded
  intervals-loaded intervals-unavailable
  playing-idle buffering-loading playback-error bright-video-background dark-video-background
)
for state in "${state_images[@]}"; do
  sha256sum "$OUT_DIR/$state.png"
done >"$OUT_DIR/state-hashes.txt"
duplicates="$(awk '{print $1}' "$OUT_DIR/state-hashes.txt" | sort | uniq -d)"
[[ -z "$duplicates" ]] || {
  echo "Acceptance states produced duplicate image hashes: $duplicates" >&2
  exit 1
}

buffered_bytes="$(sed -n 's/.*served path=\/buffered\.mkv bytes=\([0-9][0-9]*\).*/\1/p' "$OUT_DIR/http-server.log" | sort -nr | head -n1)"
if [[ -z "$buffered_bytes" ]] || (( buffered_bytes < 65536 )); then
  echo "throttled buffered source did not serve enough media: ${buffered_bytes:-0} bytes" >&2
  exit 1
fi

bright_mean="$(magick "$OUT_DIR/bright-video-background.png" -crop 700x360+120+100 -colorspace gray -format '%[fx:mean]' info:)"
dark_mean="$(magick "$OUT_DIR/dark-video-background.png" -crop 700x360+120+100 -colorspace gray -format '%[fx:mean]' info:)"
idle_mean="$(magick "$OUT_DIR/playing-idle.png" -crop 700x360+120+100 -colorspace gray -format '%[fx:mean]' info:)"
awk -v value="$bright_mean" 'BEGIN { exit !(value > 0.75) }' || { echo "real bright frame missing: mean=$bright_mean" >&2; exit 1; }
awk -v value="$dark_mean" 'BEGIN { exit !(value > 0.015 && value < 0.12) }' || { echo "real dark frame missing: mean=$dark_mean" >&2; exit 1; }
awk -v value="$idle_mean" 'BEGIN { exit !(value > 0.015 && value < 0.12) }' || { echo "real playing-idle frame missing: mean=$idle_mean" >&2; exit 1; }

duration="$(ffprobe -v error -show_entries format=duration -of default=nw=1:nk=1 "$CHAPTERS")"
cat >"$OUT_DIR/functional-results.json" <<JSON
{
  "schema_version": 1,
  "fixture_duration_seconds": $duration,
  "open_file": "pass",
  "playback_start_and_duration": "pass",
  "real_buffered_http_playback": "pass",
  "canonical_single_rail_center_spread_px": $canonical_alignment,
  "wide_single_rail_center_spread_px": $wide_alignment,
  "real_chapter_seek_context": "pass",
  "real_delayed_loading": "pass",
  "real_404_failure_and_retry": "pass",
  "chapters_panel_action": "pass",
  "metadata_less_interval_fallback": "pass",
  "detect_chapters_unavailable_state": "pass",
  "saved_screenshot": "pass",
  "distinct_state_hashes": "pass",
  "buffered_http_bytes": $buffered_bytes,
  "evidence_level": "xvfb-render",
  "not_proven_here": ["file chooser", "folder chooser", "drag/drop", "clipboard", "desktop portal", "Wayland compositor", "focus behavior"]
}
JSON

echo "Playback acceptance captures written to $OUT_DIR"
