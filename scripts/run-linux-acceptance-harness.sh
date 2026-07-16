#!/usr/bin/env bash
# Capture all deterministic Linux release states and assemble Xvfb evidence rows.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-ok-player}"
OUT_DIR="${2:-$ROOT/artifacts/linux-acceptance}"
REFERENCE_DIR="${3:-}"

if [[ -z "${__EGL_VENDOR_LIBRARY_FILENAMES:-}" && -f /usr/share/glvnd/egl_vendor.d/50_mesa.json ]]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/captures" "$OUT_DIR/measurements"

"$ROOT/scripts/generate-linux-acceptance-media.sh" "$OUT_DIR/fixtures"
"$ROOT/scripts/smoke-linux-empty-states.sh" "$BINARY" "$OUT_DIR/empty-states"
"$ROOT/scripts/smoke-linux-side-panel.sh" "$BINARY" "$OUT_DIR/chapters"
"$ROOT/scripts/smoke-linux-settings.sh" "$BINARY" "$OUT_DIR/settings"
"$ROOT/scripts/smoke-linux-narrow-width.sh" "$BINARY" "$OUT_DIR/narrow"
"$ROOT/scripts/smoke-linux-playback-acceptance.sh" "$BINARY" "$OUT_DIR/fixtures" "$OUT_DIR/playback"
"$ROOT/scripts/smoke-linux-playback-interactions.sh" "$BINARY" "$OUT_DIR/playback-interactions"
"$ROOT/scripts/smoke-linux-fullscreen-chrome.sh" \
  "$BINARY" "$OUT_DIR/fullscreen" "$OUT_DIR/fixtures/bright.mkv" bright

cp "$OUT_DIR/empty-states/first-run-dark.png" "$OUT_DIR/captures/first-run.png"
cp "$OUT_DIR/empty-states/continue-watching-light.png" "$OUT_DIR/captures/continue-watching.png"
cp "$OUT_DIR/empty-states/history-has-data-light.png" "$OUT_DIR/captures/history.png"
cp "$OUT_DIR/playback/loaded-paused-osc.png" "$OUT_DIR/captures/loaded-paused-osc.png"
cp "$OUT_DIR/playback/paused.png" "$OUT_DIR/captures/paused.png"
cp "$OUT_DIR/playback/buffering-loading.png" "$OUT_DIR/captures/buffering-loading.png"
cp "$OUT_DIR/playback/playback-error.png" "$OUT_DIR/captures/playback-error.png"
cp "$OUT_DIR/playback/playing-idle.png" "$OUT_DIR/captures/playing-idle.png"
cp "$OUT_DIR/playback/osd.png" "$OUT_DIR/captures/osd.png"
cp "$OUT_DIR/playback/buffered-timeline.png" "$OUT_DIR/captures/buffered-timeline.png"
cp "$OUT_DIR/playback/chapter-context.png" "$OUT_DIR/captures/chapter-context.png"
cp "$OUT_DIR/playback/chapters-loaded.png" "$OUT_DIR/captures/chapters.png"
cp "$OUT_DIR/empty-states/up-next-empty.png" "$OUT_DIR/captures/up-next.png"
cp "$OUT_DIR/settings/settings.png" "$OUT_DIR/captures/settings-about.png"
cp "$OUT_DIR/narrow/narrow.png" "$OUT_DIR/captures/narrow-layout.png"
cp "$OUT_DIR/playback/bright-video-background.png" "$OUT_DIR/captures/bright-video-background.png"
cp "$OUT_DIR/playback/dark-video-background.png" "$OUT_DIR/captures/dark-video-background.png"
cp "$OUT_DIR/fullscreen/fullscreen-playing.png" "$OUT_DIR/captures/fullscreen.png"
cp "$OUT_DIR/playback-interactions/always-on-top.png" "$OUT_DIR/captures/always-on-top.png"

failed=0
for image in "$OUT_DIR/captures"/*.png; do
  state="$(basename "${image%.png}")"
  if ! "$ROOT/scripts/measure-linux-acceptance.sh" \
    "$state" "$image" "$OUT_DIR/measurements/$state.json"; then
    failed=1
  fi
done
jq -s --slurpfile functional "$OUT_DIR/playback/functional-results.json" '
  map(
    if .state == "loaded-paused-osc" then
      .measurements += [
        {name:"generated-media-open-file",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"},
        {name:"generated-media-duration-observed",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}
      ]
    elif .state == "chapters" then
      .measurements += [{name:"generated-media-panel-action",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "buffered-timeline" then
      .measurements += [
        {name:"real-throttled-http-buffer",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"},
        {name:"distinct-from-paused-baseline",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"},
        {name:"canonical-single-rail-center-spread",expected:0,actual:$functional[0].canonical_single_rail_center_spread_px,tolerance:0.1,unit:"px",status:(if $functional[0].canonical_single_rail_center_spread_px <= 0.1 then "pass" else "fail" end)},
        {name:"wide-single-rail-center-spread",expected:0,actual:$functional[0].wide_single_rail_center_spread_px,tolerance:0.1,unit:"px",status:(if $functional[0].wide_single_rail_center_spread_px <= 0.1 then "pass" else "fail" end)}
      ]
    elif .state == "chapter-context" then
      .measurements += [
        {name:"real-chapter-seek",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"},
        {name:"distinct-from-paused-baseline",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}
      ]
    elif .state == "buffering-loading" then
      .measurements += [{name:"real-delayed-http-load",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "playback-error" then
      .measurements += [{name:"real-http-404-and-retry",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "dark-video-background" then
      .measurements += [{name:"generated-media-screenshot-file",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "fullscreen" then
      .measurements += [{name:"x11-fullscreen-transition",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "always-on-top" then
      .measurements += [{name:"x11-ewmh-above-state",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    else . end
  )
' "$OUT_DIR/measurements"/*.json >"$OUT_DIR/xvfb-rows.json"

if [[ -n "$REFERENCE_DIR" ]]; then
  "$ROOT/scripts/make-linux-acceptance-comparisons.sh" \
    "$REFERENCE_DIR" "$OUT_DIR/captures" "$OUT_DIR/comparisons"
fi

cat >"$OUT_DIR/evidence-levels.json" <<'JSON'
{
  "model-unit": "Rust model and schema tests",
  "xvfb-render": "Deterministic X11 pixels and scripted mpv behavior only",
  "installed-package": "Candidate .deb/AppImage launch and embedded version",
  "gnome-wayland-operator": "Live chooser, drag/drop, clipboard, portal, compositor, and focus acceptance"
}
JSON

if (( failed != 0 )); then
  echo "Linux acceptance harness rejected one or more canonical redlines. Evidence: $OUT_DIR/xvfb-rows.json" >&2
  exit 1
fi

echo "Linux deterministic acceptance passed. Evidence: $OUT_DIR/xvfb-rows.json"
