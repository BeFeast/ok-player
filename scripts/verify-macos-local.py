#!/usr/bin/env python3
"""Exercise the relocated macOS preview and save a verdict even when AppKit fails."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SCENARIOS = ROOT / "macos" / "verification"
ARCHIVE = "ok-player-macos-arm64.zip"
WINDOW_CONTROLS = {
    "window-chrome": "title_identity_hidden",
    "window-fullscreen": "video_fullscreen_disabled",
    "window-fit": "initial_fit_disabled",
    "window-float": "floating_level_disabled",
}
CONTROLS = {"subtitle-lift": "subtitle_clearance_disabled", **WINDOW_CONTROLS}
# Keep both lines active across the exact seeks and the real OSC timeout.
SIDECAR = """1
00:00:02,000 --> 00:00:17,500
FIRST SUBTITLE LINE
SECOND SUBTITLE LINE

"""


def digest(path):
    sha = hashlib.sha256()
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            sha.update(block)
    return sha.hexdigest()


def command(*args, **kwargs):
    return subprocess.run(args, check=True, timeout=kwargs.pop("timeout", 30), **kwargs)


def read_scenario(path):
    spec = json.loads(path.read_text())
    actions = {"open", "pause", "seek", "capture", "play_to", "play_until_hidden", "pointer_reveal",
               "enter_fullscreen", "leave_fullscreen", "size_window", "close_media", "double_click",
               "small_work_area", "open_portrait", "resize_edges", "pin", "unpin", "overlap_window"}
    if (spec["schema_version"] != 1 or spec["id"] != path.stem
            or spec["appearances"] != ["light", "dark"] or not spec["steps"]):
        raise ValueError("Unsupported scenario ID, schema or appearances")
    for step in spec["steps"]:
        if step["action"] not in actions:
            raise ValueError(f"Unsupported action {step['action']!r}")
        if step["action"] in {"seek", "play_to"} and not isinstance(step.get("seconds"), (float, int)):
            raise ValueError("Seek and playback targets require numeric seconds")
        if step["action"] == "capture":
            if (step.get("expect") not in {"no_cue", "clearance", "base_margin", "welcome", "playing",
                                           "fullscreen", "restored", "landscape", "portrait", "idle",
                                           "floating", "normal", "repinned"}
                    or not step.get("name", "").replace("-", "").isalnum()):
                raise ValueError("Invalid capture checkpoint")
    if spec["fixture"] not in {"subtitle-lift.mp4", "01-h264-1080p24-tone.mp4", "landscape-4k.mp4"}:
        raise ValueError("Unknown scenario fixture")
    return spec


def fixture(ffmpeg, temporary, spec, receipt):
    video = temporary / spec["fixture"]
    version = command(ffmpeg, "-version", capture_output=True, text=True).stdout.splitlines()[0]
    receipt["fixture"] = {"ffmpeg_version": version, "ffmpeg_sha256": digest(Path(ffmpeg))}
    if spec["id"] == "subtitle-lift":
        subtitles = video.with_suffix(".srt")
        subtitles.write_text(SIDECAR, encoding="utf-8")
        receipt["fixture"][subtitles.name] = digest(subtitles)
        command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                "-f", "lavfi", "-i", "color=c=0x303030:s=640x360:r=24:d=18",
                "-c:v", "libx264", "-pix_fmt", "yuv420p", "-threads", "1",
                "-g", "12", "-keyint_min", "12", "-sc_threshold", "0",
                "-map_metadata", "-1", "-fflags", "+bitexact", "-flags", "+bitexact",
                "-movflags", "+faststart", str(video))
        receipt["fixture"][video.name] = digest(video)
    else:
        fixtures = [(video, "1920x1080")]
        if spec["id"] == "window-fit":
            fixtures = [(video, "3840x2160"), (temporary / "portrait.mp4", "1080x1440")]
        for path, size in fixtures:
            if spec["id"] == "window-fit":
                sources = ["-f", "lavfi", "-i", f"smptebars=s={size}:r=24:d=12"]
            else:
                # Match the reference's pattern without testsrc2's burned-in timecode.
                sources = [
                    "-f", "lavfi", "-i", f"testsrc2=s={size}:r=24:d=60",
                    "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=60",
                    "-vf", "drawbox=x=0:y=0:w=125:h=55:color=red:t=fill", "-c:a", "aac",
                ]
            command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", *sources,
                    "-c:v", "libx264", "-preset", "ultrafast", "-threads", "2", "-pix_fmt", "yuv420p",
                    "-g", "48", "-map_metadata", "-1", "-fflags", "+bitexact", "-flags", "+bitexact",
                    "-movflags", "+faststart", str(path), timeout=120)
            receipt["fixture"][path.name] = digest(path)
    return video


def prepare_app(args, temporary, receipt):
    if not args.app and not args.zip:
        build = temporary / "build"
        if not os.environ.get("OKP_MPV_PREFIX"):
            raise ValueError("Set OKP_MPV_PREFIX to build, or provide --app or --zip")
        command(ROOT / "scripts/build-macos-preview.sh", timeout=2400,
                env={**os.environ, "OKP_MACOS_OUTPUT": str(build),
                     "CARGO_TARGET_DIR": str(temporary / "cargo")})
        args.zip = build / ARCHIVE
    relocated = temporary / "relocated"
    relocated.mkdir()
    if args.zip:
        receipt["app_zip_sha256"] = digest(args.zip)
        command("ditto", "-x", "-k", args.zip, relocated)
    app = relocated / "OK Player.app"
    if args.app:
        command("ditto", args.app, app)
    executable = app / "Contents/MacOS/ok-player"
    if not executable.is_file():
        raise ValueError("The app is missing Contents/MacOS/ok-player")
    command("codesign", "--verify", "--deep", "--strict", app)
    receipt["executable_sha256"] = digest(executable)
    with (app / "Contents/Info.plist").open("rb") as stream:
        plist = plistlib.load(stream)
    if plist["CFBundleIdentifier"] != "uk.oklabs.ok-player":
        raise ValueError("Unexpected bundle identifier")
    receipt["bundle_revision"] = plist["OKPSourceRevision"]
    receipt["bundle_source_sha"] = plist["OKPSourceSHA"]
    if args.source_sha and args.source_sha != plist["OKPSourceSHA"]:
        raise ValueError("Bundle revision does not match the requested Forgejo SHA")
    return executable


def run_appearance(executable, scenario, video, temporary, output, appearance, negative_control):
    folder = output / appearance
    folder.mkdir()
    profile = temporary / appearance
    for subdir in ("data", "config", "cache", "pictures"):
        (profile / subdir).mkdir(parents=True)
    env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    env.update({"TZ": "UTC", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"})
    argv = [executable, f"--verify-scenario={scenario}", f"--verify-fixture={video}",
            f"--verify-output={folder}", f"--verify-profile={profile}",
            f"--verify-appearance={appearance}"]
    if negative_control:
        argv.append("--verify-negative-control")
    with (folder / "app.log").open("w") as log:
        try:
            run = subprocess.run(
                argv,
                stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=105,
            )
        except subprocess.TimeoutExpired:
            return {"appearance": appearance, "status": "error", "error": "App verifier timed out"}
    try:
        result = json.loads((folder / "app-result.json").read_text())
    except (OSError, ValueError):
        return {"appearance": appearance, "status": "error",
                "error": f"App produced no readable verdict (exit {run.returncode})"}
    result["exit_code"] = run.returncode
    status = result.get("status")
    if (run.returncode, status) in {(0, "passed"), (1, "failed")}:
        expected = [step["name"] for step in read_scenario(scenario)["steps"] if step["action"] == "capture"]
        if [point["name"] for point in result["checkpoints"]] != expected:
            return {**result, "status": "error", "error": "App did not return every checkpoint"}
        for checkpoint in result["checkpoints"]:
            path = folder / checkpoint["png"]
            checkpoint["png"] = f"{appearance}/{path.name}"
            checkpoint["png_sha256"] = digest(path)
    elif result.get("status") not in {"blocked_environment", "error"}:
        result.update(status="error", error="App status and exit code disagree")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", nargs="?")
    parser.add_argument("--list", action="store_true")
    bundle = parser.add_mutually_exclusive_group()
    bundle.add_argument("--app", type=Path)
    bundle.add_argument("--zip", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--ffmpeg", type=Path, default=os.environ.get("OKP_VERIFY_FFMPEG"))
    parser.add_argument("--source-sha")
    parser.add_argument("--negative-control", action="store_true",
                        help="disconnect the selected scenario’s behavior in the verifier instance")
    args = parser.parse_args()
    scenarios = {path.stem: path for path in SCENARIOS.glob("*.json")}
    if args.list:
        print("\n".join(sorted(scenarios)))
        return 0
    if args.scenario not in scenarios:
        parser.error("unknown scenario; use --list")
    if args.negative_control and args.scenario not in CONTROLS:
        parser.error("this scenario has no negative control")
    scenario = scenarios[args.scenario]
    try:
        spec = read_scenario(scenario)
    except (OSError, KeyError, ValueError) as error:
        parser.error(str(error))
    output = args.output.absolute() if args.output else Path(tempfile.mkdtemp(prefix="okp-verify-result-"))
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        parser.error("--output must be empty")
    control = CONTROLS[args.scenario] if args.negative_control else "none"
    receipt = {"schema_version": 1, "scenario": spec["id"], "source_sha": args.source_sha, "control": control,
               "host": {"os": platform.system(), "version": platform.mac_ver()[0], "architecture": platform.machine()},
               "scenario_sha256": digest(scenario), "status": "error", "appearances": []}
    try:
        if platform.system() != "Darwin" or platform.machine() != "arm64":
            receipt["status"] = "blocked_environment"
            raise ValueError("This AppKit verifier requires Apple Silicon macOS")
        if not args.ffmpeg:
            raise ValueError("Set OKP_VERIFY_FFMPEG to the pinned ffmpeg executable")
        with tempfile.TemporaryDirectory(prefix="okp-verify-") as work:
            temporary = Path(work)
            executable = prepare_app(args, temporary, receipt)
            video = fixture(args.ffmpeg, temporary, spec, receipt)
            for appearance in spec["appearances"]:
                result = run_appearance(executable, scenario, video, temporary, output, appearance, args.negative_control)
                if result["status"] in {"passed", "failed"}:
                    if result.get("source_sha") != receipt["bundle_source_sha"]:
                        result.update(status="error", error="Running bundle SHA differed from the relocated package")
                    elif result.get("control") != control:
                        result.update(status="error", error="App did not report the requested control mode")
                receipt["appearances"].append(result)
            states = {result["status"] for result in receipt["appearances"]}
            receipt["status"] = next((state for state in ("error", "blocked_environment", "failed") if state in states), "passed")
            if args.negative_control and receipt["status"] in {"passed", "failed"}:
                if spec["id"] == "subtitle-lift":
                    expected_failures = {
                        f"subtitle_clearance:{step['name']}" for step in spec["steps"]
                        if step.get("expect") == "clearance"
                    }
                else:
                    checkpoints = {
                        "window-chrome": ["welcome", "playing"],
                        "window-fullscreen": ["fullscreen", "restored"],
                        "window-fit": ["fit-4k", "fit-portrait"],
                        "window-float": ["pinned", "pinned-after-fullscreen"],
                    }
                    prefix = spec["id"].replace("-", "_")
                    expected_failures = {f"{prefix}:{name}" for name in checkpoints[spec["id"]]}
                if not all(expected_failures <= set(result["assertion_failures"])
                           for result in receipt["appearances"]):
                    receipt.update(status="error", error="Negative control did not expose all expected failures in both themes")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        receipt["error"] = str(error)
    finally:
        verdict = output / "result.json"
        verdict.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(f"{receipt['status']}: {verdict}", flush=True)
    return {"passed": 0, "failed": 1}.get(receipt["status"], 2)


if __name__ == "__main__":
    raise SystemExit(main())
