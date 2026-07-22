#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
LEASE="$ROOT/scripts/ok-player-qa-lease.sh"
HOST_RUNNER="$ROOT/scripts/ok-player-night-gui-host.sh"
CONTROLLER="$ROOT/scripts/ok-player-night-gui-qa.sh"
TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local file="$1" expected="$2"
  local contents
  contents="$(<"$file")"
  [[ "$contents" == *"$expected"* ]] || fail "$file does not contain: $expected"
}

lease_home="$TEST_ROOT/lease-home"
mkdir -p "$lease_home"
HOME="$lease_home" OKP_QA_NOW_EPOCH=1000 "$LEASE" acquire slava suite-a 45 101 >/dev/null
if HOME="$lease_home" OKP_QA_NOW_EPOCH=1001 "$LEASE" acquire slava suite-b 45 102 >/dev/null 2>&1; then
  fail 'a valid lease did not exclude a second suite'
fi
HOME="$lease_home" OKP_QA_NOW_EPOCH=4000 "$LEASE" acquire slava suite-b 45 102 >/dev/null
assert_contains "$lease_home/qa/LEASE" 'SUITE_ID=suite-b'
if HOME="$lease_home" "$LEASE" release suite-a >/dev/null 2>&1; then
  fail 'a non-owner released the lease'
fi
HOME="$lease_home" "$LEASE" release suite-b >/dev/null

if HOME="$lease_home" "$LEASE" acquire sindri suite-s 45 103 >/dev/null 2>&1; then
  fail 'sindri was leased without explicit operator authorization'
fi
if HOME="$lease_home" OKP_QA_ALLOW_SINDRI=1 "$LEASE" acquire sindri suite-s 45 103 >/dev/null 2>&1; then
  fail 'one sindri override was sufficient'
fi
HOME="$lease_home" OKP_QA_ALLOW_SINDRI=1 OKP_QA_OPERATOR_GO=1 \
  "$LEASE" acquire sindri suite-s 45 103 >/dev/null
HOME="$lease_home" "$LEASE" release suite-s >/dev/null

make_hooks() {
  local home="$1" dual="$2"
  local hooks="$home/.local/lib/ok-player-night-gui-qa/hooks"
  mkdir -p "$hooks"
  cat >"$hooks/probe-seat" <<'EOF'
#!/usr/bin/env bash
printf 'seat=seat0 status=ready\n'
EOF
  cat >"$hooks/prepare-candidate" <<'EOF'
#!/usr/bin/env bash
cat >"$1/candidate.env" <<'IDENTITY'
acceptance=accepted
source_sha=1111111111111111111111111111111111111111
version=0.11.0-beta.0.test
package_name=ok-player_test_amd64.deb
package_sha256=2222222222222222222222222222222222222222222222222222222222222222
manifest_sha256=3333333333333333333333333333333333333333333333333333333333333333
IDENTITY
EOF
  cat >"$hooks/run-action" <<'EOF'
#!/usr/bin/env bash
printf 'action=%s status=pass\n' "$1"
EOF
  if [[ "$dual" == 1 ]]; then
    cat >"$hooks/probe-dual-head" <<'EOF'
#!/usr/bin/env bash
printf 'active_heads=2\n'
EOF
  else
    cat >"$hooks/probe-dual-head" <<'EOF'
#!/usr/bin/env bash
printf 'active_heads=1\n'
exit 1
EOF
  fi
  chmod +x "$hooks"/*
}

slava_home="$TEST_ROOT/slava-home"
make_hooks "$slava_home" 0
HOME="$slava_home" "$HOST_RUNNER" run slava slava 20260722 suite-host >/dev/null
slava_results="$slava_home/qa/okp-night-20260722/slava/runs/suite-host/results.tsv"
assert_contains "$slava_results" $'A\tnon_osc_drag_10\tPASS'
assert_contains "$slava_results" $'A\tcandidate_identity\tPASS'
assert_contains "$slava_results" $'B\topen_each_head\tSKIP'
assert_contains "$slava_results" $'C\t4k_weak_host_stress\tPASS'
[[ -s "${slava_results%/*}/SHA256SUMS" ]] || fail 'host runner did not write artifact checksums'

baldr_home="$TEST_ROOT/baldr-home"
make_hooks "$baldr_home" 1
HOME="$baldr_home" "$HOST_RUNNER" run baldr baldr 20260722 suite-dual >/dev/null
baldr_results="$baldr_home/qa/okp-night-20260722/baldr/runs/suite-dual/results.tsv"
assert_contains "$baldr_results" $'B\topen_each_head\tPASS'
assert_contains "$baldr_results" $'C\t4k_weak_host_stress\tSKIP'

unprepared_home="$TEST_ROOT/unprepared-home"
mkdir -p "$unprepared_home/.local/lib/ok-player-night-gui-qa/hooks"
cat >"$unprepared_home/.local/lib/ok-player-night-gui-qa/hooks/probe-seat" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat >"$unprepared_home/.local/lib/ok-player-night-gui-qa/hooks/run-action" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$1" >>"$HOME/action-was-run"
EOF
chmod +x "$unprepared_home/.local/lib/ok-player-night-gui-qa/hooks"/*
if HOME="$unprepared_home" "$HOST_RUNNER" run slava slava 20260722 suite-unprepared >/dev/null 2>&1; then
  fail 'host runner passed without a candidate preparation hook'
fi
[[ ! -e "$unprepared_home/action-was-run" ]] || fail 'GUI action ran without an accepted candidate'
assert_contains \
  "$unprepared_home/qa/okp-night-20260722/slava/runs/suite-unprepared/results.tsv" \
  $'A\tcold_launch\tNOT RUN\taccepted candidate was not prepared'

fake_root="$TEST_ROOT/fleet"
fake_ssh="$TEST_ROOT/fake-ssh"
fake_log="$TEST_ROOT/fake-ssh.log"
cat >"$fake_ssh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
host="$1"
shift
printf '%s %s\n' "$host" "$*" >>"$FAKE_SSH_LOG"
host_home="$FAKE_FLEET_ROOT/$host"
mkdir -p "$host_home"
HOME="$host_home" "$@"
EOF
chmod +x "$fake_ssh"
for host in slava mimir baldr; do
  make_hooks "$fake_root/$host" 0
done

FAKE_FLEET_ROOT="$fake_root" FAKE_SSH_LOG="$fake_log" \
  OKP_QA_SSH_COMMAND="$fake_ssh" OKP_QA_UTC_HOUR=1 OKP_QA_RUN_DATE=20260722 \
  OKP_QA_SUITE_ID=suite-controller "$CONTROLLER" >/dev/null

mapfile -t run_hosts < <(awk '/ok-player-night-gui-host.sh/ { next } / bash -s -- run / { print $1 }' "$fake_log")
if [[ "${run_hosts[*]}" != 'slava mimir baldr' ]]; then
  fail "unexpected automatic host order: ${run_hosts[*]}"
fi
[[ "$(<"$fake_log")" == *sindri* ]] && fail 'sindri appeared in the automatic host list'

for host in slava mimir baldr; do
  acquire_line="$(awk -v host="$host" '$1 == host && / bash -s -- acquire / { print NR; exit }' "$fake_log")"
  run_line="$(awk -v host="$host" '$1 == host && / bash -s -- run / { print NR; exit }' "$fake_log")"
  release_line="$(awk -v host="$host" '$1 == host && / bash -s -- release / { print NR; exit }' "$fake_log")"
  [[ -n "$acquire_line" && -n "$run_line" && -n "$release_line" ]] ||
    fail "missing lease/run lifecycle for $host"
  (( acquire_line < run_line && run_line < release_line )) ||
    fail "GUI run was not enclosed by the lease for $host"
  [[ -s "$fake_root/$host/qa/okp-night-20260722/$host/runs/suite-controller/results.tsv" ]] ||
    fail "missing required artifact path for $host"
done

if OKP_QA_UTC_HOUR=1 "$CONTROLLER" --host sindri >/dev/null 2>&1; then
  fail 'controller accepted sindri without explicit operator authorization'
fi

printf 'Night GUI QA driver tests passed.\n'
