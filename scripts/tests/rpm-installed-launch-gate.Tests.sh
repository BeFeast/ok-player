#!/usr/bin/env bash
# Policy tests for scripts/smoke-linux-rpm-installed-launch.sh.
#
# The launch gate is only meaningful while it refuses to run in a root that
# already carries the libraries the RPM is supposed to bring with it, AND while
# it still asserts, after installing, that the package's own closure supplied
# them. These tests drive the whole gate - both halves - through a fully
# synthetic PATH: every tool it probes is a stub, `dnf`/`rpm`/`ldconfig` are
# scripted, and the installed binary and the launch harness are files in a
# temporary directory. So it runs on any Linux host, without Fedora, dnf, or a
# display.
#
# The gate exposes run_gate() with the installed-binary path and the launch
# harness as parameters, and pins them to the real ones in main(). These tests
# source the gate and call run_gate directly; nothing in the gate's executed
# path can be redirected by an environment variable, which is the point.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
GATE="$ROOT/scripts/smoke-linux-rpm-installed-launch.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Tools the gate genuinely needs from the host, resolved once and re-exposed
# inside each synthetic PATH.
REAL_TOOLS=(env bash dirname find sort grep)
# Everything the gate probes for. Except for the ones scripted below, all are
# no-op stubs; the gate must never reach a point where their behaviour matters.
HARNESS_TOOLS=(dnf rpm ldconfig Xvfb xauth mcookie flock dbus-run-session gdbus
  xfwm4 xdotool xwininfo xprop import magick ffmpeg ffprobe rg stat python3)

failures=0

# A driver that sources the gate and calls run_gate with test-supplied paths.
# Sourcing is the only way in: the gate runs main() - with /usr/bin/ok-player
# and the real launch harness - whenever it is executed as a program.
DRIVER="$WORK/drive-gate.sh"
{
  printf '#!/usr/bin/env bash\n'
  printf 'set -euo pipefail\n'
  printf '# shellcheck source=/dev/null\n'
  printf 'source "%s"\n' "$GATE"
  printf 'run_gate "$@"\n'
} >"$DRIVER"
chmod 0755 "$DRIVER"

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

new_state() {
  # new_state <name> -> a directory the scripted stubs share
  local dir="$WORK/state-$1"
  mkdir -p "$dir"
  printf '%s\n' "$dir"
}

set_dnf() {
  # set_dnf <bindir> <state-dir> <exit-code>
  # Records its argv, and on success marks the root as carrying the package.
  local dir="$1" state="$2" code="$3"
  {
    printf '#!/bin/sh\n'
    # printf, not tee/cat: the synthetic PATH deliberately carries no coreutils
    # beyond what the gate itself needs.
    printf 'printf "%%s\\n" "$*" >>"%s/dnf-args"\n' "$state"
    printf '[ "%s" -eq 0 ] || exit %s\n' "$code" "$code"
    printf ': >"%s/installed"\n' "$state"
    printf 'exit 0\n'
  } >"$dir/dnf"
  chmod 0755 "$dir/dnf"
}

set_ldconfig() {
  # set_ldconfig <bindir> <state-dir> <output-before-install> <output-after>
  local dir="$1" state="$2" before="$3" after="$4"
  {
    printf '#!/bin/sh\n'
    printf 'if [ -f "%s/installed" ]; then\n' "$state"
    printf "  printf '%%s\\\\n' '%s'\n" "$after"
    printf 'else\n'
    printf "  printf '%%s\\\\n' '%s'\n" "$before"
    printf 'fi\n'
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

make_binary() {
  # make_binary <name> <executable:0|1> -> path the gate should treat as installed
  local name="$1" executable="$2"
  local dir="$WORK/installed-$name"
  mkdir -p "$dir"
  printf '#!/bin/sh\nexit 0\n' >"$dir/ok-player"
  if [[ "$executable" == 1 ]]; then
    chmod 0755 "$dir/ok-player"
  else
    chmod 0644 "$dir/ok-player"
  fi
  printf '%s\n' "$dir/ok-player"
}

make_harness() {
  # make_harness <name> <exit-code> <state-dir> -> a stand-in for
  # scripts/smoke-linux-main-window.sh that records how it was invoked
  local name="$1" code="$2" state="$3"
  local path="$WORK/harness-$name.sh"
  {
    printf '#!/bin/sh\n'
    printf 'printf "%%s\\n" "$OKP_MAIN_WINDOW_IDLE_ONLY $*" >>"%s/launched"\n' "$state"
    printf 'exit %s\n' "$code"
  } >"$path"
  chmod 0755 "$path"
  printf '%s\n' "$path"
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

run_gate_under_test() {
  # run_gate_under_test <bindir> <rpm-dir> <installed-binary> <launch-harness>
  # -> writes $WORK/out, $WORK/err, echoes exit code
  local bindir="$1" rpmdir="$2" binary="$3" harness="$4"
  local status=0
  env -i PATH="$bindir" HOME="$WORK" \
    bash "$DRIVER" "$rpmdir" "$WORK/evidence" "$binary" "$harness" \
    >"$WORK/out" 2>"$WORK/err" || status=$?
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

expect_file_contains() {
  # expect_file_contains <label> <file> <substring>
  local label="$1" file="$2" needle="$3"
  if [[ ! -f "$file" ]]; then
    echo "FAIL: $label - $file was never written" >&2
    failures=$((failures + 1))
    return
  fi
  if ! grep -qF -- "$needle" "$file"; then
    echo "FAIL: $label - $file did not contain: $needle" >&2
    sed 's/^/    /' "$file" >&2 || true
    failures=$((failures + 1))
    return
  fi
  echo "ok: $label"
}

HAS_GLES='	libGLESv2.so.2 (libc6,x86-64) => /lib64/libGLESv2.so.2'
NO_GLES='	libEGL.so.1 (libc6,x86-64) => /lib64/libEGL.so.1'

rpms="$(make_rpm_dir clean-rpms 1)"

# 1. The whole gate, end to end: a clean root, a package install that brings the
#    dlopen'ed soname with it, an installed binary where the gate expects it, and
#    a launch that succeeds. The installable RPM is resolved without picking up
#    the debuginfo/debugsource packages sitting next to it.
bin="$(make_path clean)"
state="$(new_state clean)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
binary="$(make_binary clean 1)"
harness="$(make_harness clean 0 "$state")"
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
check 'a clean root that the package fills passes the whole gate' 0 "$status" "$WORK/out" \
  'Installed-RPM launch gate passed'
expect_file_contains 'the post-install soname assertion is reported' "$WORK/out" \
  "Post-install: libGLESv2.so.2 arrived with the package's dependency closure"
expect_file_contains 'the gate launches the installed binary through the harness' \
  "$state/launched" "1 $binary $WORK/evidence"
# The harness install in rpm.yml drops weak dependencies to keep the launch root
# clean; the package install must drop them too, or a `Recommends` would satisfy
# the post-install assertion and the gate would pass an under-declared package.
expect_file_contains 'the package is installed without weak dependencies' \
  "$state/dnf-args" '--setopt=install_weak_deps=False'
if [[ "$status" == 0 ]] && grep -q 'debuginfo\|debugsource' "$WORK/out"; then
  echo 'FAIL: the gate resolved a debug package as the installable RPM' >&2
  failures=$((failures + 1))
fi

# 2. The masking case this gate exists for: the launch root already provides a
#    library the package is supposed to pull in, so the gate cannot prove the
#    declared closure and must refuse to report a pass.
bin="$(make_path masked)"
state="$(new_state masked)"
set_ldconfig "$bin" "$state" "$HAS_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
check 'a pre-provided dlopen soname fails the gate' 1 "$status" "$WORK/err" \
  'Launch root already provides libGLESv2.so.2'

# 3. The other half of the same guard, and the reason the tests exist: the
#    package installs, but its closure did not bring the dlopen'ed soname. This
#    is the exact defect the pre-fix RPM had. Deleting the post-install
#    assertion from the gate must turn this red.
bin="$(make_path underdeclared)"
state="$(new_state underdeclared)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$NO_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
check 'a package whose closure misses the dlopen soname fails the gate' 1 "$status" \
  "$WORK/err" 'ok-player is installed but libGLESv2.so.2 is still missing'
if [[ -f "$state/launched" ]]; then
  echo 'FAIL: the gate launched the application despite an incomplete closure' >&2
  failures=$((failures + 1))
fi

# 4. A root with ok-player already installed cannot prove anything about the
#    package's own dependency resolution.
bin="$(make_path preinstalled)"
state="$(new_state preinstalled)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 0
set_dnf "$bin" "$state" 0
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
check 'an already-installed package fails the gate' 1 "$status" "$WORK/err" \
  'already installed'

# 5. Missing harness tooling must fail loudly instead of silently not launching.
bin="$(make_path no-xdotool xdotool)"
state="$(new_state no-xdotool)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
check 'missing harness tooling fails with 127' 127 "$status" "$WORK/err" \
  'Missing required tool for the installed-RPM launch gate: xdotool'

# 6. A failing package install is a failure, not a skipped launch.
bin="$(make_path dnf-fails)"
state="$(new_state dnf-fails)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 4
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$harness")"
if [[ "$status" == 0 ]]; then
  echo 'FAIL: a failing dnf install did not fail the gate' >&2
  failures=$((failures + 1))
else
  echo 'ok: a failing package install fails the gate'
fi

# 7. The package installed, but not to the path the desktop entry and this gate
#    expect. Nothing downstream would notice on its own.
bin="$(make_path no-binary)"
state="$(new_state no-binary)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
non_executable="$(make_binary not-executable 0)"
status="$(run_gate_under_test "$bin" "$rpms" "$non_executable" "$harness")"
check 'a package that installs no runnable binary fails the gate' 1 "$status" \
  "$WORK/err" 'is not an executable file'

# 8. A failing launch must reach the caller. The gate adds nothing between the
#    harness and its own exit status, and this pins that.
bin="$(make_path launch-fails)"
state="$(new_state launch-fails)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
failing_harness="$(make_harness failing 3 "$state")"
status="$(run_gate_under_test "$bin" "$rpms" "$binary" "$failing_harness")"
if [[ "$status" != 3 ]]; then
  echo "FAIL: a failing launch harness did not fail the gate - expected exit 3, got $status" >&2
  failures=$((failures + 1))
else
  echo 'ok: a failing launch fails the gate with the harness exit code'
fi

# 9. An ambiguous artifact directory must not be resolved by guesswork.
bin="$(make_path ambiguous)"
state="$(new_state ambiguous)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 0
two_rpms="$(make_rpm_dir ambiguous-rpms 2)"
status="$(run_gate_under_test "$bin" "$two_rpms" "$binary" "$harness")"
check 'an ambiguous artifact directory fails the gate' 2 "$status" "$WORK/err" \
  'Expected exactly one installable ok-player RPM'

# 10. An artifact directory with no installable package is a build failure, not
#     a pass.
empty_rpms="$(make_rpm_dir empty-rpms 0)"
status="$(run_gate_under_test "$bin" "$empty_rpms" "$binary" "$harness")"
check 'an empty artifact directory fails the gate' 2 "$status" "$WORK/err" \
  'found 0'

# 11. The gate has no environment-variable escape hatch. Executing it - which is
#     what every workflow does - always runs main(), whatever the environment
#     says. This is a regression test for a removed selftest switch that made
#     the gate print a pass and exit 0 right after the pre-install guards.
#     The removed switch made the gate print a pass and exit 0 immediately after
#     the pre-install guards, before `dnf install` ever ran. So: execute the gate
#     with every plausible switch name set, and require that it still got as far
#     as invoking dnf. The dnf stub fails here on purpose, so this case never
#     depends on whether the host that runs the tests happens to have a real
#     /usr/bin/ok-player.
bin="$(make_path executed)"
state="$(new_state executed)"
set_ldconfig "$bin" "$state" "$NO_GLES" "$HAS_GLES"
set_rpm_installed "$bin" 1
set_dnf "$bin" "$state" 4
status=0
env -i PATH="$bin" HOME="$WORK" \
  OKP_RPM_LAUNCH_GATE_SELFTEST=1 OKP_SELFTEST=1 OKP_SKIP=1 CI=true \
  bash "$GATE" "$rpms" "$WORK/evidence" >"$WORK/out" 2>"$WORK/err" || status=$?
if [[ "$status" == 0 ]]; then
  echo 'FAIL: the executed gate reported success without installing anything' >&2
  sed 's/^/    /' "$WORK/out" >&2 || true
  failures=$((failures + 1))
else
  echo 'ok: no environment variable can short-circuit the executed gate'
fi
expect_file_contains 'the executed gate reaches the package install' \
  "$state/dnf-args" 'install'
if ! grep -qF -- 'Pre-install: libGLESv2.so.2 is absent' "$WORK/out"; then
  echo 'FAIL: the executed gate did not run the pre-install guards' >&2
  failures=$((failures + 1))
fi

if [[ "$failures" -ne 0 ]]; then
  echo "$failures installed-RPM launch gate policy test(s) failed" >&2
  exit 1
fi
echo 'Installed-RPM launch gate policy tests passed.'
