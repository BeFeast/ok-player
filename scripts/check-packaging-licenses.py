#!/usr/bin/env python3
"""Fail when a packaging lane stops shipping the licence documents.

OK Player is GPL-3.0-or-later and its shipped chrome icons are Adwaita artwork
taken under the LGPL-3 option, so every artifact handed to a user must be
accompanied by both licence documents (GPLv3 §4, LGPLv3 §4(b)) plus the notices
for what it bundles. That obligation is spread over five recipes in four
languages, none of which is exercised by the three checks that gate a pull
request - so dropping a document from a lane is invisible until a user asks for
the licence and it is not there.

This check reads each lane's recipe and asserts, per lane, that the documents
are installed at the path that ecosystem looks for them at:

  deb       /usr/share/doc/ok-player/{copyright,LICENSE,LICENSE.LGPL-3.0,
            THIRD-PARTY-NOTICES.md}   (Debian policy §12.5)
  appimage  usr/share/doc/ok-player/ inside the AppDir payload
  rpm       %license / %doc under %{_licensedir}/%{_docdir}
  flatpak   /app/share/licenses/com.befeast.okplayer/
  windows   next to the app in the Velopack publish directory

The two archive lanes route through `scripts/stage-license-documents.sh`, so
for those the check *runs* the staging code into a scratch directory and looks
at what actually landed, rather than trusting the script's text. The other
three lanes are declarative recipes - an rpm spec, a Flatpak manifest and a
PowerShell staging script - so there the declaration is the thing to read.

The runtime smokes that assert the installed paths are checked too: a lane can
also fall out of compliance by keeping the install and gutting the gate.

Usage: check-packaging-licenses.py [--root .]
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

# The documents, and the first line each real text starts with. A lane that
# installs an empty placeholder named `LICENSE` satisfies no obligation, so the
# check looks at the content of the root documents too.
ROOT_DOCUMENTS = {
    "LICENSE": "GNU GENERAL PUBLIC LICENSE",
    "LICENSE.LGPL-3.0": "GNU LESSER GENERAL PUBLIC LICENSE",
    "THIRD-PARTY-NOTICES.md": "# Third-Party Notices",
}

SHARED_DOCUMENTS = ("LICENSE", "LICENSE.LGPL-3.0", "THIRD-PARTY-NOTICES.md")

DEB_DOC_DIR = "usr/share/doc/ok-player"


class LaneResult:
    def __init__(self, name: str, recipe: str) -> None:
        self.name = name
        self.recipe = recipe
        self.failures: list[str] = []
        self.shipped: list[str] = []

    def require(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    @property
    def ok(self) -> bool:
        return not self.failures


def check_root_documents(root: Path) -> LaneResult:
    result = LaneResult("repository", "<root>")
    for name, opening in ROOT_DOCUMENTS.items():
        path = root / name
        if not path.is_file():
            result.require(False, f"{name} is missing from the repository root")
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        result.require(
            opening in text.split("\n\n", 1)[0],
            f"{name} does not open with {opening!r}; it is not the document it claims to be",
        )
        result.shipped.append(name)
    copyright_file = root / "rust/packaging/linux/copyright"
    result.require(
        copyright_file.is_file(),
        "rust/packaging/linux/copyright is missing (the Debian lane installs it)",
    )
    if copyright_file.is_file():
        text = copyright_file.read_text(encoding="utf-8", errors="replace")
        result.require(
            "copyright-format/1.0" in text,
            "rust/packaging/linux/copyright is not in the DEP-5 machine-readable format",
        )
        for token in ("GPL-3.0-or-later", "LGPL-3.0"):
            result.require(
                token in text,
                f"rust/packaging/linux/copyright never mentions {token}",
            )
        result.shipped.append("copyright")
    return result


def check_staging_helper(root: Path, lane: str, expected: tuple[str, ...]) -> list[str]:
    """Run the shared staging script and report what it failed to produce."""
    helper = root / "scripts/stage-license-documents.sh"
    if not helper.is_file():
        return ["scripts/stage-license-documents.sh is missing"]
    failures: list[str] = []
    with tempfile.TemporaryDirectory() as scratch:
        doc_dir = Path(scratch) / DEB_DOC_DIR
        completed = subprocess.run(
            ["bash", str(helper), lane, str(doc_dir)],
            capture_output=True,
            text=True,
            check=False,
        )
        if completed.returncode != 0:
            return [
                f"stage-license-documents.sh {lane} exited {completed.returncode}: "
                f"{completed.stderr.strip() or completed.stdout.strip()}"
            ]
        for name in expected:
            staged = doc_dir / name
            if not staged.is_file():
                failures.append(f"staging produced no {name} for the {lane} lane")
            elif staged.stat().st_size == 0:
                failures.append(f"staging produced an empty {name} for the {lane} lane")
    return failures


def check_deb(root: Path) -> LaneResult:
    recipe = "scripts/package-linux-deb.sh"
    result = LaneResult("deb", recipe)
    expected = SHARED_DOCUMENTS + ("copyright",)
    for failure in check_staging_helper(root, "deb", expected):
        result.require(False, failure)
    text = (root / recipe).read_text(encoding="utf-8")
    result.require(
        re.search(r"stage-license-documents\.sh[\"']?\s+deb\b", text) is not None,
        f"{recipe} no longer calls stage-license-documents.sh for the deb lane",
    )
    result.require(
        DEB_DOC_DIR in text,
        f"{recipe} no longer stages the documents under /{DEB_DOC_DIR}",
    )
    smoke = "scripts/smoke-linux-install-upgrade.sh"
    smoke_text = (root / smoke).read_text(encoding="utf-8")
    for name in expected:
        result.require(
            f"{DEB_DOC_DIR}/{name}" in smoke_text,
            f"{smoke} does not assert {DEB_DOC_DIR}/{name} after install",
        )
    result.shipped = list(expected)
    return result


def check_appimage(root: Path) -> LaneResult:
    recipe = "scripts/package-linux-velopack.sh"
    result = LaneResult("appimage", recipe)
    for failure in check_staging_helper(root, "appimage", SHARED_DOCUMENTS):
        result.require(False, failure)
    text = (root / recipe).read_text(encoding="utf-8")
    result.require(
        re.search(r"stage-license-documents\.sh[\"']?\s+appimage\b", text) is not None,
        f"{recipe} no longer calls stage-license-documents.sh for the appimage lane",
    )
    result.require(
        DEB_DOC_DIR in text,
        f"{recipe} no longer stages the documents under the AppDir's {DEB_DOC_DIR}",
    )
    result.shipped = list(SHARED_DOCUMENTS)
    return result


def check_rpm(root: Path) -> LaneResult:
    recipe = "rust/packaging/fedora/ok-player.spec"
    result = LaneResult("rpm", recipe)
    text = (root / recipe).read_text(encoding="utf-8")
    installs = "\n".join(
        line for line in text.splitlines() if line.strip().startswith(("install ", "install\t"))
        or "%{buildroot}" in line
    )
    files_section = section_of(text, "%files")
    for name in ("LICENSE", "LICENSE.LGPL-3.0"):
        result.require(
            f"_licensedir}}/%{{name}}/{name}" in installs,
            f"{recipe} %install no longer puts {name} in %{{_licensedir}}",
        )
        result.require(
            re.search(rf"^%license .*{re.escape(name)}$", files_section, re.M) is not None,
            f"{recipe} %files no longer marks {name} as %license",
        )
    result.require(
        "_docdir}/%{name}/THIRD-PARTY-NOTICES.md" in installs,
        f"{recipe} %install no longer puts THIRD-PARTY-NOTICES.md in %{{_docdir}}",
    )
    result.require(
        re.search(r"^%doc .*THIRD-PARTY-NOTICES\.md$", files_section, re.M) is not None,
        f"{recipe} %files no longer marks THIRD-PARTY-NOTICES.md as %doc",
    )
    smoke = "scripts/smoke-linux-rpm-install-upgrade.sh"
    smoke_text = (root / smoke).read_text(encoding="utf-8")
    for path in (
        "/usr/share/licenses/ok-player/LICENSE",
        "/usr/share/licenses/ok-player/LICENSE.LGPL-3.0",
        "/usr/share/doc/ok-player/THIRD-PARTY-NOTICES.md",
    ):
        result.require(
            path in smoke_text,
            f"{smoke} does not assert {path} after install",
        )
    result.shipped = list(SHARED_DOCUMENTS)
    return result


def section_of(text: str, header: str) -> str:
    """Return one rpm spec section body, up to the next section header."""
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == header)
    except StopIteration:
        return ""
    body: list[str] = []
    for line in lines[start + 1 :]:
        if re.match(r"^%[a-z]+\b", line) and not line.startswith(("%license", "%doc", "%{", "%dir")):
            break
        body.append(line)
    return "\n".join(body)


def check_flatpak(root: Path) -> LaneResult:
    recipe = "rust/packaging/flatpak/com.befeast.okplayer.json"
    result = LaneResult("flatpak", recipe)
    manifest = json.loads((root / recipe).read_text(encoding="utf-8"))
    app_id = manifest.get("app-id", "com.befeast.okplayer")
    commands: list[str] = []
    for module in manifest.get("modules", []):
        if isinstance(module, dict):
            commands.extend(str(c) for c in module.get("build-commands", []))
    joined = "\n".join(commands)
    for name in SHARED_DOCUMENTS:
        result.require(
            re.search(
                rf"install\b.*\b{re.escape(name)}\s+/app/share/licenses/{re.escape(app_id)}/{re.escape(name)}",
                joined,
            )
            is not None,
            f"{recipe} no longer installs {name} into /app/share/licenses/{app_id}/",
        )
    result.shipped = list(SHARED_DOCUMENTS)
    return result


def check_windows(root: Path) -> LaneResult:
    recipe = "installer/build-velopack.ps1"
    result = LaneResult("windows", recipe)
    text = (root / recipe).read_text(encoding="utf-8")
    # The GPL text is renamed on Windows, where a bare extensionless `LICENSE`
    # opens in no application by default.
    result.require(
        "'LICENSE.txt'" in text and "'LICENSE'" in text,
        f"{recipe} no longer stages LICENSE as LICENSE.txt next to the app",
    )
    result.require(
        "LICENSE.LGPL-3.0.txt" in text and "LICENSE.LGPL-3.0'" in text,
        f"{recipe} no longer stages LICENSE.LGPL-3.0 next to the app",
    )
    result.require(
        "THIRD-PARTY-NOTICES.md" in text,
        f"{recipe} no longer stages THIRD-PARTY-NOTICES.md next to the app",
    )
    assertion = "scripts/assert-windows-installed-tree.ps1"
    assertion_text = (root / assertion).read_text(encoding="utf-8")
    for name in ("LICENSE.txt", "LICENSE.LGPL-3.0.txt", "THIRD-PARTY-NOTICES.md"):
        result.require(
            name in assertion_text,
            f"{assertion} does not assert {name} in the installed tree",
        )
    result.shipped = ["LICENSE.txt", "LICENSE.LGPL-3.0.txt", "THIRD-PARTY-NOTICES.md"]
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", default=".", help="repository root to check")
    args = parser.parse_args()
    root = Path(args.root).resolve()

    checks = (
        check_root_documents,
        check_deb,
        check_appimage,
        check_rpm,
        check_flatpak,
        check_windows,
    )
    results = []
    for check in checks:
        try:
            results.append(check(root))
        except FileNotFoundError as error:
            broken = LaneResult(check.__name__.removeprefix("check_"), str(error.filename))
            broken.require(False, f"recipe is missing: {error.filename}")
            results.append(broken)

    width = max(len(result.name) for result in results)
    for result in results:
        status = "ok  " if result.ok else "FAIL"
        print(f"{status} {result.name.ljust(width)}  {result.recipe}")
        for failure in result.failures:
            print(f"       - {failure}")

    if all(result.ok for result in results):
        print("\nEvery packaging lane ships the GPL-3, the LGPL-3 and the third-party notices.")
        return 0
    print(
        "\nA packaging lane stopped shipping a licence document. "
        "GPLv3 §4 and LGPLv3 §4(b) both require them to travel with the package.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
