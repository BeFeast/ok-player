#!/usr/bin/env bash
# Canonical Chapters / Up Next visual guard. Captures the established states plus
# the metadata-less interval fallback and its honest Detect chapters outcome at
# 1120x680, and checks the exact Windows-contract geometry before launching GTK.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-side-panel-smoke}"

for tool in xvfb-run dbus-run-session xfwm4 xdotool xwininfo import magick rg; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done

MAIN_RS="$ROOT/rust/crates/okp-linux-gtk/src/main.rs"
CONTROLS_RS="$ROOT/rust/crates/okp-linux-gtk/src/controls.rs"
CSS_RS="$ROOT/rust/crates/okp-linux-gtk/src/css.rs"
rg -q '^const SIDE_PANEL_WIDTH: i32 = 316;$' "$MAIN_RS"
rg -q '^const SIDE_PANEL_TOP_INSET: i32 = 44;$' "$MAIN_RS"
rg -q '^const SIDE_PANEL_BOTTOM_INSET: i32 = 80;$' "$MAIN_RS"
rg -q '^const SIDE_PANEL_TRANSITION_MS: u32 = 250;$' "$MAIN_RS"
rg -q 'RevealerTransitionType::SlideRight' "$CONTROLS_RS"
rg -q 'RevealerTransitionType::Crossfade' "$CONTROLS_RS"
rg -q 'border-radius: 12px 0 0 12px;' "$CSS_RS"

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

kill_app() {
  [[ -n "${app_pid:-}" ]] && kill "$app_pid" 2>/dev/null || true
}
trap 'kill_app; kill "$wm_pid" 2>/dev/null || true' EXIT

sleep 1

# Contract at the 1120x680 acceptance viewport: x=1120-316=804, y=44,
# right edge flush at 1120, bottom=680-80=600, height=556.
panel_x=804
panel_y=44
panel_w=316
panel_h=556

capture_state() {
  local fixture="$1" shot="$2" label="$3" substrate="$4"
  rm -f "$OUT_DIR/app.log" "$OUT_DIR/window.ids"
  OKP_OPEN_SIDE_PANEL_ON_STARTUP="$fixture" \
  OKP_SIDE_PANEL_PREVIEW_SUBSTRATE="$substrate" \
  OKP_DEBUG_INTERACTIONS=1 \
  OKP_SKIP_OPEN_INSTALLER=1 \
  OKP_SKIP_DEB_SELF_INSTALL=1 \
  timeout 12s "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!

  sleep 5
  xdotool search --name "OK Player" >"$OUT_DIR/window.ids" || true
  window_id="$(head -n1 "$OUT_DIR/window.ids" || true)"
  if [[ -z "$window_id" ]]; then
    echo "$label: main window did not appear" >&2
    cat "$OUT_DIR/app.log" >&2 || true
    exit 1
  fi

  xwininfo -id "$window_id" >"$OUT_DIR/${label}.xwininfo"
  width="$(awk '/Width:/ { print $2; exit }' "$OUT_DIR/${label}.xwininfo")"
  height="$(awk '/Height:/ { print $2; exit }' "$OUT_DIR/${label}.xwininfo")"
  border="$(awk '/Border width:/ { print $3; exit }' "$OUT_DIR/${label}.xwininfo")"
  state="$(awk -F': ' '/Map State:/ { print $2; exit }' "$OUT_DIR/${label}.xwininfo")"
  if [[ "$width" != "1120" || "$height" != "680" || "$border" != "0" || "$state" != "IsViewable" ]]; then
    echo "$label: unexpected main window geometry: ${width}x${height}, border=${border}, state=${state}" >&2
    exit 1
  fi

  import -window "$window_id" "$shot"

  # The light Mica-equivalent panel must fill the exact expected band. Sample
  # inside all four edges, leaving one pixel for antialiasing and the hairline.
  panel_mean="$(magick "$shot" -crop "$((panel_w - 16))x$((panel_h - 16))+$((panel_x + 8))+$((panel_y + 8))" -colorspace gray -format '%[fx:mean]' info:)"
  right_mean="$(magick "$shot" -crop "2x$((panel_h - 20))+$((panel_x + panel_w - 2))+$((panel_y + 10))" -colorspace gray -format '%[fx:mean]' info:)"
  if ! awk -v panel="$panel_mean" -v right="$right_mean" 'BEGIN { exit !(panel > 0.55 && right > 0.55) }'; then
    echo "$label: panel did not fill the canonical band: panel=${panel_mean} right=${right_mean}" >&2
    exit 1
  fi

  # The panel ends at y=600, leaving 80px for the OSC. The two-pixel strips just
  # above and below its vertical bounds must remain substrate, not panel material.
  above_mean="$(magick "$shot" -crop "180x2+$((panel_x + 30))+$((panel_y - 3))" -colorspace gray -format '%[fx:mean]' info:)"
  below_mean="$(magick "$shot" -crop "180x2+$((panel_x + 30))+$((panel_y + panel_h + 2))" -colorspace gray -format '%[fx:mean]' info:)"
  left_mean="$(magick "$shot" -crop "2x420+$((panel_x - 4))+$((panel_y + 40))" -colorspace gray -format '%[fx:mean]' info:)"
  if [[ "$substrate" == "bright" ]]; then
    if ! awk -v above="$above_mean" -v below="$below_mean" -v left="$left_mean" 'BEGIN { exit !(above > 0.70 && below > 0.70 && left > 0.70) }'; then
      echo "$label: bright substrate proof failed: above=${above_mean} below=${below_mean} left=${left_mean}" >&2
      exit 1
    fi
  else
    if ! awk -v above="$above_mean" -v below="$below_mean" -v left="$left_mean" 'BEGIN { exit !(above < 0.20 && below < 0.20 && left < 0.20) }'; then
      echo "$label: dark substrate or exact panel bounds failed: above=${above_mean} below=${below_mean} left=${left_mean}" >&2
      exit 1
    fi
  fi

  content_max="$(magick "$shot" -crop "292x500+$((panel_x + 12))+$((panel_y + 12))" -colorspace gray -format '%[fx:maxima]' info:)"
  if ! awk -v max="$content_max" 'BEGIN { exit !(max > 0.75) }'; then
    echo "$label: panel content looks blank: maxima=${content_max}" >&2
    exit 1
  fi

  if [[ "$fixture" == "intervals" ]]; then
    # The Detect chapters row is deliberately kept at the initial scroll position.
    # Activate it and require the no-engine build to report an honest unavailable
    # state instead of starting fake progress or blocking playback.
    xdotool mousemove --window "$window_id" 950 145 click 1
    sleep 1
    if ! rg -q '^interaction: chapter-detection=unavailable$' "$OUT_DIR/app.log"; then
      echo "$label: Detect chapters did not resolve to the honest unavailable state" >&2
      cat "$OUT_DIR/app.log" >&2 || true
      exit 1
    fi
    import -window "$window_id" "$OUT_DIR/intervals-toast.png"
  fi

  echo "$label: panel=${panel_x},${panel_y} ${panel_w}x${panel_h} material=${panel_mean} substrate=${left_mean}"
  kill_app
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
}

capture_state "chapters" "$OUT_DIR/chapters-populated.png" "chapters-populated" "dark"
capture_state "bookmarks" "$OUT_DIR/bookmarks.png" "bookmarks" "dark"
capture_state "up-next" "$OUT_DIR/up-next-populated.png" "up-next-populated" "dark"
capture_state "chapters" "$OUT_DIR/chapters-bright.png" "chapters-bright" "bright"
capture_state "intervals" "$OUT_DIR/intervals.png" "intervals" "dark"
capture_state "intervals-unavailable" "$OUT_DIR/intervals-unavailable.png" "intervals-unavailable" "dark"
SMOKE
then
  echo "Side panel smoke failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

echo "Side panel smoke passed. Screenshots: $OUT_DIR/chapters-populated.png $OUT_DIR/bookmarks.png $OUT_DIR/up-next-populated.png $OUT_DIR/chapters-bright.png $OUT_DIR/intervals.png $OUT_DIR/intervals-toast.png $OUT_DIR/intervals-unavailable.png"
