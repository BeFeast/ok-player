"""Check the Flatpak offline Cargo vendor set against a lockfile.

`cargo --offline --locked` builds from `cargo-sources.json`, which is generated
from one `rust/Cargo.lock`. The application pin can move across a lockfile
change, and nothing regenerates the vendor set when it does - so the pair can
drift into "the manifest pins a commit whose lockfile needs crates the vendor
set does not carry". That fails deep inside an eight-minute offline build with a
cargo error about a missing crate; checked here it fails in a second with the
reason.

The comparison is the crate identity set: every registry package in the lockfile
must have a vendored archive, and every vendored archive must be in the lockfile.
It does not check crate contents - the `sha256` on each archive source is what
does that, and the offline build is what proves the set is usable.
"""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path


def locked_crates(lock_path: Path) -> set[str]:
    lock = tomllib.loads(lock_path.read_text())
    return {
        f"{package['name']}-{package['version']}"
        for package in lock.get("package", [])
        if str(package.get("source", "")).startswith("registry+")
    }


def vendored_crates(sources_path: Path) -> set[str]:
    sources = json.loads(sources_path.read_text())
    prefix = "cargo/vendor/"
    return {
        source["dest"][len(prefix) :]
        for source in sources
        if isinstance(source, dict)
        and source.get("type") == "archive"
        and str(source.get("dest", "")).startswith(prefix)
    }


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(
            "usage: flatpak_cargo_sources.py <Cargo.lock> <cargo-sources.json>",
            file=sys.stderr,
        )
        return 2
    lock_path, sources_path = Path(argv[1]), Path(argv[2])
    locked = locked_crates(lock_path)
    vendored = vendored_crates(sources_path)

    problems = []
    missing = sorted(locked - vendored)
    if missing:
        problems.append(
            "the pinned lockfile needs crates the offline vendor set does not carry:\n"
            + "\n".join(missing)
        )
    extra = sorted(vendored - locked)
    if extra:
        problems.append(
            "the offline vendor set carries crates the pinned lockfile does not use:\n"
            + "\n".join(extra)
        )
    if problems:
        print(
            "Flatpak offline Cargo sources do not match the pinned lockfile.\n"
            + "\n\n".join(problems)
            + "\n\nRegenerate rust/packaging/flatpak/cargo-sources.json for the pinned "
            "commit's rust/Cargo.lock and commit it with the pin.",
            file=sys.stderr,
        )
        return 1
    print(f"Flatpak offline Cargo sources match the pinned lockfile ({len(locked)} crates)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
