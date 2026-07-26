#!/usr/bin/env python3
"""Fail when a Rust test proves nothing but the presence of source text.

A test that loads its own crate's source with `include_str!` and then asserts
`source.contains("some snippet")` passes against any implementation that happens
to contain the snippet, including a completely broken one. Those tests look like
regression coverage in a diff and are worthless as a merge gate.

This check classifies every `#[test]` function in the Rust workspace:

  * source-text bindings are `let x = include_str!(...)` and anything derived
    from them (`x.split(..)`, `x.find(..)`, ...);
  * an assertion is *source-grep* when its arguments only mention those
    bindings, and *behavioural* otherwise;
  * a test with at least one source-text binding and zero behavioural
    assertions is a source-text-only test.

Existing offenders are grandfathered by an explicit allowlist that may only
shrink. See the allowlist header for the rules.

Usage:
  check-source-grep-tests.py [--root DIR] [--base-ref REF] [--list]
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ALLOWLIST_PATH = ".github/source-grep-test-allowlist.txt"

# Attributes that introduce a test function.
TEST_ATTR = re.compile(r"#\[(?:[A-Za-z_][A-Za-z0-9_]*::)*test(?:\s*\(|\])")
FN_DECL = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
LET_BINDING = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=;]*)?=")
CONST_BINDING = re.compile(
    r"\b(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:[^=;]*=\s*([^;]*);"
)
ASSERT_MACRO = re.compile(r"\bassert(?:_eq|_ne|_matches)?!\s*\(")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def mask_literals(src: str) -> str:
    """Return src with string/char/comment bodies blanked, length preserved.

    Brace and paren matching runs on the mask so that braces inside string
    literals (`r#"...{..."#`) and comments cannot desynchronise the parser.
    """
    out = list(src)
    i, n = 0, len(src)

    def blank(start: int, end: int) -> None:
        for k in range(start, min(end, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            blank(i, j)
            i = j
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        if c == "r" and i + 1 < n and src[i + 1] in '#"':
            j = i + 1
            hashes = 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                terminator = '"' + "#" * hashes
                end = src.find(terminator, j + 1)
                end = n if end == -1 else end + len(terminator)
                blank(i, end)
                i = end
                continue
        if c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            blank(i, j)
            i = j
            continue
        if c == "'":
            # Character literal, but `'a` is a lifetime. Only mask when it closes.
            j = i + 1
            if j < n and src[j] == "\\":
                j += 2
            elif j < n:
                j += 1
            if j < n and src[j] == "'":
                blank(i, j + 1)
                i = j + 1
                continue
        i += 1
    return "".join(out)


def match_delimiter(mask: str, open_at: int, opener: str, closer: str) -> int:
    """Index just past the delimiter closing the one at open_at, or -1."""
    depth = 0
    for i in range(open_at, len(mask)):
        if mask[i] == opener:
            depth += 1
        elif mask[i] == closer:
            depth -= 1
            if depth == 0:
                return i + 1
    return -1


@dataclass
class TestFn:
    name: str
    line: int
    body: str
    body_mask: str


def find_tests(src: str, mask: str) -> list[TestFn]:
    tests: list[TestFn] = []
    for attr in TEST_ATTR.finditer(mask):
        decl = FN_DECL.search(mask, attr.end())
        if not decl:
            continue
        # Guard against an attribute that belongs to something else entirely.
        between = mask[attr.end() : decl.start()]
        if "{" in between:
            continue
        brace = mask.find("{", decl.end())
        if brace == -1:
            continue
        end = match_delimiter(mask, brace, "{", "}")
        if end == -1:
            continue
        tests.append(
            TestFn(
                name=decl.group(1),
                line=src.count("\n", 0, attr.start()) + 1,
                body=src[brace:end],
                body_mask=mask[brace:end],
            )
        )
    return tests


def statement_spans(body: str, body_mask: str) -> list[tuple[int, int]]:
    """Split a function body into top-level-ish statements on `;` outside literals."""
    spans, start = [], 0
    for i, ch in enumerate(body_mask):
        if ch == ";":
            spans.append((start, i))
            start = i + 1
    if start < len(body):
        spans.append((start, len(body)))
    return spans


def source_bindings(body: str, body_mask: str, seeds: set[str]) -> set[str]:
    """Names in the body that hold source text (directly or by derivation).

    A binding is derived only when the right-hand side *starts* with a known
    source-text name, i.e. it slices that text (`window.find(..)`,
    `playback.split(..)`). `let output = Command::new(script)` merely passes the
    text along and is not itself source text, so it stays behavioural.
    """
    names = set(seeds)
    for start, end in statement_spans(body, body_mask):
        stmt_mask = body_mask[start:end]
        let = LET_BINDING.search(stmt_mask)
        if not let:
            continue
        # Read the right-hand side from the mask so that a string literal that
        # merely mentions a name cannot be mistaken for code that uses it.
        rhs = stmt_mask[let.end() :]
        if "include_str" in set(IDENT.findall(rhs)):
            names.add(let.group(1))
            continue
        head = IDENT.match(rhs.lstrip().lstrip("&*").lstrip())
        if head and head.group(0) in names:
            names.add(let.group(1))
    return names


def assertions(body: str, body_mask: str) -> list[tuple[str, str]]:
    """(original, masked) argument text of every assert-family call in the body."""
    args = []
    for m in ASSERT_MACRO.finditer(body_mask):
        open_at = body_mask.index("(", m.start())
        end = match_delimiter(body_mask, open_at, "(", ")")
        if end == -1:
            continue
        args.append((body[open_at + 1 : end - 1], body_mask[open_at + 1 : end - 1]))
    return args


def classify(test: TestFn, seeds: set[str]) -> tuple[bool, int, int]:
    """(is_source_text_only, source_grep_assertions, behavioural_assertions)."""
    names = source_bindings(test.body, test.body_mask, seeds)
    uses_source = "include_str" in test.body_mask or bool(
        seeds & set(IDENT.findall(test.body_mask))
    )
    grep_count = behavioural_count = 0
    for _, arg_mask in assertions(test.body, test.body_mask):
        idents = set(IDENT.findall(arg_mask))
        if "include_str" in idents or idents & names:
            grep_count += 1
        else:
            behavioural_count += 1
    return (uses_source and behavioural_count == 0, grep_count, behavioural_count)


def file_seeds(src: str, mask: str) -> set[str]:
    """Module-level consts/statics whose initialiser is source text."""
    seeds = set()
    for m in CONST_BINDING.finditer(mask):
        rhs = mask[m.start(2) : m.end(2)]
        if "include_str" in rhs:
            seeds.add(m.group(1))
    return seeds


def scan(root: Path) -> list[tuple[str, TestFn, int, int]]:
    findings = []
    rust_root = root / "rust"
    for path in sorted(rust_root.rglob("*.rs")):
        if "target" in path.parts:
            continue
        src = path.read_text(encoding="utf-8", errors="replace")
        if "#[test" not in src and "#[tokio::test" not in src:
            continue
        mask = mask_literals(src)
        seeds = file_seeds(src, mask)
        rel = path.relative_to(root).as_posix()
        for test in find_tests(src, mask):
            only, grep, behav = classify(test, seeds)
            if only:
                findings.append((rel, test, grep, behav))
    return findings


def read_allowlist(text: str) -> list[str]:
    entries = []
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            entries.append(line)
    return entries


def git_show(root: Path, ref: str, path: str) -> str | None:
    proc = subprocess.run(
        ["git", "-C", str(root), "show", f"{ref}:{path}"],
        capture_output=True,
        text=True,
    )
    return proc.stdout if proc.returncode == 0 else None


def annotate(level: str, title: str, message: str, file: str | None = None,
             line: int | None = None) -> None:
    location = ""
    if file:
        location = f" file={file}"
        if line:
            location += f",line={line}"
    flat = message.replace("\n", "%0A")
    print(f"::{level}{location},title={title}::{flat}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument(
        "--base-ref",
        default=os.environ.get("BASE_REF", ""),
        help="Base revision; when given, the allowlist is required not to grow.",
    )
    parser.add_argument(
        "--list", action="store_true", help="Print findings as allowlist entries."
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    findings = scan(root)

    if args.list:
        for rel, test, _, _ in findings:
            print(f"{rel}::{test.name}")
        return 0

    allowlist_file = root / ALLOWLIST_PATH
    allowed = read_allowlist(
        allowlist_file.read_text(encoding="utf-8") if allowlist_file.exists() else ""
    )
    allowed_set = set(allowed)

    failures = 0

    unlisted = [(rel, t, g, b) for rel, t, g, b in findings
                if f"{rel}::{t.name}" not in allowed_set]
    for rel, test, grep, _ in unlisted:
        failures += 1
        annotate(
            "error",
            "Test asserts on source text only",
            (
                f"`{test.name}` has {grep} assertion(s), all of which only check that "
                "source text contains a string. It would pass against a broken "
                "implementation, so it is not regression coverage.\n"
                "Fix: assert on behaviour - call the function or the state machine and "
                "check its output/events. Prove it by reverting the production change "
                "and watching this test fail.\n"
                f"This check does not accept new entries in {ALLOWLIST_PATH}."
            ),
            file=rel,
            line=test.line,
        )

    found_keys = {f"{rel}::{t.name}" for rel, t, _, _ in findings}
    stale = [entry for entry in allowed if entry not in found_keys]
    for entry in stale:
        failures += 1
        annotate(
            "error",
            "Stale source-grep allowlist entry",
            (
                f"`{entry}` is allowlisted but is no longer a source-text-only test "
                "(it was fixed, renamed, or deleted). Delete the line from "
                f"{ALLOWLIST_PATH}; the allowlist may only shrink."
            ),
            file=ALLOWLIST_PATH,
        )

    if args.base_ref:
        base_text = git_show(root, args.base_ref, ALLOWLIST_PATH)
        if base_text is None:
            annotate(
                "warning",
                "Allowlist growth check skipped",
                f"Could not read {ALLOWLIST_PATH} at {args.base_ref}. Check out the "
                "full history so the allowlist can be compared with its base state.",
            )
        else:
            base_entries = read_allowlist(base_text)
            added = sorted(set(allowed) - set(base_entries))
            if len(allowed) > len(base_entries):
                failures += 1
                annotate(
                    "error",
                    "Source-grep allowlist grew",
                    (
                        f"{ALLOWLIST_PATH} went from {len(base_entries)} to "
                        f"{len(allowed)} entries. It may only shrink. Added: "
                        + ", ".join(added)
                    ),
                    file=ALLOWLIST_PATH,
                )

    print(
        f"source-text-only tests found: {len(findings)}; "
        f"allowlisted: {len(findings) - len(unlisted)}; "
        f"unlisted: {len(unlisted)}; stale allowlist entries: {len(stale)}"
    )
    if failures:
        print(f"FAILED: {failures} problem(s).", file=sys.stderr)
        return 1
    print("OK: no unlisted source-text-only tests.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
