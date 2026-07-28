#!/usr/bin/env bash
# Build the signed APT repository served from the static GitHub Pages site (issue #683),
# so Debian/Ubuntu users receive OK Player updates through their own system updater.
#
# Output layout (served at https://befeast.github.io/ok-player/apt/):
#   apt/ok-player-archive-keyring.asc          armored public signing key (users may keep .asc)
#   apt/ok-player-archive-keyring.gpg          the same key dearmored, for /usr/share/keyrings
#   apt/ok-player.sources                      ready-made deb822 source stanza
#   apt/pool/main/o/ok-player/*.deb            the packages themselves
#   apt/dists/stable/Release                   the archive index
#   apt/dists/stable/{InRelease,Release.gpg}   the OpenPGP signatures over it
#   apt/dists/stable/main/binary-amd64/{Packages,Packages.gz,Release}
#
# Derived, never authored — the same rule build-win-feed.sh and build-linux-feed.sh follow.
# The published linux-v* GitHub releases are the source of truth, so this script is a pure
# function of them plus the signing key:
#
#   * Additive. A new release only ever adds a pool file and a Packages paragraph. Older
#     retained versions keep the exact bytes, and therefore the exact Size/SHA256, they had
#     in the previous run — a release can never rewrite the checksum of a version already
#     installed on someone's machine.
#   * Idempotent. Re-running over an unchanged release set reproduces the repository content
#     byte for byte: pool contents come from immutable release assets, the Packages ordering
#     is dpkg-scanpackages' stable version ordering, Packages.gz is written with `gzip -n`,
#     and Release's Date is taken from the newest retained release's publication timestamp
#     rather than from the clock. The detached OpenPGP signatures are deliberately NOT
#     byte-stable: every signature carries its own creation time, and dating a signature from
#     an old release just to make bytes repeat would be a lie about when it was made.
#   * Bounded. GitHub Pages publishes at most ~1 GB per site, and Linux packages that bundle
#     libmpv are ~85 MB each, so the archive is a rolling window: the newest
#     OKP_APT_MAX_VERSIONS releases that also fit inside OKP_APT_POOL_BUDGET_BYTES. Dropping
#     the tail removes the pool file and its Packages paragraph together, so the archive never
#     carries a dangling reference; versions outside the window stay downloadable from their
#     GitHub release, which is what the .deb self-updater already uses.
#
# The signing key is fetched at run time from Infisical (never a GitHub Actions secret, never
# an artifact) into an ephemeral GNUPGHOME that is destroyed on exit including on failure, the
# imported key's fingerprint is compared against the expected one before anything is signed,
# and a missing or unreadable secret aborts the lane. Publishing an unsigned or
# unexpectedly-signed archive is worse than publishing nothing: apt would either refuse it or,
# worse, trust it.
#
# Usage: build-apt-repo.sh <owner/repo> <output-dir>
# Requires: gh (authenticated; GH_TOKEN in CI), jq, curl, gpg, dpkg-scanpackages (dpkg-dev),
#           gzip, sha256sum, md5sum.
# Requires in the environment: INFISICAL_CLIENT_ID, INFISICAL_CLIENT_SECRET.
set -euo pipefail

# --- Archive identity -----------------------------------------------------------------
OKP_APT_ORIGIN='OK Player'
OKP_APT_LABEL='OK Player'
OKP_APT_SUITE='stable'
OKP_APT_COMPONENT='main'
OKP_APT_ARCH='amd64'
OKP_APT_POOL_SUBDIR='pool/main/o/ok-player'
OKP_APT_KEYRING_BASENAME='ok-player-archive-keyring'

# --- Rolling-window bounds (see the header) -------------------------------------------
OKP_APT_MAX_VERSIONS="${OKP_APT_MAX_VERSIONS:-10}"
OKP_APT_POOL_BUDGET_BYTES="${OKP_APT_POOL_BUDGET_BYTES:-$((900 * 1024 * 1024))}"

# --- Infisical coordinates ------------------------------------------------------------
# These are NOT secrets (issue #683) and belong in the workflow surface; only the two client
# credentials come from GitHub Actions secrets.
OKP_APT_INFISICAL_API='https://secrets.oklabs.uk/api'
OKP_APT_INFISICAL_PROJECT='5b5038c7-46d5-496f-bfa6-6184cb41e143'
OKP_APT_INFISICAL_ENVIRONMENT='prod'
OKP_APT_INFISICAL_SECRET_PATH='/ok-player'

# The real stdout, saved before anything can capture it. ::add-mask:: only takes effect when
# GitHub Actions actually sees it, and every masked value is produced inside a command
# substitution whose stdout belongs to the caller — so masks are written here instead.
exec {OKP_APT_LOG_FD}>&1

okp_apt_fail() {
  printf '::error::%s\n' "$*" >&2
  exit 1
}

okp_apt_secret_reference() {
  printf 'services/%s%s:%s' \
    "$OKP_APT_INFISICAL_ENVIRONMENT" "$OKP_APT_INFISICAL_SECRET_PATH" "$1"
}

okp_apt_require_tools() {
  local tool
  for tool in "$@"; do
    command -v "$tool" >/dev/null 2>&1 \
      || okp_apt_fail "the APT repository lane requires ${tool}, which is not on PATH."
  done
}

# --- Infisical ------------------------------------------------------------------------
# The access token and the secret values never appear in a command line: curl reads the
# request from a --config stanza on stdin, so nothing sensitive lands in the process table
# of a shared runner. Failure bodies are never echoed either, because an auth response can
# carry a token.
OKP_APT_INFISICAL_TOKEN=''

okp_apt_infisical_login() {
  local client_id="${INFISICAL_CLIENT_ID:-}" client_secret="${INFISICAL_CLIENT_SECRET:-}"
  [[ -n "$client_id" ]] \
    || okp_apt_fail "INFISICAL_CLIENT_ID is empty; the APT lane cannot read $(okp_apt_secret_reference gpg-private-key) and refuses to publish an unsigned repository."
  [[ -n "$client_secret" ]] \
    || okp_apt_fail "INFISICAL_CLIENT_SECRET is empty; the APT lane cannot read $(okp_apt_secret_reference gpg-private-key) and refuses to publish an unsigned repository."

  local body status token
  body="$(mktemp)"
  status="$(
    jq -nc --arg clientId "$client_id" --arg clientSecret "$client_secret" \
      '{clientId: $clientId, clientSecret: $clientSecret}' \
      | curl --config <(
          printf 'silent\nshow-error\nrequest = "POST"\nurl = "%s"\nheader = "Content-Type: application/json"\ndata-binary = "@-"\noutput = "%s"\nwrite-out = "%%{http_code}"\nmax-time = 30\n' \
            "${OKP_APT_INFISICAL_API}/v1/auth/universal-auth/login" "$body"
        )
  )" || { rm -f "$body"; okp_apt_fail "Infisical universal-auth login to ${OKP_APT_INFISICAL_API} failed at the transport level."; }
  if [[ "$status" != 200 ]]; then
    rm -f "$body"
    okp_apt_fail "Infisical universal-auth login to ${OKP_APT_INFISICAL_API} returned HTTP ${status}; the APT lane cannot read $(okp_apt_secret_reference gpg-private-key)."
  fi
  token="$(jq -r '.accessToken // empty' <"$body")"
  rm -f "$body"
  [[ -n "$token" ]] \
    || okp_apt_fail "Infisical universal-auth login to ${OKP_APT_INFISICAL_API} returned no accessToken."
  okp_apt_mask "$token"
  OKP_APT_INFISICAL_TOKEN="$token"
}

# Hide a value from GitHub Actions logs. ::add-mask:: is line-oriented, so an armored key is
# masked line by line; the directive itself is only emitted under Actions, where it is not
# echoed back.
okp_apt_mask() {
  [[ -n "${GITHUB_ACTIONS:-}" ]] || return 0
  local line
  while IFS= read -r line; do
    [[ -n "$line" ]] || continue
    printf '::add-mask::%s\n' "$line" >&"$OKP_APT_LOG_FD"
  done <<<"$1"
}

# The production secret reader: prints one secret value on stdout, non-zero on any failure.
# main() pins this into okp_apt_build_signed_repo; the tests pin a stub in its place, so the
# "which secret is missing" message below is produced by the same production code path in
# both cases.
okp_apt_infisical_secret() {
  local name="$1" body status value
  [[ -n "$OKP_APT_INFISICAL_TOKEN" ]] || okp_apt_infisical_login
  body="$(mktemp)"
  status="$(
    curl --config <(
      printf 'silent\nshow-error\nurl = "%s"\nheader = "Authorization: Bearer %s"\noutput = "%s"\nwrite-out = "%%{http_code}"\nmax-time = 30\n' \
        "${OKP_APT_INFISICAL_API}/v3/secrets/raw/${name}?workspaceId=${OKP_APT_INFISICAL_PROJECT}&environment=${OKP_APT_INFISICAL_ENVIRONMENT}&secretPath=$(okp_apt_urlencode "$OKP_APT_INFISICAL_SECRET_PATH")" \
        "$OKP_APT_INFISICAL_TOKEN" "$body"
    )
  )" || { rm -f "$body"; return 1; }
  if [[ "$status" != 200 ]]; then
    printf 'Infisical returned HTTP %s for %s\n' "$status" "$(okp_apt_secret_reference "$name")" >&2
    rm -f "$body"
    return 1
  fi
  value="$(jq -r '.secret.secretValue // empty' <"$body")"
  rm -f "$body"
  [[ -n "$value" ]] || return 1
  okp_apt_mask "$value"
  printf '%s' "$value"
}

okp_apt_urlencode() {
  local string="$1" out='' index char
  for ((index = 0; index < ${#string}; index++)); do
    char="${string:index:1}"
    case "$char" in
      [a-zA-Z0-9._~-]) out+="$char" ;;
      *) out+="$(printf '%%%02X' "'$char")" ;;
    esac
  done
  printf '%s' "$out"
}

# Read one required secret into a file, or abort naming exactly which secret is missing.
# Deliberately not a command substitution at the call site: an abort has to end the lane,
# not just a subshell.
okp_apt_require_secret() {
  local reader="$1" name="$2" destination="$3" value
  if ! value="$("$reader" "$name")" || [[ -z "$value" ]]; then
    okp_apt_fail "cannot read $(okp_apt_secret_reference "$name"); the APT lane refuses to publish an unsigned repository."
  fi
  printf '%s\n' "$value" >"$destination"
  chmod 600 "$destination"
}

# --- Ephemeral GNUPGHOME --------------------------------------------------------------
# One cleanup handler for the whole script: the signing home and the download staging
# directory are registered in globals rather than in competing traps, so entering the signed
# build can never disarm the staging cleanup (or the other way round). It runs on success, on
# failure, and on a signal.
OKP_APT_GNUPGHOME=''
OKP_APT_STAGING=''

okp_apt_cleanup() {
  okp_apt_destroy_gnupghome "$OKP_APT_GNUPGHOME"
  OKP_APT_GNUPGHOME=''
  if [[ -n "$OKP_APT_STAGING" ]]; then
    rm -rf -- "$OKP_APT_STAGING"
    OKP_APT_STAGING=''
  fi
}

okp_apt_make_gnupghome() {
  local base="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
  local home
  home="$(mktemp -d "${base%/}/okp-apt-gnupg.XXXXXXXX")"
  chmod 700 "$home"
  printf '%s' "$home"
}

okp_apt_destroy_gnupghome() {
  local home="${1:-}"
  [[ -n "$home" && -d "$home" ]] || return 0
  GNUPGHOME="$home" gpgconf --kill all >/dev/null 2>&1 || true
  rm -rf -- "$home"
}

okp_apt_normalize_fingerprint() {
  printf '%s' "${1//[[:space:]]/}" | tr '[:lower:]' '[:upper:]'
}

# Import the private key and prove it is the intended one. Only public listings are read —
# there is no --list-secret-keys style dump anywhere in this script.
okp_apt_import_signing_key() {
  local home="$1" key_file="$2" expected_file="$3" imported expected
  GNUPGHOME="$home" gpg --batch --quiet --no-tty --import "$key_file" >/dev/null 2>&1 \
    || okp_apt_fail "the value of $(okp_apt_secret_reference gpg-private-key) is not an importable OpenPGP key."
  imported="$(
    GNUPGHOME="$home" gpg --batch --no-tty --with-colons --fingerprint --list-keys 2>/dev/null \
      | awk -F: '$1 == "fpr" { print $10; exit }'
  )"
  imported="$(okp_apt_normalize_fingerprint "$imported")"
  expected="$(okp_apt_normalize_fingerprint "$(cat "$expected_file")")"
  [[ -n "$imported" ]] \
    || okp_apt_fail "no OpenPGP key was imported from $(okp_apt_secret_reference gpg-private-key)."
  [[ -n "$expected" ]] \
    || okp_apt_fail "$(okp_apt_secret_reference gpg-fingerprint) is empty; the imported signing key cannot be verified."
  if [[ "$imported" != "$expected" ]]; then
    okp_apt_fail "signing key fingerprint mismatch: imported ${imported}, but $(okp_apt_secret_reference gpg-fingerprint) expects ${expected}. Refusing to sign the APT repository with an unexpected key."
  fi
  printf '%s' "$imported"
}

# --- Repository generation (pure) -----------------------------------------------------
# Turn a directory of .deb files into a complete unsigned archive under <repo-root>.
# <epoch> dates the Release file, so the output depends only on the inputs.
okp_apt_generate_repo() {
  local staging="$1" repo_root="$2" epoch="$3"
  local pool="${repo_root}/${OKP_APT_POOL_SUBDIR}"
  local dist="${repo_root}/dists/${OKP_APT_SUITE}"
  local binary_dir="${dist}/${OKP_APT_COMPONENT}/binary-${OKP_APT_ARCH}"

  local packages=()
  local deb
  while IFS= read -r -d '' deb; do packages+=("$deb"); done \
    < <(find "$staging" -maxdepth 1 -name '*.deb' -type f -print0 | sort -z)
  ((${#packages[@]} > 0)) \
    || okp_apt_fail "no .deb reached the APT staging directory; refusing to publish an empty archive."

  rm -rf -- "${repo_root}/dists" "${repo_root}/pool"
  mkdir -p "$pool" "$binary_dir"
  for deb in "${packages[@]}"; do
    cp -- "$deb" "${pool}/$(basename -- "$deb")"
  done

  # An explicit override file pins Section/Priority and keeps dpkg-scanpackages from
  # warning about every package on every run.
  local override diagnostics
  override="$(mktemp)"
  diagnostics="$(mktemp)"
  printf 'ok-player optional video\n' >"$override"
  if ! (
    cd "$repo_root"
    LC_ALL=C dpkg-scanpackages --multiversion "$OKP_APT_POOL_SUBDIR" "$override"
  ) >"${binary_dir}/Packages" 2>"$diagnostics"; then
    cat "$diagnostics" >&2
    rm -f "$override" "$diagnostics"
    okp_apt_fail "dpkg-scanpackages could not index the APT pool."
  fi
  rm -f "$override" "$diagnostics"
  [[ -s "${binary_dir}/Packages" ]] \
    || okp_apt_fail "the generated Packages index is empty; refusing to publish an archive apt cannot resolve."
  # -n: no timestamp or original name in the gzip header, so the byte stream repeats.
  gzip -9nc "${binary_dir}/Packages" >"${binary_dir}/Packages.gz"

  {
    printf 'Archive: %s\n' "$OKP_APT_SUITE"
    printf 'Component: %s\n' "$OKP_APT_COMPONENT"
    printf 'Origin: %s\n' "$OKP_APT_ORIGIN"
    printf 'Label: %s\n' "$OKP_APT_LABEL"
    printf 'Architecture: %s\n' "$OKP_APT_ARCH"
  } >"${binary_dir}/Release"

  local indexed=(
    "${OKP_APT_COMPONENT}/binary-${OKP_APT_ARCH}/Packages"
    "${OKP_APT_COMPONENT}/binary-${OKP_APT_ARCH}/Packages.gz"
    "${OKP_APT_COMPONENT}/binary-${OKP_APT_ARCH}/Release"
  )
  {
    printf 'Origin: %s\n' "$OKP_APT_ORIGIN"
    printf 'Label: %s\n' "$OKP_APT_LABEL"
    printf 'Suite: %s\n' "$OKP_APT_SUITE"
    printf 'Codename: %s\n' "$OKP_APT_SUITE"
    printf 'Architectures: %s\n' "$OKP_APT_ARCH"
    printf 'Components: %s\n' "$OKP_APT_COMPONENT"
    printf 'Date: %s\n' "$(LC_ALL=C date -u -d "@${epoch}" '+%a, %d %b %Y %H:%M:%S UTC')"
    # No Valid-Until on purpose. It would bound a freeze/rollback attack, but it also makes apt
    # reject the archive outright once it lapses — and OK Player releases on no schedule, so any
    # window short enough to be worth having is short enough to strand every user during a quiet
    # month. The archive is served over HTTPS from GitHub Pages and regenerated on every deploy;
    # revisit this if the release cadence ever becomes predictable.
    printf 'Acquire-By-Hash: no\n'
    printf 'Description: OK Player Debian packages (%s)\n' "$OKP_APT_ARCH"
    printf 'MD5Sum:\n'
    okp_apt_checksum_block "$dist" md5sum "${indexed[@]}"
    printf 'SHA256:\n'
    okp_apt_checksum_block "$dist" sha256sum "${indexed[@]}"
  } >"${dist}/Release"
}

okp_apt_checksum_block() {
  local dist="$1" algorithm="$2"
  shift 2
  local relative size digest
  for relative in "$@"; do
    size="$(stat -c '%s' "${dist}/${relative}")"
    digest="$("$algorithm" "${dist}/${relative}" | cut -d' ' -f1)"
    printf ' %s %16s %s\n' "$digest" "$size" "$relative"
  done
}

# --- Signing --------------------------------------------------------------------------
# The passphrase arrives on file descriptor 3 from a here-string: bash materializes it
# without forking a process, so it never appears in argv or in the process table.
okp_apt_sign_repo() {
  local repo_root="$1" home="$2" fingerprint="$3" passphrase="$4"
  local dist="${repo_root}/dists/${OKP_APT_SUITE}"
  GNUPGHOME="$home" gpg --batch --yes --no-tty --pinentry-mode loopback --passphrase-fd 3 \
    --local-user "$fingerprint" --digest-algo SHA512 --armor --detach-sign \
    --output "${dist}/Release.gpg" "${dist}/Release" 3<<<"$passphrase" \
    || okp_apt_fail "detached signing of the APT Release file failed."
  GNUPGHOME="$home" gpg --batch --yes --no-tty --pinentry-mode loopback --passphrase-fd 3 \
    --local-user "$fingerprint" --digest-algo SHA512 --clearsign \
    --output "${dist}/InRelease" "${dist}/Release" 3<<<"$passphrase" \
    || okp_apt_fail "inline signing of the APT InRelease file failed."
}

# --- Client-facing material -----------------------------------------------------------
okp_apt_write_client_material() {
  local repo_root="$1" home="$2" public_key_file="$3" base_url="$4"
  cp -- "$public_key_file" "${repo_root}/${OKP_APT_KEYRING_BASENAME}.asc"
  chmod 644 "${repo_root}/${OKP_APT_KEYRING_BASENAME}.asc"
  GNUPGHOME="$home" gpg --batch --yes --no-tty --dearmor \
    --output "${repo_root}/${OKP_APT_KEYRING_BASENAME}.gpg" \
    <"${repo_root}/${OKP_APT_KEYRING_BASENAME}.asc" \
    || okp_apt_fail "the value of $(okp_apt_secret_reference gpg-public-key) is not an armored OpenPGP key."
  chmod 644 "${repo_root}/${OKP_APT_KEYRING_BASENAME}.gpg"
  {
    printf 'Types: deb\n'
    printf 'URIs: %s\n' "$base_url"
    printf 'Suites: %s\n' "$OKP_APT_SUITE"
    printf 'Components: %s\n' "$OKP_APT_COMPONENT"
    printf 'Architectures: %s\n' "$OKP_APT_ARCH"
    printf 'Signed-By: /usr/share/keyrings/%s.gpg\n' "$OKP_APT_KEYRING_BASENAME"
  } >"${repo_root}/ok-player.sources"
}

# --- The whole signed build, with the secret source as a parameter ---------------------
# Everything that can abort the lane — a missing secret, an unexpected fingerprint — runs
# here, so the tests exercise the production control flow rather than a copy of it.
okp_apt_build_signed_repo() {
  local staging="$1" repo_root="$2" epoch="$3" base_url="$4" reader="$5"
  local home
  home="$(okp_apt_make_gnupghome)"
  OKP_APT_GNUPGHOME="$home"
  trap okp_apt_cleanup EXIT INT TERM

  okp_apt_require_secret "$reader" gpg-private-key "${home}/signing-key.asc"
  okp_apt_require_secret "$reader" gpg-public-key "${home}/public-key.asc"
  okp_apt_require_secret "$reader" gpg-fingerprint "${home}/fingerprint"
  okp_apt_require_secret "$reader" gpg-passphrase "${home}/passphrase"

  local fingerprint
  fingerprint="$(okp_apt_import_signing_key "$home" "${home}/signing-key.asc" "${home}/fingerprint")"
  printf 'APT archive will be signed by %s\n' "$fingerprint"

  okp_apt_generate_repo "$staging" "$repo_root" "$epoch"
  okp_apt_sign_repo "$repo_root" "$home" "$fingerprint" "$(cat "${home}/passphrase")"
  okp_apt_write_client_material "$repo_root" "$home" "${home}/public-key.asc" "$base_url"

  okp_apt_destroy_gnupghome "$home"
  OKP_APT_GNUPGHOME=''
}

# --- Release discovery ----------------------------------------------------------------
# Candidate releases, newest publication first: every published linux-v* release that actually
# carries a .deb, as "<published_at>\t<tag>\t<deb count>\t<total deb bytes>".
okp_apt_fetch_release_candidates() {
  local repo="$1" candidates
  candidates="$(gh api --paginate "repos/${repo}/releases?per_page=100" \
    --jq '.[] | select((.draft | not) and .published_at != null)
              | select(.tag_name | test("^linux-v"))
              | [ .published_at, .tag_name,
                  ([.assets[] | select(.name | test("^ok-player_.*_amd64\\.deb$"))] | length),
                  ([.assets[] | select(.name | test("^ok-player_.*_amd64\\.deb$")) | .size] | add // 0)
                ] | @tsv' \
    | awk -F'\t' '$3 > 0' | sort -r)"
  [[ -n "$candidates" ]] \
    || okp_apt_fail "no published linux-v* release carries an ok-player_*_amd64.deb; refusing to build an APT repository from nothing (issue #683)."
  printf '%s\n' "$candidates"
}

# Apply the rolling window to candidates on stdin, newest first.
#
# The current release is not optional. Skipping it because it does not fit and keeping older,
# smaller ones would leave the archive advertising a version older than the one the JSON feeds
# advertise — a stale-but-signed repository, which is a worse failure than no publish at all
# because apt clients would accept it. So a current release that cannot fit aborts the lane
# instead of falling through to its predecessors. "Newest" is publication order, the same
# ordering build-linux-feed.sh uses to pick the release its manifests describe.
okp_apt_apply_window() {
  local kept=0 total=0 published tag size newest_candidate first=1
  while IFS=$'\t' read -r published tag _ size; do
    [[ -n "$tag" ]] || continue
    newest_candidate=$first
    first=0
    if ((kept >= OKP_APT_MAX_VERSIONS)); then
      ((newest_candidate == 0)) \
        || okp_apt_fail "OKP_APT_MAX_VERSIONS is ${OKP_APT_MAX_VERSIONS}, so the archive cannot carry the current release ${tag}; refusing to publish an APT repository without it."
      printf 'APT window: %s is outside the newest %s releases; still downloadable from its GitHub release.\n' \
        "$tag" "$OKP_APT_MAX_VERSIONS" >&2
      continue
    fi
    if ((total + size > OKP_APT_POOL_BUDGET_BYTES)); then
      ((newest_candidate == 0)) \
        || okp_apt_fail "the current release ${tag} needs ${size} bytes, which does not fit the ${OKP_APT_POOL_BUDGET_BYTES}-byte APT pool budget; refusing to publish an archive that would advertise an older version than the update feeds do."
      printf 'APT window: %s does not fit the %s-byte pool budget; still downloadable from its GitHub release.\n' \
        "$tag" "$OKP_APT_POOL_BUDGET_BYTES" >&2
      continue
    fi
    total=$((total + size))
    kept=$((kept + 1))
    printf '%s\t%s\n' "$published" "$tag"
  done

  ((kept > 0)) \
    || okp_apt_fail "the rolling window retained no release; refusing to publish an empty APT repository."
}

okp_apt_select_releases() {
  okp_apt_fetch_release_candidates "$1" | okp_apt_apply_window
}

main() {
  local repo="${1:?usage: build-apt-repo.sh <owner/repo> <output-dir>}"
  local out="${2:?usage: build-apt-repo.sh <owner/repo> <output-dir>}"

  okp_apt_require_tools gh jq curl gpg gpgconf dpkg-scanpackages gzip sha256sum md5sum stat

  # Authenticate before spending minutes on downloads: a lane that cannot reach the signing
  # key must fail immediately, not after building an archive it is not allowed to sign.
  okp_apt_infisical_login

  local selected
  selected="$(okp_apt_select_releases "$repo")"

  local staging
  staging="$(mktemp -d)"
  OKP_APT_STAGING="$staging"
  trap okp_apt_cleanup EXIT INT TERM

  local newest_published='' published tag
  while IFS=$'\t' read -r published tag; do
    [[ -n "$tag" ]] || continue
    [[ -n "$newest_published" ]] || newest_published="$published"
    gh release download "$tag" --repo "$repo" --dir "$staging" \
      --pattern 'ok-player_*_amd64.deb' --clobber \
      || okp_apt_fail "could not download the .deb attached to ${tag}."
  done <<<"$selected"

  local epoch
  epoch="$(date -u -d "$newest_published" '+%s')"

  local owner="${repo%%/*}" name="${repo##*/}"
  local base_url="https://${owner,,}.github.io/${name}/apt"
  local repo_root="${out}/apt"
  mkdir -p "$repo_root"
  # Idempotent, and also staged by the two feed builders — but this script has to stand on its
  # own: without it a Jekyll pass could rewrite or hide archive files, and apt would be served
  # something other than what was signed.
  touch "${out}/.nojekyll"

  okp_apt_build_signed_repo "$staging" "$repo_root" "$epoch" "$base_url" okp_apt_infisical_secret

  printf 'APT archive built at %s from:\n' "$base_url"
  awk '/^Package:/ { package = $2 } /^Version:/ { printf "  %s %s\n", package, $2 }' \
    "${repo_root}/dists/${OKP_APT_SUITE}/${OKP_APT_COMPONENT}/binary-${OKP_APT_ARCH}/Packages"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
