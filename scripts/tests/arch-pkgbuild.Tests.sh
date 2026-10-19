#!/usr/bin/env bash
# The Arch recipe's checkout mode stamps the package with what it builds.
#
# `_okp_source=/path makepkg` clones a working tree instead of the pinned
# release tag. Such a build must not claim to be the pinned release: About
# would report the wrong version and the updater would compare the wrong
# installed version against the feeds. The recipe derives both the pacman
# pkgver and the app's OKP_BUILD_VERSION from the checkout's nearest linux-v*
# tag and the commits since it - or, since releases are tagged off the main
# line, from the newest tag the clone knows and its own commit count. This
# proves that, and that the tag mode is untouched, by sourcing the PKGBUILD the
# way makepkg does and running its functions against scratch repositories.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PKGBUILD="$ROOT/rust/packaging/arch/PKGBUILD"
WORK="$(mktemp -d -t okp-arch-pkgbuild-XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
failures=0

fail() { printf 'FAIL: %s\n' "$*" >&2; failures=$((failures + 1)); }
ok() { printf 'ok: %s\n' "$*"; }

# A repository with a release tag and two commits after it.
REPO="$WORK/checkout"
git init -q "$REPO"
git -C "$REPO" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "release"
git -C "$REPO" tag linux-v0.1.0-linux-alpha.112
git -C "$REPO" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "one"
git -C "$REPO" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "two"
HEAD_SHA="$(git -C "$REPO" rev-parse --short=7 HEAD)"
TAG_SHA="$(git -C "$REPO" rev-parse --short=7 linux-v0.1.0-linux-alpha.112)"

# What makepkg would have after extracting the source: the clone under srcdir.
srcdir="$WORK/src"
mkdir -p "$srcdir"
git clone -q "$REPO" "$srcdir/ok-player-checkout"

# --- checkout mode, two commits past the tag --------------------------------
got="$(
  cd "$srcdir"
  _okp_source="$REPO" source "$PKGBUILD"
  declare -F pkgver >/dev/null || { echo "no-pkgver-function"; exit 0; }
  pkgver
)"
expected="0.1.0linuxalpha.112.r2.g$HEAD_SHA"
[[ "$got" == "$expected" ]] && ok "a checkout two commits past the tag is pkgver $expected" || fail "pkgver for a checkout past the tag: got '$got', want '$expected'"

got="$(
  cd "$srcdir"
  _okp_source="$REPO" source "$PKGBUILD"
  _build_env >/dev/null 2>&1 || true
  printf '%s' "${OKP_BUILD_VERSION:-unset}"
)"
expected="0.1.0-linux-alpha.112+2.g$HEAD_SHA"
[[ "$got" == "$expected" ]] && ok "the app is stamped $expected" || fail "OKP_BUILD_VERSION for a checkout past the tag: got '$got', want '$expected'"

# --- checkout mode, exactly at the tag: the release itself --------------------
git -C "$srcdir/ok-player-checkout" checkout -q linux-v0.1.0-linux-alpha.112
got="$(
  cd "$srcdir"
  _okp_source="$REPO" source "$PKGBUILD"
  pkgver 2>/dev/null || echo "pkgver-failed"
)"
[[ "$got" == "0.1.0linuxalpha.112" ]] && ok "a checkout at the tag is the release's pkgver" || fail "pkgver at the tag: got '$got', want '0.1.0linuxalpha.112'"
got="$(
  cd "$srcdir"
  _okp_source="$REPO" source "$PKGBUILD"
  _build_env >/dev/null 2>&1 || true
  printf '%s' "${OKP_BUILD_VERSION:-unset}"
)"
[[ "$got" == "0.1.0-linux-alpha.112" ]] && ok "a checkout at the tag is stamped as the release ($TAG_SHA)" || fail "OKP_BUILD_VERSION at the tag: got '$got', want '0.1.0-linux-alpha.112'"

# --- checkout mode with the release tags off the main line (the real repo) ----
# Releases are tagged on their own commits, so main never has one in reach;
# the clone still carries the tags, and the newest names the base.
OFFLINE="$WORK/offline"
git init -q -b main "$OFFLINE"
git -C "$OFFLINE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "main one"
git -C "$OFFLINE" checkout -q --orphan release-line
git -C "$OFFLINE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "release 111"
git -C "$OFFLINE" tag linux-v0.1.0-linux-alpha.111
git -C "$OFFLINE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "release 112"
git -C "$OFFLINE" tag linux-v0.1.0-linux-alpha.112
git -C "$OFFLINE" checkout -q main
git -C "$OFFLINE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "main two"
git -C "$OFFLINE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "main three"
OFFLINE_SHA="$(git -C "$OFFLINE" rev-parse --short=7 HEAD)"
srcdir3="$WORK/src3"
mkdir -p "$srcdir3"
git clone -q "$OFFLINE" "$srcdir3/ok-player-checkout"
got="$(
  cd "$srcdir3"
  _okp_source="$OFFLINE" source "$PKGBUILD"
  pkgver 2>/dev/null || echo "pkgver-failed"
)"
expected="0.1.0linuxalpha.112.r3.g$OFFLINE_SHA"
[[ "$got" == "$expected" ]] && ok "a main checkout with the release tags off its line is pkgver $expected" || fail "pkgver with tags off the line: got '$got', want '$expected'"
got="$(
  cd "$srcdir3"
  srcdir="$srcdir3"
  _okp_source="$OFFLINE" source "$PKGBUILD"
  _build_env >/dev/null 2>&1 || true
  printf '%s' "${OKP_BUILD_VERSION:-unset}"
)"
expected="0.1.0-linux-alpha.112+3.g$OFFLINE_SHA"
[[ "$got" == "$expected" ]] && ok "the app is stamped $expected from the newest tag the clone knows" || fail "OKP_BUILD_VERSION with tags off the line: got '$got', want '$expected'"

# --- checkout mode with no release tag at all: the crate's version, or refused -
UNTAGGED="$WORK/untagged"
git init -q "$UNTAGGED"
mkdir -p "$UNTAGGED/rust/crates/okp-linux-gtk"
printf '[package]\nname = "okp-linux-gtk"\nversion = "0.1.0"\n' > "$UNTAGGED/rust/crates/okp-linux-gtk/Cargo.toml"
git -C "$UNTAGGED" add -A
git -C "$UNTAGGED" -c user.name=t -c user.email=t@t commit -q -m "orphan"
UNTAGGED_SHA="$(git -C "$UNTAGGED" rev-parse --short=7 HEAD)"
srcdir2="$WORK/src2"
mkdir -p "$srcdir2"
git clone -q "$UNTAGGED" "$srcdir2/ok-player-checkout"
got="$(
  cd "$srcdir2"
  _okp_source="$UNTAGGED" source "$PKGBUILD"
  pkgver 2>/dev/null || echo "pkgver-failed"
)"
expected="0.1.0.r1.g$UNTAGGED_SHA"
[[ "$got" == "$expected" ]] && ok "a clone with no linux-v tag is stamped from the crate version: $expected" || fail "pkgver with no tags: got '$got', want '$expected'"
BARE="$WORK/bare"
git init -q "$BARE"
git -C "$BARE" -c user.name=t -c user.email=t@t commit -q --allow-empty -m "nothing"
srcdir4="$WORK/src4"
mkdir -p "$srcdir4"
git clone -q "$BARE" "$srcdir4/ok-player-checkout"
if (cd "$srcdir4" && _okp_source="$BARE" source "$PKGBUILD" && pkgver >/dev/null 2>&1); then
  fail "a clone with neither a linux-v tag nor a crate version must refuse to be versioned"
else
  ok "a clone with neither a linux-v tag nor a crate version is refused"
fi

# --- the default is the checkout the PKGBUILD sits in --------------------------
got="$(
  cd "$srcdir"
  source "$PKGBUILD"
  declare -F pkgver >/dev/null && fn=stamped || fn=static
  printf '%s %s %s' "$_srcname" "$_okp_source" "$fn"
)"
expected="ok-player-checkout $(git -C "$ROOT" rev-parse --show-toplevel) stamped"
[[ "$got" == "$expected" ]] && ok "with nothing set the recipe builds the checkout it sits in" || fail "default mode: got '$got', want '$expected'"

# --- a release is asked for by name, pinned statically, and fetched by tag ----
got="$(
  cd "$srcdir"
  _okp_release=0.1.0-linux-alpha.112 source "$PKGBUILD"
  declare -F pkgver >/dev/null && fn=stamped || fn=static
  _build_env >/dev/null 2>&1 || true
  printf '%s %s %s %s' "$fn" "$pkgver" "${source[0]##*#}" "${OKP_BUILD_VERSION:-unset}"
)"
expected="static 0.1.0linuxalpha.112 tag=linux-v0.1.0-linux-alpha.112 0.1.0-linux-alpha.112"
[[ "$got" == "$expected" ]] && ok "release mode pins the named release statically and stamps it" || fail "release mode: got '$got', want '$expected'"

# --- a source without the lane is refused before anything is built ------------
LANELESS="$WORK/laneless"; mkdir -p "$LANELESS/rust/packaging/linux"
if (cd "$LANELESS" && _okp_release=0.1.0-linux-alpha.112 source "$PKGBUILD" && _okp_require_arch_lane 2>/dev/null); then
  fail "a release whose source lacks rust/packaging/arch/PKGBUILD must be refused"
else
  ok "a release that predates the lane is refused"
fi
WITHLANE="$WORK/withlane"; mkdir -p "$WITHLANE/rust/packaging/arch"; : > "$WITHLANE/rust/packaging/arch/PKGBUILD"
if (cd "$WITHLANE" && source "$PKGBUILD" && _okp_require_arch_lane); then
  ok "a source that carries the lane passes the guard"
else
  fail "a source carrying rust/packaging/arch/PKGBUILD must pass the guard"
fi

if (( failures > 0 )); then
  echo "arch-pkgbuild: $failures failure(s)" >&2
  exit 1
fi
echo "ok: the Arch recipe stamps a checkout with what it builds and the tag with the release"
