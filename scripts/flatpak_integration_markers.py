"""Integration markers for the Flatpak application source.

The Flatpak manifest builds a frozen pair: a pinned upstream commit plus the
checked-in integration patch that carries the Flatpak work not yet on that
commit. This module answers one question about that pair: does the tree
flatpak-builder will actually build still contain the integration this lane
packages?

Each repository path the patch touches declares markers of the behaviour it
carries, and every marker must be found in the pinned-and-patched tree. Markers
are checked against the built tree rather than against the patch, so a marker
keeps passing once the behaviour lands upstream and the hunk disappears - and
starts failing the moment a hunk is dropped without the pin having caught up.

What a marker match does and does not prove
-------------------------------------------
A marker must occur on a line that is *not* a comment line: a commented-out copy
of the integration does not satisfy it. That is the whole of the guarantee. A
marker match does not prove the line is reachable, compiled, or correct - a
string literal or dead-but-uncommented code would still satisfy it. The "and the
result builds" half of the contract is the offline Flatpak build; the "and it
behaves" half is the lifecycle and renderer lanes.

Comment recognition is deliberately shallow and line-oriented: a line counts as
a comment only when its first non-whitespace characters open a comment, and a
block comment runs until the line that closes it. That is enough to reject a
commented-out block, which is the laundering path this guards against. An
unknown file suffix is an error rather than a silent pass, so a newly patched
file type cannot slip through with no comment syntax defined.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# One entry per repository path the integration patch carries.
REQUIRED: dict[str, list[str]] = {
    "THIRD-PARTY-NOTICES.md": [
        "source-built rendering library in the Flatpak beta",
        "libplacebo",
    ],
    "rust/crates/okp-core/src/linux_renderer.rs": [
        "software-no-dri",
        "pub const fn select_linux_renderer(flatpak: bool, dri_accessible: bool)",
    ],
    "rust/crates/okp-core/src/playback_failure.rs": [
        "org.freedesktop.Platform.codecs-extra",
        "CodecEnvironment::Flatpak",
        "pub fn diagnose_mpv_runtime(",
    ],
    "rust/crates/okp-mpv/src/ffi.rs": [
        "pub const MPV_RENDER_PARAM_ADVANCED_CONTROL",
    ],
    "rust/crates/okp-mpv/src/player.rs": [
        "MPV_RENDER_PARAM_ADVANCED_CONTROL",
        "DecoderFailed {",
    ],
    "rust/crates/okp-mpv/src/pump.rs": [
        "codec_failure_reported",
        "MpvEvent::DecoderFailed {",
    ],
    "rust/crates/okp-linux-gtk/src/about.rs": [
        "if flatpak_update_managed() {",
        '"Flatpak managed"',
    ],
    "rust/crates/okp-linux-gtk/src/main.rs": [
        "enum LinuxExternalUpdateManager {",
        '"Updates are managed by Flatpak"',
    ],
    "rust/crates/okp-linux-gtk/src/mpv_bridge.rs": [
        "mpv.create_render_context(native_wayland_display, true)",
        "mpv.create_render_context(native_wayland_display, false)",
    ],
    "rust/crates/okp-linux-gtk/src/playlist_ops.rs": [
        "CodecEnvironment::Flatpak",
        "pub(crate) fn apply_runtime_decoder_failure(",
        "diagnose_mpv_runtime(",
    ],
    "rust/crates/okp-linux-gtk/src/screenshots.rs": [
        "fn screenshot_staging_dir() -> PathBuf {",
        "fn copy_to_destination_stage(",
    ],
    "rust/crates/okp-linux-gtk/src/tests.rs": [
        "fn native_wayland_screenshots_use_libmpv_advanced_control() {",
        "fn runtime_decoder_failure_stops_partial_playback_state_immediately() {",
        '"Updates are managed by Flatpak"',
    ],
    "rust/crates/okp-linux-gtk/src/track_popovers.rs": [
        "MpvEvent::DecoderFailed {",
    ],
    "rust/crates/okp-linux-gtk/src/updates.rs": [
        "Managed by Flatpak",
        "pub(crate) fn flatpak_update_managed() -> bool {",
    ],
    "rust/crates/okp-linux-gtk/src/window.rs": [
        "&& !flatpak_update_managed()",
    ],
}

# Comment syntax per file suffix: (line-comment openers, (block open, block close)).
COMMENT_SYNTAX: dict[str, tuple[tuple[str, ...], tuple[str, str] | None]] = {
    ".md": ((), ("<!--", "-->")),
    ".rs": (("//",), ("/*", "*/")),
    ".sh": (("#",), None),
    ".yml": (("#",), None),
    ".yaml": (("#",), None),
    ".py": (("#",), None),
}


class UnknownCommentSyntax(ValueError):
    """Raised when a checked file has no comment syntax defined."""


def code_text(name: str, text: str) -> str:
    """Return `text` with whole-line comments blanked out.

    Line count is preserved so that reported context stays comparable. A line is
    blanked when its first non-whitespace characters open a line comment, or when
    it falls inside a block comment that was itself opened at the start of a
    line.
    """
    suffix = Path(name).suffix
    if suffix not in COMMENT_SYNTAX:
        raise UnknownCommentSyntax(
            f"{name}: no comment syntax defined for '{suffix}' files. "
            "Add one to COMMENT_SYNTAX rather than letting comments count as code."
        )
    line_openers, block = COMMENT_SYNTAX[suffix]
    out: list[str] = []
    in_block = False
    for line in text.splitlines():
        stripped = line.lstrip()
        if in_block:
            out.append("")
            if block is not None and block[1] in line:
                in_block = False
            continue
        if block is not None and stripped.startswith(block[0]):
            out.append("")
            if block[1] not in stripped[len(block[0]) :]:
                in_block = True
            continue
        if line_openers and stripped.startswith(line_openers):
            out.append("")
            continue
        out.append(line)
    return "\n".join(out)


def patched_paths(patch_text: str) -> set[str]:
    return {
        match.group(1)
        for match in re.finditer(r"^diff --git a/(\S+) b/\S+$", patch_text, re.MULTILINE)
    }


def unmarked_paths(patch_text: str) -> list[str]:
    """Paths the patch touches that declare no marker at all."""
    return sorted(patched_paths(patch_text) - set(REQUIRED))


def missing_markers(tree: Path) -> list[str]:
    """Markers absent from non-comment lines of the pinned-and-patched tree."""
    missing: list[str] = []
    for relative, markers in REQUIRED.items():
        path = tree / relative
        if not path.is_file():
            missing.append(f"{relative}: missing from the pinned and patched tree")
            continue
        text = code_text(relative, path.read_text())
        for marker in markers:
            if marker not in text:
                missing.append(
                    f"{relative}: missing {marker!r} outside comments"
                )
    return missing


def main(argv: list[str]) -> int:
    if not 2 <= len(argv) <= 3:
        print(
            "usage: flatpak_integration_markers.py <pinned-and-patched-tree> [<patch>]",
            file=sys.stderr,
        )
        return 2
    tree = Path(argv[1])
    problems: list[str] = []

    if len(argv) == 3:
        patch = Path(argv[2])
        if patch.is_file():
            # Without this, a newly patched file could be added with no marker
            # and its hunk could then be deleted with the gate green.
            unmarked = unmarked_paths(patch.read_text())
            if unmarked:
                problems.append(
                    "The Flatpak integration patch touches files with no integration marker:\n"
                    + "\n".join(unmarked)
                )

    missing = missing_markers(tree)
    if missing:
        problems.append(
            "The pinned Flatpak source does not carry the integration it packages\n"
            "(a marker only found on a comment line does not count):\n"
            + "\n".join(missing)
        )

    if problems:
        print("\n\n".join(problems), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
