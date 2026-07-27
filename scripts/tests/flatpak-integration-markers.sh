#!/usr/bin/env bash
# Behavioural self-test for the Flatpak integration marker check.
#
# scripts/flatpak_integration_markers.py is what stops the packaged Flatpak from
# silently losing a feature: it requires every marker of the integration to be
# present in the tree flatpak-builder builds. A bare substring search cannot do
# that job, because a commented-out copy of the code satisfies it - the patch is
# normally regenerated mechanically, but a hand-edited patch that comments the
# integration out would have passed.
#
# This test drives the checker against synthetic trees built from its own marker
# table and asserts the outcomes that decide whether the check means anything:
#
#   1. a tree carrying every marker as code passes,
#   2. a marker present only inside line comments fails,
#   3. a marker present only inside a block comment fails,
#   4. an HTML-commented marker in Markdown fails,
#   5. a Markdown heading is not treated as a comment (no over-stripping),
#   6. a file missing from the tree fails,
#   7. a patch touching a path with no declared marker fails, and
#   8. a file type with no comment syntax defined is an error, not a pass.
#
# The trees are synthetic on purpose: this test is about the checker, not about
# the real pin. The real pinned-and-patched tree is checked by
# scripts/smoke-linux-flatpak.sh on every run of the packaging lane.
set -euo pipefail

export PYTHONDONTWRITEBYTECODE=1

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECKER="$ROOT/scripts/flatpak_integration_markers.py"

command -v python3 >/dev/null 2>&1 || {
  echo "Missing required tool: python3" >&2
  exit 127
}

WORK="$(mktemp -d "${TMPDIR:-/tmp}/okp-flatpak-markers.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

failures=0

# Build a tree that satisfies every declared marker. Modes place the markers of
# one chosen path differently so the check can be probed one file at a time.
build_tree() {
  local destination="$1" target="${2:-}" mode="${3:-code}"
  python3 - "$CHECKER" "$destination" "$target" "$mode" <<'PY'
import importlib.util
import sys
from pathlib import Path

checker_path, destination, target, mode = sys.argv[1:5]
spec = importlib.util.spec_from_file_location("okp_markers", checker_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

root = Path(destination)


def render(relative, markers, mode):
    if mode == "code":
        return "\n".join(markers) + "\n"
    if mode == "line-comment":
        prefix = "//" if relative.endswith(".rs") else "#"
        return "\n".join(f"{prefix} {marker}" for marker in markers) + "\n"
    if mode == "block-comment":
        return "/*\n" + "\n".join(markers) + "\n*/\n"
    if mode == "html-comment":
        return "<!--\n" + "\n".join(markers) + "\n-->\n"
    if mode == "markdown-heading":
        # '#' opens a heading in Markdown, not a comment. Stripping it would
        # make this checker reject honest documentation.
        return "# Notices\n" + "\n".join(markers) + "\n"
    if mode == "absent":
        return ""
    raise SystemExit(f"unknown fixture mode: {mode}")


for relative, markers in module.REQUIRED.items():
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    if relative == target:
        if mode == "delete":
            continue
        path.write_text(render(relative, markers, mode))
    else:
        path.write_text(render(relative, markers, "code"))
PY
}

expect() {
  local label="$1" expected_status="$2" actual_status="$3" log="$4" expected_text="${5:-}"
  if [[ "$actual_status" != "$expected_status" ]]; then
    echo "FAIL $label: expected status $expected_status, got $actual_status" >&2
    sed -e 's/^/    /' "$log" >&2
    failures=$((failures + 1))
    return
  fi
  if [[ -n "$expected_text" ]] && ! grep -qF "$expected_text" "$log"; then
    echo "FAIL $label: expected output to contain: $expected_text" >&2
    sed -e 's/^/    /' "$log" >&2
    failures=$((failures + 1))
    return
  fi
  echo "ok $label"
}

run_checker() {
  local tree="$1" log="$2" patch="${3:-}" status=0
  if [[ -n "$patch" ]]; then
    python3 "$CHECKER" "$tree" "$patch" >"$log" 2>&1 || status=$?
  else
    python3 "$CHECKER" "$tree" >"$log" 2>&1 || status=$?
  fi
  printf '%s\n' "$status"
}

RUST_TARGET="rust/crates/okp-linux-gtk/src/about.rs"
MARKDOWN_TARGET="THIRD-PARTY-NOTICES.md"

build_tree "$WORK/honest"
status="$(run_checker "$WORK/honest" "$WORK/honest.log")"
expect "a tree carrying every marker as code passes" 0 "$status" "$WORK/honest.log"

build_tree "$WORK/line-comment" "$RUST_TARGET" line-comment
status="$(run_checker "$WORK/line-comment" "$WORK/line-comment.log")"
expect "markers only inside line comments are rejected" 1 "$status" \
  "$WORK/line-comment.log" "about.rs: missing 'if flatpak_update_managed() {' outside comments"

build_tree "$WORK/block-comment" "$RUST_TARGET" block-comment
status="$(run_checker "$WORK/block-comment" "$WORK/block-comment.log")"
expect "markers only inside a block comment are rejected" 1 "$status" \
  "$WORK/block-comment.log" "about.rs: missing"

build_tree "$WORK/html-comment" "$MARKDOWN_TARGET" html-comment
status="$(run_checker "$WORK/html-comment" "$WORK/html-comment.log")"
expect "markers only inside an HTML comment are rejected" 1 "$status" \
  "$WORK/html-comment.log" "THIRD-PARTY-NOTICES.md: missing"

build_tree "$WORK/heading" "$MARKDOWN_TARGET" markdown-heading
status="$(run_checker "$WORK/heading" "$WORK/heading.log")"
expect "a Markdown heading is not treated as a comment" 0 "$status" "$WORK/heading.log"

build_tree "$WORK/deleted" "$RUST_TARGET" delete
status="$(run_checker "$WORK/deleted" "$WORK/deleted.log")"
expect "a file missing from the tree is reported" 1 "$status" \
  "$WORK/deleted.log" "about.rs: missing from the pinned and patched tree"

cat >"$WORK/unmarked.patch" <<'PATCH'
diff --git a/rust/crates/okp-core/src/presentation_evidence.rs b/rust/crates/okp-core/src/presentation_evidence.rs
index 0000000000000000000000000000000000000000..1111111111111111111111111111111111111111 100644
--- a/rust/crates/okp-core/src/presentation_evidence.rs
+++ b/rust/crates/okp-core/src/presentation_evidence.rs
@@ -1 +1,2 @@
 // placeholder
+// added by the integration patch
PATCH
status="$(run_checker "$WORK/honest" "$WORK/unmarked.log" "$WORK/unmarked.patch")"
expect "a patched path with no declared marker is rejected" 1 "$status" \
  "$WORK/unmarked.log" "touches files with no integration marker"

# A file type with no comment syntax defined must stop the check rather than be
# scanned as if nothing in it could be a comment.
unknown_status=0
python3 - "$CHECKER" >"$WORK/unknown.log" 2>&1 <<'PY' || unknown_status=$?
import importlib.util
import sys

spec = importlib.util.spec_from_file_location("okp_markers", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.code_text("integration.cs", "// commented out")
PY
expect "an unknown file type is an error rather than a silent pass" 1 "$unknown_status" \
  "$WORK/unknown.log" "no comment syntax defined"

if (( failures > 0 )); then
  echo "$failures Flatpak integration marker checks failed" >&2
  exit 1
fi

echo "Flatpak integration marker self-test passed"
