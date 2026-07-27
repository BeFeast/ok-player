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
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

RPM_INPUT="${1:?usage: smoke-linux-rpm-installed-launch.sh <rpm-or-directory> [evidence-dir]}"
EVIDENCE_DIR="${2:-$ROOT/artifacts/linux/rpm/installed-launch}"
INSTALLED_BINARY=/usr/bin/ok-player

# Libraries the application reaches through dlopen, so no ELF entry exists for
# rpm's automatic dependency extraction to find. Each one must be absent before
# the package is installed and present afterwards: absent first proves the
# environment is not masking the package's closure, present after proves the
# package's own Requires pulled it in.
DLOPEN_RUNTIME_SONAMES=(libGLESv2.so.2)

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

# The launch harness is a hard requirement: a host that cannot run a headless
# desktop session must fail loudly rather than pass by not launching anything.
for tool in dnf rpm ldconfig Xvfb xauth mcookie flock dbus-run-session gdbus \
  xfwm4 xdotool xwininfo xprop import magick ffmpeg ffprobe rg stat python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool for the installed-RPM launch gate: $tool" >&2
    exit 127
  }
done

RPM_PATH="$(resolve_rpm "$RPM_INPUT")"
echo "Installed-RPM launch gate: $RPM_PATH"

if rpm -q ok-player >/dev/null 2>&1; then
  echo "ok-player is already installed in this root; the launch gate needs a clean root" >&2
  exit 1
fi

for soname in "${DLOPEN_RUNTIME_SONAMES[@]}"; do
  if soname_present "$soname"; then
    echo "Launch root already provides $soname before ok-player is installed." >&2
    echo "This gate can no longer prove the RPM's declared dependency closure covers it." >&2
    echo "Remove whatever pulls $soname from the launch environment; it must arrive with the package." >&2
    exit 1
  fi
  echo "Pre-install: $soname is absent, as this gate requires"
done

# Only used by scripts/tests/rpm-installed-launch-gate.Tests.sh, which exercises
# the guards above without a Fedora root or a display.
if [[ "${OKP_RPM_LAUNCH_GATE_SELFTEST:-0}" == "1" ]]; then
  echo "selftest: pre-install guards passed"
  exit 0
fi

dnf install -y "$RPM_PATH"

test -x "$INSTALLED_BINARY"

for soname in "${DLOPEN_RUNTIME_SONAMES[@]}"; do
  if ! soname_present "$soname"; then
    echo "ok-player is installed but $soname is still missing." >&2
    echo "The application dlopens it during startup, so its declared Requires are incomplete." >&2
    rpm -q --requires ok-player >&2
    exit 1
  fi
  echo "Post-install: $soname arrived with the package's dependency closure"
done

echo "Launching the installed RPM binary: $INSTALLED_BINARY"
OKP_MAIN_WINDOW_IDLE_ONLY=1 \
  "$ROOT/scripts/smoke-linux-main-window.sh" \
  "$INSTALLED_BINARY" "$EVIDENCE_DIR"

echo "Installed-RPM launch gate passed. Evidence: $EVIDENCE_DIR"
