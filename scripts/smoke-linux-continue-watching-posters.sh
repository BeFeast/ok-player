#!/usr/bin/env bash
# Deterministic visual smoke for Continue Watching / History posters (#360).
#
# It exercises the *production* poster path end to end: three real, distinctly-coloured videos
# are decoded by the app's ffmpeg poster worker, cached under XDG, and rendered on both the
# welcome "Continue watching" shelf and the full History surface. The assertions prove each
# card carries real, distinct image content — a renamed placeholder (a flat gradient with a
# grey film glyph) cannot satisfy the per-card dominant-colour and saturation checks.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-continue-watching-posters-smoke}"
IDLE_OSC_ASSERT="$ROOT/scripts/assert-linux-idle-osc-absent.sh"

for tool in xvfb-run dbus-run-session ffmpeg xfwm4 xdotool xwininfo import magick; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# Three bright, single-colour sources. Each mean luma is well above the usable floor (22) and
# clears the lit threshold (48) on the first sample, and each has a different dominant channel
# so the three cards must render three visibly different frames.
media_dir="$OUT_DIR/media"
mkdir -p "$media_dir"
generate_color_video() {
  local color="$1" output="$2"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "color=c=${color}:s=1280x720:r=24:d=40" \
    -map 0:v:0 -c:v libx264 -preset ultrafast -tune stillimage -crf 30 \
    -pix_fmt yuv420p -g 24 -an \
    "$output"
}
generate_color_video "0xc03030" "$media_dir/Crimson.mkv"     # red-dominant
generate_color_video "0x2fa02f" "$media_dir/Forest.mkv"      # green-dominant
generate_color_video "0x4060c8" "$media_dir/Harbor.mkv"      # blue-dominant

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi
export LIBGL_ALWAYS_SOFTWARE=1

xvfb_args=(-a)
if [[ -n "${OKP_XVFB_SERVER_NUM:-}" ]]; then
  xvfb_args=(-n "$OKP_XVFB_SERVER_NUM")
fi

if ! xvfb-run "${xvfb_args[@]}" --server-args='-screen 0 1280x900x24 -nolisten tcp -extension GLX' \
  dbus-run-session -- bash -s -- "$BINARY" "$OUT_DIR" "$media_dir" "$IDLE_OSC_ASSERT" >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

BINARY="$1"
OUT_DIR="$2"
MEDIA_DIR="$3"
IDLE_OSC_ASSERT="$4"
export GDK_BACKEND=x11
export GSK_RENDERER=cairo
export OKP_SKIP_UPDATE_CHECK=1
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export XDG_STATE_HOME="$OUT_DIR/state"
export XDG_CONFIG_HOME="$OUT_DIR/config"
export XDG_CACHE_HOME="$OUT_DIR/cache"

xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
wm_pid=$!
app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT
sleep 1

mkdir -p "$XDG_STATE_HOME/ok-player" "$XDG_CONFIG_HOME/ok-player" "$XDG_CACHE_HOME"
cat >"$XDG_CONFIG_HOME/ok-player/settings.json" <<'JSON'
{"version":2,"updates":{"auto_check":false}}
JSON

now="$(date +%s)"
# Newest-first ordering fixes which colour lands in which card: Crimson, Forest, Harbor.
cat >"$XDG_STATE_HOME/ok-player/history.json" <<JSON
{
  "version": 2,
  "files": {
    "$MEDIA_DIR/Crimson.mkv": {"position":20,"duration":40,"finished":false,"updated_at_unix":$((now-100)),"title":"Crimson"},
    "$MEDIA_DIR/Forest.mkv": {"position":20,"duration":40,"finished":false,"updated_at_unix":$((now-200)),"title":"Forest"},
    "$MEDIA_DIR/Harbor.mkv": {"position":20,"duration":40,"finished":false,"updated_at_unix":$((now-300)),"title":"Harbor"}
  }
}
JSON

launch() {
  rm -f "$OUT_DIR/app.log"
  env OKP_SKIP_OPEN_INSTALLER=1 OKP_SKIP_DEB_SELF_INSTALL=1 OKP_IDLE_THEME=dark "$@" \
    timeout 40s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
}

window_id() { xdotool search --name "OK Player" | head -n1; }

# Wait for the poster worker to decode and cache all three frames before capturing, so the
# assertions never race generation. Bounded so a stuck worker fails loudly instead of hanging.
wait_for_posters() {
  local deadline=$((SECONDS + 25))
  while (( SECONDS < deadline )); do
    local count
    count="$(find "$XDG_CACHE_HOME/ok-player/continue-watching-posters" -name '*.jpg' 2>/dev/null | wc -l)"
    if (( count >= 3 )); then
      return 0
    fi
    sleep 1
  done
  echo "poster generation did not produce three cached frames in time" >&2
  ls -la "$XDG_CACHE_HOME/ok-player/continue-watching-posters" 2>/dev/null >&2 || true
  return 1
}

# mean r/g/b of a crop; asserts the named channel dominates the other two by a clear margin
# (a grey placeholder has r≈g≈b, so it fails), and echoes the triple for the log.
assert_dominant() {
  local image="$1" crop="$2" channel="$3" label="$4"
  local r g b
  read -r r g b < <(magick "$image" -crop "$crop" +repage \
    -format '%[fx:mean.r] %[fx:mean.g] %[fx:mean.b]\n' info:)
  awk -v r="$r" -v g="$g" -v b="$b" -v ch="$channel" -v label="$label" 'BEGIN {
    margin = 0.10
    if (ch == "r" && (r - g > margin) && (r - b > margin)) { ok = 1 }
    if (ch == "g" && (g - r > margin) && (g - b > margin)) { ok = 1 }
    if (ch == "b" && (b - r > margin) && (b - g > margin)) { ok = 1 }
    printf "%s: r=%.3f g=%.3f b=%.3f dominant=%s\n", label, r, g, b, ch
    if (!ok) { printf "%s: expected %s-dominant poster, got a flat/placeholder frame\n", label, ch > "/dev/stderr"; exit 1 }
  }'
}

# A single app instance drives both surfaces: the welcome shelf, then the full History list
# reached by the in-canvas recents arrow. Both read the same poster cache, so History needs no
# extra wait once the shelf's frames are cached.
launch
sleep 4
wid="$(window_id || true)"
if [[ -z "$wid" ]]; then
  echo "main window did not appear" >&2
  cat "$OUT_DIR/app.log" >&2 || true
  exit 1
fi
wait_for_posters
sleep 2 # let the 200 ms idle poll re-project the freshly cached frames onto the cards
import -window "$wid" "$OUT_DIR/continue-watching.png"
"$IDLE_OSC_ASSERT" "$OUT_DIR/continue-watching.png" "Continue Watching"

# The three cards sit at x≈220, 428, 636 (194px + 14px gap), thumbnails y≈146..256. Sample an
# inner region that avoids the progress bar and the time-left label.
assert_dominant "$OUT_DIR/continue-watching.png" "140x60+235+160" r "card-1 Crimson"
assert_dominant "$OUT_DIR/continue-watching.png" "140x60+443+160" g "card-2 Forest"
assert_dominant "$OUT_DIR/continue-watching.png" "140x60+651+160" b "card-3 Harbor"

# The three cards must differ from each other — a single renamed placeholder shared by all
# would make these identical.
d12="$(magick compare -metric RMSE \
  "(" "$OUT_DIR/continue-watching.png" -crop 140x60+235+160 +repage ")" \
  "(" "$OUT_DIR/continue-watching.png" -crop 140x60+443+160 +repage ")" null: 2>&1 || true)"
d12="$(sed -n 's/.*(\([^)]*\)).*/\1/p' <<<"$d12")"
awk -v d="$d12" 'BEGIN { if (!(d > 0.05)) { print "cards 1 and 2 are identical — placeholder, not real posters" > "/dev/stderr"; exit 1 } }'

# ---- History surface (shares the same cache), opened in-canvas via the recents arrow ----
if xdotool search --name '^History$' >/dev/null 2>&1; then
  echo "History opened a separate window before it was requested" >&2
  exit 1
fi
xdotool mousemove --window "$wid" 868 220 click 1
sleep 2
if xdotool search --name '^History$' >/dev/null 2>&1; then
  echo "recents arrow opened a separate History window instead of the in-canvas surface" >&2
  exit 1
fi
import -window "$wid" "$OUT_DIR/history.png"
"$IDLE_OSC_ASSERT" "$OUT_DIR/history.png" "History"
# The centered History column stacks the three row thumbnails at x≈402 and
# y≈197..341. It must carry saturated colour, not the near-grey placeholder.
history_sat="$(magick "$OUT_DIR/history.png" -crop 64x160+402+190 +repage \
  -colorspace HSL -channel G -separate -format '%[fx:mean]' info:)"
awk -v s="$history_sat" 'BEGIN {
  printf "history thumbnail column saturation: %.3f\n", s
  if (!(s > 0.10)) { print "History rows show placeholders, not cached posters" > "/dev/stderr"; exit 1 }
}'

kill "$app_pid" 2>/dev/null || true
wait "$app_pid" 2>/dev/null || true
app_pid=""

kill "$wm_pid" 2>/dev/null || true
trap - EXIT
SMOKE
then
  cat "$OUT_DIR/session.log" >&2 || true
  exit 1
fi

cat "$OUT_DIR/session.log"
echo "Linux Continue Watching / History poster smoke captured in $OUT_DIR"
