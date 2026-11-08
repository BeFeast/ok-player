#!/usr/bin/env bash
# macOS only. Two builds signed through scripts/macos-signing-identity.sh with one
# certificate carry the same designated requirement; ad hoc they do not.
#
# The real certificate is a CI secret this test never sees. It makes a throwaway one
# the way macos/README.md tells the operator to make the real one, so the recipe, the
# keychain import and the signing path are exercised on every run.
set -Eeuo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
identity_script="$root/scripts/macos-signing-identity.sh"
work="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/okp-signing-test.XXXXXX")"
keychain=""
cleanup() {
  test -z "$keychain" || bash "$identity_script" cleanup "$keychain" || true
  rm -rf -- "$work"
}
trap cleanup EXIT
failures=0
fail() { echo "FAIL: $*"; failures=$((failures + 1)); }
pass() { echo "PASS: $*"; }

# Two "builds": app bundles with the same bundle id and different executables.
make_build() {
  local name="$1" exit_code="$2" app="$work/$1/OK Player.app"
  mkdir -p "$app/Contents/MacOS"
  printf 'int main(void) { return %s; }\n' "$exit_code" > "$work/$name.c"
  xcrun clang -o "$app/Contents/MacOS/ok-player" "$work/$name.c"
  cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>ok-player</string>
<key>CFBundleIdentifier</key><string>uk.oklabs.ok-player</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
PLIST
  printf '%s\n' "$app"
}
requirement() { codesign -d -r- "$1" 2>&1 | sed -n 's/^designated => //p'; }

# Without a secret the build signs ad hoc, and says so.
output="$(env -u OKP_MACOS_SIGNING_P12_BASE64 -u OKP_MACOS_SIGNING_P12_PASSWORD bash "$identity_script" import "$work/none")"
if grep -qx 'OKP_SIGNING_MODE=ad-hoc' <<<"$output" && grep -qx 'OKP_CODESIGN_IDENTITY=-' <<<"$output"; then
  pass "no secret: ad hoc"
else
  fail "no secret did not choose ad hoc: $output"
fi

# Half a secret is refused rather than silently signing ad hoc.
if OKP_MACOS_SIGNING_P12_BASE64=Zm9v bash "$identity_script" import "$work/half" >/dev/null 2>&1; then
  fail "a certificate without its password was accepted"
else
  pass "half-configured secret refused"
fi

# Negative control: two ad-hoc builds do not share a requirement.
first="$(make_build adhoc-1 0)"
second="$(make_build adhoc-2 1)"
OKP_CODESIGN_IDENTITY=- bash "$identity_script" sign "$first" "$second"
if [[ "$(requirement "$first")" != "$(requirement "$second")" ]]; then
  pass "ad hoc: requirements differ ($(requirement "$first"))"
else
  fail "two different ad-hoc builds share a requirement, so the comparison below proves nothing"
fi

# The documented recipe for the certificate and its PKCS#12 file.
cat > "$work/cert.cnf" <<'CNF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = OK Player test signing
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
CNF
openssl req -x509 -newkey rsa:2048 -nodes -days 1 -config "$work/cert.cnf" \
  -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
password="$(uuidgen)"
openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" -name "OK Player test signing" \
  -out "$work/identity.p12" -passout "pass:$password" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1
fingerprint="$(openssl x509 -in "$work/cert.pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g')"

output="$(OKP_MACOS_SIGNING_P12_BASE64="$(base64 < "$work/identity.p12" | tr -d '\n')" \
  OKP_MACOS_SIGNING_P12_PASSWORD="$password" bash "$identity_script" import "$work/keychain")"
identity="$(sed -n 's/^OKP_CODESIGN_IDENTITY=//p' <<<"$output")"
keychain="$(sed -n 's/^OKP_CODESIGN_KEYCHAIN=//p' <<<"$output")"
if grep -qx 'OKP_SIGNING_MODE=self-signed' <<<"$output" && [[ "$identity" == "$fingerprint" ]]; then
  pass "secret: identity $identity imported"
else
  fail "import did not select the certificate $fingerprint: $output"
fi

first="$(make_build signed-1 0)"
second="$(make_build signed-2 1)"
OKP_CODESIGN_IDENTITY="$identity" OKP_CODESIGN_KEYCHAIN="$keychain" bash "$identity_script" sign "$first" "$second"
lower="$(tr '[:upper:]' '[:lower:]' <<<"$fingerprint")"
expected="identifier \"uk.oklabs.ok-player\" and certificate leaf = H\"$lower\""
for app in "$first" "$second"; do
  echo "${app#"$work"/}: $(requirement "$app")"
  codesign --verify --deep --strict "$app" || fail "${app#"$work"/} does not verify"
done
if [[ "$(requirement "$first")" == "$expected" && "$(requirement "$second")" == "$expected" ]]; then
  pass "two builds share the designated requirement: $expected"
else
  fail "designated requirements differ from: $expected"
fi

bash "$identity_script" cleanup "$keychain"
if test -e "$keychain" || security list-keychains -d user | grep -qF "$keychain"; then
  fail "cleanup left the keychain behind"
else
  pass "cleanup removed the keychain"
fi
keychain=""

echo "$failures failure(s)"
test "$failures" -eq 0
