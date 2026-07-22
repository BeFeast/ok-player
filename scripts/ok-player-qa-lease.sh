#!/usr/bin/env bash
# Exclusive, TTL-bounded lease for live OK Player GUI QA on one graphical host.
set -euo pipefail

LEASE_DIR="${OKP_QA_LEASE_DIR:-$HOME/qa}"
LEASE_FILE="$LEASE_DIR/LEASE"
LOCK_FILE="$LEASE_DIR/.LEASE.lock"

usage() {
  cat >&2 <<'EOF'
usage:
  ok-player-qa-lease.sh acquire <host-role> <suite-id> [ttl-minutes] [owner-pid]
  ok-player-qa-lease.sh release <suite-id>
  ok-player-qa-lease.sh status

The operator-first role "sindri" additionally requires both
OKP_QA_ALLOW_SINDRI=1 and OKP_QA_OPERATOR_GO=1.
EOF
}

fail() {
  printf 'ERROR: %s\n' "$*" >&2
  exit 64
}

require_token() {
  local label="$1" value="$2"
  [[ "$value" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] ||
    fail "$label must use only letters, digits, dot, underscore, colon, or dash"
}

now_epoch() {
  if [[ -n "${OKP_QA_NOW_EPOCH:-}" ]]; then
    [[ "$OKP_QA_NOW_EPOCH" =~ ^[0-9]+$ ]] || fail "OKP_QA_NOW_EPOCH must be an epoch integer"
    printf '%s\n' "$OKP_QA_NOW_EPOCH"
  else
    date -u +%s
  fi
}

iso_at() {
  date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ
}

lease_field() {
  local key="$1"
  awk -F= -v key="$key" '$1 == key { sub(/^[^=]*=/, ""); print; exit }' "$LEASE_FILE"
}

lease_is_valid() {
  [[ -f "$LEASE_FILE" ]] || return 1
  local suite expires
  suite="$(lease_field SUITE_ID)"
  expires="$(lease_field EXPIRES_EPOCH)"
  [[ "$suite" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || return 1
  [[ "$expires" =~ ^[0-9]+$ ]] || return 1
  (( expires > $(now_epoch) ))
}

print_status() {
  if lease_is_valid; then
    printf 'HELD suite=%s host_role=%s until=%s owner_pid=%s\n' \
      "$(lease_field SUITE_ID)" \
      "$(lease_field HOST_ROLE)" \
      "$(lease_field EXPIRES_AT)" \
      "$(lease_field OWNER_PID)"
  else
    printf 'FREE\n'
  fi
}

command -v flock >/dev/null 2>&1 || fail "flock is required"
mkdir -p "$LEASE_DIR"
exec 9>"$LOCK_FILE"
flock -x 9

command_name="${1:-}"
case "$command_name" in
  status)
    [[ $# -eq 1 ]] || { usage; exit 64; }
    print_status
    ;;
  acquire)
    [[ $# -ge 3 && $# -le 5 ]] || { usage; exit 64; }
    role="$2"
    suite_id="$3"
    ttl_minutes="${4:-45}"
    owner_pid="${5:-$PPID}"
    require_token host-role "$role"
    require_token suite-id "$suite_id"
    [[ "$ttl_minutes" =~ ^[0-9]+$ ]] || fail "ttl-minutes must be an integer"
    [[ "$owner_pid" =~ ^[0-9]+$ ]] || fail "owner-pid must be an integer"
    (( ttl_minutes >= 1 && ttl_minutes <= 180 )) || fail "ttl-minutes must be between 1 and 180"

    if [[ "$role" == "sindri" ]] &&
      [[ "${OKP_QA_ALLOW_SINDRI:-0}" != "1" || "${OKP_QA_OPERATOR_GO:-0}" != "1" ]]; then
      printf 'REFUSE: sindri is operator-first and requires explicit operator authorization\n' >&2
      exit 3
    fi

    if lease_is_valid; then
      held_suite="$(lease_field SUITE_ID)"
      if [[ "$held_suite" != "$suite_id" ]]; then
        printf 'BUSY suite=%s until=%s\n' "$held_suite" "$(lease_field EXPIRES_AT)" >&2
        exit 2
      fi
    fi

    acquired_epoch="$(now_epoch)"
    expires_epoch=$((acquired_epoch + ttl_minutes * 60))
    temporary_file="$(mktemp "$LEASE_DIR/.LEASE.XXXXXX")"
    trap 'rm -f "$temporary_file"' EXIT
    {
      printf 'SCHEMA=1\n'
      printf 'HOST_ROLE=%s\n' "$role"
      printf 'SUITE_ID=%s\n' "$suite_id"
      printf 'OWNER_PID=%s\n' "$owner_pid"
      printf 'ACQUIRED_EPOCH=%s\n' "$acquired_epoch"
      printf 'ACQUIRED_AT=%s\n' "$(iso_at "$acquired_epoch")"
      printf 'EXPIRES_EPOCH=%s\n' "$expires_epoch"
      printf 'EXPIRES_AT=%s\n' "$(iso_at "$expires_epoch")"
    } >"$temporary_file"
    chmod 600 "$temporary_file"
    mv -f "$temporary_file" "$LEASE_FILE"
    trap - EXIT
    printf 'ACQUIRED suite=%s host_role=%s until=%s\n' \
      "$suite_id" "$role" "$(iso_at "$expires_epoch")"
    ;;
  release)
    [[ $# -eq 2 ]] || { usage; exit 64; }
    suite_id="$2"
    require_token suite-id "$suite_id"
    if [[ ! -f "$LEASE_FILE" ]]; then
      printf 'NOOP lease=missing\n'
      exit 0
    fi
    held_suite="$(lease_field SUITE_ID)"
    if [[ "$held_suite" != "$suite_id" ]]; then
      printf 'REFUSE: lease is owned by suite=%s\n' "${held_suite:-unknown}" >&2
      exit 2
    fi
    rm -f "$LEASE_FILE"
    printf 'RELEASED suite=%s\n' "$suite_id"
    ;;
  *)
    usage
    exit 64
    ;;
esac
