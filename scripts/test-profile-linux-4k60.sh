#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMP_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEMP_ROOT"' EXIT

BIN_DIR="$TEMP_ROOT/bin"
OUT_DIR="$TEMP_ROOT/output"
CALL_LOG="$TEMP_ROOT/mpv-calls.log"
mkdir -p "$BIN_DIR"

cat >"$BIN_DIR/ffmpeg" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
touch "${!#}"
EOF

cat >"$BIN_DIR/ffprobe" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' 'hevc,Main 10,3840,2160,yuv420p10le,60/1'
EOF

cat >"$BIN_DIR/mpv" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$PROFILE_TEST_CALL_LOG"
EOF

cat >"$BIN_DIR/timeout" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
shift 2
if [[ "$(basename "$1")" == "mpv" ]]; then
  "$@"
  exit 124
fi
exec "$@"
EOF

cat >"$BIN_DIR/ok-player" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test "$OKP_FIXED_VIEWPORT_SMOKE" = 1
if [[ "$OKP_RENDER_PROFILE_PATH" == *auto-safe.json ]]; then
  hwdec='"vaapi-copy"'
  configured='auto-safe'
else
  hwdec='null'
  configured='no'
fi
printf '%s\n' "{\
\"schema_version\":2,\
\"configured_hwdec\":\"$configured\",\
\"hwdec_current\":$hwdec,\
\"render_fps\":60.0,\
\"render_calls\":600,\
\"update_frame_requests\":600,\
\"fallback_redraws\":600,\
\"frame_clock_ticks\":600,\
\"callback_notifications\":87,\
\"vo_dropped_frames\":0,\
\"decoder_dropped_frames\":0,\
\"render_target_width\":1120,\
\"render_target_height\":680}" >"$OKP_RENDER_PROFILE_PATH"
EOF

chmod +x "$BIN_DIR"/*

PATH="$BIN_DIR:$PATH" \
  DISPLAY=:99 \
  PROFILE_TEST_CALL_LOG="$CALL_LOG" \
  OKP_4K60_FRAMES=1 \
  "$ROOT/scripts/profile-linux-4k60.sh" profile "$BIN_DIR/ok-player" "$OUT_DIR" \
  >"$TEMP_ROOT/profile.log"

test -s "$OUT_DIR/ok-player-no.json"
test -s "$OUT_DIR/ok-player-auto-safe.json"
test "$(wc -l <"$CALL_LOG")" -eq 2
if [[ "$(<"$TEMP_ROOT/profile.log")" != *"4K60 acceptance passed"* ]]; then
  echo "profile validation success was not reported" >&2
  exit 1
fi

echo "profile-linux-4k60 timeout smoke passed"
