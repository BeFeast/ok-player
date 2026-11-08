#!/usr/bin/env bash
# The macOS app's code-signing identity: a stable self-signed certificate when one is
# configured, ad hoc otherwise.
#
#   macos-signing-identity.sh import DIR       print OKP_* lines for $GITHUB_ENV
#   macos-signing-identity.sh sign PATH...     sign with the identity import chose
#   macos-signing-identity.sh cleanup KEYCHAIN delete the temporary keychain
#
# An ad-hoc signature is the hash of the binary, so every build is a new app to macOS
# and privacy grants (Downloads, Desktop, network volumes, local network) are asked
# for again after each install. A certificate gives every build the same designated
# requirement, `identifier "<bundle id>" and certificate leaf = H"<SHA-1>"`, so the
# grants survive an update.
#
# import reads the identity from two optional environment variables:
#   OKP_MACOS_SIGNING_P12_BASE64    base64 of a PKCS#12 file holding the certificate and key
#   OKP_MACOS_SIGNING_P12_PASSWORD  its password
# With both set it creates a keychain under DIR, imports the identity so codesign can
# use it without a prompt, adds the keychain to the user search list and prints
#   OKP_CODESIGN_IDENTITY=<certificate SHA-1>
#   OKP_CODESIGN_KEYCHAIN=<keychain path>
#   OKP_SIGNING_MODE=self-signed
# With neither set it prints OKP_CODESIGN_IDENTITY=- and OKP_SIGNING_MODE=ad-hoc. One
# without the other is an error: a half-configured secret must not fall back quietly.
# security(1) reports the certificate as not trusted, which is expected for a
# self-signed one; codesign signs with it by hash all the same.
#
# sign reads OKP_CODESIGN_IDENTITY (default "-", ad hoc) and OKP_CODESIGN_KEYCHAIN.
# Nothing here prints the certificate, its password or the keychain password.
set -Eeuo pipefail

import_identity() {
  local dir="$1"
  local p12="${OKP_MACOS_SIGNING_P12_BASE64:-}" password="${OKP_MACOS_SIGNING_P12_PASSWORD:-}"
  if [[ -z "$p12" && -z "$password" ]]; then
    printf 'OKP_CODESIGN_IDENTITY=-\nOKP_SIGNING_MODE=ad-hoc\n'
    return 0
  fi
  if [[ -z "$p12" || -z "$password" ]]; then
    echo "macos-signing-identity: set both OKP_MACOS_SIGNING_P12_BASE64 and OKP_MACOS_SIGNING_P12_PASSWORD, or neither" >&2
    return 1
  fi
  mkdir -p "$dir"
  local keychain="$dir/ok-player-signing.keychain-db" keychain_password file identity
  keychain_password="$(uuidgen)"
  security create-keychain -p "$keychain_password" "$keychain"
  security set-keychain-settings -lut 21600 "$keychain"
  security unlock-keychain -p "$keychain_password" "$keychain"
  file="$(mktemp "$dir/identity.XXXXXX")"
  # shellcheck disable=SC2064 # expand now: the trap must remove this exact file
  trap "rm -f -- '$file'" EXIT
  printf '%s' "$p12" | base64 --decode > "$file"
  security import "$file" -k "$keychain" -P "$password" -f pkcs12 -T /usr/bin/codesign >/dev/null
  rm -f -- "$file"
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
  # codesign finds identities through the user search list; keep what is already on it.
  local searched=()
  while IFS= read -r entry; do
    entry="${entry#"${entry%%[![:space:]]*}"}"
    entry="${entry#\"}"
    searched+=("${entry%\"}")
  done < <(security list-keychains -d user)
  security list-keychains -d user -s "$keychain" "${searched[@]}"
  identity="$(security find-identity -p codesigning "$keychain" | awk '$1 ~ /^[0-9]+\)$/ && $2 ~ /^[0-9A-F]{40}$/ {print $2; exit}')"
  if [[ -z "$identity" ]]; then
    echo "macos-signing-identity: the PKCS#12 file holds no code-signing identity" >&2
    return 1
  fi
  printf 'OKP_CODESIGN_IDENTITY=%s\nOKP_CODESIGN_KEYCHAIN=%s\nOKP_SIGNING_MODE=self-signed\n' "$identity" "$keychain"
}

sign() {
  local identity="${OKP_CODESIGN_IDENTITY:--}" keychain="${OKP_CODESIGN_KEYCHAIN:-}"
  if [[ "$identity" != - && -n "$keychain" ]]; then
    codesign --force --sign "$identity" --keychain "$keychain" "$@"
  else
    codesign --force --sign "$identity" "$@"
  fi
}

cleanup() {
  local keychain="${1:-}"
  if [[ -n "$keychain" && -f "$keychain" ]]; then
    security delete-keychain "$keychain"
  fi
}

command="${1:-}"
shift || true
case "$command" in
  import) [[ $# -eq 1 ]] || { echo "usage: $0 import DIR" >&2; exit 2; }; import_identity "$1" ;;
  sign) [[ $# -ge 1 ]] || { echo "usage: $0 sign PATH..." >&2; exit 2; }; sign "$@" ;;
  cleanup) cleanup "${1:-}" ;;
  *) echo "usage: $0 import DIR | sign PATH... | cleanup KEYCHAIN" >&2; exit 2 ;;
esac
