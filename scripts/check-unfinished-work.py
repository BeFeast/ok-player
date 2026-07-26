#!/usr/bin/env python3
"""Fail a pull request that declares itself unfinished, or ships unfinished code.

Two cheap, mechanical rules that a merge robot cannot talk itself out of:

1. Declaration checks (skipped when no pull request context is supplied):
   * a title that starts with WIP / Draft / Do not merge;
   * the maestro WIP marker HTML comment in the body;
   * unchecked boxes inside an "Operator acceptance" block in the body;
   * an empty body, which cannot state what "done" means.

2. Tree checks: unfinished-code markers in shipped source - `todo!()`,
   `unimplemented!()`, and bare TODO / FIXME comments. A marker that names a
   tracking issue (`TODO(#123)`) is accepted: the work is tracked, not lost.

Fenced code blocks are ignored when scanning the body, so a pull request may
quote these rules without tripping them. HTML comments are ignored when looking
for unchecked boxes, so a template can carry a commented-out acceptance block.

Usage:
  check-unfinished-work.py [--root DIR] [--title-file F] [--body-file F]
Environment: PR_TITLE, PR_BODY (overridden by the file arguments).
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

# Titles that announce the change is not finished. "Draft"/"Do not merge" need a
# delimiter so that a real title such as "Draft the release notes" survives.
TITLE_UNFINISHED = re.compile(
    r"""^\s*(?:
          \[\s*(?:wip|draft|do\s*not\s*merge|dnm)\s*\]
        | wip\b
        | (?:draft|do\s*not\s*merge|dnm)\s*(?:[:\-–—]|$)
    )""",
    re.IGNORECASE | re.VERBOSE,
)
WIP_MARKER = re.compile(r"<!--\s*maestro:wip\b[^>]*-->", re.IGNORECASE)
ACCEPTANCE_HEADING = re.compile(
    r"^\s*(?:#{1,6}\s*|\*\*\s*|__\s*)?operator\s+(?:acceptance|sign[\s\-]?off)",
    re.IGNORECASE,
)
# What ends a block: a markdown heading, a rule, or a bold/underlined label line
# used as a heading (pull request bodies in this repository do all three).
HEADING = re.compile(
    r"^\s*(?:#{1,6}\s|---\s*$|\*\*[^*]+\*\*\s*:?\s*$|__[^_]+__\s*:?\s*$"
    r"|<[A-Za-z/][^>]*>)"
)
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+\.)\s+(.*)$")
UNCHECKED_BOX = re.compile(r"^\[\s\]\s*(.*)$")
CHECKED_BOX = re.compile(r"^\[[xX]\]\s*(.*)$")
FENCE = re.compile(r"^\s*(?:```|~~~)")
HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)

# Unfinished-code markers. A marker that references a tracking issue is fine.
RUST_STUBS = re.compile(r"\b(?:todo|unimplemented)!\s*[\(\[\{]")
STRING_LITERAL = re.compile(r"\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\])*'")
BARE_MARKER = re.compile(r"\b(TODO|FIXME)\b(?!\s*\(\s*#\d+\s*\))")
CODE_SUFFIXES = {
    ".rs", ".cs", ".sh", ".ps1", ".psm1", ".py", ".c", ".h", ".cpp", ".xaml",
}
SKIP_DIRS = {".git", "target", "bin", "obj", "node_modules"}
# The gate's own sources and its tests must spell the markers out to detect them.
SELF_PATHS = {
    "scripts/check-unfinished-work.py",
    "scripts/check-source-grep-tests.py",
    "rust/crates/okp-core/tests/unfinished_work_gate.rs",
}


def summarise(items: list[str], limit: int = 5) -> str:
    shown = "; ".join(items[:limit])
    if len(items) > limit:
        shown += f"; and {len(items) - limit} more"
    return shown


def strip_fenced_blocks(text: str) -> str:
    out, inside = [], False
    for line in text.splitlines():
        if FENCE.match(line):
            inside = not inside
            continue
        out.append("" if inside else line)
    return "\n".join(out)


def acceptance_blocks(body: str) -> list[list[str]]:
    """The body lines of every Operator acceptance block, HTML comments removed."""
    blocks, current = [], None
    for line in HTML_COMMENT.sub("", body).splitlines():
        if ACCEPTANCE_HEADING.match(line):
            if current is not None:
                blocks.append(current)
            current = []
            continue
        if current is None:
            continue
        if HEADING.match(line):
            blocks.append(current)
            current = None
            continue
        current.append(line)
    if current is not None:
        blocks.append(current)
    return blocks


def acceptance_problems(body: str) -> list[tuple[str, str]]:
    """(title, detail) for every Operator acceptance block that is not resolved.

    An acceptance block is resolved only when it consists of ticked checkboxes.
    Anything else - an unticked box, a plain bullet, or a paragraph such as "Do
    not merge until a packaged build passes dual-display QA" - states a hold
    that nothing can record as performed. Every real acceptance hold in this
    repository's history took one of those two unrecordable shapes.
    """
    problems = []
    for block in acceptance_blocks(body):
        unchecked, unresolvable, checked = [], [], 0
        prose = []
        for line in block:
            item = LIST_ITEM.match(line)
            if not item:
                if line.strip():
                    prose.append(line.strip())
                continue
            text = item.group(1).strip()
            if CHECKED_BOX.match(text):
                checked += 1
            elif UNCHECKED_BOX.match(text):
                unchecked.append(UNCHECKED_BOX.match(text).group(1).strip() or "(unnamed item)")
            else:
                unresolvable.append(text or "(unnamed item)")
        if unchecked:
            problems.append(
                (
                    "Operator acceptance is not complete",
                    "The body has an Operator acceptance block with unchecked items: "
                    + summarise(unchecked)
                    + ". Perform each item and tick its box, or delete the block if "
                    "the change genuinely needs no operator acceptance. Ticking a box "
                    "you did not perform is the failure mode this check exists to "
                    "stop.",
                )
            )
        if unresolvable:
            problems.append(
                (
                    "Operator acceptance block has items that cannot be resolved",
                    "These Operator acceptance items are plain bullets, so nothing "
                    "can record that they were performed: "
                    + summarise(unresolvable)
                    + ". Write every acceptance item as a checkbox and tick it once "
                    "it is actually done. A prose hold is how an acceptance block "
                    "ends up merged unperformed.",
                )
            )
        if not unchecked and not unresolvable and not checked:
            problems.append(
                (
                    "Operator acceptance block states a hold in prose",
                    "This Operator acceptance block has no checkbox at all, so there "
                    "is nothing to record that it was performed: "
                    + (summarise(prose) if prose else "(the block is empty)")
                    + ". State each condition as a checkbox and tick it once it is "
                    "done, or delete the block if this change needs no operator "
                    "acceptance.",
                )
            )
    return problems


def code_files(root: Path) -> list[Path]:
    files = []
    for path in root.rglob("*"):
        if not path.is_file() or path.suffix not in CODE_SUFFIXES:
            continue
        rel = path.relative_to(root)
        if SKIP_DIRS & set(rel.parts):
            continue
        if rel.as_posix() in SELF_PATHS:
            continue
        files.append(path)
    return sorted(files)


def unfinished_code_markers(root: Path) -> list[tuple[str, int, str]]:
    hits = []
    for path in code_files(root):
        rel = path.relative_to(root).as_posix()
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for number, line in enumerate(text.splitlines(), start=1):
            # Blank out string literals first: `const LABEL: &str = "TODO"` is
            # data, not unfinished work. A marker inside a multi-line raw string
            # still reports; that has not happened, and a false positive there is
            # cheap to resolve by naming a tracking issue.
            code = STRING_LITERAL.sub('""', line)
            if RUST_STUBS.search(code) or BARE_MARKER.search(code):
                hits.append((rel, number, line.strip()[:160]))
    return hits


def annotate(title: str, message: str, file: str | None = None,
             line: int | None = None) -> None:
    location = ""
    if file:
        location = f" file={file}"
        if line:
            location += f",line={line}"
    print(f"::error{location},title={title}::{message.replace(chr(10), '%0A')}")
    print(f"FAIL [{title}] {message}", file=sys.stderr)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=".")
    parser.add_argument("--title-file")
    parser.add_argument("--body-file")
    parser.add_argument(
        "--skip-tree", action="store_true", help="Only run the declaration checks."
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    title = os.environ.get("PR_TITLE", "")
    body = os.environ.get("PR_BODY", "")
    if args.title_file:
        title = Path(args.title_file).read_text(encoding="utf-8")
    if args.body_file:
        body = Path(args.body_file).read_text(encoding="utf-8")
    title = title.strip()

    failures = 0
    has_pr_context = bool(args.title_file or args.body_file or "PR_TITLE" in os.environ)

    if has_pr_context:
        if TITLE_UNFINISHED.search(title):
            failures += 1
            annotate(
                "Pull request title declares unfinished work",
                f"Title starts with an unfinished-work prefix: {title!r}. "
                "A pull request whose own title says it is not done must not merge. "
                "Finish the change and retitle it as the change it makes, or close it.",
            )

        prose = strip_fenced_blocks(body)

        if WIP_MARKER.search(prose):
            failures += 1
            annotate(
                "Pull request carries the WIP marker",
                "The body contains the maestro WIP marker HTML comment. The author "
                "declared this change unfinished, so it is blocked. Remove the marker "
                "only when the work it stands for is actually done; do not remove it "
                "to get a green check.",
            )

        for title, detail in acceptance_problems(prose):
            failures += 1
            annotate(title, detail)

        if not HTML_COMMENT.sub("", body).strip():
            failures += 1
            annotate(
                "Pull request has no description",
                "The body is empty. State what the change does, how it was verified, "
                "and what it deliberately does not do. Use the pull request template.",
            )

    if not args.skip_tree:
        for rel, number, line in unfinished_code_markers(root):
            failures += 1
            annotate(
                "Unfinished-code marker in shipped source",
                f"{line}\nRemove it, or reference a tracking issue so the work is not "
                "lost: TODO(#1234). Panicking stubs must not reach main at all.",
                file=rel,
                line=number,
            )

    if failures:
        print(f"FAILED: {failures} unfinished-work problem(s).", file=sys.stderr)
        return 1
    print("OK: nothing declares this change unfinished.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
