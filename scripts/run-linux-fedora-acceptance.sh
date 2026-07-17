#!/usr/bin/env bash
# Fedora release-acceptance collector.
#
# Runs non-interactively inside a fresh Fedora KDE (Plasma) or Workstation
# (GNOME) VM after a clean snapshot restore. It gathers the host facts the
# acceptance contract needs — Fedora version/desktop, SELinux mode and AVC
# denials, renderer and VA-API capability — shapes them into the machine-
# readable Fedora acceptance manifest defined by okp_core::fedora_acceptance,
# and validates that manifest. Every pass/fail/blocked decision lives in the
# core validator, not in this script.
#
# The script never relaxes SELinux: it reads the current mode and collects
# denials. A permissive or disabled guest, or an unexplained AVC denial, is
# reported by the validator as a failure. Missing package artifacts are a
# blocked precondition, not a false pass.
#
# It embeds no private hostnames, addresses, or credentials, and AVC denial
# signatures are reduced to SELinux context fields so no host path can leak
# into the evidence bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  cat >&2 <<'USAGE'
usage: run-linux-fedora-acceptance.sh --state STATE --out DIR [options]

  --state STATE          one of: stock-repos | rpmfusion | flatpak | native-rpm | copr
  --out DIR              output directory for the evidence bundle
  --desktop DESKTOP      override auto-detect: workstation-gnome | kde-plasma
  --artifact-file PATH   package file under test (required for flatpak/native-rpm/copr)
  --codecs FILE          JSON array of codec_checks entries from the live playback steps
  --media FILE           JSON array of media_profiles entries from the live media steps
  --coverage FILE        JSON array of coverage entries from the live desktop steps
  --avc-justify FILE     JSON object mapping AVC signature -> justification string

The collected environment, SELinux, and GPU facts are always auto-detected.
STATE selects which repositories/package delivery are under test and whether a
package artifact hash is a hard precondition.
USAGE
}

STATE=""
OUT_DIR=""
DESKTOP_OVERRIDE=""
ARTIFACT_FILE=""
CODECS_FILE=""
MEDIA_FILE=""
COVERAGE_FILE=""
AVC_JUSTIFY_FILE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --state) STATE="${2:?}"; shift 2 ;;
    --out) OUT_DIR="${2:?}"; shift 2 ;;
    --desktop) DESKTOP_OVERRIDE="${2:?}"; shift 2 ;;
    --artifact-file) ARTIFACT_FILE="${2:?}"; shift 2 ;;
    --codecs) CODECS_FILE="${2:?}"; shift 2 ;;
    --media) MEDIA_FILE="${2:?}"; shift 2 ;;
    --coverage) COVERAGE_FILE="${2:?}"; shift 2 ;;
    --avc-justify) AVC_JUSTIFY_FILE="${2:?}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage; exit 2 ;;
  esac
done

if [[ -z "$STATE" || -z "$OUT_DIR" ]]; then
  usage
  exit 2
fi

case "$STATE" in
  stock-repos) STATE_JSON="stock-repos" ;;
  rpmfusion) STATE_JSON="rpm-fusion-codecs" ;;
  flatpak) STATE_JSON="flatpak-package" ;;
  native-rpm|copr) STATE_JSON="native-rpm-copr" ;;
  *) echo "invalid --state: $STATE" >&2; usage; exit 2 ;;
esac

command -v jq >/dev/null 2>&1 || { echo "Missing required tool: jq" >&2; exit 127; }

# Locate the evidence binary: an explicit override, one on PATH, or the built
# workspace binary.
EVIDENCE_BIN="${OKP_EVIDENCE_BIN:-}"
if [[ -z "$EVIDENCE_BIN" ]]; then
  if command -v okp-acceptance-evidence >/dev/null 2>&1; then
    EVIDENCE_BIN="okp-acceptance-evidence"
  elif [[ -x "$ROOT/rust/target/release/okp-acceptance-evidence" ]]; then
    EVIDENCE_BIN="$ROOT/rust/target/release/okp-acceptance-evidence"
  elif [[ -x "$ROOT/rust/target/debug/okp-acceptance-evidence" ]]; then
    EVIDENCE_BIN="$ROOT/rust/target/debug/okp-acceptance-evidence"
  else
    echo "Missing okp-acceptance-evidence binary; build okp-core or set OKP_EVIDENCE_BIN" >&2
    exit 127
  fi
fi

mkdir -p "$OUT_DIR"

# --- Environment ------------------------------------------------------------
FEDORA_VERSION="unknown"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  FEDORA_VERSION="$(. /etc/os-release; echo "${VERSION_ID:-unknown}")"
fi

detect_desktop() {
  if [[ -n "$DESKTOP_OVERRIDE" ]]; then
    echo "$DESKTOP_OVERRIDE"
    return
  fi
  local current="${XDG_CURRENT_DESKTOP:-}"
  case "${current^^}" in
    *KDE*|*PLASMA*) echo "kde-plasma" ;;
    *GNOME*) echo "workstation-gnome" ;;
    *) echo "workstation-gnome" ;;
  esac
}
DESKTOP="$(detect_desktop)"

SESSION="x11"
if [[ "${XDG_SESSION_TYPE:-}" == "wayland" || -n "${WAYLAND_DISPLAY:-}" ]]; then
  SESSION="wayland"
fi

# --- SELinux ----------------------------------------------------------------
SELINUX_MODE="disabled"
if command -v getenforce >/dev/null 2>&1; then
  case "$(getenforce 2>/dev/null || true)" in
    Enforcing) SELINUX_MODE="enforcing" ;;
    Permissive) SELINUX_MODE="permissive" ;;
    *) SELINUX_MODE="disabled" ;;
  esac
fi

# Collect AVC denials from this boot, reduced to SELinux context fields so no
# host path leaks into the bundle. Deduplicate by signature and count.
AVC_JSON="[]"
AVC_LINES="$(
  if command -v ausearch >/dev/null 2>&1; then
    ausearch -m AVC,USER_AVC -ts boot 2>/dev/null || true
  elif command -v journalctl >/dev/null 2>&1; then
    journalctl -b -g 'avc:  denied' -o cat 2>/dev/null || true
  fi
)"
if [[ -n "$AVC_LINES" ]]; then
  AVC_JSON="$(
    echo "$AVC_LINES" \
      | grep 'denied' \
      | sed -nE 's/.*(comm=[^ ]+).*(scontext=[^ ]+).*(tcontext=[^ ]+).*(tclass=[^ ]+).*/\1 \2 \3 \4/p' \
      | sort | uniq -c \
      | jq -Rn '[inputs
          | capture("^\\s*(?<count>\\d+)\\s+(?<sig>.+)$")
          | {signature: .sig, count: (.count | tonumber), justification: null}]'
  )"
  [[ -z "$AVC_JSON" ]] && AVC_JSON="[]"
fi

# Apply operator justifications for known-benign denials, if supplied.
if [[ -n "$AVC_JUSTIFY_FILE" && -r "$AVC_JUSTIFY_FILE" ]]; then
  AVC_JSON="$(jq --slurpfile j "$AVC_JUSTIFY_FILE" '
    map(.justification = ($j[0][.signature] // .justification))
  ' <<<"$AVC_JSON")"
fi

# --- GPU capability ---------------------------------------------------------
RENDERER="unknown"
if command -v glxinfo >/dev/null 2>&1; then
  RENDERER="$(glxinfo -B 2>/dev/null | sed -nE 's/.*OpenGL renderer string:[[:space:]]*(.+)/\1/p' | head -n1 || true)"
elif command -v eglinfo >/dev/null 2>&1; then
  RENDERER="$(eglinfo 2>/dev/null | sed -nE 's/.*OpenGL renderer string:[[:space:]]*(.+)/\1/p' | head -n1 || true)"
fi
[[ -z "$RENDERER" ]] && RENDERER="unknown"

VIRTUAL_GPU="true"
case "${RENDERER,,}" in
  *llvmpipe*|*softpipe*|*swrast*|*virgl*|*virtio*|*vmware*|*svga3d*|*software*) VIRTUAL_GPU="true" ;;
  unknown) VIRTUAL_GPU="true" ;;
  *) VIRTUAL_GPU="false" ;;
esac

VAAPI_AVAILABLE="false"
if command -v vainfo >/dev/null 2>&1; then
  if vainfo 2>/dev/null | grep -qE 'VAEntrypointVLD'; then
    VAAPI_AVAILABLE="true"
  fi
fi

# --- Artifact ---------------------------------------------------------------
ARTIFACT_JSON="null"
if [[ -n "$ARTIFACT_FILE" ]]; then
  if [[ ! -f "$ARTIFACT_FILE" ]]; then
    echo "Note: --artifact-file $ARTIFACT_FILE not found; leaving artifact unset (blocked precondition)" >&2
  else
    case "$STATE" in
      flatpak) ART_KIND="flatpak" ;;
      copr) ART_KIND="copr" ;;
      *) ART_KIND="rpm" ;;
    esac
    ARTIFACT_JSON="$("$EVIDENCE_BIN" fedora-artifact --kind "$ART_KIND" --file "$ARTIFACT_FILE")"
  fi
fi

# --- Optional live-step fragments ------------------------------------------
read_array() {
  local file="$1"
  if [[ -n "$file" && -r "$file" ]]; then
    jq -c 'if type == "array" then . else error("expected a JSON array") end' "$file"
  else
    echo "[]"
  fi
}
CODECS_JSON="$(read_array "$CODECS_FILE")"
MEDIA_FRAGMENT="$(read_array "$MEDIA_FILE")"
COVERAGE_FRAGMENT="$(read_array "$COVERAGE_FILE")"

# Seed every required coverage area not-run, then overlay any live results so an
# incomplete run is visible rather than silently short.
REQUIRED_AREAS='["install","update","removal","desktop-entry","mime-associations","app-stream","file-portals","drag-and-drop","screenshots","audio-pipe-wire","mpris","subtitles","stereo-downmix","settings-sizing","menus","window-dragging","window-geometry"]'
COVERAGE_JSON="$(jq -cn --argjson areas "$REQUIRED_AREAS" --argjson live "$COVERAGE_FRAGMENT" '
  ($live | map({key: .area, value: .}) | from_entries) as $byArea
  | $areas | map($byArea[.] // {area: ., status: "not-run", evidence: ""})
')"

# Default media profiles: low-resource must run; real-hardware is skipped with
# renderer capability evidence on a virtual GPU. Live results overlay these.
MEDIA_JSON="$(jq -cn \
  --argjson virtual "$VIRTUAL_GPU" \
  --arg renderer "$RENDERER" \
  --argjson live "$MEDIA_FRAGMENT" '
  ($live | map({key: .profile, value: .}) | from_entries) as $byProfile
  | [
      ($byProfile["low-resource"] // {profile:"low-resource", status:"not-run", evidence:""}),
      ($byProfile["real-hardware"] //
        (if $virtual
         then {profile:"real-hardware", status:"skipped", evidence:("GPU gates skipped: virtual renderer " + $renderer)}
         else {profile:"real-hardware", status:"not-run", evidence:""}
         end))
    ]
')"

# --- Assemble and validate --------------------------------------------------
MANIFEST="$OUT_DIR/fedora-acceptance-manifest.json"
jq -n \
  --argjson version 1 \
  --arg fedora "$FEDORA_VERSION" \
  --arg desktop "$DESKTOP" \
  --arg session "$SESSION" \
  --arg state "$STATE_JSON" \
  --argjson artifact "$ARTIFACT_JSON" \
  --arg selinux "$SELINUX_MODE" \
  --argjson denials "$AVC_JSON" \
  --arg renderer "$RENDERER" \
  --argjson virtual "$VIRTUAL_GPU" \
  --argjson vaapi "$VAAPI_AVAILABLE" \
  --argjson codecs "$CODECS_JSON" \
  --argjson media "$MEDIA_JSON" \
  --argjson coverage "$COVERAGE_JSON" '
  {
    schema_version: $version,
    environment: {fedora_version: $fedora, desktop: $desktop, session: $session},
    test_state: $state,
    artifact: $artifact,
    selinux: {mode: $selinux, denials: $denials},
    gpu: {renderer: $renderer, virtual_gpu: $virtual, vaapi_available: $vaapi},
    codec_checks: $codecs,
    media_profiles: $media,
    coverage: $coverage
  }
' >"$MANIFEST"

echo "Fedora acceptance facts collected: $MANIFEST"
echo "  Fedora $FEDORA_VERSION / $DESKTOP / $SESSION session"
echo "  SELinux: $SELINUX_MODE, renderer: $RENDERER (virtual-gpu=$VIRTUAL_GPU, vaapi=$VAAPI_AVAILABLE)"

set +e
"$EVIDENCE_BIN" fedora-validate --manifest "$MANIFEST" >"$OUT_DIR/fedora-acceptance-outcome.json" 2>"$OUT_DIR/fedora-acceptance-outcome.txt"
STATUS=$?
set -e
cat "$OUT_DIR/fedora-acceptance-outcome.txt" >&2 || true

case "$STATUS" in
  0) echo "Fedora acceptance PASS. Evidence: $MANIFEST" ;;
  3) echo "Fedora acceptance BLOCKED on a precondition (not a pass). Evidence: $MANIFEST" >&2 ;;
  *) echo "Fedora acceptance FAILED. Evidence: $MANIFEST" >&2 ;;
esac
exit "$STATUS"
