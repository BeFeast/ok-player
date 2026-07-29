#!/usr/bin/env bash
# Behavioural coverage for the GNOME/Wayland acceptance harness (#691).
#
# The harness itself needs a live desktop, but its judgements do not: this drives the
# decision helpers against fabricated evidence, because the interesting cases are the ones
# where a row must FAIL and a live run only ever shows one outcome at a time. It also
# holds the harness to the two properties that make the level worth having - it refuses to
# run off a Wayland session, and it never emits a PASS row it did not observe.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS="$ROOT/scripts/gnome-wayland-harness"
RUNNER="$ROOT/scripts/run-linux-gnome-wayland-acceptance.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

failures=0
fail() { echo "FAIL: $1" >&2; failures=$((failures + 1)); }
ok() { echo "ok: $1"; }

#-- focus traversal -----------------------------------------------------------------

focus_log() {
  local path="$WORK/$1.log"; shift
  : >"$path"
  local index=0
  for target in "$@"; do
    index=$((index + 1))
    printf 'interaction: focus target=%s seq=%s\n' "$target" "$index" >>"$path"
  done
  printf '%s\n' "$path"
}

log="$(focus_log traversal a b c d e f e d c)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --forward 6 --backward 3 >/dev/null; then
  ok "a real traversal passes"
else
  fail "a real traversal must pass"
fi

log="$(focus_log stuck a a a a a a a a a)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --forward 6 --backward 3 >/dev/null; then
  fail "focus that never moves must not pass"
else
  ok "focus stuck on one widget fails"
fi

log="$(focus_log silent)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --forward 6 --backward 3 >/dev/null; then
  fail "no focus stops at all must not pass"
else
  ok "a session that reports no focus stops fails"
fi

log="$(focus_log oscillating a b a b a b a b a)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --forward 6 --backward 3 >/dev/null; then
  fail "focus bouncing between two widgets must not pass"
else
  ok "focus oscillating between two widgets fails"
fi

log="$(focus_log noreturn a b c d e f g h i)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --forward 6 --backward 3 >/dev/null; then
  fail "Shift+Tab that never revisits a stop must not pass"
else
  ok "a traversal that never walks back fails"
fi

# Stops already on the stream before the gesture must not be counted as traversal.
log="$(focus_log prefixed a a a a a a a a a b c)"
if python3 "$HARNESS/focus-traversal.py" --log "$log" --from-line 9 --forward 6 --backward 3 >/dev/null; then
  fail "stops from before the gesture must not be reused"
else
  ok "earlier focus stops are excluded by --from-line"
fi

#-- row emission --------------------------------------------------------------------

facts="$WORK/facts.json"
cat >"$facts" <<'JSON'
{
  "session_type": "wayland",
  "compositor": "gnome-shell",
  "compositor_version": "50.1",
  "monitors": [{"connector": "eDP-1", "width": 1920, "height": 1080, "scale": 1.5}],
  "input_injector": "uinput"
}
JSON

PACKAGE_SHA256="$(printf 'a%.0s' $(seq 64))"

emit() {
  local results="$1" artifacts="$2" output="$3"
  shift 3
  python3 "$HARNESS/emit-rows.py" \
    --results "$results" --artifacts "$artifacts" --facts "$facts" \
    --harness-revision "$(printf 'c%.0s' $(seq 40))" \
    --package-sha256 "$PACKAGE_SHA256" \
    --execution-environment-sha256 "$(printf 'e%.0s' $(seq 64))" \
    --output "$output" "$@"
}

results="$WORK/results"; artifacts="$WORK/artifacts"
mkdir -p "$results" "$artifacts"
printf 'pass\nobserved\n' >"$results/wayland-clipboard.result"
printf 'sample\n' >"$artifacts/wayland-clipboard.png"
emit "$results" "$artifacts" "$WORK/rows.json" >/dev/null

python3 - "$WORK/rows.json" <<'PY'
import json, sys
rows = {row["id"]: row for row in json.load(open(sys.argv[1]))}
assert len(rows) == 7, rows.keys()
passing = rows["wayland-clipboard"]
assert passing["automated_status"] == "pass"
assert passing["level"] == "gnome-wayland-automated"
assert passing["operator_status"] == "not-run", "a machine row must never claim a person looked"
assert passing["automation"] is not None
assert passing["artifacts"] and len(passing["artifacts"][0]["sha256"]) == 64
missing = rows["keyboard-focus-navigation"]
assert missing["automated_status"] == "not-run", "a check that did not run is not a pass"
assert missing["automation"] is None, "a row that did not run carries no attestation"
assert missing["execution_environment_sha256"] is None
PY
if [[ $? -eq 0 ]]; then
  ok "emitted rows carry the attestation only where the check observed a pass"
else
  fail "row emission does not match the contract"
fi

# Rows the project has decided stay operator-gated must never appear in a harness manifest.
python3 - "$WORK/rows.json" <<'PY'
import json, sys
ids = {row["id"] for row in json.load(open(sys.argv[1]))}
for operator_only in ("gnome-folder-chooser", "wayland-drag-drop"):
    assert operator_only not in ids, f"{operator_only} must stay operator-gated"
PY
if [[ $? -eq 0 ]]; then
  ok "operator-only rows are absent from the harness manifest"
else
  fail "the harness emitted an operator-only row"
fi

# A passing row names the exact bytes it exercised, or it could be merged into another
# candidate's manifest without anything noticing.
python3 - "$WORK/rows.json" "$PACKAGE_SHA256" <<'PY'
import json, sys
rows = {row["id"]: row for row in json.load(open(sys.argv[1]))}
assert rows["wayland-clipboard"]["automation"]["package_sha256"] == sys.argv[2]
PY
if [[ $? -eq 0 ]]; then
  ok "a passing row is bound to the package digest under test"
else
  fail "a passing row does not name the package it exercised"
fi

# A subset selection emits that subset only: emitting the rest as not-run would overwrite
# states an earlier run already collected, because the merge replaces rows by state.
emit "$results" "$artifacts" "$WORK/subset.json" --rows wayland-clipboard >/dev/null
python3 - "$WORK/subset.json" <<'PY'
import json, sys
rows = json.load(open(sys.argv[1]))
assert [row["id"] for row in rows] == ["wayland-clipboard"], rows
PY
if [[ $? -eq 0 ]]; then
  ok "a subset selection emits only the selected rows"
else
  fail "a subset selection must not emit the unselected states"
fi

# An empty selection is the whole automatable set, which is what the workflow advertises.
emit "$results" "$artifacts" "$WORK/empty.json" --rows "" >/dev/null
python3 - "$WORK/empty.json" <<'PY'
import json, sys
assert len(json.load(open(sys.argv[1]))) == 7
PY
if [[ $? -eq 0 ]]; then
  ok "an empty selection means every automatable row"
else
  fail "an empty selection must mean every automatable row"
fi

# Naming an operator-only row is a hard error, not a quietly emitted extra row.
if emit "$results" "$artifacts" "$WORK/operator.json" --rows wayland-drag-drop >/dev/null 2>&1; then
  fail "an operator-only row must be refused by the selector"
else
  ok "the selector refuses an operator-only row"
fi

# An unusable status is a hard error, never a silent downgrade to not-run.
printf 'probably\n' >"$results/desktop-portal.result"
if emit "$results" "$artifacts" "$WORK/bad.json" >/dev/null 2>&1; then
  fail "an unrecognised row status must be refused"
else
  ok "an unrecognised row status is refused"
fi
rm -f "$results/desktop-portal.result"

#-- portal attribution --------------------------------------------------------------

# Any application on a logged-in desktop calls the FileChooser portal, so the row has to
# key on *who* called it. This drives the real resolver against a real session bus: a
# process owns a connection, writes a capture naming it, and asks whether that call is
# attributed to itself and denied to anyone else. Nothing here stubs the bus, because the
# lookup is the thing under test.
command -v dbus-run-session >/dev/null 2>&1 ||
  { echo "FAIL: dbus-run-session is required to test portal attribution" >&2; exit 1; }

cat >"$WORK/attribution.py" <<'PY'
import os, pathlib, subprocess, sys
import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio

helper, work = sys.argv[1], pathlib.Path(sys.argv[2])
bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
mine = bus.get_unique_name()

def capture(name, path):
    path.write_text(
        f"method call time=1.0 sender={name} -> destination=org.freedesktop.portal.Desktop "
        "serial=1 path=/org/freedesktop/portal/desktop; "
        "interface=org.freedesktop.portal.FileChooser; member=OpenFile\n"
    )

def run(log, pid):
    return subprocess.run(
        [sys.executable, helper, "--log", str(log), "--pid", str(pid)],
        capture_output=True, text=True,
    )

own = work / "own.log"
capture(mine, own)

# The call this process made is attributed to this process.
result = run(own, os.getpid())
assert result.returncode == 0, f"own call rejected: {result.stdout}{result.stderr}"
assert "player-calls=1" in result.stdout, result.stdout

# The same call is not attributed to anyone else.
result = run(own, os.getpid() + 100000)
assert result.returncode != 0, f"another process claimed the call: {result.stdout}"
assert "player-calls=0" in result.stdout and "foreign-calls=1" in result.stdout, result.stdout

# A call from a connection that no longer exists is foreign, never a pass.
gone = work / "gone.log"
capture(":1.999999", gone)
result = run(gone, os.getpid())
assert result.returncode != 0, f"an unresolvable sender passed: {result.stdout}"
assert "unresolved" in result.stdout, result.stdout

# No traffic at all is not a pass either.
empty = work / "empty.log"
empty.write_text("")
result = run(empty, os.getpid())
assert result.returncode != 0, f"an empty capture passed: {result.stdout}"

print("attribution ok")
PY

if dbus-run-session -- python3 "$WORK/attribution.py" "$HARNESS/portal-calls.py" "$WORK" >/dev/null 2>"$WORK/attribution.err"; then
  ok "portal calls are attributed to the connection that made them, and to no one else"
else
  fail "portal attribution is wrong: $(cat "$WORK/attribution.err")"
fi

#-- the harness refuses a session it must not attest --------------------------------

fake_bin="$WORK/bin"; mkdir -p "$fake_bin"
for tool in ydotool ydotoold wl-paste wl-copy dbus-monitor gst-launch-1.0 dconf; do
  printf '#!/bin/sh\nexit 0\n' >"$fake_bin/$tool"
  chmod +x "$fake_bin/$tool"
done
printf '#!/bin/sh\nexit 0\n' >"$WORK/binary"; chmod +x "$WORK/binary"
: >"$WORK/fixture.mp4"

output="$(PATH="$fake_bin:$PATH" XDG_SESSION_TYPE=x11 WAYLAND_DISPLAY= \
  "$RUNNER" --binary "$WORK/binary" --fixture "$WORK/fixture.mp4" --out "$WORK/x11" \
  --package-sha256 "$PACKAGE_SHA256" 2>&1)"
status=$?
if (( status != 0 )) && [[ "$output" == *"not a wayland session"* ]]; then
  ok "the harness refuses to attest an X11 session"
else
  fail "the harness must refuse a non-Wayland session (status=$status): $output"
fi

# Evidence with no package digest is evidence about nothing in particular.
output="$(PATH="$fake_bin:$PATH" \
  "$RUNNER" --binary "$WORK/binary" --fixture "$WORK/fixture.mp4" --out "$WORK/nodigest" 2>&1)"
status=$?
if (( status != 0 )) && [[ "$output" == *"--package-sha256"* ]]; then
  ok "the harness refuses to run without the package digest under test"
else
  fail "the harness must require the package digest (status=$status): $output"
fi

if (( failures == 0 )); then
  echo "gnome-wayland-acceptance: all checks passed"
else
  echo "gnome-wayland-acceptance: $failures check(s) failed" >&2
  exit 1
fi
