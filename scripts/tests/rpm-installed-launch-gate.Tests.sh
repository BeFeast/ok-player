#!/usr/bin/env bash
# Policy tests for scripts/smoke-linux-rpm-installed-launch.sh.
#
# The launch gate is only meaningful while it refuses to run in a root that
# already carries the libraries the RPM is supposed to bring with it. These
# tests drive the gate through a fully synthetic PATH - every tool it probes is
# a stub, and `ldconfig`/`rpm` are scripted - so the guards can be exercised on
# any Linux host, without Fedora, dnf, or a display.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
GATE="$ROOT/scripts/smoke-linux-rpm-installed-launch.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Tools the gate genuinely needs from the host, resolved once and re-exposed
# inside each synthetic PATH.
REAL_TOOLS=(env bash dirname find sort grep)
# Everything the gate probes for. All of them are no-op stubs; the gate must
# never reach a point where their behaviour matters in these tests.
HARNESS_TOOLS=(dnf rpm ldconfig Xvfb xauth mcookie flock dbus-run-session gdbus
  xfwm4 xdotool xwininfo xprop import magick ffmpeg ffprobe rg stat python3)

failures=0

make_path() {
  # make_path <name> [tool-to-omit]
  local name="$1" omit="${2:-}"
  local dir="$WORK/$name/bin"
  mkdir -p "$dir"
  local tool
  for tool in "${REAL_TOOLS[@]}"; do
    ln -sf "$(command -v "$tool")" "$dir/$tool"
  done
  for tool in "${HARNESS_TOOLS[@]}"; do
    [[ "$tool" == "$omit" ]] && continue
    printf '#!/bin/sh\nexit 0\n' >"$dir/$tool"
    chmod 0755 "$dir/$tool"
  done
  printf '%s\n' "$dir"
}

set_ldconfig() {
  # set_ldconfig <bindir> <output-for--p>
  local dir="$1" output="$2"
  # printf, not cat: the synthetic PATH deliberately carries no coreutils beyond
  # what the gate itself needs.
  {
    printf '#!/bin/sh\n'
    printf "printf '%%s\\\\n' '%s'\n" "$output"
  } >"$dir/ldconfig"
  chmod 0755 "$dir/ldconfig"
}

set_rpm_installed() {
  # set_rpm_installed <bindir> <0-if-ok-player-is-installed>
  local dir="$1" installed="$2"
  {
    printf '#!/bin/sh\n'
    printf 'exit %s\n' "$installed"
  } >"$dir/rpm"
  chmod 0755 "$dir/rpm"
}

make_rpm_dir() {
  # make_rpm_dir <name> <count-of-installable-rpms>
  local name="$1" count="$2"
  local dir="$WORK/$name"
  mkdir -p "$dir"
  local i
  for ((i = 1; i <= count; i++)); do
    : >"$dir/ok-player-0.11.0~beta.$i-1.fc43.x86_64.rpm"
  done
  # Debug packages live next to the real one in the build artifact and must
  # never be picked up as the installable package.
  : >"$dir/ok-player-debuginfo-0.11.0~beta.1-1.fc43.x86_64.rpm"
  : >"$dir/ok-player-debugsource-0.11.0~beta.1-1.fc43.x86_64.rpm"
  printf '%s\n' "$dir"
}

run_gate() {
  # run_gate <bindir> <rpm-dir> -> writes $WORK/out, $WORK/err, echoes exit code
  local bindir="$1" rpmdir="$2"
  local status=0
  env -i PATH="$bindir" HOME="$WORK" OKP_RPM_LAUNCH_GATE_SELFTEST=1 \
    bash "$GATE" "$rpmdir" "$WORK/evidence" >"$WORK/out" 2>"$WORK/err" || status=$?
  printf '%s\n' "$status"
}

check() {
  # check <label> <expected-exit> <actual-exit> <stream-file> <expected-substring>
  local label="$1" expected="$2" actual="$3" stream="$4" needle="$5"
  if [[ "$actual" != "$expected" ]]; then
    echo "FAIL: $label - expected exit $expected, got $actual" >&2
    sed 's/^/    /' "$WORK/out" "$WORK/err" >&2 || true
    failures=$((failures + 1))
    return
  fi
  if ! grep -qF -- "$needle" "$stream"; then
    echo "FAIL: $label - output did not contain: $needle" >&2
    sed 's/^/    /' "$stream" >&2 || true
    failures=$((failures + 1))
    return
  fi
  echo "ok: $label"
}

MASKED_LDCONFIG='	libGLESv2.so.2 (libc6,x86-64) => /lib64/libGLESv2.so.2'
CLEAN_LDCONFIG='	libEGL.so.1 (libc6,x86-64) => /lib64/libEGL.so.1'

# 1. A clean root passes the guards, and the installable RPM is resolved without
#    picking up the debuginfo/debugsource packages sitting next to it.
bin="$(make_path clean)"
set_ldconfig "$bin" "$CLEAN_LDCONFIG"
set_rpm_installed "$bin" 1
rpms="$(make_rpm_dir clean-rpms 1)"
status="$(run_gate "$bin" "$rpms")"
check 'clean root passes the pre-install guards' 0 "$status" "$WORK/out" \
  'selftest: pre-install guards passed'
if [[ "$status" == 0 ]] && grep -q 'debuginfo\|debugsource' "$WORK/out"; then
  echo 'FAIL: the gate resolved a debug package as the installable RPM' >&2
  failures=$((failures + 1))
fi

# 2. The masking case this gate exists for: the launch root already provides a
#    library the package is supposed to pull in, so the gate cannot prove the
#    declared closure and must refuse to report a pass.
bin="$(make_path masked)"
set_ldconfig "$bin" "$MASKED_LDCONFIG"
set_rpm_installed "$bin" 1
status="$(run_gate "$bin" "$rpms")"
check 'a pre-provided dlopen soname fails the gate' 1 "$status" "$WORK/err" \
  'Launch root already provides libGLESv2.so.2'

# 3. A root with ok-player already installed cannot prove anything about the
#    package's own dependency resolution.
bin="$(make_path preinstalled)"
set_ldconfig "$bin" "$CLEAN_LDCONFIG"
set_rpm_installed "$bin" 0
status="$(run_gate "$bin" "$rpms")"
check 'an already-installed package fails the gate' 1 "$status" "$WORK/err" \
  'already installed'

# 4. Missing harness tooling must fail loudly instead of silently not launching.
bin="$(make_path no-xdotool xdotool)"
set_ldconfig "$bin" "$CLEAN_LDCONFIG"
set_rpm_installed "$bin" 1
status="$(run_gate "$bin" "$rpms")"
check 'missing harness tooling fails with 127' 127 "$status" "$WORK/err" \
  'Missing required tool for the installed-RPM launch gate: xdotool'

# 5. An ambiguous artifact directory must not be resolved by guesswork.
bin="$(make_path ambiguous)"
set_ldconfig "$bin" "$CLEAN_LDCONFIG"
set_rpm_installed "$bin" 1
two_rpms="$(make_rpm_dir ambiguous-rpms 2)"
status="$(run_gate "$bin" "$two_rpms")"
check 'an ambiguous artifact directory fails the gate' 2 "$status" "$WORK/err" \
  'Expected exactly one installable ok-player RPM'

# 6. An artifact directory with no installable package is a build failure, not a
#    pass.
empty_rpms="$(make_rpm_dir empty-rpms 0)"
status="$(run_gate "$bin" "$empty_rpms")"
check 'an empty artifact directory fails the gate' 2 "$status" "$WORK/err" \
  'found 0'

if [[ "$failures" -ne 0 ]]; then
  echo "$failures installed-RPM launch gate policy test(s) failed" >&2
  exit 1
fi
echo 'Installed-RPM launch gate policy tests passed.'
