#!/usr/bin/env python3
"""Fail a pull request that declares itself unfinished, or ships unfinished code.

Two cheap, mechanical rules that a merge robot cannot talk itself out of:

1. Declaration checks (skipped when no pull request context is supplied):
   * a title that starts with WIP / Draft / Do not merge;
   * the maestro WIP marker HTML comment in the body;
   * unchecked boxes anywhere in an acceptance section - "Operator acceptance"
     and its near neighbours, "Acceptance criteria", "Live acceptance hold",
     "Before merge";
   * an acceptance block holding plain bullets, prose, or nothing at all, none
     of which anyone can record as performed;
   * an empty body, which cannot state what "done" means.

2. Tree checks: unfinished-code markers in shipped source and shipped
   configuration - `todo!()`, `unimplemented!()`, and bare TODO / FIXME
   comments. A marker that names a tracking issue (`TODO(#123)`) is accepted:
   the work is tracked, not lost.

*Balanced* fenced code blocks are ignored when scanning the body, so a pull
request may quote these rules without tripping them; an unbalanced fence is
treated as text, because stripping it would silently disable every rule below
it. HTML comments are ignored when looking for boxes, so a template can carry a
commented-out acceptance block.

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
# The phrases that open an acceptance hold. "Operator acceptance" is the shape
# the template asks for, but a hold does not stop being a hold because it was
# titled differently: #621 used "Live acceptance hold". One leading word is
# allowed before "acceptance" and one qualifier after it.
ACCEPTANCE_PHRASE = (
    r"(?:(?:[A-Za-z]+\s+)?(?:acceptance|sign[\s\-]?off)"
    r"(?:\s+(?:hold|holds|criteria|checklist|gate|required|needed|pending))?"
    r"|before\s+merge)"
)
# A real heading or label line, not a sentence that happens to start with the
# phrase: "Operator acceptance is not required for this change." is prose.
ACCEPTANCE_HEADING = re.compile(
    rf"""^\s*(?:
          \#{{1,6}}\s*{ACCEPTANCE_PHRASE}[^.!?]{{0,60}}
        | (?:\*\*|__)\s*{ACCEPTANCE_PHRASE}[^*_.!?]{{0,60}}(?:\*\*|__)\s*:?
        | {ACCEPTANCE_PHRASE}[^.!?:]{{0,40}}:
    )\s*$""",
    re.IGNORECASE | re.VERBOSE,
)
# What ends a block: a markdown heading, a rule, or a bold/underlined label line
# used as a heading (pull request bodies in this repository do all three).
HEADING = re.compile(
    r"^\s*(?:#{1,6}\s|---\s*$|\*\*[^*]+\*\*\s*:?\s*$|__[^_]+__\s*:?\s*$"
    r"|<[A-Za-z/][^>]*>)"
)
# A markdown heading is the only terminator strong enough to end an acceptance
# *section*, and only one at the same level or above: `### Windows` nested under
# `## Operator acceptance` groups the checks, it does not end them. A bold
# label, a rule or an HTML tag is a sub-label inside the section.
SECTION_BREAK = re.compile(r"^\s*(#{1,6})\s")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+\.)\s+(.*)$")
UNCHECKED_BOX = re.compile(r"^\[\s\]\s*(.*)$")
CHECKED_BOX = re.compile(r"^\[[xX]\]\s*(.*)$")
FENCE = re.compile(r"^\s*(?:```|~~~)")
HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)

# Unfinished-code markers. A marker that references a tracking issue is fine.
RUST_STUBS = re.compile(r"\b(?:todo|unimplemented)!\s*[\(\[\{]")
# A character literal holds exactly one character or one escape. Allowing more
# let `fn choose<'a /* TODO */, 'b>` read as one literal from `'a` to `'b`,
# blanking a real marker sitting between two Rust lifetimes.
STRING_LITERAL = re.compile(r"\"(?:\\.|[^\"\\])*\"|'(?:\\.|[^'\\\n])'")
BARE_MARKER = re.compile(r"\b(TODO|FIXME)\b(?!\s*\(\s*#\d+\s*\))")
# Shipped source and shipped configuration. Workflow and manifest files are
# here because a half-finished CI job is unfinished work like any other - the
# two workflows this gate itself adds are scanned by it. Markdown is out of
# scope on purpose: prose discusses markers legitimately, and a check that
# fires on documentation gets muted.
CODE_SUFFIXES = {
    ".rs", ".cs", ".sh", ".ps1", ".psm1", ".py", ".c", ".h", ".cpp", ".xaml",
    ".yml", ".yaml", ".toml", ".json",
}
SKIP_DIRS = {".git", "target", "node_modules"}
# `bin` and `obj` are .NET build output, but only where a project file puts
# them. Skipping every directory with those names would blind the scan to a
# checked-in source directory that happens to be called `bin`.
DOTNET_OUTPUT_DIRS = {"bin", "obj"}
PROJECT_FILE_SUFFIXES = {".csproj", ".vbproj", ".fsproj", ".vcxproj"}
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
    """Blank out fenced code blocks so a body may quote these rules.

    Only a fence with a closing partner is stripped. Toggling on every fence
    meant a single unbalanced ``` blanked the whole rest of the body and
    silently disabled every rule below it - the WIP marker scan included. A
    body's markdown is edited by review bots in this repository, so an odd
    fence count is not a hypothetical.
    """
    lines = text.splitlines()
    fences = [i for i, line in enumerate(lines) if FENCE.match(line)]
    paired = set()
    for opener, closer in zip(fences[0::2], fences[1::2]):
        paired.add(opener)
        paired.add(closer)
    out, inside = [], False
    for i, line in enumerate(lines):
        if i in paired:
            inside = not inside
            continue
        out.append("" if inside else line)
    return "\n".join(out)


def acceptance_blocks(body: str) -> list[list[str]]:
    """The body lines of every acceptance block, HTML comments removed.

    A block ends at the first terminator of any strength: a heading, a rule, a
    bold label, an HTML tag. These tight bounds exist because the prose and
    plain-bullet rules read whatever is inside, and a review bot's appended
    bullet summary must not be mistaken for acceptance items.
    """
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


def acceptance_sections(body: str) -> list[list[str]]:
    """Every acceptance section, bounded only by a terminator of equal strength.

    The tight bounds above were a laundering route: one ticked box followed by
    any terminator - `<div>`, `**Still pending**`, `---` - hid every unticked
    box after it, and exit 0 was the answer to

        ## Operator acceptance
        - [x] smoke run
        **Still pending**
        - [ ] dual-display QA

    An unticked checkbox is unambiguous wherever it sits, so it is scanned over
    the whole section. A section opened by a markdown heading runs to the next
    heading at the same level or above - a nested `### Windows` groups checks
    rather than ending them. A section opened by a weaker label (a bold line, a
    bare label ending in a colon) still ends at any terminator, so a bold label
    cannot swallow an unrelated follow-up list below it.
    """
    lines = HTML_COMMENT.sub("", body).splitlines()
    sections: list[list[str]] = []
    current: list[str] | None = None
    level = 0
    for line in lines:
        if ACCEPTANCE_HEADING.match(line):
            if current is not None:
                sections.append(current)
            opener = SECTION_BREAK.match(line)
            current = []
            level = len(opener.group(1)) if opener else 0
            continue
        if current is None:
            continue
        if level:
            heading = SECTION_BREAK.match(line)
            ends = heading is not None and len(heading.group(1)) <= level
        else:
            ends = HEADING.match(line) is not None
        if ends:
            sections.append(current)
            current = None
            continue
        current.append(line)
    if current is not None:
        sections.append(current)
    return sections


def acceptance_problems(body: str) -> list[tuple[str, str]]:
    """(title, detail) for every Operator acceptance block that is not resolved.

    An acceptance block is resolved only when it consists of ticked checkboxes.
    Anything else - an unticked box, a plain bullet, or a paragraph such as "Do
    not merge until a packaged build passes dual-display QA" - states a hold
    that nothing can record as performed. Every real acceptance hold in this
    repository's history took one of those two unrecordable shapes.
    """
    problems = []

    # Unticked boxes are scanned over the whole section, so that a terminator
    # dropped between two items cannot hide the ones below it.
    for section in acceptance_sections(body):
        unchecked = []
        for line in section:
            item = LIST_ITEM.match(line)
            if not item:
                continue
            box = UNCHECKED_BOX.match(item.group(1).strip())
            if box:
                unchecked.append(box.group(1).strip() or "(unnamed item)")
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
        if prose:
            problems.append(
                (
                    "Operator acceptance block states a hold in prose",
                    "An Operator acceptance block may contain checkboxes and nothing "
                    "else, because a sentence cannot record that it was performed. "
                    "This prose states or qualifies a hold: "
                    + summarise(prose)
                    + ". Move each condition into its own checkbox and tick it once "
                    "it is done. A ticked box elsewhere in the block does not cover "
                    "it.",
                )
            )
        elif not unchecked and not unresolvable and not checked:
            problems.append(
                (
                    "Operator acceptance block is empty",
                    "This Operator acceptance block states nothing, so nothing can be "
                    "performed or recorded. State each condition as a checkbox, or "
                    "delete the block if this change needs no operator acceptance.",
                )
            )
    return problems


def is_dotnet_output(root: Path, rel: Path) -> bool:
    """Is any component of rel a .NET output directory beside a project file?"""
    current = root
    for part in rel.parts[:-1]:
        if part in DOTNET_OUTPUT_DIRS and any(
            sibling.suffix in PROJECT_FILE_SUFFIXES
            for sibling in current.iterdir()
            if sibling.is_file()
        ):
            return True
        current = current / part
    return False


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
        if is_dotnet_output(root, rel):
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

        for problem, detail in acceptance_problems(prose):
            failures += 1
            annotate(problem, detail)

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
