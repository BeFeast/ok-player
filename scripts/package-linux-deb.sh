#!/usr/bin/env bash
set -euo pipefail

# candidate-required-tools: cargo chmod cp dpkg-deb gpg install ln mkdir rm

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export OKP_CANDIDATE_TOOLCHAIN_GATE_SCRIPTS="$ROOT/scripts/package-linux-deb.sh
$ROOT/scripts/collect-linux-bundled-mpv-runtime.sh
$ROOT/scripts/verify-linux-bundled-mpv.sh"
export OKP_CANDIDATE_TOOLCHAIN_REQUIRE_DOTNET_TOOLS=false
VERSION="${1:-0.1.0-linux-alpha.1}"
ARCH="${OKP_DEB_ARCH:-amd64}"
# The build version is what the binary stamps into itself, what About reports and what the
# artifact is named by. `Version:` needs the Debian encoding of it, because dpkg orders the
# raw build version wrongly against the release it precedes (issue #709). One rule, one file.
source "$ROOT/scripts/linux-package-version.sh"
DEB_VERSION="$(okp_debian_version_for_build "$VERSION")"
PACKAGE="ok-player"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/rust/target}"
DEB_DIR="$ROOT/artifacts/linux/deb"
BUILD_ROOT="$DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}"
ICON="$ROOT/rust/packaging/linux/com.befeast.okplayer.svg"
FIXED_ICONS="$ROOT/rust/packaging/linux/icons/hicolor"
DESKTOP="$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
METAINFO="$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

# --- Which archive this package provisions (issue #726) --------------------------------
# Until now a .deb installed a player that could never update itself: postinst refreshed the
# desktop and icon caches and nothing else, so the machine ended up with OK Player installed
# and no OK Player apt source. The suite has to match the artifact — a package cut from a
# linux-v* release subscribes the machine to `stable`, one from the rolling candidate lane to
# `candidate`. Getting that wrong is not cosmetic: a tester who installed a candidate .deb and
# was quietly put on `stable` would stop receiving the builds they installed it for.
#
# The default is `candidate` because that is the honest description of an undeclared build. A
# release is a deliberate act and declares itself — release-linux.yml sets this explicitly,
# and scripts/tests/deb-apt-provisioning.Tests.sh fails if that line ever disappears. Every
# other lane, including a local build, is a rolling build.
source "$ROOT/scripts/apt-archive-identity.sh"
APT_SUITE="${OKP_DEB_APT_SUITE:-$OKP_APT_CANDIDATE_SUITE}"
# The three below are overridable only so scripts/verify-deb-apt-provisioning.sh can point a
# real package at a real archive it built locally with a throwaway key. A production build
# takes the committed key and the published archive.
APT_BASE_URL="${OKP_DEB_APT_BASE_URL:-$OKP_APT_BASE_URL_DEFAULT}"
APT_PUBLIC_KEY="${OKP_DEB_APT_PUBLIC_KEY:-$ROOT/$OKP_APT_PUBLIC_KEY_RELATIVE}"
APT_EXPECT_FINGERPRINT="${OKP_DEB_APT_FINGERPRINT:-$OKP_APT_SIGNING_FINGERPRINT}"

case "$APT_SUITE" in
  "$OKP_APT_STABLE_SUITE" | "$OKP_APT_CANDIDATE_SUITE") ;;
  *)
    echo "OKP_DEB_APT_SUITE must be $OKP_APT_STABLE_SUITE or $OKP_APT_CANDIDATE_SUITE, not '$APT_SUITE'" >&2
    exit 2
    ;;
esac

source "$ROOT/scripts/linux-bundled-mpv-env.sh"
okp_use_linux_bundled_mpv package

OKP_BUILD_VERSION="$VERSION" OKP_PACKAGE_KIND=deb \
  cargo build --manifest-path "$ROOT/rust/Cargo.toml" -p okp-linux-gtk --release

rm -rf "$BUILD_ROOT"
mkdir -p "$BUILD_ROOT/DEBIAN"
mkdir -p "$BUILD_ROOT/usr/lib/ok-player"
mkdir -p "$BUILD_ROOT/usr/bin"
mkdir -p "$BUILD_ROOT/usr/share/applications"
mkdir -p "$BUILD_ROOT/usr/share/metainfo"
mkdir -p "$BUILD_ROOT/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$BUILD_ROOT$OKP_APT_CARRIED_DIR"

install -Dm755 "$TARGET_DIR/release/okp-linux-gtk" "$BUILD_ROOT/usr/lib/ok-player/ok-player"
cp -a "$OKP_BUNDLED_MPV_RUNTIME_DIR/." "$BUILD_ROOT/usr/lib/ok-player/"
ln -s ../lib/ok-player/ok-player "$BUILD_ROOT/usr/bin/ok-player"
install -Dm644 "$DESKTOP" "$BUILD_ROOT/usr/share/applications/com.befeast.okplayer.desktop"
install -Dm644 "$METAINFO" "$BUILD_ROOT/usr/share/metainfo/com.befeast.okplayer.metainfo.xml"
install -Dm644 "$ICON" "$BUILD_ROOT/usr/share/icons/hicolor/scalable/apps/com.befeast.okplayer.svg"
for size in 16 24 32 48 64; do
  install -Dm644 \
    "$FIXED_ICONS/${size}x${size}/apps/com.befeast.okplayer.svg" \
    "$BUILD_ROOT/usr/share/icons/hicolor/${size}x${size}/apps/com.befeast.okplayer.svg"
done

# GPLv3 §4 and LGPLv3 §4(b): the licence documents have to travel with the
# package. Debian policy §12.5 puts them in /usr/share/doc/<package>.
"$ROOT/scripts/stage-license-documents.sh" deb "$BUILD_ROOT/usr/share/doc/ok-player"
# --- The repository material the package carries (issue #726) --------------------------
# Both files travel inside the package, so provisioning needs no network at install time — a
# package that had to fetch a key would fail on exactly the machines that are offline or
# behind a proxy. The key is asserted before it is packaged: a package shipping some other key
# would install a keyring that cannot verify the archive its own stanza points at, which is a
# hard `apt-get update` failure on the user's machine, worse than the silence being fixed here
# and far harder to diagnose.
[[ -f "$APT_PUBLIC_KEY" ]] || {
  echo "APT archive public key not found: $APT_PUBLIC_KEY" >&2
  exit 1
}
APT_GNUPGHOME="$(mktemp -d)"
trap 'rm -rf -- "$APT_GNUPGHOME"' EXIT
chmod 700 "$APT_GNUPGHOME"
export GNUPGHOME="$APT_GNUPGHOME"

APT_ACTUAL_FINGERPRINT="$(okp_apt_key_fingerprint "$APT_PUBLIC_KEY")"
if [[ "$APT_ACTUAL_FINGERPRINT" != "$APT_EXPECT_FINGERPRINT" ]]; then
  echo "Refusing to ship an APT key the archive is not signed with." >&2
  echo "  key:      $APT_PUBLIC_KEY" >&2
  echo "  expected: $APT_EXPECT_FINGERPRINT" >&2
  echo "  found:    ${APT_ACTUAL_FINGERPRINT:-<not an OpenPGP public key>}" >&2
  exit 1
fi

gpg --batch --yes --no-tty --dearmor \
  --output "$BUILD_ROOT$OKP_APT_CARRIED_DIR/${OKP_APT_KEYRING_BASENAME}.gpg" \
  <"$APT_PUBLIC_KEY"
okp_apt_write_sources_stanza \
  "$BUILD_ROOT$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME" "$APT_BASE_URL" "$APT_SUITE"
chmod 644 \
  "$BUILD_ROOT$OKP_APT_CARRIED_DIR/${OKP_APT_KEYRING_BASENAME}.gpg" \
  "$BUILD_ROOT$OKP_APT_CARRIED_DIR/$OKP_APT_SOURCES_BASENAME"

"$ROOT/scripts/verify-linux-bundled-mpv.sh" \
  "$BUILD_ROOT/usr/lib/ok-player/ok-player" \
  "$BUILD_ROOT/usr/lib/ok-player"

cat > "$BUILD_ROOT/DEBIAN/control" <<CONTROL
Package: $PACKAGE
Version: $DEB_VERSION
Section: video
Priority: optional
Architecture: $ARCH
Maintainer: BeFeast <noreply@github.com>
Depends: libc6, libgcc-s1, libffi8, libdbus-1-3, libsystemd0, libudev1, libasound2 | libasound2t64, libpipewire-0.3-0t64 | libpipewire-0.3-0, libpulse0, libjack-jackd2-0 | libjack0, libwebp7, libwebpmux3, libpng16-16 | libpng16-16t64, libglib2.0-0 | libglib2.0-0t64, libgraphene-1.0-0, libgtk-4-1, libcairo2, libcairo-gobject2, libfontconfig1, libfreetype6, libfribidi0, libgdk-pixbuf-2.0-0, libharfbuzz0b, libpango-1.0-0, libpangocairo-1.0-0, libgl1, libegl1, libglx0, libglvnd0, libdrm2, libgbm1, libvulkan1, libwayland-client0, libwayland-cursor0, libwayland-egl1, libx11-6, libx11-xcb1, libxcursor1, libxext6, libxfixes3, libxi6, libxkbcommon0, libxpresent1, libxrandr2, libxss1, libxv1, libxcb1, libxcb-dri3-0, libxcb-shape0, libxcb-shm0, libxcb-xfixes0, libdecor-0-0
Recommends: ffmpeg
Homepage: https://github.com/BeFeast/ok-player
Description: Elegant mpv-based media player
 OK Player is a native desktop media player built over its packaged libmpv.
 This Linux package is an early GTK4/Rust alpha.
CONTROL

# The archive coordinates are written in as a prologue so the maintainer-script bodies below
# stay literal: nothing in them is expanded at build time, and every `$` they contain is the
# installing machine's.
apt_script_prologue() {
  printf '#!/bin/sh\n'
  printf 'set -e\n'
  printf '\n'
  printf "OKP_APT_BASE_URL='%s'\n" "$APT_BASE_URL"
  printf "OKP_APT_CARRIED='%s'\n" "$OKP_APT_CARRIED_DIR"
  printf "OKP_APT_KEYRING='%s/%s.gpg'\n" "$OKP_APT_KEYRING_DIR" "$OKP_APT_KEYRING_BASENAME"
  printf "OKP_APT_SOURCES='%s/%s'\n" "$OKP_APT_SOURCES_DIR" "$OKP_APT_SOURCES_BASENAME"
  printf '\n'
  cat <<'PROLOGUE'
root="${DPKG_ROOT:-}"

# Any configured source that names the OK Player archive — whichever file it
# lives in, and whatever it is called. Both maintainer scripts need the same
# answer: postinst must not overwrite a subscription the user chose, and postrm
# must not delete a keyring a surviving source still names. Commented-out lines
# are not sources.
okp_apt_source_configured() {
  for okp_candidate in \
    "$root/etc/apt/sources.list" \
    "$root"/etc/apt/sources.list.d/*.sources \
    "$root"/etc/apt/sources.list.d/*.list; do
    [ -f "$okp_candidate" ] || continue
    if grep -v '^[[:space:]]*#' "$okp_candidate" 2>/dev/null |
      grep -qF "$OKP_APT_BASE_URL"; then
      return 0
    fi
  done
  return 1
}

PROLOGUE
}

{
  apt_script_prologue
  cat <<'POSTINST'
# --- Provision the OK Player APT repository (issue #726) ---------------------
# Everything this needs travels inside the package, so it works on a machine
# with no network at all.
#
# An existing source is the user's own choice of suite and is left exactly as
# it is: a reinstall must not move a `candidate` subscriber back to `stable`.
# That is also why neither file is a dpkg conffile — dpkg compares a conffile
# against the md5 of what the *previous package* shipped, so an untouched
# stanza is replaced silently, which is precisely the move this has to
# prevent (Debian Policy 10.7.3 covers config files a maintainer script owns
# instead).

# The keyring is refreshed unconditionally. It is this package's copy of the key
# the archive is signed with, and a machine that missed a rotation would stop
# being able to verify the archive at all — a hard `apt-get update` failure
# rather than a missed update.
if [ -f "$root$OKP_APT_CARRIED/ok-player-archive-keyring.gpg" ]; then
  mkdir -p "$root${OKP_APT_KEYRING%/*}"
  cp -f "$root$OKP_APT_CARRIED/ok-player-archive-keyring.gpg" "$root$OKP_APT_KEYRING.dpkg-tmp"
  chmod 644 "$root$OKP_APT_KEYRING.dpkg-tmp"
  mv -f "$root$OKP_APT_KEYRING.dpkg-tmp" "$root$OKP_APT_KEYRING"
fi

if [ -f "$root$OKP_APT_CARRIED/ok-player.sources" ] && ! okp_apt_source_configured; then
  mkdir -p "$root${OKP_APT_SOURCES%/*}"
  cp -f "$root$OKP_APT_CARRIED/ok-player.sources" "$root$OKP_APT_SOURCES.dpkg-tmp"
  chmod 644 "$root$OKP_APT_SOURCES.dpkg-tmp"
  mv -f "$root$OKP_APT_SOURCES.dpkg-tmp" "$root$OKP_APT_SOURCES"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q "$root/usr/share/applications" || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$root/usr/share/icons/hicolor" || true
fi

exit 0
POSTINST
} > "$BUILD_ROOT/DEBIAN/postinst"

{
  apt_script_prologue
  cat <<'POSTRM'
# `purge` removes the repository this package configured; a plain `remove` leaves
# it, which is how apt-repository-shipping packages behave and is what lets a
# reinstall not have to re-add the source. postinst owns both files rather than
# dpkg, so postrm is what has to remove them.
if [ "$1" = purge ]; then
  rm -f "$root$OKP_APT_SOURCES"
  # The keyring is only this package's to remove while nothing else needs it. A
  # user who subscribed through a file of their own — ok-player-candidate.sources,
  # say — keeps a source whose `Signed-By` names this keyring, and taking it out
  # from under them breaks `apt update` for the whole machine rather than just
  # for OK Player.
  if ! okp_apt_source_configured; then
    rm -f "$root$OKP_APT_KEYRING"
  fi
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q "$root/usr/share/applications" || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f "$root/usr/share/icons/hicolor" || true
fi

exit 0
POSTRM
} > "$BUILD_ROOT/DEBIAN/postrm"

chmod 755 "$BUILD_ROOT/DEBIAN/postinst" "$BUILD_ROOT/DEBIAN/postrm"
chmod -R u+rwX,go+rX,go-w "$BUILD_ROOT"
dpkg-deb --root-owner-group --build "$BUILD_ROOT" "$DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}.deb"

echo "Debian package written to $DEB_DIR/${PACKAGE}_${VERSION}_${ARCH}.deb (Version: $DEB_VERSION)"
echo "It provisions $APT_BASE_URL suite $APT_SUITE, signed by $APT_EXPECT_FINGERPRINT."
echo "Run write-linux-acceptance-template.sh after both package lanes complete; publishing requires evidence for this exact artifact hash."
