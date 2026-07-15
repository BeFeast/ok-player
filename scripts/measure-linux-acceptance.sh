#!/usr/bin/env bash
# Convert one acceptance screenshot into a machine-readable evidence row.
set -euo pipefail

STATE="${1:?state is required}"
IMAGE="${2:?image is required}"
OUT="${3:?output json is required}"

for tool in identify magick jq awk; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Missing required tool: $tool" >&2
    exit 127
  fi
done
[[ -f "$IMAGE" ]] || { echo "Missing image: $IMAGE" >&2; exit 1; }

width="$(identify -format '%w' "$IMAGE")"
height="$(identify -format '%h' "$IMAGE")"
viewport_width=1120
viewport_height=680
theme=dark
reference=windows-player-redlines
expected_width=1120
expected_height=680

case "$STATE" in
  narrow-layout)
    viewport_width=480
    viewport_height=540
    expected_width=480
    expected_height=540
    reference=compact-modes-handoff
    ;;
  settings-about)
    theme=light
    reference=about-handoff
    expected_width=760
    expected_height=560
    ;;
  history)
    theme=light
    reference=history-handoff
    ;;
  continue-watching)
    theme=light
    reference=history-handoff
    ;;
esac

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

add_measurement() {
  local name="$1" expected="$2" actual="$3" tolerance="$4" unit="$5" status="$6"
  jq -cn \
    --arg name "$name" --argjson expected "$expected" --argjson actual "$actual" \
    --argjson tolerance "$tolerance" --arg unit "$unit" --arg status "$status" \
    '{name:$name,expected:$expected,actual:$actual,tolerance:$tolerance,unit:$unit,status:$status}' >>"$tmp"
}

within() {
  awk -v actual="$1" -v expected="$2" -v tolerance="$3" \
    'BEGIN { exit !((actual - expected <= tolerance) && (expected - actual <= tolerance)) }'
}

status_for() {
  if within "$1" "$2" "$3"; then printf pass; else printf fail; fi
}

add_measurement window-width "$expected_width" "$width" 0 px "$(status_for "$width" "$expected_width" 0)"
add_measurement window-height "$expected_height" "$height" 0 px "$(status_for "$height" "$expected_height" 0)"

if [[ "$width" == "1120" && "$height" == "680" ]]; then
  case "$STATE" in
    first-run)
      # Canonical first-run is a full-canvas surface, not alpha.112's floating card.
      inside="$(magick "$IMAGE" -crop 48x240+306+170 -colorspace gray -format '%[fx:mean]' info:)"
      outside="$(magick "$IMAGE" -crop 48x240+250+170 -colorspace gray -format '%[fx:mean]' info:)"
      delta="$(awk -v a="$inside" -v b="$outside" 'BEGIN { d=a-b; if (d<0) d=-d; print d }')"
      ok=0; within "$delta" 0 0.04 && ok=1
      add_measurement canvas-card-edge-contrast 0 "$delta" 0.04 normalized "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    continue-watching)
      bright="$(magick "$IMAGE" -crop 620x110+80+55 -colorspace gray -threshold 58% -format '%[fx:mean*w*h]' info:)"
      bright="${bright%.*}"
      ok=0; (( bright >= 900 )) && ok=1
      add_measurement left-heading-region-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    loaded-paused-osc|paused|buffered-timeline|chapter-context|bright-video-background|dark-video-background)
      bottom_max="$(magick "$IMAGE" -crop 1088x80+16+582 -colorspace gray -format '%[fx:maxima]' info:)"
      ok=0; awk -v value="$bottom_max" 'BEGIN { exit !(value > 0.45) }' && ok=1
      add_measurement osc-visible-in-canonical-bottom-band 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      if [[ "$STATE" == "bright-video-background" ]]; then
        frame_mean="$(magick "$IMAGE" -crop 700x360+120+100 -colorspace gray -format '%[fx:mean]' info:)"
        ok=0; awk -v value="$frame_mean" 'BEGIN { exit !(value > 0.75) }' && ok=1
        add_measurement bright-frame-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      elif [[ "$STATE" == "dark-video-background" ]]; then
        frame_mean="$(magick "$IMAGE" -crop 700x360+120+100 -colorspace gray -format '%[fx:mean]' info:)"
        ok=0; awk -v value="$frame_mean" 'BEGIN { exit !(value < 0.12) }' && ok=1
        add_measurement dark-frame-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      elif [[ "$STATE" == "paused" ]]; then
        cue_max="$(magick "$IMAGE" -crop 140x50+490+300 -colorspace gray -format '%[fx:maxima]' info:)"
        ok=0; awk -v value="$cue_max" 'BEGIN { exit !(value > 0.45) }' && ok=1
        add_measurement paused-cue-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      elif [[ "$STATE" == "buffered-timeline" ]]; then
        rail_deviation="$(magick "$IMAGE" -crop 300x12+245+632 -colorspace gray -format '%[fx:standard_deviation]' info:)"
        ok=0; awk -v value="$rail_deviation" 'BEGIN { exit !(value > 0.02) }' && ok=1
        add_measurement buffered-band-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      elif [[ "$STATE" == "chapter-context" ]]; then
        title_max="$(magick "$IMAGE" -crop 460x42+0+0 -colorspace gray -format '%[fx:maxima]' info:)"
        ok=0; awk -v value="$title_max" 'BEGIN { exit !(value > 0.55) }' && ok=1
        add_measurement chapter-context-title-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      fi
      ;;
    buffering-loading)
      center_max="$(magick "$IMAGE" -crop 180x100+470+270 -colorspace gray -format '%[fx:maxima]' info:)"
      ok=0; awk -v value="$center_max" 'BEGIN { exit !(value > 0.45) }' && ok=1
      add_measurement loading-ring-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      timeline_deviation="$(magick "$IMAGE" -crop 300x12+245+632 -colorspace gray -format '%[fx:standard_deviation]' info:)"
      ok=0; awk -v value="$timeline_deviation" 'BEGIN { exit !(value > 0.02) }' && ok=1
      add_measurement loading-timeline-shimmer-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    playback-error)
      card_mean="$(magick "$IMAGE" -crop 360x180+380+240 -colorspace gray -format '%[fx:mean]' info:)"
      ok=0; awk -v value="$card_mean" 'BEGIN { exit !(value > 0.035 && value < 0.55) }' && ok=1
      add_measurement in-canvas-error-card-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    osd)
      toast_max="$(magick "$IMAGE" -crop 300x60+410+52 -colorspace gray -format '%[fx:maxima]' info:)"
      ok=0; awk -v value="$toast_max" 'BEGIN { exit !(value > 0.50) }' && ok=1
      add_measurement top-osd-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    playing-idle)
      bottom_max="$(magick "$IMAGE" -crop 1120x96+0+584 -colorspace gray -format '%[fx:maxima]' info:)"
      ok=0; awk -v value="$bottom_max" 'BEGIN { exit !(value < 0.18) }' && ok=1
      add_measurement idle-bottom-band-clear 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
    chapters|up-next)
      # Canonical floating panel is 316px wide with a 24px right inset. The 28px
      # strip immediately to its left must remain video, catching alpha.112's 344px panel.
      strip_mean="$(magick "$IMAGE" -crop 28x500+752+24 -colorspace gray -format '%[fx:mean]' info:)"
      ok=0; awk -v value="$strip_mean" 'BEGIN { exit !(value < 0.035) }' && ok=1
      add_measurement panel-left-clear-strip 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      panel_max="$(magick "$IMAGE" -crop 316x500+780+24 -colorspace gray -format '%[fx:maxima]' info:)"
      ok=0; awk -v value="$panel_max" 'BEGIN { exit !(value > 0.45) }' && ok=1
      add_measurement panel-content-visible 1 "$ok" 0 boolean "$([[ "$ok" == 1 ]] && echo pass || echo fail)"
      ;;
  esac
fi

measurements="$(jq -s '.' "$tmp")"
if jq -e 'all(.[]; .status == "pass")' <<<"$measurements" >/dev/null; then
  result=pass
else
  result=fail
fi

jq -n \
  --arg id "xvfb-$STATE" --arg state "$STATE" --arg theme "$theme" \
  --arg reference "$reference" --arg result "$result" \
  --argjson viewport_width "$viewport_width" --argjson viewport_height "$viewport_height" \
  --argjson measurements "$measurements" \
  '{id:$id,level:"xvfb-render",viewport:{width:$viewport_width,height:$viewport_height},theme:$theme,state:$state,reference:$reference,measurement_result:$result,operator_status:"not-run",measurements:$measurements,notes:"Deterministic Xvfb render only; no live desktop behavior is attested."}' >"$OUT"

[[ "$result" == pass ]]
