#!/usr/bin/env python3
"""Build an exact Forgejo commit and run its macOS scenario on GitHub-hosted Apple Silicon."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tempfile
import uuid
import zipfile

ROOT = Path(__file__).resolve().parent.parent
FORGEJO = "https://git.oklabs.uk/BeFeast/ok-player.git"
GITHUB = "BeFeast/ok-player"
FULL_SHA = re.compile(r"[0-9a-f]{40}")
CONTROLS = {
    "subtitle-lift": "subtitle_clearance_disabled",
    "window-chrome": "title_identity_hidden",
    "window-fullscreen": "video_fullscreen_disabled",
    "window-fit": "initial_fit_disabled",
    "window-float": "floating_level_disabled",
    "osc-layout": "slot_collapse_disabled",
    "osc-idle-and-pins": "panel_pin_disabled",
    "osc-volume-speed": "volume_wheel_disabled",
    "osc-status": "copy_details_disabled",
    "settings-shell-appearance": "appearance_persistence_disabled",
    "audio-switcher": "track_pin_disabled",
    "subtitle-switcher": "track_pin_disabled",
    "welcome-first-run": "welcome_focus_disabled",
    "open-files-folders": "folder_picker_files_enabled",
    "open-drag-drop": "welcome_drop_highlight_disabled",
    "context-idle-and-playback": "context_clamp_disabled",
    "command-keyboard": "text_priority_disabled",
    "about-identity": "about_copy_disabled",
    "about-geometry": "about_owner_cleanup_disabled",
}


def git(*args):
    return subprocess.check_output(["git", "-C", ROOT, *args], text=True, timeout=90).strip()


def main():
    scenarios = {path.stem for path in (ROOT / "macos/verification").glob("*.json")}
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", nargs="?")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--sha", help="full, published Forgejo SHA (default: Forgejo main)")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--negative-control", action="store_true",
                        help="disconnect the selected scenario’s behavior in the verifier instance")
    args = parser.parse_args()
    if args.list:
        print("\n".join(sorted(scenarios)))
        return 0
    if args.scenario not in scenarios:
        parser.error("unknown scenario; use --list")
    if args.negative_control and args.scenario not in CONTROLS:
        parser.error("this scenario has no negative control")
    if args.sha is not None and not FULL_SHA.fullmatch(args.sha):
        parser.error("--sha must be a full 40-character Forgejo SHA")
    try:
        sha = args.sha or git("ls-remote", FORGEJO, "refs/heads/main").split()[0]
        git("fetch", "--no-tags", "--no-write-fetch-head", FORGEJO, sha)
        # The packaged-app protocol is carried by the tested commit.
        git("cat-file", "-e", f"{sha}:macos/verification/{args.scenario}.json")
    except (OSError, subprocess.SubprocessError, ValueError, IndexError) as error:
        parser.exit(2, f"Cannot resolve a Forgejo commit with the requested scenario: {error}\n")
    env = os.environ.copy()
    if not env.get("MAC_CI_GITHUB_TOKEN"):
        try:
            env["MAC_CI_GITHUB_TOKEN"] = subprocess.check_output(
                ["gh", "auth", "token", "--hostname", "github.com"], text=True, timeout=10).strip()
        except (OSError, subprocess.SubprocessError):
            parser.exit(2, "Set MAC_CI_GITHUB_TOKEN or authenticate gh with GitHub workflow access.\n")

    output = args.output.absolute() if args.output else Path(tempfile.mkdtemp(prefix="okp-macos-verify-"))
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("--output must be empty")
    tag = "negative-" if args.negative_control else ""
    ref = f"mac-ci/verify/{args.scenario}/{tag}{uuid.uuid4().hex}"
    run = subprocess.run([
        sys.executable, str(ROOT / "scripts/mac-ci-bridge.py"), "run",
        "--sha", sha, "--ref", ref, "--repo", str(ROOT), "--github-repo", GITHUB,
        "--download-dir", str(output), "--summary", str(output / "hosted-run.md"),
    ], env={**env, "PYTHONUNBUFFERED": "1"})
    verdict = output / "verify/result.json"
    try:
        if not verdict.is_file():
            verdict.parent.mkdir(exist_ok=True)
            verdict.write_text(json.dumps({
                "schema_version": 1, "scenario": args.scenario, "source_sha": sha,
                "status": "error", "error": "Hosted run did not return a scenario verdict",
            }, indent=2) + "\n")
        result = json.loads(verdict.read_text())
        status = result["status"]
        print(f"{status}: {verdict}", flush=True)
        if result["source_sha"] != sha or result["scenario"] != args.scenario:
            raise ValueError("Returned verdict names the wrong source or scenario")
        if status in ("passed", "failed"):
            control = CONTROLS[args.scenario] if args.negative_control else "none"
            if result["control"] != control:
                raise ValueError("Returned verdict names the wrong control mode")
            if (output / "source-sha.txt").read_text().strip() != sha:
                raise ValueError("Returned artifact names the wrong Forgejo SHA")
            archive = output / "ok-player-macos-arm64.zip"
            archive_hash = hashlib.sha256()
            with archive.open("rb") as stream:
                for data in iter(lambda: stream.read(1024 * 1024), b""):
                    archive_hash.update(data)
            archive_digest = archive_hash.hexdigest()
            if result["app_zip_sha256"] != archive_digest or archive_digest != (
                    output / "SHA256SUMS.txt").read_text().split()[0]:
                raise ValueError("Returned verdict is not bound to the app ZIP")
            with zipfile.ZipFile(archive) as package:
                info = plistlib.loads(package.read("OK Player.app/Contents/Info.plist"))
            if info["OKPSourceSHA"] != sha:
                raise ValueError("The downloaded ZIP embeds a different source SHA")
            if {case["appearance"] for case in result["appearances"]} != {"light", "dark"}:
                raise ValueError("The hosted verdict is missing an appearance")
            for appearance in result["appearances"]:
                for checkpoint in appearance["checkpoints"]:
                    png = verdict.parent / checkpoint["png"]
                    if not png.is_relative_to(verdict.parent) or ".." in png.parts:
                        raise ValueError("Invalid PNG path in the hosted verdict")
                    if hashlib.sha256(png.read_bytes()).hexdigest() != checkpoint["png_sha256"]:
                        raise ValueError(f"Screenshot checksum mismatch: {png.name}")
        if status == "passed" and run.returncode == 0:
            return 0
        if status == "failed" and run.returncode == 1:
            return 1
    except (OSError, ValueError, KeyError, IndexError, TypeError,
            plistlib.InvalidFileException, zipfile.BadZipFile) as error:
        print(f"Invalid hosted evidence: {error}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
