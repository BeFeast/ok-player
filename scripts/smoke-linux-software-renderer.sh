#!/usr/bin/env bash
# Mapped-window X11 pixel gate for the Flatpak no-DRI renderer contract.
# The caller supplies either the packaged Flatpak command or a local binary;
# this script appends a supported moving-red fixture through the production
# command-line media-open path and captures the real GTK top-level window.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OKP_SOFTWARE_RENDERER_OUT_DIR:-$ROOT/artifacts/manual-ui/linux-software-renderer-smoke}"
SOURCE_COMMIT="${OKP_ACCEPTANCE_SOURCE_COMMIT:-}"
ARTIFACT_MANIFEST="${OKP_FLATPAK_ARTIFACT_MANIFEST:-}"

[[ -n "$SOURCE_COMMIT" ]] || {
  echo "OKP_ACCEPTANCE_SOURCE_COMMIT is required" >&2
  exit 2
}
[[ -n "$ARTIFACT_MANIFEST" && -f "$ARTIFACT_MANIFEST" ]] || {
  echo "OKP_FLATPAK_ARTIFACT_MANIFEST must name the generated artifact manifest" >&2
  exit 2
}

if [[ "${1:-}" == "--" ]]; then
  shift
fi
if [[ "$#" -eq 0 ]]; then
  echo "Usage: $0 -- <ok-player command...>" >&2
  exit 2
fi
COMMAND=("$@")

for tool in cargo xvfb-run dbus-run-session ffmpeg ffprobe xdotool xprop xwininfo import od rg awk sed tail timeout ps readlink cp sha256sum python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done
if ! command -v magick >/dev/null 2>&1; then
  for tool in convert compare; do
    command -v "$tool" >/dev/null 2>&1 || {
      echo "Missing required ImageMagick command: magick or $tool" >&2
      exit 127
    }
  done
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

# xdg-pictures is the only persistent host filesystem exposed by the package.
# Keep both the fixture and presentation log inside that grant so the packaged
# smoke exercises the production permission model.
if [[ -n "${XDG_PICTURES_DIR:-}" ]]; then
  PICTURES_ROOT="$XDG_PICTURES_DIR"
elif command -v xdg-user-dir >/dev/null 2>&1; then
  PICTURES_ROOT="$(xdg-user-dir PICTURES)"
  PICTURES_ROOT="${PICTURES_ROOT:-$HOME/Pictures}"
else
  PICTURES_ROOT="$HOME/Pictures"
fi
mkdir -p "$PICTURES_ROOT"
FIXTURE_DIR="$(mktemp -d "$PICTURES_ROOT/.ok-player-no-dri.XXXXXX")"
FIXTURE="$FIXTURE_DIR/no-dri-red-moving.mkv"
PRESENT_LOG="$FIXTURE_DIR/presentation.jsonl"
cleanup_fixture() {
  rm -rf "$FIXTURE_DIR"
}
trap cleanup_fixture EXIT

# A dark-red field with a bright-red box moving horizontally. FFV1 in Matroska
# is lossless, supported by the production media classifier, and avoids a
# hardware-codec dependency in the software-renderer gate.
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i 'color=c=0x601010:s=320x180:r=24' \
  -f lavfi -i 'color=c=0xff3030:s=80x80:r=24' \
  -filter_complex "[0:v][1:v]overlay=x='mod(t*80,240)':y=50:shortest=1" \
  -t 60 -an -c:v ffv1 -level 3 -pix_fmt yuv420p "$FIXTURE"

ffprobe -v error \
  -show_entries stream=codec_name,width,height,avg_frame_rate \
  -show_entries format=duration \
  -of default=noprint_wrappers=1 "$FIXTURE" >"$OUT_DIR/fixture.txt"

if [[ "${OKP_SIMULATE_FLATPAK:-0}" == "1" ]]; then
  export FLATPAK_ID=com.befeast.okplayer
fi

if ! env __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json \
  xvfb-run -a --server-args='-screen 0 1280x900x24 -nolisten tcp +extension GLX +render -noreset' \
  dbus-run-session -- bash -s -- "$OUT_DIR" "$FIXTURE" "$PRESENT_LOG" "$SOURCE_COMMIT" "${COMMAND[@]}" \
  >"$OUT_DIR/session.log" 2>&1 <<'SMOKE'
set -euo pipefail

OUT_DIR="$1"
FIXTURE="$2"
PRESENT_LOG="$3"
SOURCE_COMMIT="$4"
shift 4
COMMAND=("$@")

if command -v magick >/dev/null 2>&1; then
  image_convert() { magick "$@"; }
  image_compare() { magick compare "$@"; }
else
  image_convert() { convert "$@"; }
  image_compare() { compare "$@"; }
fi

export GDK_BACKEND=x11
export GTK_USE_PORTAL=0
export NO_AT_BRIDGE=1
export XDG_SESSION_TYPE=x11
export XDG_CURRENT_DESKTOP=XFCE
export OKP_SKIP_UPDATE_CHECK=1
export OKP_DISABLE_MPRIS=1
export OKP_PRESENT_LOG="$PRESENT_LOG"

command_pid=""
window_pid=""
cleanup_app() {
  if [[ -n "$window_pid" ]]; then
    kill "$window_pid" 2>/dev/null || true
  fi
  if [[ -n "$command_pid" ]]; then
    kill "$command_pid" 2>/dev/null || true
    wait "$command_pid" 2>/dev/null || true
  fi
}
trap cleanup_app EXIT

"${COMMAND[@]}" "$FIXTURE" >"$OUT_DIR/app.log" 2>&1 &
command_pid=$!

window_id=""
for _ in $(seq 1 200); do
  mapfile -t windows < <(xdotool search --onlyvisible --name '^OK Player$' 2>/dev/null || true)
  if [[ "${#windows[@]}" -eq 1 ]]; then
    window_id="${windows[0]}"
    break
  fi
  if ! kill -0 "$command_pid" 2>/dev/null; then
    echo "player command exited before mapping its GTK top-level" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  fi
  sleep 0.1
done
[[ -n "$window_id" ]] || {
  echo "no unique mapped OK Player top-level appeared" >&2
  cat "$OUT_DIR/app.log" >&2
  exit 1
}

window_title="$(xdotool getwindowname "$window_id")"
window_pid="$(xdotool getwindowpid "$window_id" 2>/dev/null || true)"
window_pid_source="wm-pid"
if [[ -z "$window_pid" ]] && command -v flatpak >/dev/null 2>&1; then
  mapfile -t flatpak_pids < <(
    flatpak ps --columns=child-pid,application 2>/dev/null \
      | awk '$2 == "com.befeast.okplayer" { print $1 }'
  )
  if [[ "${#flatpak_pids[@]}" -eq 1 ]]; then
    queue=("${flatpak_pids[0]}")
    flatpak_player_pids=()
    while [[ "${#queue[@]}" -gt 0 ]]; do
      parent="${queue[0]}"
      queue=("${queue[@]:1}")
      mapfile -t children < <(ps --ppid "$parent" -o pid= | awk '{$1=$1; print}')
      for child in "${children[@]}"; do
        [[ -n "$child" ]] || continue
        child_process="$(ps -o comm= -p "$child" | awk '{$1=$1; print}')"
        if [[ "$child_process" == "ok-player" ]]; then
          flatpak_player_pids+=("$child")
        fi
        queue+=("$child")
      done
    done
    if [[ "${#flatpak_player_pids[@]}" -eq 1 ]]; then
      window_pid="${flatpak_player_pids[0]}"
      window_pid_source="flatpak-descendant-pid"
    fi
  fi
fi
[[ -n "$window_pid" ]] || {
  echo "mapped top-level has neither a WM PID nor one OK Player Flatpak child PID" >&2
  exit 1
}
captured_window_pid="$window_pid"
window_process="$(ps -o comm= -p "$window_pid" | awk '{$1=$1; print}')"
window_class="$(xprop -id "$window_id" WM_CLASS)"
window_type="$(xprop -id "$window_id" _NET_WM_WINDOW_TYPE)"
xwininfo -id "$window_id" >"$OUT_DIR/window.xwininfo.txt"
window_map_state="$(sed -n 's/^[[:space:]]*Map State: //p' "$OUT_DIR/window.xwininfo.txt")"
[[ "$window_title" == "OK Player" ]] || {
  echo "mapped top-level has the wrong title: $window_title" >&2
  exit 1
}
[[ "$window_process" == "ok-player" || "$window_process" == "okp-linux-gtk" ]] || {
  echo "mapped top-level is not owned by the player executable: $window_process" >&2
  exit 1
}
[[ "$window_class" == *"ok-player"* || "$window_class" == *"okp-linux-gtk"* ]] || {
  echo "mapped top-level has the wrong WM_CLASS: $window_class" >&2
  exit 1
}
[[ "$window_type" == *"_NET_WM_WINDOW_TYPE_NORMAL"* ]] || {
  echo "mapped top-level is not a normal application window: $window_type" >&2
  exit 1
}
[[ "$window_map_state" == "IsViewable" ]] || {
  echo "GTK top-level is not mapped and viewable: $window_map_state" >&2
  exit 1
}

dri_fd_count=0
for fd in "/proc/$window_pid/fd/"*; do
  target="$(readlink "$fd" 2>/dev/null || true)"
  if [[ "$target" == /dev/dri/* ]]; then
    dri_fd_count=$((dri_fd_count + 1))
  fi
done
[[ "$dri_fd_count" -eq 0 ]] || {
  echo "software-renderer process unexpectedly opened $dri_fd_count DRI descriptor(s)" >&2
  exit 1
}

rg -q 'Renderer policy: mode=software-no-dri flatpak=true dri-accessible=false backend=libmpv-software hwdec=no render-api=sw gsk-renderer=cairo' \
  "$OUT_DIR/app.log" || {
    echo "no-DRI software renderer policy was not selected" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
rg -q 'Software renderer: backend=libmpv-software format=bgr0 scene-renderer=cairo' \
  "$OUT_DIR/app.log" || {
    echo "mapped software surface did not report the libmpv CPU renderer" >&2
    cat "$OUT_DIR/app.log" >&2
    exit 1
  }
rg -Fq 'Launch request: 1 item(s), 0 playlist(s), 0 subtitle(s)' "$OUT_DIR/app.log" || {
  echo "fixture did not enter the production media-open path" >&2
  cat "$OUT_DIR/app.log" >&2
  exit 1
}
if rg -q 'backend=egl-pbuffer|Rendered frame probe' "$OUT_DIR/app.log"; then
  echo "mapped-window gate unexpectedly ran the offscreen EGL probe" >&2
  exit 1
fi

time_pos_start=""
time_pos_end=""
for _ in $(seq 1 200); do
  if [[ -f "$PRESENT_LOG" ]]; then
    mapfile -t positions < <(sed -n 's/.*"time_pos":\([0-9][0-9.]*\).*/\1/p' "$PRESENT_LOG")
    if [[ "${#positions[@]}" -ge 2 ]]; then
      time_pos_start="${positions[0]}"
      time_pos_end="${positions[${#positions[@]}-1]}"
      if awk -v start="$time_pos_start" -v end="$time_pos_end" \
        'BEGIN { exit !(end - start >= 0.75) }'; then
        break
      fi
    fi
  fi
  sleep 0.1
done
[[ -n "$time_pos_start" && -n "$time_pos_end" ]] || {
  echo "presentation log did not publish playback positions" >&2
  exit 1
}
time_pos_delta="$(awk -v start="$time_pos_start" -v end="$time_pos_end" 'BEGIN { printf "%.3f", end - start }')"
awk -v delta="$time_pos_delta" 'BEGIN { exit !(delta >= 0.75) }' || {
  echo "playback did not advance enough: start=$time_pos_start end=$time_pos_end" >&2
  exit 1
}

# Wait for the production chrome to auto-hide, then calculate the actual 16:9
# video rectangle within the mapped content and inset it to avoid letterbox
# edges or transient overlays.
sleep 3
geometry="$(xdotool getwindowgeometry --shell "$window_id")"
window_width="$(sed -n 's/^WIDTH=//p' <<<"$geometry")"
window_height="$(sed -n 's/^HEIGHT=//p' <<<"$geometry")"
(( window_width >= 320 && window_height >= 180 )) || {
  echo "mapped GTK top-level has trivial geometry: ${window_width}x${window_height}" >&2
  exit 1
}
if (( window_width * 9 <= window_height * 16 )); then
  video_width="$window_width"
  video_height=$((window_width * 9 / 16))
  video_x=0
  video_y=$(((window_height - video_height) / 2))
else
  video_height="$window_height"
  video_width=$((window_height * 16 / 9))
  video_x=$(((window_width - video_width) / 2))
  video_y=0
fi
crop_x=$((video_x + video_width / 20))
crop_y=$((video_y + video_height / 20))
crop_width=$((video_width * 9 / 10))
crop_height=$((video_height * 9 / 10))

capture_a="$OUT_DIR/mapped-gtk-player-window.png"
capture_b="$OUT_DIR/mapped-gtk-player-window-later.png"
region_a="$OUT_DIR/video-region.png"
region_b="$OUT_DIR/video-region-later.png"
measure_region() {
  image_convert "$1" -alpha off -depth 8 rgb:- | od -An -v -tu1 | awk '
    {
      for (i = 1; i <= NF; i++) {
        channel[channel_index++] = $i
        if (channel_index == 3) {
          red = channel[0]; green = channel[1]; blue = channel[2]
          total++
          if (red + green + blue > 60) nonblack++
          if (red >= 70 && red > green * 1.4 && red > blue * 1.4) dominant++
          channel_index = 0
        }
      }
    }
    END { printf "%d %d %d\n", total, nonblack, dominant }
  '
}

capture_passed=false
for _ in $(seq 1 20); do
  import -window "$window_id" "$capture_a"
  image_convert "$capture_a" -crop "${crop_width}x${crop_height}+${crop_x}+${crop_y}" +repage "$region_a"
  read -r total_pixels nonblack_pixels red_dominant_pixels < <(measure_region "$region_a")
  nonblack_ratio="$(awk -v count="$nonblack_pixels" -v total="$total_pixels" 'BEGIN { printf "%.6f", count / total }')"
  red_dominant_ratio="$(awk -v count="$red_dominant_pixels" -v total="$total_pixels" 'BEGIN { printf "%.6f", count / total }')"
  if awk -v total="$total_pixels" -v nonblack="$nonblack_ratio" -v red="$red_dominant_ratio" \
    'BEGIN { exit !(total >= 10000 && nonblack >= 0.55 && red >= 0.55) }'; then
    capture_passed=true
    break
  fi
  sleep 0.25
done
[[ "$capture_passed" == true ]] || {
    echo "mapped video region failed pixel assertions: total=$total_pixels nonblack=$nonblack_ratio red=$red_dominant_ratio" >&2
    exit 1
}

moving_capture_passed=false
for _ in $(seq 1 10); do
  sleep 0.25
  import -window "$window_id" "$capture_b"
  image_convert "$capture_b" -crop "${crop_width}x${crop_height}+${crop_x}+${crop_y}" +repage "$region_b"
  read -r later_total later_nonblack later_red < <(measure_region "$region_b")
  later_nonblack_ratio="$(awk -v count="$later_nonblack" -v total="$later_total" 'BEGIN { printf "%.6f", count / total }')"
  later_red_ratio="$(awk -v count="$later_red" -v total="$later_total" 'BEGIN { printf "%.6f", count / total }')"
  changed_pixels="$(image_compare -metric AE "$region_a" "$region_b" null: 2>&1 || true)"
  changed_pixels="$(awk '{print $1}' <<<"$changed_pixels")"
  if awk -v total="$later_total" -v nonblack="$later_nonblack_ratio" -v red="$later_red_ratio" -v changed="$changed_pixels" \
    'BEGIN { exit !(total >= 10000 && nonblack >= 0.55 && red >= 0.55 && changed >= 500) }'; then
    moving_capture_passed=true
    break
  fi
done
[[ "$moving_capture_passed" == true ]] || {
  echo "mapped video region did not visibly change: changed_pixels=$changed_pixels" >&2
  exit 1
}
screenshot_sha256="$(sha256sum "$capture_a" | awk '{print $1}')"
later_screenshot_sha256="$(sha256sum "$capture_b" | awk '{print $1}')"

xdotool windowclose "$window_id" 2>/dev/null || true
for _ in $(seq 1 50); do
  kill -0 "$command_pid" 2>/dev/null || break
  sleep 0.1
done
kill "$command_pid" 2>/dev/null || true
wait "$command_pid" 2>/dev/null || true
command_pid=""
window_pid=""
cp "$PRESENT_LOG" "$OUT_DIR/presentation.jsonl"

printf '%s\n' \
  'renderer_mode=software-no-dri' \
  'backend=libmpv-software' \
  'gtk_scene_renderer=cairo' \
  'software_pixel_format=bgr0' \
  'opengl_renderer=not-used' \
  'probe_backend=not-run' \
  'production_media_open=pass' \
  'mapped_gtk_player_window=pass' \
  "window_map_state=$window_map_state" \
  'non_trivial_geometry=pass' \
  "window_id=$window_id" \
  "window_pid=$captured_window_pid" \
  "window_pid_source=$window_pid_source" \
  "window_process=$window_process" \
  "window_title=$window_title" \
  "dri_fd_count=$dri_fd_count" \
  "window_width=$window_width" \
  "window_height=$window_height" \
  "time_pos_start=$time_pos_start" \
  "time_pos_end=$time_pos_end" \
  "time_pos_delta=$time_pos_delta" \
  'playback_advances=pass' \
  "video_region_x=$crop_x" \
  "video_region_y=$crop_y" \
  "video_region_width=$crop_width" \
  "video_region_height=$crop_height" \
  "total_pixels=$total_pixels" \
  "nonblack_pixels=$nonblack_pixels" \
  "nonblack_ratio=$nonblack_ratio" \
  "red_dominant_pixels=$red_dominant_pixels" \
  "red_dominant_ratio=$red_dominant_ratio" \
  "changed_pixels=$changed_pixels" \
  'screenshot=mapped-gtk-player-window.png' \
  "screenshot_sha256=$screenshot_sha256" \
  'later_screenshot=mapped-gtk-player-window-later.png' \
  "later_screenshot_sha256=$later_screenshot_sha256" \
  'visible_video_region=pass' >"$OUT_DIR/results.txt"

python3 - "$OUT_DIR/results.json" \
  "$SOURCE_COMMIT" \
  "$window_id" "$captured_window_pid" "$window_pid_source" "$window_process" "$window_title" "$window_map_state" \
  "$dri_fd_count" "$window_width" "$window_height" \
  "$time_pos_start" "$time_pos_end" "$time_pos_delta" \
  "$crop_x" "$crop_y" "$crop_width" "$crop_height" "$total_pixels" "$nonblack_pixels" \
  "$nonblack_ratio" "$red_dominant_pixels" "$red_dominant_ratio" "$changed_pixels" \
  "$screenshot_sha256" "$later_screenshot_sha256" <<'PYRESULT'
import json
import sys
from pathlib import Path

(
    output,
    source_commit,
    window_id,
    window_pid,
    window_pid_source,
    window_process,
    window_title,
    window_map_state,
    dri_fd_count,
    window_width,
    window_height,
    time_pos_start,
    time_pos_end,
    time_pos_delta,
    crop_x,
    crop_y,
    crop_width,
    crop_height,
    total_pixels,
    nonblack_pixels,
    nonblack_ratio,
    red_dominant_pixels,
    red_dominant_ratio,
    changed_pixels,
    screenshot_sha256,
    later_screenshot_sha256,
) = sys.argv[1:]

result = {
    "source_commit": source_commit,
    "renderer_mode": "software-no-dri",
    "backend": "libmpv-software",
    "gtk_scene_renderer": "cairo",
    "software_pixel_format": "bgr0",
    "opengl_renderer": "not-used",
    "probe_backend": "not-run",
    "production_media_open": "pass",
    "mapped_gtk_player_window": "pass",
    "window": {
        "id": int(window_id),
        "pid": int(window_pid),
        "pid_source": window_pid_source,
        "process": window_process,
        "title": window_title,
        "map_state": window_map_state,
        "non_trivial_geometry": "pass",
        "dri_fd_count": int(dri_fd_count),
        "width": int(window_width),
        "height": int(window_height),
    },
    "playback": {
        "time_pos_start": float(time_pos_start),
        "time_pos_end": float(time_pos_end),
        "time_pos_delta": float(time_pos_delta),
        "advances": "pass",
    },
    "video_region": {
        "x": int(crop_x),
        "y": int(crop_y),
        "width": int(crop_width),
        "height": int(crop_height),
        "total_pixels": int(total_pixels),
        "nonblack_pixels": int(nonblack_pixels),
        "nonblack_ratio": float(nonblack_ratio),
        "red_dominant_pixels": int(red_dominant_pixels),
        "red_dominant_ratio": float(red_dominant_ratio),
        "changed_pixels": int(changed_pixels),
        "visible": "pass",
    },
    "screenshots": {
        "mapped_window": {
            "file": "mapped-gtk-player-window.png",
            "sha256": screenshot_sha256,
        },
        "mapped_window_later": {
            "file": "mapped-gtk-player-window-later.png",
            "sha256": later_screenshot_sha256,
        },
    },
}
Path(output).write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
PYRESULT
SMOKE
then
  echo "Software renderer mapped-window gate failed. Session log: $OUT_DIR/session.log" >&2
  cat "$OUT_DIR/session.log" >&2
  exit 1
fi

# The evidence archive is public. Preserve diagnostics while replacing known
# checkout and user-directory prefixes with stable, non-machine-specific labels.
python3 - "$OUT_DIR" "$ROOT" "$HOME" "$PICTURES_ROOT" "$FIXTURE_DIR" <<'PY'
import sys
from pathlib import Path

out_dir = Path(sys.argv[1])
replacements = {
    sys.argv[2]: "<repo>",
    sys.argv[3]: "<home>",
    sys.argv[4]: "<pictures>",
    sys.argv[5]: "<fixture>",
}
for path in (out_dir / "app.log", out_dir / "session.log", out_dir / "presentation.jsonl"):
    text = path.read_text(errors="replace")
    for source, replacement in sorted(
        replacements.items(), key=lambda item: len(item[0]), reverse=True
    ):
        if source:
            text = text.replace(source, replacement)
    path.write_text(text)
PY

cargo run --quiet --locked --manifest-path "$ROOT/rust/Cargo.toml" \
  -p okp-core --bin okp-acceptance-evidence -- \
  flatpak-software-renderer-validate \
  --manifest "$OUT_DIR/results.json" \
  --artifact-manifest "$ARTIFACT_MANIFEST" \
  --source-commit "$SOURCE_COMMIT"

echo "Software renderer mapped-window gate passed. Results: $OUT_DIR/results.json"
