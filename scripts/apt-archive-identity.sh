#!/usr/bin/env bash
# The identity of the OK Player APT archive, shared by the two sides that have to agree about
# it: scripts/build-apt-repo.sh, which publishes the archive, and scripts/package-linux-deb.sh,
# which now ships a source stanza and the archive key inside every .deb (issue #726).
#
# Until #726 only the archive had an opinion about any of this, and a .deb downloaded from the
# releases page installed a player that could never update itself: the machine ended up with
# OK Player installed and no OK Player apt source at all, which is what the operator measured
# in #725 (`apt-cache policy ok-player` listing nothing but /var/lib/dpkg/status). A package
# that provisions the repository has to name the same URI, the same suite vocabulary, the same
# component and architecture, the same keyring path, and above all the same key as the archive
# it points at. One definition is what makes that true by construction instead of by review.
#
# Sourced, never executed.

OKP_APT_STABLE_SUITE='stable'
OKP_APT_CANDIDATE_SUITE='candidate'
OKP_APT_COMPONENT='main'
OKP_APT_ARCH='amd64'
OKP_APT_KEYRING_BASENAME='ok-player-archive-keyring'

# Where a client keeps the two files. `Signed-By` names an absolute path, so the archive's own
# stanza and the one the package installs must agree on it down to the character.
OKP_APT_KEYRING_DIR='/usr/share/keyrings'
OKP_APT_SOURCES_DIR='/etc/apt/sources.list.d'
OKP_APT_SOURCES_BASENAME='ok-player.sources'

# Where the .deb carries its copies before postinst puts them in place. They are ordinary
# packaged files under /usr/share, so the provisioning needs no network at install time — the
# whole point of #726, since a package that had to fetch a key would fail exactly on the
# machines that are offline or behind a proxy.
OKP_APT_CARRIED_DIR='/usr/share/ok-player/apt'

# The published archive. Overridable only so the container gate can point a real package at a
# real archive it built locally; production packages get this value.
OKP_APT_BASE_URL_DEFAULT='https://befeast.github.io/ok-player/apt'

# The key the archive is signed with. scripts/build-apt-repo.sh refuses to sign with a key
# that is not trusted here, and scripts/package-linux-deb.sh refuses to ship a keyring that
# does not carry exactly the trusted set. Either check alone would let a rotation move one
# side and not the other, publishing an archive no installed keyring can verify.
OKP_APT_SIGNING_FINGERPRINT='77D0FCDEB0D594E13E50F43A9337815EB0F78C63'

# Keys the shipped keyring carries besides the signer, and which the archive may therefore be
# signed with. Empty in steady state; it exists because a rotation cannot be a single change.
#
# apt has to verify `InRelease` before it can download anything, so a client that does not
# already trust the new key cannot fetch the package that would give it that key: signing with
# a key nobody trusts yet strands every installed machine with no way to self-heal. A rotation
# is therefore three changes, in this order, each published before the next:
#
#   1. Add the new fingerprint here and the new public key to the committed keyring, then
#      publish that package to BOTH suites — a linux-v* release for `stable` and an accepted
#      candidate for `candidate`. The suites are deliberately separate (#689), so a release
#      alone never reaches candidate subscribers, and they are the testers most likely to be
#      the first to see a broken archive. Clients then trust both keys; the archive is still
#      signed by the old one.
#   2. Once that keyring has reached the installs on both suites, move the Infisical secrets to
#      the new key and make it OKP_APT_SIGNING_FINGERPRINT, leaving the old fingerprint here.
#   3. When no supported install can still be carrying only the old key, drop it from here and
#      from the committed keyring.
#
# See docs/apt-repository.md.
OKP_APT_ADDITIONAL_TRUSTED_FINGERPRINTS=''

# Every fingerprint a client is expected to trust, newest first, one per line.
okp_apt_trusted_fingerprints() {
  printf '%s\n' "$OKP_APT_SIGNING_FINGERPRINT"
  local fingerprint
  for fingerprint in $OKP_APT_ADDITIONAL_TRUSTED_FINGERPRINTS; do
    printf '%s\n' "$fingerprint"
  done
}

# The committed armored public key, relative to the repository root. Armored on purpose: it is
# reviewable in a diff, and the packaging dearmors it at build time into the binary keyring apt
# wants at Signed-By.
OKP_APT_PUBLIC_KEY_RELATIVE='rust/packaging/linux/ok-player-archive-keyring.asc'

# One deb822 stanza. Both callers go through this, so the file the package installs is byte for
# byte the file the archive publishes for the same suite.
okp_apt_write_sources_stanza() {
  local destination="$1" base_url="$2" suite="$3"
  {
    printf 'Types: deb\n'
    printf 'URIs: %s\n' "$base_url"
    printf 'Suites: %s\n' "$suite"
    printf 'Components: %s\n' "$OKP_APT_COMPONENT"
    printf 'Architectures: %s\n' "$OKP_APT_ARCH"
    printf 'Signed-By: %s/%s.gpg\n' "$OKP_APT_KEYRING_DIR" "$OKP_APT_KEYRING_BASENAME"
  } >"$destination"
}

# The primary key fingerprints in an armored or dearmored OpenPGP public key file, one per
# line, or nothing if the file is not one. gpg is asked in --with-colons form so the answer is
# parsed rather than read out of prose that changes between gpg versions, and only the
# fingerprint that follows a `pub` record is taken — the others belong to subkeys.
okp_apt_key_fingerprints() {
  gpg --batch --no-tty --show-keys --with-colons "$1" 2>/dev/null \
    | awk -F: '$1 == "pub" { primary = 1; next } $1 == "fpr" && primary { print $10; primary = 0 }'
}
