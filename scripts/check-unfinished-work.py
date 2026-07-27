#!/usr/bin/env python3
"""Fail a pull request that declares itself unfinished, or ships unfinished code.

Two cheap, mechanical rules that a merge robot cannot talk itself out of:

1. Declaration checks (skipped when no pull request context is supplied):
   * a title that starts with WIP / Draft / Do not merge;
   * the maestro WIP marker HTML comment in the body;
   * an acceptance section - "Operator acceptance" and its near neighbours,
     "Acceptance criteria", "Live acceptance hold", "Before merge" - that holds
     anything other than checkboxes and nested headings. An unticked box, a
     plain bullet, a sentence, a rule, a label, an HTML tag, or nothing at all:
     none of those can be recorded as performed;
   * an empty body, which cannot state what "done" means.

2. Tree checks: unfinished-code markers in shipped source and shipped
   configuration - `todo!()`, `unimplemented!()`, and bare TODO / FIXME
   comments. A marker that names a tracking issue (`TODO(#123)`) is accepted:
   the work is tracked, not lost. String literals are masked first, so a
   helper that *searches* for the marker text - `grep -n 'TODO' file`,
   `x = 'FIXME'` - is data rather than a declaration. Single quotes are masked
   only where they delimit a string (shell, Python, PowerShell, YAML, TOML);
   in Rust and the C family they open a lifetime or a character literal.

*Balanced* fenced code blocks are ignored when scanning the body, so a pull
request may quote these rules without tripping them; an unbalanced fence is
treated as text, because stripping it would silently disable every rule below
it. HTML comments are ignored when looking for boxes, so a template can carry a
commented-out acceptance block.

What this does NOT catch, on purpose or for want of a cheap rule:
  * a hold written outside an acceptance heading. A sentence in "Notes for
    review" saying the change needs manual verification is invisible here.
  * a hold inside a *balanced* fenced code block, or inside an HTML comment,
    anywhere - including inside an acceptance section. Both are blanked before
    any rule runs, so that a body may quote these rules and the template may
    ship a commented-out acceptance block. A fenced hold still renders and a
    human still reads it, so this one is an escape and not only a convenience;
    it is left open because closing it means unpicking the fence machinery for
    a shape nobody reaches by accident. Measured: exit 0.
  * an acceptance heading phrased outside `ACCEPTANCE_PHRASE`, e.g. "Manual
    verification required". The phrase list is a floor, not a parser.
  * a body that says nothing. Only a body that is *empty* after HTML comments
    are removed is rejected, so the pull request template with every placeholder
    comment left untouched passes: its section headings are visible text.
    Requiring prose under each heading was rejected deliberately - a mandatory
    "I verified this" field trains people to fill it in, which is the failure
    this whole check exists to stop.
  * a ticked box that nobody performed. No mechanical check can see this; it is
    the reason the rules above exist rather than a substitute for honesty.
  * a marker inside a multi-line raw string in shipped source. Literals are
    masked one line at a time, so `r#"... TODO ..."#` spanning lines reports.
  * an identifier spelled TODO or FIXME - `const TODO: bool = false;` reports.
    Only string literals are masked; the marker scan cannot tell a name from a
    comment, and every rule that could (extract comments, then scan only those)
    gives up bare markers in code, which are the shape a half-written function
    leaves behind. Rename the constant, or name a tracking issue.
  * a marker inside a single-quoted string in a language that is not in
    `SINGLE_QUOTE_STRING_SUFFIXES` - a C# or C++ verbatim-ish `'...'` does not
    exist, but a marker inside an XML element's text, e.g.
    `<Text>TODO</Text>`, reports. That is intentional: shipped UI text saying
    TODO is unfinished work.
  * unfinished work in Markdown. `.md` is out of scope: prose discusses markers
    legitimately, and a check that fires on documentation gets muted.
  * a setext heading used as a *terminator*. A setext underline opens an
    acceptance section but never ends one, because `**Windows**` followed by
    `---` is also a valid setext heading and honouring it as a terminator would
    hand back the laundering route the section bounds exist to close. The cost
    is over-blocking: a genuine setext heading below an acceptance section does
    not get its content out of the section, a `##` heading does.

Known FALSE POSITIVE, not fixed: the acceptance-heading phrase accepts up to 60
characters of suffix, so a results section such as `## Acceptance test results`
opens a section and its prose is rejected as an unrecordable hold. Constraining
the suffix to the recognised qualifiers would drop real holds titled, say,
`## Operator acceptance for the packaged build`, and this check is deliberately
tuned to over-block rather than miss. Rename the heading (`## Acceptance
testing: results`, `## QA results`) or move the prose above the block.

A known cost of the acceptance rules: because a heading-opened acceptance
section runs to the next heading of its own level or above, anything appended
below a trailing acceptance section - a review bot's bulleted summary, for
instance - is read as part of that section and blocks the merge. Separate it
with a heading, or keep the acceptance block from being the last section. This
direction is deliberate: a false alarm is visible and self-correcting, a missed
prose hold is neither.

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
# What ends a *weakly* opened block - one opened by a bold label or a bare label
# ending in a colon, which cannot claim more of the body than the next label of
# the same kind: a markdown heading, or another bold/underlined label line.
#
# Deliberately NOT a rule or an HTML tag. `**Operator acceptance**`, a ticked
# box, `---`, an unticked box exited 0 while they were terminators, which is the
# same laundering the heading-opened bounds close - a rule and a `<details>` are
# markup dropped between two items, not a new section. The cost is the same too:
# a weakly opened block runs to the next label or heading, so a list under a
# rule below it is read as part of the block.
#
# Deliberately NOT a bare label ending in a colon either, even though that form
# *opens* a block. Measured: adding `[^*_#\n.!?]{1,60}:\s*$` here fixes the
# false positive it is meant to fix - `Operator acceptance:` / `- [x] Verified`
# / `Review notes:` / prose stops being reported - and in the same move takes
# `Operator acceptance:` / `- [x] Verified` / `Still to do before merge:` /
# plain bullets from exit 1 to exit 0. That second body is the historical hold
# shape with a shorter sentence, so the trade is a real escape for a cosmetic
# rejection. Open an acceptance block with a `##` heading and the question does
# not arise: the section ends at the next heading and prose below it is outside.
WEAK_BREAK = re.compile(
    r"^\s*(?:#{1,6}\s|\*\*[^*]+\*\*\s*:?\s*$|__[^_]+__\s*:?\s*$)"
)
# A markdown heading is the only terminator strong enough to end an acceptance
# *section*, and only one at the same level or above: `### Windows` nested under
# `## Operator acceptance` groups the checks, it does not end them. A bold
# label, a rule or an HTML tag is markup *inside* the section - the whole point
# of the section bounds is that dropping one of those above a hold must not
# hide it.
SECTION_BREAK = re.compile(r"^\s*(#{1,6})\s")
LIST_ITEM = re.compile(r"^\s*(?:[-*+]|\d+\.)\s+(.*)$")
UNCHECKED_BOX = re.compile(r"^\[\s\]\s*(.*)$")
CHECKED_BOX = re.compile(r"^\[[xX]\]\s*(.*)$")
# CommonMark allows a fenced block to be indented by at most three spaces; four
# make it an indented code block, and its ``` is literal text. Accepting any
# indentation let an indented pair of fences strip a WIP marker sitting between
# them. The strict bound only ever strips less, so it cannot hide a rule.
# The trailing group is the info string. Only an *opening* fence may carry one;
# a line such as ```python is content, not a closer. Accepting it as a closer
# paired an opener with the wrong line and stripped a marker sitting between
# them.
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")
HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)
# `Operator acceptance` on one line and `---` (or `===`) on the next is a valid
# setext heading, and the phrase alone matches no ATX or label form, so the
# section it opened was invisible. A setext underline only ever *opens* here: it
# never ends an acceptance section, because `**Windows**` followed by `---` is
# also a setext heading and letting one terminate would reopen exactly the
# laundering route this file spent a round closing.
SETEXT_UNDERLINE = re.compile(r"^ {0,3}(=+|-+)\s*$")
ACCEPTANCE_LABEL_ONLY = re.compile(
    rf"^\s*{ACCEPTANCE_PHRASE}[^.!?:]{{0,40}}\s*$",
    re.IGNORECASE | re.VERBOSE,
)

# Unfinished-code markers. A marker that references a tracking issue is fine.
# Rust tokenises `todo !()` the same as `todo!()`, so the bang may sit apart.
RUST_STUBS = re.compile(r"\b(?:todo|unimplemented)\s*!\s*[\(\[\{]")
DOUBLE_QUOTED = r"\"(?:\\.|[^\"\\])*\""
# A character literal holds exactly one character or one escape. Allowing more
# let `fn choose<'a /* TODO */, 'b>` read as one literal from `'a` to `'b`,
# blanking a real marker sitting between two Rust lifetimes.
CHAR_LITERAL = r"'(?:\\.|[^'\\\n])'"
# Where `'...'` delimits a *string*, its contents are data exactly like a
# double-quoted string's: `grep -n 'TODO' file` searches for the marker, it does
# not declare one. Two narrowings keep this from blanking real markers:
#   * the opening quote may not follow a word character, so the apostrophes in
#     `# don't drop the TODO marker: it's tracked` do not pair up around it.
#     A Python string prefix (b/f/r/u) is the one thing allowed in front;
#   * the string may not span lines, so an unmatched apostrophe masks nothing.
# There is no escape handling: `'a\'` in shell is a complete string, and
# treating a backslash as an escape would run the mask past the closing quote.
# The cost is the safe direction - `'don\'t TODO'` in Python reports.
SINGLE_QUOTED = r"(?<![A-Za-z0-9_])[bBfFrRuU]{0,2}'[^'\n]*'"
STRING_LITERAL = re.compile(f"{DOUBLE_QUOTED}|{CHAR_LITERAL}")
STRING_OR_SINGLE_QUOTED = re.compile(f"{DOUBLE_QUOTED}|{SINGLE_QUOTED}")
# Languages whose `'...'` is a string. Rust, C, C++ and C# are absent on
# purpose: there `'a` is a lifetime or a character literal, and pairing two of
# them blanks whatever sits between - the defect the char-literal rule above
# was narrowed to fix. XML-family files are absent too: `'` delimits an
# attribute value there but is also an apostrophe in element text.
SINGLE_QUOTE_STRING_SUFFIXES = {
    ".sh", ".spec", ".dockerfile", ".Dockerfile",  # shell command lines
    ".py", ".ps1", ".psm1", ".yml", ".yaml", ".toml",
}
BARE_MARKER = re.compile(r"\b(TODO|FIXME)\b(?!\s*\(\s*#\d+\s*\))")
# Shipped source and shipped configuration. Workflow and manifest files are
# here because a half-finished CI job is unfinished work like any other - the
# two workflows this gate itself adds are scanned by it. Markdown is out of
# scope on purpose: prose discusses markers legitimately, and a check that
# fires on documentation gets muted.
CODE_SUFFIXES = {
    ".rs", ".cs", ".sh", ".ps1", ".psm1", ".py", ".c", ".h", ".cpp", ".xaml",
    ".yml", ".yaml", ".toml", ".json", ".manifest",
    ".xml", ".props", ".targets", ".csproj", ".sln",
    # Shipped packaging and build configuration. A half-finished RPM spec, a
    # desktop entry with a missing MimeType, or an unpinned base image is
    # unfinished work that reaches users through an artifact.
    ".spec", ".desktop", ".dockerfile", ".Dockerfile", ".service",
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
    # A fence closes only with the same character and at least as many of them.
    # Pairing by position alone let a ``` opener pair with a later ~~~ line, so
    # a marker written between two mismatched fences was stripped away.
    fences = []
    for index, line in enumerate(lines):
        match = FENCE.match(line)
        if match:
            marker = match.group(1)
            fences.append((index, marker[0], len(marker), match.group(2).strip()))
    paired: set[int] = set()
    open_at: tuple[int, str, int] | None = None
    for index, char, width, info in fences:
        if open_at is None:
            open_at = (index, char, width)
            continue
        # A closing fence carries no info string. ```python inside a block is
        # content; treating it as the closer let the real closer open a second
        # block and a marker between them was stripped.
        if char == open_at[1] and width >= open_at[2] and not info:
            paired.add(open_at[0])
            paired.add(index)
            open_at = None
    out, inside = [], False
    for i, line in enumerate(lines):
        if i in paired:
            inside = not inside
            continue
        out.append("" if inside else line)
    return "\n".join(out)


def acceptance_sections(body: str) -> list[list[str]]:
    """Every acceptance section, bounded only by a terminator of equal strength.

    Tight bounds - ending a block at the first terminator of any strength - were
    a laundering route: one ticked box followed by `<details>`, `**Windows**` or
    `---` hid everything after it, and exit 0 was the answer to

        ## Operator acceptance
        - [x] smoke run
        **Still pending**
        - [ ] dual-display QA

    and to the same body with a prose hold in place of the unticked box. That
    second shape is the one every real acceptance hold in this repository took,
    so *all* the acceptance rules read these bounds; there is deliberately no
    second, narrower notion of a "block".

    A section opened by a markdown heading runs to the next heading at the same
    level or above - a nested `### Windows` groups checks rather than ending
    them. A section opened by a weaker label (a bold line, a bare label ending
    in a colon) still ends at any terminator, so a bold label cannot swallow an
    unrelated follow-up list below it - unless it opened *inside* a
    heading-opened section, in which case it inherits that section's level.
    Without that inheritance, writing `**Acceptance criteria:**` under
    `## Operator acceptance` downgraded the bounds back to the weak ones and
    reopened the laundering route this function exists to close.
    """
    lines = HTML_COMMENT.sub("", body).splitlines()
    sections: list[list[str]] = []
    current: list[str] | None = None
    level = 0
    skip_next = False
    for index, line in enumerate(lines):
        if skip_next:
            skip_next = False
            continue
        setext = None
        if not ACCEPTANCE_HEADING.match(line) and ACCEPTANCE_LABEL_ONLY.match(line):
            underline = (
                SETEXT_UNDERLINE.match(lines[index + 1])
                if index + 1 < len(lines)
                else None
            )
            if underline:
                setext = 1 if underline.group(1).startswith("=") else 2
        if setext or ACCEPTANCE_HEADING.match(line):
            opener = SECTION_BREAK.match(line)
            if setext:
                level = setext
                skip_next = True
            elif opener:
                level = len(opener.group(1))
            elif current is None:
                level = 0
            # else: a weak label inside a section keeps the enclosing level.
            if current is not None:
                sections.append(current)
            current = []
            continue
        if current is None:
            continue
        if level:
            heading = SECTION_BREAK.match(line)
            ends = heading is not None and len(heading.group(1)) <= level
        else:
            ends = WEAK_BREAK.match(line) is not None
        if ends:
            sections.append(current)
            current = None
            level = 0
            continue
        current.append(line)
    if current is not None:
        sections.append(current)
    return sections


def acceptance_problems(body: str) -> list[tuple[str, str]]:
    """(title, detail) for every Operator acceptance block that is not resolved.

    An acceptance block is resolved only when it consists of ticked checkboxes,
    optionally grouped under nested headings. Anything else - an unticked box, a
    plain bullet, a paragraph such as "Do not merge until a packaged build
    passes dual-display QA", or markup dropped in to break the block up - states
    or hides a hold that nothing can record as performed. Every real acceptance
    hold in this repository's history took one of the first three shapes.

    Every rule below reads `acceptance_sections`. They used to disagree: only
    the unticked-box rule saw the whole section, so a `<details>`, a `**bold**`
    label or a `---` above a *prose* hold hid it - and prose is the shape the
    historical holds took.
    """
    problems = []

    for section in acceptance_sections(body):
        unchecked, unresolvable, checked = [], [], 0
        prose = []
        for line in section:
            item = LIST_ITEM.match(line)
            if not item:
                # A nested heading groups the items below it; it is structure,
                # not a hold. Everything else that is not a checkbox - a
                # sentence, a rule, a label, an HTML tag - is reported, because
                # the contract for this block is "checkboxes and nothing else"
                # and because letting markup through is what hid the holds.
                if line.strip() and not SECTION_BREAK.match(line):
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
        if prose:
            problems.append(
                (
                    "Operator acceptance block states a hold in prose",
                    "An Operator acceptance block may contain checkboxes, and nested "
                    "headings to group them, and nothing else: a sentence cannot "
                    "record that it was performed, and a rule, a label or an HTML "
                    "tag dropped into the block is how a hold below it gets hidden. "
                    "This line is neither a checkbox nor a nested heading: "
                    + summarise(prose)
                    + ". Move each condition into its own checkbox and tick it once "
                    "it is done, and put context and review summaries above the "
                    "block or under a heading of their own. A ticked box elsewhere "
                    "in the block does not cover it.",
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
    hits: dict[tuple[str, int], str] = {}
    for path in code_files(root):
        rel = path.relative_to(root).as_posix()
        try:
            text = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        lines = text.splitlines()
        # Blank out string literals first: `const LABEL: &str = "TODO"` is data,
        # not unfinished work. Masking runs one line at a time, so a marker
        # inside a multi-line raw string still reports; that has not happened,
        # and a false positive there is cheap to resolve by naming a tracking
        # issue.
        literals = (
            STRING_OR_SINGLE_QUOTED
            if path.suffix in SINGLE_QUOTE_STRING_SUFFIXES
            else STRING_LITERAL
        )
        masked = [literals.sub('""', line) for line in lines]
        for number, code in enumerate(masked, start=1):
            if BARE_MARKER.search(code):
                hits[(rel, number)] = lines[number - 1].strip()[:160]
        # `todo` and its `!()` may sit on different lines - rustc tokenises
        # `todo\n!()` exactly like `todo!()`, so a per-line scan missed a real
        # panicking stub. Search the joined text and report the line the
        # identifier is on.
        joined = "\n".join(masked)
        for match in RUST_STUBS.finditer(joined):
            number = joined.count("\n", 0, match.start()) + 1
            hits.setdefault((rel, number), lines[number - 1].strip()[:160])
    return [(rel, number, line) for (rel, number), line in sorted(hits.items())]


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
