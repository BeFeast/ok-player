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
printf '%s\n' '{"completed":true}' >"$OKP_RENDER_PROFILE_PATH"
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

echo "profile-linux-4k60 timeout smoke passed"
