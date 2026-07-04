#!/usr/bin/env bash
set -euo pipefail

# Visual smoke for the welcome surface's "Continue watching" shelf and its private-session state.
# Renders the empty player shell with OKP_WELCOME_RECENTS_PREVIEW so the recents-forward layout
# draws a deterministic fixture set (no seeded history needed), captures a screenshot, and asserts:
#   - the shelf is present and teal-tinted (poster placeholders + progress fills), not blank;
#   - a private session shows the private note instead and leaks no recents (the shelf is absent).

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-continue-watching-smoke}"

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

app_pid=""
cleanup() {
  [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
  kill "$wm_pid" 2>/dev/null || true
}
trap cleanup EXIT

# Capture one welcome variant to "$OUT_DIR/$1.png" driven by OKP_WELCOME_RECENTS_PREVIEW="$2".
capture() {
  local name="$1"
  local preview="$2"
  sleep 1
  OKP_WELCOME_RECENTS_PREVIEW="$preview" \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 12s "$BINARY" >"$OUT_DIR/$name.app.log" 2>&1 &
  app_pid=$!

  sleep 4
  xdotool search --name "OK Player" >"$OUT_DIR/$name.window.ids"
  local window_id
  window_id="$(head -n1 "$OUT_DIR/$name.window.ids")"
  xwininfo -id "$window_id" >"$OUT_DIR/$name.xwininfo"
  import -window "$window_id" "$OUT_DIR/$name.png"

  local width height state
  width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/$name.xwininfo")"
  height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/$name.xwininfo")"
  state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/$name.xwininfo")"
  if [[ "$width" != "1120" || "$height" != "680" || "$state" != "IsViewable" ]]; then
    echo "Unexpected welcome window geometry ($name): ${width}x${height}, state=${state}" >&2
    exit 1
  fi

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

# The card row occupies this band once the recents-forward layout anchors to the top.
CARD_BAND="690x90+215+268"

capture recents on

# The shelf's placeholder posters and progress fills are teal-tinted, so the card band reads
# clearly green-dominant — a near-neutral band means the shelf failed to draw.
recents_gr="$(magick "$OUT_DIR/recents.png" -crop "$CARD_BAND" -format '%[fx:mean.g-mean.r]' info:)"
if ! awk -v gr="$recents_gr" 'BEGIN { exit !(gr > 0.06) }'; then
  echo "Continue-watching shelf missing or not teal: green-red=${recents_gr}" >&2
  exit 1
fi
# Badges, play glyphs and progress fills are bright, so the band must not be a flat dark block.
recents_max="$(magick "$OUT_DIR/recents.png" -crop "$CARD_BAND" -colorspace gray -format '%[fx:maxima]' info:)"
if ! awk -v max="$recents_max" 'BEGIN { exit !(max > 0.6) }'; then
  echo "Continue-watching shelf looks blank: card-band maxima=${recents_max}" >&2
  exit 1
fi

capture private private

# A private session must not leak recents: the card band stays neutral (no shelf), and the private
# note carries visible text below the identity.
private_gr="$(magick "$OUT_DIR/private.png" -crop "$CARD_BAND" -format '%[fx:mean.g-mean.r]' info:)"
if ! awk -v gr="$private_gr" 'BEGIN { exit !(gr < 0.03) }'; then
  echo "Private session appears to leak the recents shelf: green-red=${private_gr}" >&2
  exit 1
fi
private_note_max="$(magick "$OUT_DIR/private.png" -crop 460x26+330+355 -colorspace gray -format '%[fx:maxima]' info:)"
if ! awk -v max="$private_note_max" 'BEGIN { exit !(max > 0.5) }'; then
  echo "Private-session note missing: note-band maxima=${private_note_max}" >&2
  exit 1
fi
SMOKE
then
  echo "Continue-watching smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Continue-watching smoke passed. Screenshots: $OUT_DIR/recents.png, $OUT_DIR/private.png"
