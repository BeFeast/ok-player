#!/usr/bin/env bash
# Install a built ok-player RPM into the current root and start it headlessly.
#
# This gate exists to catch a package whose declared dependencies do not cover
# what the application needs at runtime. That only works if the root the package
# is installed into does NOT already carry those runtime libraries: an
# environment that pre-installs them makes the gate structurally incapable of
# failing. The RPM build root cannot satisfy that requirement, because
# mesa-libEGL-devel/mesa-libGL-devel pull libglvnd-devel, which pulls
# libglvnd-gles - so libGLESv2.so.2 is present there no matter what the package
# declares. Run this script in a clean root that carries only the headless
# harness (see the "Fedora <n> installed-RPM launch" job in .github/workflows/rpm.yml).
#
# There is deliberately no environment variable that changes what this gate does
# when it is executed. Everything the tests need to redirect - the installed
# binary path and the launch harness - is a positional argument of run_gate,
# which only a caller that `source`s this file can supply. Adding an `env:` key
# to a workflow cannot weaken, shorten, or short-circuit this file.
#
# The scope of that claim is this file. smoke-linux-main-window.sh below it does
# read OKP_MAIN_WINDOW_* mode switches; this gate pins the mode it wants on the
# invocation, and the two switches that could select a different suite are
# mutually exclusive with it, so the harness exits non-zero rather than running
# something weaker. That is a loud failure, not a guarantee that the environment
# is irrelevant to everything downstream.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

# Libraries the application reaches through dlopen, so no ELF entry exists for
# rpm's automatic dependency extraction to find. Each one must be absent before
# the package is installed and present afterwards: absent first proves the
# environment is not masking the package's closure, present after proves the
# package's own Requires pulled it in.
DLOPEN_RUNTIME_SONAMES=(libGLESv2.so.2)

# Everything the gate itself and the launch harness beneath it need. A host that
# cannot run a headless desktop session must fail loudly rather than pass by not
# launching anything.
HARNESS_TOOLS=(dnf rpm ldconfig Xvfb xauth mcookie flock dbus-run-session gdbus
  xfwm4 xdotool xwininfo xprop import magick ffmpeg ffprobe rg stat python3)

resolve_rpm() {
  local input="$1"
  if [[ -f "$input" ]]; then
    printf '%s\n' "$input"
    return 0
  fi
  if [[ ! -d "$input" ]]; then
    echo "No such RPM or directory: $input" >&2
    return 2
  fi
  local matches=()
  while IFS= read -r candidate; do
    matches+=("$candidate")
  done < <(find "$input" -type f -name 'ok-player-*.rpm' \
    ! -name '*-debuginfo-*' ! -name '*-debugsource-*' ! -name '*.src.rpm' \
    | sort)
  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "Expected exactly one installable ok-player RPM in $input, found ${#matches[@]}:" >&2
    printf '  %s\n' "${matches[@]}" >&2
    return 2
  fi
  printf '%s\n' "${matches[0]}"
}

soname_present() {
  ldconfig -p | grep -q "[[:space:]]$1 ("
}

require_harness() {
  local tool
  for tool in "${HARNESS_TOOLS[@]}"; do
    command -v "$tool" >/dev/null 2>&1 || {
      echo "Missing required tool for the installed-RPM launch gate: $tool" >&2
      exit 127
    }
  done
}

run_gate() {
  # run_gate <rpm-or-directory> <evidence-dir> <installed-binary> <launch-harness>
  #
  # The last two are parameters rather than constants so that
  # scripts/tests/rpm-installed-launch-gate.Tests.sh can drive the whole gate -
  # including everything after `dnf install` - against a synthetic root. main()
  # below pins them to the real installed location and the real harness, and
  # main() is what running this file executes.
  local rpm_input="$1"
  local evidence_dir="$2"
  local installed_binary="$3"
  local launch_harness="$4"

  require_harness

  local rpm_path
  rpm_path="$(resolve_rpm "$rpm_input")"
  echo "Installed-RPM launch gate: $rpm_path"

  if rpm -q ok-player >/dev/null 2>&1; then
    echo "ok-player is already installed in this root; the launch gate needs a clean root" >&2
    exit 1
  fi

  local soname
  for soname in "${DLOPEN_RUNTIME_SONAMES[@]}"; do
    if soname_present "$soname"; then
      echo "Launch root already provides $soname before ok-player is installed." >&2
      echo "This gate can no longer prove the RPM's declared dependency closure covers it." >&2
      echo "Remove whatever pulls $soname from the launch environment; it must arrive with the package." >&2
      exit 1
    fi
    echo "Pre-install: $soname is absent, as this gate requires"
  done

  # install_weak_deps=False on the package install too, not just on the harness
  # install in rpm.yml. Without it this gate proves only that *something in the
  # transaction* supplied the soname: a `Recommends` - which a user running
  # `dnf install --setopt=install_weak_deps=False` or a minimal image would not
  # get - would satisfy the post-install assertion below and the gate would call
  # an under-declared package green.
  dnf install -y --setopt=install_weak_deps=False "$rpm_path"

  if [[ ! -x "$installed_binary" ]]; then
    echo "The package installed but $installed_binary is not an executable file." >&2
    echo "The RPM did not put the application where the desktop entry and this gate expect it." >&2
    exit 1
  fi

  for soname in "${DLOPEN_RUNTIME_SONAMES[@]}"; do
    if ! soname_present "$soname"; then
      echo "ok-player is installed but $soname is still missing." >&2
      echo "The application dlopens it during startup, so its declared Requires are incomplete." >&2
      rpm -q --requires ok-player >&2 || true
      exit 1
    fi
    echo "Post-install: $soname arrived with the package's dependency closure"
  done

  echo "Launching the installed RPM binary: $installed_binary"
  OKP_MAIN_WINDOW_IDLE_ONLY=1 "$launch_harness" "$installed_binary" "$evidence_dir"

  echo "Installed-RPM launch gate passed. Evidence: $evidence_dir"
}

main() {
  local rpm_input="${1:?usage: smoke-linux-rpm-installed-launch.sh <rpm-or-directory> [evidence-dir]}"
  local evidence_dir="${2:-$ROOT/artifacts/linux/rpm/installed-launch}"
  run_gate "$rpm_input" "$evidence_dir" \
    /usr/bin/ok-player "$ROOT/scripts/smoke-linux-main-window.sh"
}

# Executed as a program -> run the gate against the real installed path and the
# real launch harness. Sourced -> define the functions and let the caller drive
# run_gate; only scripts/tests/rpm-installed-launch-gate.Tests.sh does that.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
