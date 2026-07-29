#!/usr/bin/env bash
# The narrow-width smoke aims at the overflow entry and its seam using the
# rectangles the bar publishes, not offsets measured from the window edge.
#
# This exists because the offsets were wrong and the failure looked like a
# product defect: after #730 the bar tightens its gaps (16->8) and pill inset
# (14->8) as the window narrows, so `width - 14 - 34` pointed 6px left of the
# real overflow entry and the seam crop landed squarely on the volume glyph.
# The candidate lane then failed 15 captures out of 15 with "no disjoint seam",
# on a bar that was laid out correctly.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."
SMOKE=scripts/smoke-linux-narrow-width.sh

pass_count=0
fail_count=0
pass() { printf 'PASS %s\n' "$1"; pass_count=$((pass_count + 1)); }
fail() { printf 'FAIL %s: %s\n' "$1" "$2"; fail_count=$((fail_count + 1)); }

# The block under test, lifted from the smoke so the arithmetic runs without a
# built binary or a display.
block="$(sed -n '/^overflow_x="\$(geometry_field osc-overflow-0 local-x)"/,/^echo "overflow=/p' "$SMOKE")"
if [[ -z "$block" ]]; then
  fail "the smoke still derives the overflow entry from published geometry" "block not found"
  exit 1
fi
pass "the seam derivation is present in the smoke"

if grep -qE 'overflow_x=\$\(\(width' "$SMOKE"; then
  fail "the overflow entry must not be computed from the window width" \
    "an edge-relative offset misses the entry once the bar tightens its inset"
else
  pass "the overflow entry is not computed from the window width"
fi

run_case() {
  # run_case <label> <volume_right> <overflow_x> <expect_gap> <expect_seam_x> <expect_seam_w>
  local label="$1" volume_right="$2" overflow="$3" want_gap="$4" want_seam_x="$5" want_seam_w="$6"
  local T; T="$(mktemp -d)"
  local OUT_DIR="$T"
  # The shape the shell really publishes: indexed plane names, window-local
  # `local-x`, and `w`/`h` rather than `width`/`height`. Getting these wrong is
  # what made the first attempt at this fix report "no osc-overflow rectangle"
  # in the lane, so the fixture mirrors a real record line for line.
  {
    printf 'interaction: geometry part=window reason=configure seq=3 origin=window x=0 y=0 w=480 h=540 scale=1 fullscreen=false maximized=false compact=false\n'
    printf 'interaction: geometry part=osc-play-0 reason=configure seq=3 local-x=16 local-y=481 w=34 h=34 x=16 y=481 interactive=true\n'
    printf 'interaction: geometry part=osc-volume-0 reason=configure seq=3 local-x=%s local-y=481 w=34 h=34 x=%s y=481 interactive=true\n' \
      "$((volume_right - 34))" "$((volume_right - 34))"
    printf 'interaction: geometry part=osc-overflow-0 reason=configure seq=3 local-x=%s local-y=481 w=34 h=34 x=%s y=481 interactive=true\n' \
      "$overflow" "$overflow"
  } >"$T/app.log"

  geometry_field() {
    awk -v part="$1" -v key="$2" '
      index($0, "interaction: geometry part=" part " ") == 1 {
        for (i = 1; i <= NF; i++) { split($i, pair, "="); if (pair[1] == key) { value = pair[2] } }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$OUT_DIR/app.log"
  }
  local width=480 out status
  set +e
  out="$(eval "$block" 2>&1)"
  status=$?
  set -e
  rm -rf "$T"

  if (( status != 0 )); then
    if [[ "$want_gap" == "reject" ]]; then
      pass "$label is rejected: ${out}"
    else
      fail "$label" "exited ${status}: ${out}"
    fi
    return
  fi
  if [[ "$want_gap" == "reject" ]]; then
    fail "$label must be rejected" "$out"
    return
  fi
  if [[ "$out" == "overflow=${overflow}+34 neighbour-right=${volume_right} gap=${want_gap}px seam=${want_seam_x}+${want_seam_w}" ]]; then
    pass "$label aims at the published gap"
  else
    fail "$label" "$out"
  fi
}

# The tightened layout the candidate lane actually failed on: gap 8px.
run_case "the tightened bar" 430 438 8 432 4
# The roomy layout, gap 16px - the shape the old offsets were written for.
run_case "the roomy bar" 422 438 16 428 4
# Controls flush against each other: that is the defect the seam exists to catch.
run_case "a bar whose controls share bounds" 438 438 reject "" ""
