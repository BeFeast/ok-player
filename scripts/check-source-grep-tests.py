#!/usr/bin/env python3
"""Fail when a Rust test proves nothing but the presence of source text.

A test that loads its own crate's source with `include_str!` and then asserts
`source.contains("some snippet")` passes against any implementation that happens
to contain the snippet, including a completely broken one. Those tests look like
regression coverage in a diff and are worthless as a merge gate.

This check classifies every `#[test]` function in the Rust workspace:

  * source-text bindings are `let x = include_str!(...)`, a same-file helper
    that returns source text, and anything derived from them (`x.split(..)`,
    `x.find(..)`, a tuple element, a `for` loop variable, ...);
  * an assertion is *source-grep* when it looks at those bindings rather than
    handing them to production code;
  * an assertion is *behavioural evidence* only when it involves something the
    code produced - a binding that came out of a call, source text passed into
    production code, or a call made inside the assertion itself;
  * a test that uses source text and produces no behavioural evidence is a
    source-text-only test.

That last rule used to read "zero non-grep assertions", which a single
`assert_eq!(2 + 2, 4);` was enough to satisfy. A comparison between constants is
not evidence about an implementation.

Existing offenders are grandfathered by an explicit allowlist that may only
shrink. See the allowlist header for the rules and for what this does not
detect.

Usage:
  check-source-grep-tests.py [--root DIR] [--base-ref REF] [--require-base-ref]
                             [--list]
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
# `let (source, _) = (include_str!(..), 1);` binds through a tuple pattern.
LET_TUPLE = re.compile(r"\blet\s+(?:mut\s+)?\(([^)]*)\)\s*(?::[^=;]*)?=")
# `for source in files`, `for source in files.iter()`, `for source in
# [include_str!(..), include_str!(..)]` - a collection of source texts walked
# one element at a time. The iterable runs to the opening brace of the body.
FOR_BINDING = re.compile(
    r"\bfor\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s+in\s+([^{]*)\{"
)
CONST_BINDING = re.compile(
    r"\b(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:[^=;]*=\s*([^;]*);"
)
# A helper that hands back source text: `fn main_rs() -> &'static str {
# include_str!("main.rs") }`. Calling it is the same as writing the macro.
FN_SIGNATURE = re.compile(
    r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^>]*>)?\s*\([^()]*\)\s*->\s*([^{;]+)"
)
ASSERT_MACRO = re.compile(r"\bassert(?:_eq|_ne|_matches)?!\s*\(")
# `parse(sample).expect(..)` asserts too: it fails the test when production code
# cannot handle the input. So does `source.find(..).expect(..)`, about the text.
FALLIBLE_CALL = re.compile(r"\.(?:expect\s*\(|unwrap\s*\(\s*\))")
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
# Any call: `parse(x)`, `x.len()`, `Type::new()`. Used to tell an assertion that
# runs code from one that only compares constants.
ANY_CALL = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:::\s*[A-Za-z_][A-Za-z0-9_]*\s*)*\(")
# Searching a string is not running the code under test, whatever the string is.
# Without this, a grep against a constant that the detector does not track as
# source text would count as evidence and clear the real greps beside it.
TEXT_METHODS = {
    "contains", "find", "rfind", "matches", "match_indices", "starts_with",
    "ends_with", "split", "splitn", "rsplit", "split_once", "rsplit_once",
    "lines", "chars", "bytes", "len", "count", "nth", "next", "trim",
    "to_string", "to_owned", "as_str", "is_empty", "iter", "collect",
}


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


def source_bindings(
    body: str, body_mask: str, seeds: set[str]
) -> tuple[set[str], set[str], set[str]]:
    """(direct, derived, production) names bound in the body.

    Direct names are bound straight from `include_str!` (or from a helper that
    returns source text - see `file_seeds`). Derived names are cut out of a
    direct one (`window.find(..)`, `playback.split(..)`) and therefore describe
    the text rather than anything the code did with it.

    Production names are everything else that was bound from a call:
    `let output = Command::new(script)`, `let cues = parse_srt(sample)`. They
    hold what the code under test produced, so an assertion that mentions one is
    evidence that something was actually executed.

    Resolution runs to a fixpoint because a `for` loop can bind from a
    collection declared after it in source order inside a nested block.
    """
    direct, derived, production = set(seeds), set(), set()

    def classify_rhs(names: set[str], rhs: str) -> None:
        if "include_str" in set(IDENT.findall(rhs)):
            direct.update(names)
            return
        head = IDENT.match(rhs.lstrip().lstrip("&*").lstrip())
        if head and head.group(0) in direct | derived:
            derived.update(names)
            return
        if ANY_CALL.search(rhs):
            production.update(names - direct - derived)

    for _ in range(4):
        before = (len(direct), len(derived), len(production))
        for start, end in statement_spans(body, body_mask):
            stmt_mask = body_mask[start:end]
            tuple_let = LET_TUPLE.search(stmt_mask)
            if tuple_let:
                # Read the right-hand side from the mask so that a string
                # literal that merely mentions a name cannot be mistaken for
                # code that uses it.
                classify_rhs(
                    set(IDENT.findall(tuple_let.group(1))),
                    stmt_mask[tuple_let.end() :],
                )
                continue
            let = LET_BINDING.search(stmt_mask)
            if let:
                classify_rhs({let.group(1)}, stmt_mask[let.end() :])
        for loop in FOR_BINDING.finditer(body_mask):
            iterable = loop.group(2)
            names = set(IDENT.findall(iterable))
            if "include_str" in names:
                direct.add(loop.group(1))
            elif names & (direct | derived):
                derived.add(loop.group(1))
        if (len(direct), len(derived), len(production)) == before:
            break
    return direct, derived, production - direct - derived


def assertions(body: str, body_mask: str) -> list[tuple[str, str, bool]]:
    """(original, masked, is_macro) text of every assertion in the body.

    An assertion is an assert-family macro - classified by its arguments - or a
    fallible call that panics on failure, classified by the whole statement it
    sits in, since that is what names the value being unwrapped.

    The two are distinguished because a fallible call is a much weaker signal: a
    macro is written to check something, while `.unwrap()` also appears in
    routine setup that asserts nothing about the subject under test.
    """
    args: list[tuple[str, str, bool]] = []
    for m in ASSERT_MACRO.finditer(body_mask):
        open_at = body_mask.index("(", m.start())
        end = match_delimiter(body_mask, open_at, "(", ")")
        if end == -1:
            continue
        args.append(
            (body[open_at + 1 : end - 1], body_mask[open_at + 1 : end - 1], True)
        )
    for start, end in statement_spans(body, body_mask):
        statement_mask = body_mask[start:end]
        if not FALLIBLE_CALL.search(statement_mask):
            continue
        if ASSERT_MACRO.search(statement_mask):
            continue  # already counted through its arguments
        args.append((body[start:end], statement_mask, False))
    return args


def passed_into_a_call(head: str, tail: str) -> bool:
    """Is this occurrence handed to a function whole, as `parse(sample)`?

    Text passed into production code and then asserted on is behaviour, whatever
    loaded the text. Everything else - slicing it, comparing it, measuring it -
    is an assertion about the text itself.
    """
    if tail.lstrip(" \t\r\n")[:1] not in {",", ")"}:
        return False
    before = head.rstrip(" \t\r\n").rstrip("&*").rstrip(" \t\r\n")
    if not before.endswith("("):
        return False
    callee = before[:-1].rstrip(" \t\r\n")
    name = re.search(r"[A-Za-z0-9_]+$", callee)
    if not name:
        return False
    # `assert_source_contains(source, ..)` is an assertion helper, not production
    # code the text is being fed to. Helpers that grep under another name are not
    # detected: telling a grep helper from a parser needs flow analysis, and this
    # check is a floor against accident, not against a determined author.
    return "assert" not in name.group(0).lower()


def inspects_source_text(arg_mask: str, direct: set[str], derived: set[str]) -> bool:
    """Does this assertion look *at* source text rather than run code on it?

    `source.contains("x")`, `assert_eq!(source, expected)` and `renderer < gtk`
    (indices cut out of the text) all look at it. `parse(sample).len()` hands the
    text to production code and asserts on what came back.
    """
    if set(IDENT.findall(arg_mask)) & derived:
        return True
    for name in direct | {"include_str"}:
        for match in re.finditer(rf"\b{re.escape(name)}\b", arg_mask):
            head, tail = arg_mask[: match.start()], arg_mask[match.end() :]
            if name == "include_str":
                call = re.match(r"!\s*\([^()]*\)", tail)
                if not call:
                    continue
                tail = tail[call.end() :]
            if not passed_into_a_call(head, tail):
                return True
    return False


def is_behavioural_evidence(
    arg_mask: str, direct: set[str], derived: set[str], production: set[str]
) -> bool:
    """Does this non-grep assertion show that code was actually executed?

    Counting every non-grep assertion as behaviour let a single unrelated
    assertion - `assert_eq!(2 + 2, 4);` - clear a test made entirely of source
    greps. An assertion is evidence only when it involves something the code
    produced:

      * it mentions a binding that came out of a call (`cues`, `output`); or
      * it hands source text into production code (`parse_srt(sample).len()`);
        or
      * it calls something itself, where searching a string does not count
        (`assert_eq!(parse("x"), Ok(1))` does, `SOME_CONST.contains("x")` does
        not).

    A comparison between constants mentions no binding and calls nothing, so it
    proves nothing about the implementation and does not launder a source grep.
    """
    names = set(IDENT.findall(arg_mask))
    if names & production:
        return True
    if names & (direct | derived) or "include_str" in names:
        # The caller already established this is not an inspection, so the text
        # is being handed to production code.
        return True
    return any(
        call.group(1) not in TEXT_METHODS for call in ANY_CALL.finditer(arg_mask)
    )


def classify(test: TestFn, seeds: set[str]) -> tuple[bool, int, int]:
    """(is_source_text_only, source_grep_assertions, behavioural_assertions)."""
    direct, derived, production = source_bindings(test.body, test.body_mask, seeds)
    uses_source = "include_str" in test.body_mask or bool(
        seeds & set(IDENT.findall(test.body_mask))
    )
    grep_count = evidence_count = 0
    for _, arg_mask, is_macro in assertions(test.body, test.body_mask):
        if inspects_source_text(arg_mask, direct, derived):
            grep_count += 1
            continue
        if not is_behavioural_evidence(arg_mask, direct, derived, production):
            continue
        if not is_macro and not (
            set(IDENT.findall(arg_mask)) & (direct | derived | {"include_str"})
        ):
            # A bare `.unwrap()` that never touches the fixture is setup, not an
            # assertion: `std::env::current_dir().unwrap()` beside a wall of
            # source greps says nothing about the subject under test.
            continue
        evidence_count += 1
    return (uses_source and evidence_count == 0, grep_count, evidence_count)


def file_seeds(src: str, mask: str) -> set[str]:
    """Names that stand for source text: consts/statics and helper functions.

    A helper such as

        fn window_source() -> &'static str { include_str!("window.rs") }

    is `include_str!` behind one indirection, so calling it must seed the same
    bindings the macro would. Only functions declared in the same file are
    resolved: a helper in another module is not followed.
    """
    seeds = set()
    for m in CONST_BINDING.finditer(mask):
        rhs = mask[m.start(2) : m.end(2)]
        if "include_str" in rhs:
            seeds.add(m.group(1))
    for m in FN_SIGNATURE.finditer(mask):
        # Only a function that hands back the text itself is source text. A
        # helper that parses a fixture and returns a value is production code,
        # and treating it as source text would flag every test that uses it.
        if "str" not in m.group(2) and "String" not in m.group(2):
            continue
        brace = mask.find("{", m.end())
        if brace == -1:
            continue
        end = match_delimiter(mask, brace, "{", "}")
        if end == -1 or "include_str" not in mask[brace:end]:
            continue
        seeds.add(m.group(1))
    return seeds


def scan(root: Path) -> tuple[list[tuple[str, TestFn, int, int]], set[str]]:
    """(offenders, keys of every test that still contains a source grep).

    The second set is what the allowlist ledger is keyed on. An entry may be
    deleted only once its test has stopped greping source text altogether -
    acquiring one behavioural assertion is not progress, it is laundering.
    """
    findings: list[tuple[str, TestFn, int, int]] = []
    still_greping: set[str] = set()
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
            if grep:
                still_greping.add(f"{rel}::{test.name}")
    return findings, still_greping


def read_allowlist(text: str) -> list[str]:
    entries = []
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            entries.append(line)
    return entries


NULL_SHA = "0" * 40


def git_show(root: Path, ref: str, path: str) -> str | None:
    proc = subprocess.run(
        ["git", "-C", str(root), "show", f"{ref}:{path}"],
        capture_output=True,
        text=True,
    )
    return proc.stdout if proc.returncode == 0 else None


def base_allowlist_state(root: Path, ref: str) -> tuple[str, str | None]:
    """("present"|"absent"|"unreachable", text).

    The growth guard is the only rule that catches a new offender added
    together with its allowlist entry, so it must fail closed. `git show` alone
    cannot say why it failed - a missing file and an unfetched ref look the
    same, and this check degraded to a warning on exactly that ambiguity. Ask
    the two questions separately: does the ref resolve, and does the path exist
    in it.
    """
    if not ref or ref == NULL_SHA:
        return "unreachable", None
    resolves = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"],
        capture_output=True,
        text=True,
    )
    if resolves.returncode != 0:
        return "unreachable", None
    exists = subprocess.run(
        ["git", "-C", str(root), "cat-file", "-e", f"{ref}:{ALLOWLIST_PATH}"],
        capture_output=True,
        text=True,
    )
    if exists.returncode != 0:
        return "absent", None
    text = git_show(root, ref, ALLOWLIST_PATH)
    if text is None:
        return "unreachable", None
    return "present", text


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
        "--require-base-ref",
        action="store_true",
        help="Fail when no base revision is available (set on pull request runs).",
    )
    parser.add_argument(
        "--list", action="store_true", help="Print findings as allowlist entries."
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    findings, still_greping = scan(root)

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

    # An entry is stale only once its test has stopped greping source text
    # altogether. Keying staleness on "is it still an offender" made the ledger
    # drainable: adding one unrelated assertion to a grandfathered test turned
    # its entry stale, and the check then demanded the line be deleted - so the
    # prescribed fix for laundering was to record the laundering as progress.
    live_keys = {f"{rel}::{t.name}" for rel, t, _, _ in findings} | still_greping
    stale = [entry for entry in allowed if entry not in live_keys]
    for entry in stale:
        failures += 1
        annotate(
            "error",
            "Stale source-grep allowlist entry",
            (
                f"`{entry}` is allowlisted but no longer asserts on source text at "
                "all (it was fixed, renamed, or deleted). Delete the line from "
                f"{ALLOWLIST_PATH}; the allowlist may only shrink."
            ),
            file=ALLOWLIST_PATH,
        )

    state, base_text = base_allowlist_state(root, args.base_ref)
    if not args.base_ref:
        if args.require_base_ref:
            failures += 1
            annotate(
                "error",
                "No base revision for the allowlist growth check",
                "This run was told to compare the allowlist with its base state but "
                "no base revision was supplied (BASE_REF is empty). The growth guard "
                "is the only rule that catches a new offender added together with "
                "its allowlist entry, so it must not be skipped on a pull request.",
                file=ALLOWLIST_PATH,
            )
    elif state == "unreachable":
        failures += 1
        annotate(
            "error",
            "Allowlist growth check could not run",
            f"Could not read {ALLOWLIST_PATH} at {args.base_ref}: the revision does "
            "not resolve in this checkout. Check out the full history "
            "(fetch-depth: 0) so the allowlist can be compared with its base state. "
            "This check fails rather than warns, because a growth guard that "
            "degrades to a warning grandfathers whatever it could not see.",
            file=ALLOWLIST_PATH,
        )
    elif state == "absent":
        # The base commit predates the ledger, so there is nothing to compare
        # against and every entry looks new. This is reachable only until the
        # ledger reaches the base branch: removing it from a branch that has it
        # fails above, because every grandfathered test becomes unlisted.
        annotate(
            "notice",
            "Allowlist introduced",
            f"{ALLOWLIST_PATH} does not exist at {args.base_ref}, so this change "
            "introduces the ledger and there is no base state to compare with. "
            "Every entry is still required to match a test that greps source text.",
            file=ALLOWLIST_PATH,
        )
    else:
        base_entries = read_allowlist(base_text or "")
        dropped = set(base_entries) - set(allowed)
        for entry in sorted(set(allowed) - set(base_entries)):
            name = entry.rpartition("::")[2]
            # Re-keying an entry that already existed - the test moved to
            # another file - is not growth, as long as the total did not rise.
            # Matching on the bare test name was not enough: a new offender
            # could take the name of an entry that is still in the list under
            # its original key and ride in on it. A move means the old key was
            # deleted here *and* nothing at that key greps source text any more.
            vacated = [e for e in dropped if e.rpartition("::")[2] == name]
            if (
                vacated
                and all(old not in live_keys for old in vacated)
                and len(allowed) <= len(base_entries)
            ):
                continue
            failures += 1
            annotate(
                "error",
                "Source-grep allowlist grew",
                (
                    f"{ALLOWLIST_PATH} adds `{entry}`. The allowlist may only "
                    "shrink: it grandfathers tests that already existed, it does "
                    "not accept new ones. Write a behavioural test instead - "
                    "drive the code and assert on what it produced."
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
