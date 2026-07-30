#!/usr/bin/env python3
"""Turn one harness run into the evidence rows the release validator reads.

A row is written as PASS only when its check file says PASS. There is no path here that
invents a status, and a row whose check never ran stays `not-run`, because the whole point
of the level this emits is that it is auditable after the fact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys

# Kept in step with `okp_core::acceptance_evidence::AUTOMATABLE_LIVE_CHECKS`; the Rust
# validator is the authority and refuses anything this gets wrong.
AUTOMATABLE = [
    "gnome-file-chooser",
    "wayland-clipboard",
    "desktop-portal",
    "wayland-compositor-fullscreen",
    "wayland-double-click-fullscreen",
    "wayland-always-on-top-unavailable",
    "keyboard-focus-navigation",
]

VIEWPORT = {"width": 1120, "height": 680}


def digest(path: pathlib.Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", required=True, help="directory of per-row result files")
    parser.add_argument("--artifacts", required=True, help="directory of captured artefacts")
    parser.add_argument("--facts", required=True, help="session facts JSON")
    parser.add_argument("--harness-revision", required=True)
    parser.add_argument(
        "--package-sha256",
        required=True,
        help="digest of the exact package the harness drove; binds the rows to it",
    )
    parser.add_argument(
        "--rows",
        default="",
        help="comma-separated states to emit; empty means every automatable row",
    )
    parser.add_argument("--execution-environment-sha256", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    facts = json.loads(pathlib.Path(args.facts).read_text())
    attestation = {
        "session_type": facts["session_type"],
        "compositor": facts["compositor"],
        "compositor_version": facts["compositor_version"],
        "monitors": facts["monitors"],
        "harness_revision": args.harness_revision,
        "input_injector": facts["input_injector"],
        "package_sha256": args.package_sha256,
    }

    # A caller asking for a subset gets exactly that subset. Emitting the rest as
    # `not-run` would overwrite states an earlier run already collected, because the merge
    # replaces rows by state.
    selected = [state.strip() for state in args.rows.split(",") if state.strip()]
    unknown = [state for state in selected if state not in AUTOMATABLE]
    if unknown:
        print(f"emit-rows: not automatable: {', '.join(unknown)}", file=sys.stderr)
        return 3
    wanted = selected or list(AUTOMATABLE)

    results = pathlib.Path(args.results)
    artifacts = pathlib.Path(args.artifacts)
    rows = []
    for state in wanted:
        result_file = results / f"{state}.result"
        status = "not-run"
        note = "the check did not run"
        if result_file.is_file():
            raw = result_file.read_text().strip().split("\n", 1)
            status = raw[0].strip()
            note = raw[1].strip() if len(raw) > 1 else ""
            if status not in {"pass", "fail", "not-run"}:
                print(f"emit-rows: {state}: unusable status {status!r}", file=sys.stderr)
                return 3

        # Captures are named "<state>.<ext>", and rows that record more than one frame
        # add "<state>-<n>.<ext>" - the double-click row's two compositor transitions are
        # why that form exists. Globbing "<state>.*" silently dropped those frames, so the
        # images the row rests on were never checksummed into the evidence and could not
        # be authenticated from the manifest. No automatable state name is a prefix of
        # another, so matching the "-" form cannot capture a neighbouring row's frames.
        captured = sorted(
            p
            for p in artifacts.glob(f"{state}*")
            if p.is_file() and p.name[len(state) :].startswith((".", "-"))
        )
        row = {
            "id": state,
            "level": "gnome-wayland-automated",
            "viewport": VIEWPORT,
            "theme": "auto",
            "state": state,
            "reference": "gnome-wayland-automated-harness",
            "measurement_result": "not-run",
            "operator_status": "not-run",
            "automated_status": status,
            "automation": attestation if status == "pass" else None,
            "artifacts": [
                {"file_name": path.name, "sha256": digest(path)} for path in captured
            ],
            "measurements": [],
            "notes": note,
            "execution_environment_sha256": (
                args.execution_environment_sha256 if status == "pass" else None
            ),
        }
        rows.append(row)

    pathlib.Path(args.output).write_text(json.dumps(rows, indent=2, sort_keys=True) + "\n")
    failed = [row["id"] for row in rows if row["automated_status"] != "pass"]
    print(f"wrote {len(rows)} automated rows to {args.output}")
    if failed:
        print("rows not passing: " + ", ".join(failed))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
