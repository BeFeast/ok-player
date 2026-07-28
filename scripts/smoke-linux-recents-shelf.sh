#!/usr/bin/env bash
# X11/Xvfb regression for the Continue-watching shelf layout (#702).
#
# The shelf is a grid of fixed-width cards, and the defect it regressed from was a shelf
# sized by its text: an ellipsized label reports its whole text as its natural width, so a
# long title or a long path used to widen the card holding it and the row came out uneven.
#
# The real player is launched idle over a fixture history that carries exactly the content
# that broke it - a very long title with emoji and hashtags, a short title, a long path, and
# an item with no thumbnail - and the per-plane rectangles the geometry diagnostic publishes
# (#690) are checked: every card rectangle must be the same width, the cards must share a
# row, the shelf must stay inside its container, and the "more" affordance must stay a
# control rather than a block sized by whatever space is left. The window is then narrowed
# and the same widths must survive the reflow.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
ISOLATED_XVFB="$ROOT/scripts/run-linux-isolated-xvfb-session.sh"
ISOLATED_DBUS="$ROOT/scripts/run-linux-isolated-dbus-session.sh"

# Width the window is narrowed to. Wide enough to keep the designed three-card row, narrow
# enough that the flow box has to re-lay the shelf out and would hand cards any width it had
# left over.
NARROW_WIDTH=700

if [[ "${1:-}" == "--inner" ]]; then
  shift
  BINARY="${1:?missing binary}"
  OUT_DIR="${2:?missing output directory}"

  export GDK_BACKEND=x11
  export GTK_USE_PORTAL=0
  export NO_AT_BRIDGE=1
  export XDG_SESSION_TYPE=x11
  export XDG_CURRENT_DESKTOP=XFCE
  export XDG_STATE_HOME="$OUT_DIR/state"
  export XDG_CONFIG_HOME="$OUT_DIR/config"
  export XDG_CACHE_HOME="$OUT_DIR/cache"
  export LIBGL_ALWAYS_SOFTWARE=1
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
  export OKP_FIXED_VIEWPORT_SMOKE=1
  export OKP_DISABLE_MPRIS=1
  export OKP_SKIP_OPEN_INSTALLER=1
  export OKP_SKIP_DEB_SELF_INSTALL=1
  export OKP_SKIP_UPDATE_CHECK=1
  # Posters are placed by file stem, so the shelf renders real thumbnails without a decoder
  # and the one item deliberately left without a poster keeps its placeholder.
  export OKP_POSTER_FIXTURE_DIR="$OUT_DIR/posters"

  xfwm4 --sm-client-disable >"$OUT_DIR/xfwm4.log" 2>&1 &
  wm_pid=$!
  app_pid=""
  cleanup() {
    [[ -n "$app_pid" ]] && kill "$app_pid" 2>/dev/null || true
    kill "$wm_pid" 2>/dev/null || true
  }
  trap cleanup EXIT

  for _ in $(seq 1 100); do
    if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id'; then
      break
    fi
    sleep 0.05
  done
  xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null | grep -q 'window id' || {
    echo "xfwm4 did not become ready" >&2
    exit 75
  }

  # Newest value of one key on one part of the geometry record, rounded to whole pixels.
  # Rounding is the "within a pixel" tolerance the card-width check is stated with.
  field() {
    awk -v part="$1" -v key="$2" '
      index($0, "interaction: geometry part=" part " ") == 1 {
        for (i = 1; i <= NF; i++) {
          split($i, pair, "=")
          if (pair[1] == key) { value = pair[2] }
        }
      }
      END { if (value == "" || value == "unknown") { print "" } else { printf "%.0f\n", value } }
    ' "$3"
  }

  # Fail unless the shelf the record describes is a uniform grid. Called once per window
  # width: a layout that only holds at the width it was designed at is not a grid.
  assert_uniform_shelf() {
    local log="$1" label="$2" expected_cards="$3"
    local index=0 first_width="" first_y="" left="" right=""

    while (( index < expected_cards )); do
      local width height x y
      width="$(field "recent-card-$index" w "$log")"
      height="$(field "recent-card-$index" h "$log")"
      x="$(field "recent-card-$index" local-x "$log")"
      y="$(field "recent-card-$index" local-y "$log")"
      [[ -n "$width" && -n "$height" && -n "$x" && -n "$y" ]] || {
        echo "$label: the shelf published no rectangle for card $index" >&2
        return 1
      }
      if [[ -z "$first_width" ]]; then
        first_width="$width"
        first_y="$y"
        left="$x"
        right=$((x + width))
      else
        (( width >= first_width - 1 && width <= first_width + 1 )) || {
          echo "$label: card $index is ${width}px wide but card 0 is ${first_width}px" >&2
          return 1
        }
        (( y == first_y )) || {
          echo "$label: card $index sits at y=${y} but card 0 sits at y=${first_y}" >&2
          return 1
        }
        if (( x < left )); then left="$x"; fi
        if (( x + width > right )); then right=$((x + width)); fi
      fi
      index=$((index + 1))
    done

    (( first_width > 0 )) || {
      echo "$label: the cards have no width" >&2
      return 1
    }

    local container_x container_w
    container_x="$(field welcome local-x "$log")"
    container_w="$(field welcome w "$log")"
    [[ -n "$container_x" && -n "$container_w" ]] || {
      echo "$label: the welcome surface published no rectangle" >&2
      return 1
    }
    (( left >= container_x && right <= container_x + container_w )) || {
      echo "$label: the shelf spans ${left}..${right} outside its container" \
        "${container_x}..$((container_x + container_w))" >&2
      return 1
    }

    # The affordance that opens History shares the shelf's grid. It must be a control
    # inside its cell, not the cell-sized slab that leftover space used to make of it.
    local more_w more_y more_h
    more_w="$(field recents-more-0 w "$log")"
    more_y="$(field recents-more-0 local-y "$log")"
    more_h="$(field recents-more-0 h "$log")"
    [[ -n "$more_w" && -n "$more_y" && -n "$more_h" ]] || {
      echo "$label: the shelf published no rectangle for the History affordance" >&2
      return 1
    }
    (( more_w > 0 && more_w <= first_width )) || {
      echo "$label: the History affordance is ${more_w}px wide against a ${first_width}px card" >&2
      return 1
    }
    (( more_h > 0 && more_h <= first_width )) || {
      echo "$label: the History affordance is ${more_h}px tall against a ${first_width}px card" >&2
      return 1
    }

    printf '%s\n' \
      "${label}_cards=${expected_cards}" \
      "${label}_card_width=${first_width}" \
      "${label}_card_row_y=${first_y}" \
      "${label}_shelf_span=${left}..${right}" \
      "${label}_container_span=${container_x}..$((container_x + container_w))" \
      "${label}_more_affordance=${more_w}x${more_h}" >>"$OUT_DIR/results.txt"
    printf '%s\n' "$first_width"
  }

  : >"$OUT_DIR/results.txt"

  timeout 90s env OKP_DEBUG_INTERACTIONS=1 "$BINARY" >"$OUT_DIR/app.log" 2>&1 &
  app_pid=$!
  window_id=""
  for _ in $(seq 1 80); do
    window_id="$(xdotool search --onlyvisible --name 'OK Player' 2>/dev/null | head -n1 || true)"
    [[ -n "$window_id" ]] && break
    sleep 0.25
  done
  [[ -n "$window_id" ]] || {
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
  xdotool windowactivate "$window_id" >/dev/null 2>&1 || true

  # A cold GTK start under Xvfb is slow, and the shelf only exists once the welcome model
  # has been built, so wait for the last card rather than a fixed sleep.
  shelf_ready=0
  for _ in $(seq 1 160); do
    if [[ -n "$(field recent-card-2 w "$OUT_DIR/app.log")" ]]; then
      shelf_ready=1
      break
    fi
    sleep 0.25
  done
  (( shelf_ready == 1 )) || {
    echo "the shelf never published a card rectangle" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
  # A fourth card would mean the shelf stopped capping itself at the welcome item limit.
  [[ -z "$(field recent-card-3 w "$OUT_DIR/app.log")" ]] || {
    echo "the shelf rendered more cards than the welcome item limit" >&2
    exit 1
  }

  # Evidence for a human reader when the host has ImageMagick; the checks below are the
  # ones that gate, so a host without it still runs the full regression.
  if command -v import >/dev/null 2>&1; then
    import -window "$window_id" "$OUT_DIR/shelf-wide.png" || true
  fi
  wide_width="$(assert_uniform_shelf "$OUT_DIR/app.log" wide 3)" || exit 1

  xdotool windowsize "$window_id" "$NARROW_WIDTH" 680
  narrowed=0
  for _ in $(seq 1 60); do
    if [[ "$(field welcome w "$OUT_DIR/app.log")" == "$NARROW_WIDTH" ]]; then
      narrowed=1
      break
    fi
    sleep 0.25
  done
  (( narrowed == 1 )) || {
    echo "the window never reported the narrowed width" >&2
    exit 1
  }
  sleep 1
  if command -v import >/dev/null 2>&1; then
    import -window "$window_id" "$OUT_DIR/shelf-narrow.png" || true
  fi
  narrow_width="$(assert_uniform_shelf "$OUT_DIR/app.log" narrow 3)" || exit 1

  # The card is a fixed tile, so narrowing the window may change how many cards share a row
  # but must never change how wide one is.
  (( narrow_width >= wide_width - 1 && narrow_width <= wide_width + 1 )) || {
    echo "cards are ${wide_width}px wide at the default width but ${narrow_width}px when narrowed" >&2
    exit 1
  }

  kill "$app_pid" 2>/dev/null || true
  wait "$app_pid" 2>/dev/null || true
  app_pid=""

  if awk '/panicked at|fatal runtime error|Aborted|core dumped/ { print FILENAME ":" FNR ":" $0; found = 1 } END { exit !found }' \
      "$OUT_DIR/app.log"; then
    echo "recents-shelf smoke observed a fatal process diagnostic" >&2
    exit 1
  fi

  printf '%s\n' \
    "card_width_survives_reflow=${wide_width}" \
    'fatal_diagnostics=absent' >>"$OUT_DIR/results.txt"
  exit 0
fi

BINARY="${1:-$ROOT/rust/target/debug/okp-linux-gtk}"
OUT_DIR="${2:-$ROOT/artifacts/manual-ui/linux-recents-shelf-smoke}"

for tool in xfwm4 xdotool xprop awk timeout python3 ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
[[ -x "$BINARY" ]] || { echo "Missing executable: $BINARY" >&2; exit 127; }

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# The fixture is the operator's case, not a friendly one: the shelf has to survive the
# content, so shortening the text here would hide the very defect this guards.
python3 "$ROOT/scripts/make-linux-recents-fixture.py" "$OUT_DIR"

__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  "$ISOLATED_XVFB" \
  "$OUT_DIR/xvfb-evidence.txt" \
  "$OUT_DIR/xvfb.log" \
  '-screen 0 1440x900x24 -nolisten tcp' \
  "$ISOLATED_DBUS" \
  "$OUT_DIR/dbus-evidence.txt" \
  "$0" --inner "$BINARY" "$OUT_DIR"

echo "Recents-shelf smoke passed. Results: $OUT_DIR/results.txt"
