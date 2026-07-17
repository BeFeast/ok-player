#!/usr/bin/env bash
# Build and exercise the Fedora native RPM in clean mock chroots. This script
# intentionally uses a privileged disposable container because mock needs mount
# namespaces; it never alters the host package database.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FEDORA_RELEASE="${1:-44}"
OUT_DIR="${2:-$ROOT/artifacts/linux/fedora-$FEDORA_RELEASE}"

case "$FEDORA_RELEASE" in
  43|44) ;;
  *) echo "supported Fedora releases are 43 and 44" >&2; exit 2 ;;
esac

command -v docker >/dev/null 2>&1 || { echo "Missing required tool: docker" >&2; exit 127; }
mkdir -p "$OUT_DIR"

docker run --rm --privileged \
  -e OKP_HOST_UID="$(id -u)" \
  -e OKP_HOST_GID="$(id -g)" \
  -e OKP_FEDORA_RELEASE="$FEDORA_RELEASE" \
  -v "$ROOT:/workspace" \
  -w /workspace \
  "fedora:$FEDORA_RELEASE" \
  bash -lc '
    set -euo pipefail
    dnf -y --setopt=install_weak_deps=False \
      install cargo git mock rpm-build rpmlint tar xz >/dev/null
    git config --global --add safe.directory /workspace

    result_root="/workspace/artifacts/linux/fedora-${OKP_FEDORA_RELEASE}"
    source_dir="${result_root}/source"
    old_dir="${result_root}/previous"
    current_dir="${result_root}/current"
    mkdir -p "$source_dir" "$old_dir" "$current_dir"

    ./scripts/package-linux-rpm.sh \
      --rpm-release 0.2.beta.1 \
      --out "$source_dir"
    srpm="$(find "$source_dir" -maxdepth 1 -name "*.src.rpm" -print -quit)"

    mock -r "fedora-${OKP_FEDORA_RELEASE}-x86_64" \
      --resultdir "$old_dir" \
      --rpmbuild-opts="--define okp_rpm_release 0.1.beta.1" \
      --rebuild "$srpm"
    mock -r "fedora-${OKP_FEDORA_RELEASE}-x86_64" \
      --resultdir "$current_dir" \
      --rpmbuild-opts="--define okp_rpm_release 0.2.beta.1" \
      --rebuild "$srpm"

    set +e
    rpmlint "$srpm" "$current_dir"/*.rpm | tee "$result_root/rpmlint.txt"
    rpmlint_status=${PIPESTATUS[0]}
    set -e
    if grep -Eq "[1-9][0-9]* errors" "$result_root/rpmlint.txt"; then
      echo "rpmlint reported errors" >&2
      exit 1
    fi
    if [[ "$rpmlint_status" -ne 0 ]]; then
      echo "rpmlint returned $rpmlint_status with warnings only; accepted severity is zero errors" >&2
    fi

    old_rpm="$(find "$old_dir" -maxdepth 1 -name "ok-player-*.x86_64.rpm" \
      ! -name "*-debuginfo-*" ! -name "*-debugsource-*" -print -quit)"
    current_rpm="$(find "$current_dir" -maxdepth 1 -name "ok-player-*.x86_64.rpm" \
      ! -name "*-debuginfo-*" ! -name "*-debugsource-*" -print -quit)"
    test -n "$old_rpm" && test -n "$current_rpm"

    # The minimal Fedora container image defaults to tsflags=nodocs. Override it
    # so the lifecycle check verifies the shipped third-party notices too.
    dnf -y --setopt=tsflags= install "$old_rpm" >/dev/null
    config_file="${XDG_CONFIG_HOME:-${HOME}/.config}/ok-player/settings.json"
    install -D -m0644 /dev/null "$config_file"
    printf "%s\n" "{\"version\":1,\"marker\":\"preserve-me\"}" \
      > "$config_file"
    dnf -y --setopt=tsflags= upgrade "$current_rpm" >/dev/null

    test -x /usr/libexec/ok-player/ok-player
    test -L /usr/bin/ok-player
    test -f /usr/share/applications/com.befeast.okplayer.desktop
    test -f /usr/share/metainfo/com.befeast.okplayer.metainfo.xml
    test -f /usr/share/doc/ok-player/THIRD-PARTY-NOTICES.md
    desktop-file-validate /usr/share/applications/com.befeast.okplayer.desktop
    appstreamcli validate --pedantic --no-color \
      /usr/share/metainfo/com.befeast.okplayer.metainfo.xml
    grep -q preserve-me "$config_file"

    dnf -y remove ok-player >/dev/null
    test ! -e /usr/libexec/ok-player/ok-player
    test ! -e /usr/share/metainfo/com.befeast.okplayer.metainfo.xml
    grep -q preserve-me "$config_file"

    chown -R "${OKP_HOST_UID}:${OKP_HOST_GID}" "$result_root"
  '

echo "Fedora $FEDORA_RELEASE RPM build and lifecycle acceptance passed: $OUT_DIR"
