#!/usr/bin/env bash
# The one encoding of a build version into the string a package manager compares (issue #709).
#
# OK Player names its builds the way semver does — `0.11.0-beta.0.209` for a candidate,
# `0.11.0` for the release it leads to. That string is what About reports, what the update
# feeds carry, and what the release artifacts are named by. It is *not* a Debian version.
#
# dpkg reads everything after the last `-` as the Debian *revision*, and a version that has
# one sorts ABOVE the same version without one. Measured with dpkg 1.23.7:
#
#   0.11.0-beta.0.208   gt  0.11.0                 -> TRUE
#
# So every `.deb` published as `0.11.0-beta.0.N` already sits above the `0.11.0` release that
# is supposed to follow it, and `apt upgrade` would never install that release: the APT lane
# could not ship a stable version to anyone at all. Debian's construct for "comes before the
# release" is `~`, which sorts below the empty string:
#
#   0.11.0~beta.0.209   lt  0.11.0                 -> TRUE
#
# but adopting `~` on its own strands every tester already subscribed to the candidate suite,
# because the corrected string now sorts *below* what they have installed:
#
#   0.11.0-beta.0.208   lt  0.11.0~beta.0.209      -> false   (apt refuses it as a downgrade)
#
# The one construct that outranks a version already on a machine, whatever that version looks
# like, is an epoch. So every Debian version this repository emits carries epoch 1:
#
#   0.11.0-beta.0.208   lt  1:0.11.0~beta.0.209    -> TRUE   (the stranded tester moves)
#   1:0.11.0~beta.0.9   lt  1:0.11.0~beta.0.10     -> TRUE   (candidates still order)
#   1:0.11.0~beta.0.209 lt  1:0.11.0               -> TRUE   (the release outranks them all)
#
# The epoch is permanent: removing it later would strand the same people again. It is also the
# only one this project should ever need — the `~` fixes the ordering itself, so no future
# release has to reach for a second epoch to climb over its own prereleases.
#
# The rule, in full:
#
#   Debian version = "1:" + the build version with every `-` replaced by `~`
#   rpm version    =        the build version with every `-` replaced by `~`
#
# rpm forbids `-` in a version outright and already read `~` correctly, so the rpm lane never
# had the ordering defect and needs no epoch; it shares the substitution so the two lanes
# cannot drift apart.
#
# Every `-` is replaced rather than only the first, so an encoded version can never carry a
# Debian revision at all — `0.1.0-linux-alpha.112` becomes `1:0.1.0~linux~alpha.112`, whose
# whole string is the upstream version. And because a build version may not contain `~` or `:`
# (semver has no use for either, and the guard below refuses them), the substitution is a
# bijection: `okp_build_version_from_debian` recovers exactly the build version a package was
# made from. That is what lets the installed-build watch (#707/#708) compare `dpkg-query`'s
# answer against the version this session is running without inventing a mapping, and it is
# why the two directions live in one file rather than one in the packaging and one in a shell.
#
# `okp_core::package_version` holds the same rule for the shells that read a version back.
#
# The artifact file name is deliberately *not* encoded: `.deb` files stay
# `ok-player_0.11.0-beta.0.209_amd64.deb`, named by the build, because the release assets, the
# candidate pointer, the AppImage beside them and the update feeds all name that same build,
# and a Debian pool file name never had to match the version inside it. One identity for the
# artifact, one encoding inside the control file, and one function mapping between them.

# The epoch every Debian version this repository emits carries. Permanent (see above).
OKP_DEBIAN_VERSION_EPOCH=1

# The newest `.deb` published before the epoch existed. Everything at or below it is a version
# from the old scheme that a rebuilt archive may still carry; anything above it must be an
# encoded version, or the ordering defect is back. Used by scripts/verify-apt-repo.sh.
OKP_DEBIAN_LEGACY_HIGHWATER='0.11.0-beta.0.208'

# The packages published before the encoding existed, read from the live archive on 2026-07-29.
# This is a closed list rather than a rule, because the set is closed: no lane can publish an
# unencoded version any more, so anything unencoded that is not one of these is a package from a
# regressed or unrecognised lane, not a survivor. A rule phrased as "unencoded prerelease at or
# below the high-water mark" admits versions that were never published at all — for example a raw
# 0.11.0-alpha.999, which dpkg happens to sort low — and those bypass every ordering assertion.
#
# The list can only shrink: entries age out of the rolling window and are deleted here. If the
# archive ever carries an unencoded version that is not listed, the verifier fails, which is the
# intended direction — a pre-encoding lane publishing again is exactly what must not pass quietly.
#
# One operational note, true only until this change merges: the candidate lane publishes an
# unencoded build on every push to main, so a candidate produced between the last refresh of this
# list and the merge would not be listed and would fail the archive rebuild. That failure is
# fail-closed — the previously deployed site keeps serving — and the fix is one line here. It
# cannot recur afterwards, because from the merge on every published build is encoded.
OKP_DEBIAN_PRE_ENCODING_VERSIONS=(
  '0.1.0-linux-alpha.103'
  '0.1.0-linux-alpha.104'
  '0.1.0-linux-alpha.105'
  '0.1.0-linux-alpha.106'
  '0.1.0-linux-alpha.107'
  '0.1.0-linux-alpha.108'
  '0.1.0-linux-alpha.109'
  '0.1.0-linux-alpha.110'
  '0.1.0-linux-alpha.111'
  '0.1.0-linux-alpha.112'
  '0.11.0-beta.0.184'
  '0.11.0-beta.0.185'
  '0.11.0-beta.0.187'
  '0.11.0-beta.0.193'
  '0.11.0-beta.0.197'
  '0.11.0-beta.0.208'
  '0.11.0-beta.0.210'
)

# Whether a build version is one of those published before the encoding.
okp_is_pre_encoding_version() {
  local candidate="${1-}" published
  for published in "${OKP_DEBIAN_PRE_ENCODING_VERSIONS[@]}"; do
    [[ "$candidate" == "$published" ]] && return 0
  done
  return 1
}

# A build version this packaging can encode reversibly.
#
# The refusals are the point: `~` and `:` are the two characters the encoding uses to mean
# something, so a build version carrying either would make the mapping ambiguous and the watch
# would read a version back that is not the one that was packaged. Refusing loudly at package
# time is the only place that can still be fixed.
okp_assert_build_version() {
  local version="${1-}"
  if [[ -z "$version" ]]; then
    printf 'package version: a build version is required.\n' >&2
    return 1
  fi
  if [[ "$version" == *'~'* || "$version" == *:* ]]; then
    printf 'package version: %s carries `~` or `:`, which this encoding reserves; a build version cannot use either.\n' \
      "$version" >&2
    return 1
  fi
  if [[ ! "$version" =~ ^[0-9][0-9A-Za-z.+-]*$ ]]; then
    printf 'package version: %s is not a build version this packaging can encode; it must start with a digit and hold only [0-9A-Za-z.+-].\n' \
      "$version" >&2
    return 1
  fi
  if [[ "$version" == *- ]]; then
    printf 'package version: %s ends in `-`; the encoded version would end in `~` and sort below its own release.\n' \
      "$version" >&2
    return 1
  fi
  return 0
}

# build version -> the `Version:` field of a `.deb`.
okp_debian_version_for_build() {
  local version="${1-}"
  okp_assert_build_version "$version" || return 1
  printf '%s:%s' "$OKP_DEBIAN_VERSION_EPOCH" "${version//-/\~}"
}

# build version -> the `Version:` field of an rpm. No epoch: see the header.
okp_rpm_version_for_build() {
  local version="${1-}"
  okp_assert_build_version "$version" || return 1
  printf '%s' "${version//-/\~}"
}

# The `Version:` field of a `.deb` -> the build version it was made from, or a refusal.
#
# Refused rather than guessed when the string is not one this packaging emits:
#
# * no epoch — a package from before the scheme, or from somebody else's archive;
# * a Debian revision — nothing here ever emits one, so the `-` is a rebuilder's, and the
#   build version behind it is unknowable. This is exactly the shape whose two comparators
#   disagree (`1.0.0` vs `1.0.0-1`: dpkg says newer, the semver-shaped ordering says older),
#   so refusing it is what keeps one comparator in charge.
okp_build_version_from_debian() {
  local version="${1-}" upstream build
  case "$version" in
    "$OKP_DEBIAN_VERSION_EPOCH":*) upstream="${version#*:}" ;;
    *)
      printf 'package version: %s does not carry the epoch this packaging stamps (%s:); refusing to guess which build it was made from.\n' \
        "$version" "$OKP_DEBIAN_VERSION_EPOCH" >&2
      return 1
      ;;
  esac
  if [[ "$upstream" == *-* ]]; then
    printf 'package version: %s carries a Debian revision, which this packaging never emits; the build behind it is unknown.\n' \
      "$version" >&2
    return 1
  fi
  build="${upstream//\~/-}"
  okp_assert_build_version "$build" || return 1
  printf '%s' "$build"
}
