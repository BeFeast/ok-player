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
"$ROOT/scripts/smoke-linux-main-window.sh" "$BINARY" "$OUT_DIR/main-window"
"$ROOT/scripts/smoke-linux-empty-states.sh" "$BINARY" "$OUT_DIR/empty-states"
"$ROOT/scripts/smoke-linux-side-panel.sh" "$BINARY" "$OUT_DIR/chapters"
"$ROOT/scripts/smoke-linux-settings.sh" "$BINARY" "$OUT_DIR/settings"
"$ROOT/scripts/smoke-linux-narrow-width.sh" "$BINARY" "$OUT_DIR/narrow"
"$ROOT/scripts/smoke-linux-playback-acceptance.sh" "$BINARY" "$OUT_DIR/fixtures" "$OUT_DIR/playback"

cp "$OUT_DIR/main-window/window.png" "$OUT_DIR/captures/first-run.png"
cp "$OUT_DIR/empty-states/continue-watching.png" "$OUT_DIR/captures/continue-watching.png"
cp "$OUT_DIR/empty-states/history.png" "$OUT_DIR/captures/history.png"
cp "$OUT_DIR/playback/loaded-paused-osc.png" "$OUT_DIR/captures/loaded-paused-osc.png"
cp "$OUT_DIR/playback/playing-idle.png" "$OUT_DIR/captures/playing-idle.png"
cp "$OUT_DIR/playback/chapters-loaded.png" "$OUT_DIR/captures/chapters.png"
cp "$OUT_DIR/empty-states/up-next-empty.png" "$OUT_DIR/captures/up-next.png"
cp "$OUT_DIR/settings/settings.png" "$OUT_DIR/captures/settings-about.png"
cp "$OUT_DIR/narrow/narrow.png" "$OUT_DIR/captures/narrow-layout.png"
cp "$OUT_DIR/playback/bright-video-background.png" "$OUT_DIR/captures/bright-video-background.png"
cp "$OUT_DIR/playback/loaded-paused-osc.png" "$OUT_DIR/captures/dark-video-background.png"

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
    elif .state == "dark-video-background" then
      .measurements += [{name:"generated-media-screenshot-file",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
    elif .state == "playing-idle" then
      .measurements += [{name:"generated-media-fullscreen-transition",expected:1,actual:1,tolerance:0,unit:"boolean",status:"pass"}]
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
