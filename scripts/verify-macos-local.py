#!/usr/bin/env python3
"""Exercise the relocated macOS preview and save a verdict even when AppKit fails."""
import argparse
from contextlib import contextmanager
from email import policy
from email.parser import BytesParser
import hashlib
from http.server import BaseHTTPRequestHandler, SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import plistlib
import subprocess
import sys
import tempfile
import time
from threading import Event, Thread
from uuid import UUID

ROOT = Path(__file__).resolve().parent.parent
SCENARIOS = ROOT / "macos" / "verification"
ARCHIVE = "ok-player-macos-arm64.zip"
WINDOW_CONTROLS = {
    "window-chrome": "title_misaligned",
    "window-fullscreen": "video_fullscreen_disabled",
    "window-fit": "initial_fit_disabled",
    "window-float": "floating_level_disabled",
}
TRANSPORT_CONTROLS = {
    "osc-layout": ("slot_collapse_disabled", {"osc_layout:narrow", "osc_layout:floor", "osc_layout:more"}),
    "osc-idle-and-pins": ("panel_pin_disabled", {"osc_idle_and_pins:panel"}),
    "osc-volume-speed": ("volume_wheel_disabled", {"osc_volume_speed:wheel"}),
    "osc-status": ("copy_details_disabled", {"osc_status:copy_details"}),
}
TRACK_CONTROLS = {
    "audio-switcher": "track_pin_disabled",
    "subtitle-switcher": "track_pin_disabled",
}
QUEUE_CONTROLS = {
    "queue-show": ("queue_pin_disabled", {"queue_show:pin"}),
    "queue-edit": ("queue_move_disabled", {"queue_edit:reorder"}),
    "queue-single": ("queue_add_disabled", {"queue_single:add"}),
}
SETTINGS = "settings-shell-appearance"
UPDATES = "updates-status"
ABOUT_CONTROLS = {
    "about-identity": ("about_copy_disabled", {"about-identity:copy_diagnostics"}),
    "about-geometry": ("about_owner_cleanup_disabled", {"about-geometry:owner_close"}),
}
SETTINGS_PHASES = ("edit", "light-relaunch", "auto-relaunch")
LEGACY_SETTINGS = {
    "version": 1,
    "appearance": {"theme": " Dark ", "accent_source": "System"},
    "playback": {"volume": 67.0, "resume": False},
    "profile_extension": {"keep": True},
}
OPEN_CONTROLS = {
    "welcome-first-run": ("welcome_focus_disabled", {"welcome_first_run:focus"}),
    "open-files-folders": ("folder_picker_files_enabled", {"open_files_folders:folder-filter"}),
    "open-drag-drop": ("welcome_drop_highlight_disabled", {"open_drag_drop:welcome-accent"}),
}
ACTION_CONTROLS = {
    **TRANSPORT_CONTROLS,
    **OPEN_CONTROLS,
    **ABOUT_CONTROLS,
    **QUEUE_CONTROLS,
    "window-chrome": ("title_misaligned", {f"window_chrome:alignment:{checkpoint}"
                                         for checkpoint in ("welcome", "playing", "revealed")}),
    "welcome-return": ("inactive_video_visible", {f"welcome_return:canvas:{checkpoint}" for checkpoint in
                       ("after-eof", "after-close-menu", "after-close-shortcut", "after-open-another")}),
    "context-idle-and-playback": ("context_clamp_disabled", {"context:work_area_clamp"}),
    "command-keyboard": ("text_priority_disabled", {"keyboard:text_priority"}),
    "compact-mini": ("compact_snap_disabled", {"compact_mini:corner_snap"}),
    "compact-mini-floor": ("compact_clearance_disabled", {"compact_mini_floor:clearance"}),
    "welcome-resume-shelf": ("history_poster_disabled", {"welcome_resume_shelf:poster"}),
    "history-page": ("history_pagination_disabled", {"history_page:pagination"}),
}
CONTROLS = {
    **TRACK_CONTROLS,
    "subtitle-lift": "subtitle_clearance_disabled",
    SETTINGS: "appearance_persistence_disabled",
    UPDATES: "bad_manifest_signature",
    **WINDOW_CONTROLS,
    **{scenario: entry[0] for scenario, entry in ACTION_CONTROLS.items()},
}
OPEN_FIXTURE = "open-preview.mp4"
HISTORY_FIXTURE = "history-preview.mkv"
FIXTURE_SUBTITLES = {
    "subtitle-lift.mp4": (".srt",),
    "A Serious Man (2009) {tmdb-12573}.mp4": (".en.hi.srt", ".he.srt", ".ru.hi.srt"),
}
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
               "enter_fullscreen", "leave_fullscreen", "check_subtitle_menu",
               "size_window", "close_media", "double_click",
               "wait_for_eof", "close_media_shortcut", "open_another", "wait_for_chrome_hide", "advance_playlist",
               "small_work_area", "open_portrait", "resize_edges", "pin", "unpin", "overlap_window",
               "size_osc", "check_more", "check_seek", "check_idle", "check_pins", "show_volume",
               "check_wheel", "check_speed_mute", "set_unity", "open_stall", "release_stall", "open_failure",
               "check_failure_actions", "check_stale", "open_buffer", "release_buffer",
               "check_audio_switcher", "check_subtitle_switcher", "check_track_pin",
               "tab_welcome", "cancel_welcome_open", "picker_files", "picker_folder", "picker_playlist",
               "finder_open", "cancel_playback_open", "bad_drop", "drag_enter", "drag_exit",
               "drop_mixed", "drop_subtitle",
               "check_context_idle", "check_context_search", "check_context_empty",
               "check_context_pages", "check_context_corners",
               "check_playback_keys", "check_queue_keys", "check_escape_priority", "check_text_priority",
               "open_queue", "open_queue_url", "show_queue", "check_queue_jump", "preview_queue_move",
               "check_queue_edit", "open_queue_menu", "check_queue_actions", "check_queue_add",
               "enter_mini", "drag_mini", "resize_mini", "restore_mini", "wheel_mini", "seek_mini",
               "idle_mini", "accessible_mini", "feedback_open", "feedback_send",
               "history_play", "history_arrow", "history_footer", "history_search", "history_clear",
               "history_back", "history_resume", "history_more", "history_compact"}
    if path.stem == SETTINGS:
        actions.update({"settings_shortcut", "settings_context", "settings_legacy", "settings_search",
                        "settings_resize", "settings_system", "settings_light", "settings_auto",
                        "settings_player_input"})
    if path.stem == UPDATES:
        actions.add("check_update")
    if path.stem in ABOUT_CONTROLS:
        actions.update({"about_playback", "about_menu", "about_rail", "about_copy",
                        "about_resize", "about_footer", "about_close"})
    if (spec["schema_version"] != 1 or spec["id"] != path.stem
            or spec["appearances"] != ["light", "dark"] or not spec["steps"]):
        raise ValueError("Unsupported scenario ID, schema or appearances")
    for step in spec["steps"]:
        if spec["id"] == SETTINGS and step.get("phase") not in SETTINGS_PHASES:
            raise ValueError("Settings steps require a relaunch phase")
        if step["action"] not in actions:
            raise ValueError(f"Unsupported action {step['action']!r}")
        if step["action"] in {"seek", "seek_mini", "play_to"} and not isinstance(step.get("seconds"), (float, int)):
            raise ValueError("Seek and playback targets require numeric seconds")
        if step["action"] == "size_osc" and not isinstance(step.get("width"), (float, int)):
            raise ValueError("OSC resize requires a numeric width")
        if step["action"] == "capture":
            if (step.get("expect") not in {"no_cue", "clearance", "base_margin", "welcome", "playing", "chrome_hidden",
                                           "fullscreen", "restored", "landscape", "portrait", "idle",
                                           "floating", "normal", "repinned", "osc_row", "osc_hidden", "osc_pinned",
                                           "volume", "speed", "loading", "buffering", "failure", "recovered",
                                           "context_welcome", "context_video", "context_results", "context_empty",
                                           "keyboard_playback", "settings_light", "settings_auto", "track_panel",
                                           "open_evidence", "about_identity", "about_footer", "queue_panel", "mini_hover",
                                           "mini_floor", "mini_hidden", "mini_accessible", "feedback_attached",
                                           "history_evidence"}
                    or not step.get("name", "").replace("-", "").isalnum()):
                raise ValueError("Invalid capture checkpoint")
        if step["action"] == "check_subtitle_menu":
            labels = step["subtitle_labels"]
            if (not isinstance(labels, list) or not all(isinstance(label, str) for label in labels)
                    or step["selected_subtitle"] not in labels):
                raise ValueError("Subtitle menu check needs labels and an expected selected item")
    if spec["fixture"] not in {*FIXTURE_SUBTITLES, "01-h264-1080p24-tone.mp4", "landscape-4k.mp4",
                               "04-mkv-2audio-2subs.mkv", "mini-landscape.mp4", "welcome-return.mp4",
                               OPEN_FIXTURE, "S01E01.mp4", HISTORY_FIXTURE}:
        raise ValueError("Unknown scenario fixture")
    return spec


def fixture(ffmpeg, temporary, spec, receipt):
    video = temporary / spec["fixture"]
    version = command(ffmpeg, "-version", capture_output=True, text=True).stdout.splitlines()[0]
    receipt["fixture"] = {"ffmpeg_version": version, "ffmpeg_sha256": digest(Path(ffmpeg))}
    if video.name == "S01E01.mp4":
        video = queue_fixtures(ffmpeg, temporary, spec["id"], receipt["fixture"])
    elif video.name == "04-mkv-2audio-2subs.mkv":
        track_fixtures(ffmpeg, video, receipt["fixture"])
    elif video.name == "welcome-return.mp4":
        command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                "-f", "lavfi", "-i", "color=c=magenta:s=640x288:r=24:d=4", "-c:v", "libx264",
                "-pix_fmt", "yuv420p", "-threads", "1", "-movflags", "+faststart", str(video))
        broken = temporary / "broken" / "invalid.mp4"
        broken.parent.mkdir()
        broken.write_text("Invalid media fixture\n", encoding="utf-8")
        next_clip = temporary / "next" / "second.mp4"
        next_clip.parent.mkdir()
        os.link(video, next_clip)
        receipt["fixture"].update({video.name: digest(video), "broken/invalid.mp4": digest(broken),
                                   "next/second.mp4": digest(next_clip)})
    elif video.name == HISTORY_FIXTURE:
        sources = [(video, "testsrc2=s=640x360:r=24:d=18"),
                   (temporary / "history-dark.mkv", "color=c=black:s=640x360:r=24:d=18")]
        for path, source in sources:
            command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                    "-f", "lavfi", "-i", source,
                    "-c:v", "libx264", "-preset", "ultrafast", "-threads", "1", "-pix_fmt", "yuv420p",
                    "-map_metadata", "-1", "-fflags", "+bitexact", "-flags", "+bitexact", str(path))
            receipt["fixture"][path.name] = digest(path)
    elif video.name == OPEN_FIXTURE:
        command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                "-f", "lavfi", "-i", "testsrc2=s=640x360:r=24:d=18", "-c:v", "libx264",
                "-preset", "ultrafast", "-pix_fmt", "yuv420p", "-threads", "1",
                "-map_metadata", "-1", "-fflags", "+bitexact", "-flags", "+bitexact",
                "-movflags", "+faststart", str(video))
        receipt["fixture"][video.name] = digest(video)
        if spec["id"] != "welcome-first-run":
            folder = temporary / "Episodes"
            folder.mkdir()
            for clip in [temporary / "second.mp4", folder / "Episode 10.mp4", folder / "Episode 2.mp4"]:
                os.link(video, clip)
                receipt["fixture"][str(clip.relative_to(temporary))] = digest(clip)
            for name, contents in {"ordered.m3u": "#EXTM3U\nsecond.mp4\nopen-preview.mp4\n",
                                   "external.he.srt": SIDECAR, "notes.txt": "Unsupported fixture\n"}.items():
                path = temporary / name
                path.write_text(contents, encoding="utf-8")
                receipt["fixture"][name] = digest(path)
    elif video.name == "mini-landscape.mp4":
        for path, size in [(video, "640x360"), (temporary / "portrait.mp4", "360x640")]:
            command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
                    "-f", "lavfi", "-i", f"color=c=0xeceef0:s={size}:r=24:d=30",
                    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-threads", "1",
                    "-g", "12", "-map_metadata", "-1", "-fflags", "+bitexact", "-flags", "+bitexact",
                    "-movflags", "+faststart", str(path))
            receipt["fixture"][path.name] = digest(path)
    elif video.name in FIXTURE_SUBTITLES:
        for suffix in FIXTURE_SUBTITLES[video.name]:
            subtitle = video.with_suffix(suffix)
            subtitle.write_text(SIDECAR, encoding="utf-8")
            receipt["fixture"][subtitle.name] = digest(subtitle)
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


def queue_fixtures(ffmpeg, temporary, scenario, provenance):
    folder = temporary / "episodes"
    folder.mkdir()
    bars = ",".join(f"drawbox=x={160 * column}:y=0:w=160:h=540:color={color}:t=fill"
                    for column, color in enumerate(("0xff1900", "0x00da00", "0xffeb00", "0x1414f8",
                                                    "0xff00ff", "0x00deec")))
    video = folder / "S01E01.mp4"
    command(ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error",
            "-f", "lavfi", "-i", f"color=c=black:s=960x540:r=24:d=18,{bars}",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-threads", "1",
            "-fflags", "+bitexact", "-flags", "+bitexact", "-movflags", "+faststart", str(video))
    provenance["episodes/S01E01.mp4"] = digest(video)
    if scenario != "queue-single":
        for name in ("S01E02.mp4", "S01E03.mp4"):
            (folder / name).write_bytes(video.read_bytes())
            provenance[f"episodes/{name}"] = digest(folder / name)
    extra = temporary / "S01E04.mp4"
    extra.write_bytes(video.read_bytes())
    provenance[extra.name] = digest(extra)
    return video


def track_fixtures(ffmpeg, video, provenance):
    # Saturated bars keep neutral-white subtitle glyphs independently measurable.
    bars = ",".join(f"drawbox=x={320 * column}:y=0:w=320:h=1080:color={color}:t=fill"
                    for column, color in enumerate(("0xff1900", "0x00da00", "0xffeb00", "0x1414f8",
                                                    "0xff00ff", "0x00deec")))
    sources = ["-f", "lavfi", "-i", f"color=c=black:s=1920x1080:r=24:d=18,{bars}"]
    subtitles = []
    for name, text in (("embedded-en", "EMBEDDED EN line one"), ("embedded-ru", "RU"),
                       ("added-external", "ADDED EXTERNAL subtitle")):
        subtitle = video.with_name(name + ".srt")
        subtitle.write_text(f"1\n00:00:01,000 --> 00:00:17,500\n{text}\n\n", encoding="utf-8")
        provenance[subtitle.name] = digest(subtitle)
        subtitles.append(subtitle)

    codec = ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-preset", "ultrafast", "-threads", "2",
             "-g", "24", "-fflags", "+bitexact", "-flags", "+bitexact"]
    prefix = [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error"]
    no_audio = video.with_name("video-only.mp4")
    command(*prefix, *sources, *codec, str(no_audio), timeout=120)
    provenance[no_audio.name] = digest(no_audio)
    command(*prefix, "-i", str(no_audio),
            "-f", "lavfi", "-i", "sine=frequency=300:sample_rate=48000:duration=18",
            "-f", "lavfi", "-i", "sine=frequency=900:sample_rate=48000:duration=18",
            "-i", str(subtitles[0]), "-i", str(subtitles[1]),
            "-map", "0:v", "-map", "1:a", "-map", "2:a", "-map", "3:s", "-map", "4:s",
            "-c:v", "copy", "-c:a", "aac", "-c:s", "srt", "-fflags", "+bitexact",
            "-metadata:s:a:0", "title=Low 300Hz", "-metadata:s:a:0", "language=eng",
            "-metadata:s:a:1", "title=High 900Hz", "-metadata:s:a:1", "language=rus",
            "-metadata:s:s:0", "language=eng", "-metadata:s:s:1", "language=rus",
            "-disposition:a:0", "default", "-disposition:s:0", "default", str(video), timeout=120)
    provenance[video.name] = digest(video)


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
    receipt["bundle_version"] = plist["CFBundleShortVersionString"]
    receipt["bundle_build"] = plist["CFBundleVersion"]
    if args.source_sha and args.source_sha != plist["OKPSourceSHA"]:
        raise ValueError("Bundle revision does not match the requested Forgejo SHA")
    return executable


def about_expected(executable, receipt):
    contents = executable.parent.parent
    plist = plistlib.loads((contents / "Info.plist").read_bytes())
    if not (contents / "Resources" / "LICENSE").is_file():
        raise ValueError("The app is missing its bundled license")
    frameworks = contents / "Frameworks"
    libmpv = next(iter(sorted(frameworks.glob("libmpv*.dylib"))), None)
    if libmpv is None:
        raise ValueError("The app is missing its bundled libmpv")
    probe = """
import ctypes as c
import json
import sys

lib = c.CDLL(sys.argv[1])
lib.mpv_create.restype = c.c_void_p
lib.mpv_initialize.argtypes = [c.c_void_p]
lib.mpv_get_property_string.argtypes = [c.c_void_p, c.c_char_p]
lib.mpv_get_property_string.restype = c.c_void_p
lib.mpv_free.argtypes = [c.c_void_p]
lib.mpv_free.restype = None
lib.mpv_terminate_destroy.argtypes = [c.c_void_p]
lib.mpv_terminate_destroy.restype = None
handle = lib.mpv_create()
if not handle:
    raise RuntimeError("Bundled libmpv could not create a probe handle")
try:
    if lib.mpv_initialize(handle) < 0:
        raise RuntimeError("Bundled libmpv could not initialize its probe handle")
    versions = {}
    for name, prop in (("libmpv", b"mpv-version"), ("FFmpeg", b"ffmpeg-version")):
        value = lib.mpv_get_property_string(handle, prop)
        if not value:
            raise RuntimeError(f"Bundled libmpv did not report {prop!r}")
        try:
            versions[name] = c.string_at(value).decode()
        finally:
            lib.mpv_free(value)
finally:
    lib.mpv_terminate_destroy(handle)
appkit = c.CDLL("/System/Library/Frameworks/AppKit.framework/AppKit")
versions["AppKit"] = c.c_double.in_dll(appkit, "NSAppKitVersionNumber").value
print(json.dumps(versions))
"""
    # The separate probe needs a dylib lookup path. The app under test launches without DYLD_*.
    env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    versions = json.loads(command(sys.executable, "-c", probe, str(libmpv), capture_output=True, text=True,
                                  env={**env, "DYLD_LIBRARY_PATH": str(frameworks)}).stdout)
    product = command("sw_vers", "-productVersion", capture_output=True, text=True).stdout.strip()
    product_parts = product.split(".")
    return {
        "name": plist["CFBundleName"],
        "App": {
            "Version": receipt["bundle_version"], "Channel": plist["OKPReleaseChannel"],
            "Build": receipt["bundle_build"], "Revision": receipt["bundle_revision"],
            "License": plist["OKPLicense"],
        },
        "Engine": {
            "libmpv": versions["libmpv"], "FFmpeg": versions["FFmpeg"],
            "Render API": "libmpv render", "Graphics": "OpenGL · CAOpenGLLayer",
        },
        "Host": {
            "macOS": ".".join(product_parts + ["0"] * (3 - len(product_parts))),
            "AppKit": str(versions["AppKit"]), "CPU": command("uname", "-m", capture_output=True, text=True).stdout.strip(),
            "Install": plist["OKPPackageKind"],
        },
    }


def check_about_copy(result, expected):
    failures = result["assertion_failures"]
    lines = result.get("copied_about_diagnostics", "").split("\n\n")
    if len(lines) != 4 or lines[0] != expected["name"]:
        failures.append("about-identity:clipboard_metadata")
        result["status"] = "failed"
        return
    for block, section in zip(lines[1:], ("App", "Engine", "Host"), strict=True):
        title, _, content = block.partition("\n")
        actual = content.splitlines()
        valid = title == section
        if section == "Engine":
            decode_rows = [row for row in actual if row.startswith("  Hardware decode: ")]
            if len(decode_rows) == 1 and decode_rows[0].removeprefix("  Hardware decode: ").strip():
                actual.remove(decode_rows[0])
            else:
                valid = False
        if not valid or sorted(actual) != sorted(f"  {key}: {value}" for key, value in expected[section].items()):
            failures.append(f"about-identity:clipboard_{section.lower()}_metadata")
    if failures:
        result["status"] = "failed"


@contextmanager
def http_fixture(video, enabled):
    if not enabled:
        yield None
        return

    body = video.read_bytes()
    release = {name: Event() for name in ("release-stall", "release-buffer", "release-stale", "allow-retry")}

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            path = self.path.removeprefix("/")
            if path in release:
                release[path].set()
                self.send_response(204)
                self.end_headers()
                return
            if path == "stall.mp4":
                if not release["release-stall"].wait(20):
                    return
            elif path == "stale.mp4":
                release["release-stale"].wait(20)
                self.send_error(404)
                return
            elif path not in {"buffer.mp4", "single.mp4"}:
                if path != "failure.mp4" or not release["allow-retry"].is_set():
                    self.send_error(404)
                    return

            start = 0
            end = len(body) - 1
            range_header = self.headers.get("Range")
            if range_header:
                first, _, last = range_header.removeprefix("bytes=").partition("-")
                start = int(first) if first else 0
                end = min(int(last), end) if last else end
                if start > end:
                    self.send_error(416)
                    return
                self.send_response(206)
                self.send_header("Content-Range", f"bytes {start}-{end}/{len(body)}")
            else:
                self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Length", str(end - start + 1))
            self.end_headers()
            try:
                if path == "buffer.mp4":
                    # Let mpv open and play an initial portion before the
                    # server stops delivering bytes. Hold all ranges after it.
                    boundary = min(end + 1, max(start, len(body) // 22))
                    self.wfile.write(body[start:boundary])
                    if boundary < end + 1 and release["release-buffer"].wait(20):
                        self.wfile.write(body[boundary:end + 1])
                else:
                    self.wfile.write(body[start:end + 1])
            except (BrokenPipeError, ConnectionResetError):
                pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}/"
        finally:
            for event in release.values():
                event.set()
            server.shutdown()
            thread.join()


@contextmanager
def feedback_fixture(folder, appearance):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            try:
                assert self.path == "/api/feedback/ok-player"
                assert "Authorization" not in self.headers and "Cookie" not in self.headers
                length = int(self.headers["Content-Length"])
                assert 0 < length < 9 * 1024 * 1024
                data = self.rfile.read(length)
                mime = BytesParser(policy=policy.default).parsebytes(
                    f"Content-Type: {self.headers['Content-Type']}\r\n\r\n".encode() + data
                )
                parts = {part.get_param("name", header="content-disposition"): part.get_payload(decode=True)
                         for part in mime.iter_parts()}
                assert set(parts) == {"text", "kind", "meta", "install_id", "screenshot", "log"}
                assert parts["text"].decode() == f"OK Player verifier — {appearance}"
                assert parts["kind"] == b"bug" and UUID(parts["install_id"].decode()).version == 4
                meta = json.loads(parts["meta"])
                assert set(meta) == {"version", "build", "source", "os", "os_version", "arch", "channel"}
                assert meta["os"] == "macOS" and meta["arch"] == "aarch64" and len(meta["source"]) == 40
                assert json.loads(parts["log"])["playback"] == "paused"
                assert parts["screenshot"].startswith(b"\x89PNG\r\n\x1a\n")
                requests.append({
                    "source_sha": meta["source"], "build": meta["build"], "issue_number": 123,
                    "screenshot_sha256": hashlib.sha256(parts["screenshot"]).hexdigest(),
                })
                payload = json.dumps({
                    "issue_number": 123, "issue_url": "https://git.oklabs.uk/BeFeast/ok-player/issues/123",
                }).encode()
            except (AssertionError, KeyError, TypeError, ValueError):
                self.send_error(400)
                return
            self.send_response(201)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}/"
        finally:
            server.shutdown()
            thread.join()
            (folder / "feedback-loopback.json").write_text(json.dumps(requests, indent=2))


def run_process(executable, scenario, video, profile, output, folder, appearance, negative_control, phase=None,
                extra_env=None):
    env = {key: value for key, value in os.environ.items() if not key.startswith("DYLD_")}
    env.update({"TZ": "UTC", "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"})
    env.update(extra_env or {})
    argv = [executable, f"--verify-scenario={scenario}", f"--verify-fixture={video}",
            f"--verify-output={folder}", f"--verify-profile={profile}",
            f"--verify-appearance={appearance}"]
    if negative_control:
        argv.append("--verify-negative-control")
    if phase:
        argv.append(f"--verify-phase={phase}")
    fixture_server = (feedback_fixture(folder, appearance) if scenario.stem == "feedback-dialog"
                      else http_fixture(video, scenario.stem in {"osc-status", "queue-single"}))
    with fixture_server as url, (folder / "app.log").open("w") as log:
        if url:
            argv.append(f"--verify-http-fixture={url}")
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
        expected = [step["name"] for step in read_scenario(scenario)["steps"]
                    if step["action"] in {"capture", "check_update"} and step.get("phase") == phase]
        if [point["name"] for point in result["checkpoints"]] != expected:
            return {**result, "status": "error", "error": "App did not return every checkpoint"}
        if scenario.stem == "feedback-dialog":
            requests = json.loads((folder / "feedback-loopback.json").read_text())
            if (len(requests) != 1 or requests[0]["source_sha"] != result.get("source_sha")
                    or requests[0]["screenshot_sha256"] != digest(folder / "attached-player.png")):
                return {**result, "status": "error", "error": "Feedback did not deliver the attached PNG exactly once"}
            result["feedback_loopback"] = requests[0]
        for checkpoint in result["checkpoints"]:
            path = folder / checkpoint["png"]
            checkpoint["png"] = path.relative_to(output).as_posix()
            checkpoint["png_sha256"] = digest(path)
    elif result.get("status") not in {"blocked_environment", "error"}:
        result.update(status="error", error="App status and exit code disagree")
    return result


@contextmanager
def update_signing_identity(work, use_hosted=True):
    script = ROOT / "scripts/macos-signing-identity.sh"
    if use_hosted and os.environ.get("OKP_SIGNING_MODE") == "self-signed":
        yield os.environ.copy()
        return
    config = work / "test-signing.cnf"
    config.write_text("""[req]
distinguished_name = dn
x509_extensions = extension
prompt = no
[dn]
CN = OK Player updater verification
[extension]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
""")
    cert = work / "cert.pem"
    secret = work / "key.pem"
    identity = work / "identity.p12"
    password = os.urandom(20).hex()
    command("openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-config", config, "-keyout", secret, "-out", cert, capture_output=True)
    command("openssl", "pkcs12", "-export", "-inkey", secret, "-in", cert, "-out", identity,
            "-passout", f"pass:{password}", "-keypbe", "PBE-SHA1-3DES",
            "-certpbe", "PBE-SHA1-3DES", "-macalg", "sha1", capture_output=True)
    import base64
    result = command("bash", script, "import", work / "signing", capture_output=True, text=True,
                     env={**os.environ,
                          "OKP_MACOS_SIGNING_P12_BASE64": base64.b64encode(identity.read_bytes()).decode(),
                          "OKP_MACOS_SIGNING_P12_PASSWORD": password})
    exported = dict(line.split("=", 1) for line in result.stdout.splitlines() if line.startswith("OKP_"))
    try:
        yield {**os.environ, **exported}
    finally:
        command("bash", script, "cleanup", exported["OKP_CODESIGN_KEYCHAIN"])


def run_update_appearance(executable, scenario, video, profile, output, folder, appearance, negative_control,
                          control="update"):
    work = profile / "update"
    work.mkdir(mode=0o700)
    app = work / "OK Player.app"
    candidate = work / "next" / app.name
    candidate.parent.mkdir(mode=0o700)
    for copy in (app, candidate):
        command("ditto", executable.parents[2], copy)
    info = candidate / "Contents/Info.plist"
    metadata = plistlib.loads(info.read_bytes())
    prior = metadata["CFBundleVersion"]
    first, second = map(int, prior.split("."))
    build = f"{first}.{second + 1}"
    metadata["CFBundleVersion"] = build
    info.write_bytes(plistlib.dumps(metadata))
    source = metadata["OKPSourceSHA"]

    def sign_bundle(copy, env):
        binaries = sorted((copy / "Contents/MacOS").iterdir()) + sorted((copy / "Contents/Frameworks").glob("*.dylib"))
        command("bash", ROOT / "scripts/macos-signing-identity.sh", "sign", *binaries, copy, env=env, timeout=120)
        command("codesign", "--verify", "--deep", "--strict", copy)

    with update_signing_identity(work) as env:
        sign_bundle(app, env)
        if control == "wrong-certificate":
            other = work / "identity"
            other.mkdir(mode=0o700)
            with update_signing_identity(other, use_hosted=False) as other_env:
                sign_bundle(candidate, other_env)
        else:
            sign_bundle(candidate, env)
    quarantine = "0081;00000000;OKPlayerVerification;"
    if not negative_control and control == "update":
        command("xattr", "-w", "com.apple.quarantine", quarantine, app)

    feed = work / "feed"
    latest = feed / "ok-player/macos/beta/latest.json"
    latest.parent.mkdir(parents=True)
    key = f"ok-player/macos/builds/{build}/{ARCHIVE}"
    package = feed / key
    package.parent.mkdir(parents=True)
    command("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", candidate, package, timeout=120)
    content = {"source": source, "build": build, "version": metadata["CFBundleShortVersionString"],
               "filename": ARCHIVE, "sha256": digest(package), "size": package.stat().st_size}
    provenance = package.with_name("provenance.json")
    provenance.write_text(json.dumps(content, sort_keys=True, separators=(",", ":")) + "\n")
    pubkey = work / "fixture.asc"
    # Darwin's Unix socket path must also fit gpg-agent's socket suffixes.
    # Keep the GnuPG home outside the deeper per-appearance fixture directory.
    with tempfile.TemporaryDirectory(prefix="okp-gpg-") as gnupg:
        gpg = ("gpg", "--homedir", gnupg, "--batch", "--pinentry-mode", "loopback", "--passphrase", "")
        try:
            command(*gpg, "--quick-generate-key", "OK Player fixture <test@example.invalid>", "ed25519", "sign", "1d")
            fingerprints = command(*gpg, "--with-colons", "--fingerprint", capture_output=True, text=True).stdout
            fingerprint = next(line.split(":")[9] for line in fingerprints.splitlines() if line.startswith("fpr:"))
            command(*gpg, "--armor", "--output", pubkey, "--export", fingerprint)
            release = {**content, "schema": 1, "platform": "macos-arm64", "channel": "beta", "key": key,
                       "fingerprint": fingerprint, "provenance_sha256": digest(provenance)}
            latest.write_text(json.dumps(release, sort_keys=True, separators=(",", ":")) + "\n")
            manifest_sig = latest.parent / "manifests" / (digest(latest) + ".sig")
            manifest_sig.parent.mkdir()
            command(*gpg, "--output", manifest_sig, "--detach-sign", latest)
            command(*gpg, "--output", package.with_suffix(".zip.sig"), "--detach-sign", package, timeout=120)
        finally:
            subprocess.run(["gpgconf", "--homedir", gnupg, "--kill", "all"], check=False)

    if negative_control:
        corrupt = bytearray(manifest_sig.read_bytes())
        corrupt[-1] ^= 1
        manifest_sig.write_bytes(corrupt)

    requests = []
    class Handler(SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=feed, **kwargs)

        def log_message(self, *_):
            pass

        def do_GET(self):
            requests.append(self.path)
            super().do_GET()

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            result = run_process(app / "Contents/MacOS/ok-player", scenario, video, profile, output, folder,
                                 appearance, negative_control, extra_env={
                                     "OKP_VERIFY_UPDATE_PORT": str(server.server_port),
                                     "OKP_VERIFY_UPDATE_KEY": str(pubkey),
                                     "OKP_VERIFY_UPDATE_RESTART_PROFILE": str(profile),
                                     "OKP_VERIFY_UPDATE_FAIL_STARTUP": str(int(control == "rollback")),
                                     "OKP_VERIFY_UPDATE_WRONG_CERTIFICATE": str(int(control == "wrong-certificate")),
                                 })
            if result["status"] != "passed":
                return result
            result.update(old_build=prior, offered_build=build, offered_zip_sha256=content["sha256"],
                          manifest_sha256=digest(latest), manifest_signature_sha256=digest(manifest_sig))
            restarted = profile / "restarted.json"
            if negative_control:
                observed = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                if (observed["CFBundleVersion"] != prior or restarted.exists()
                        or any(path.startswith("/ok-player/macos/builds/") for path in requests)):
                    raise ValueError("Bad manifest reached download or changed the installed app")
                result["refused_before_download"] = True
            elif control == "wrong-certificate":
                observed = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                if (observed["CFBundleVersion"] != prior or restarted.exists()
                        or "staged_build" in result or f"/{key}" not in requests):
                    raise ValueError("The wrong certificate was not refused before replacement")
                result["refused_certificate"] = True
            else:
                receipt = profile / "rollback.json" if control == "rollback" else restarted
                deadline = time.monotonic() + 80
                while time.monotonic() < deadline and (not receipt.is_file() or list(work.glob(".okp-update-*"))):
                    time.sleep(0.1)
                running = json.loads(receipt.read_text())
                observed = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                expected = prior if control == "rollback" else build
                if (running["pid"] == result["pid"] or running["build"] != expected
                        or observed["CFBundleVersion"] != expected or running["source_sha"] != source
                        or result["staged_sha256"] != content["sha256"]
                        or f"/{key}" not in requests or f"/{key}.sig" not in requests
                        or list(work.glob(".okp-update-*"))):
                    raise ValueError("The verified bundle did not replace or relaunch the installed app")
                if control == "rollback":
                    refusal = json.loads((profile / "startup-refusal.json").read_text())
                    if (refusal["build"] != build or refusal["source_sha"] != source
                            or refusal["pid"] in {running["pid"], result["pid"]}
                            or "before confirming startup" not in running["error"]):
                        raise ValueError("The candidate did not fail and relaunch the previous build")
                    result.update(rolled_back=running, failed_startup=refusal)
                else:
                    observed_quarantine = command("xattr", "-p", "com.apple.quarantine", app,
                                                  capture_output=True, text=True).stdout.strip()
                    if observed_quarantine != quarantine:
                        raise ValueError("The installed app's quarantine attribute was lost")
                    result.update(restarted=running, quarantine_preserved=True)
            (folder / "requests.json").write_text(json.dumps(requests, indent=2) + "\n")
            return result
        finally:
            server.shutdown()
            thread.join()


def run_appearance(executable, scenario, video, temporary, output, appearance, negative_control):
    folder = output / appearance
    folder.mkdir()
    profile = temporary / appearance
    for subdir in ("data", "config", "cache", "pictures"):
        (profile / subdir).mkdir(parents=True)
    if scenario.stem == "welcome-return":
        (profile / "data/history.json").write_text(json.dumps({
            "version": 2, "files": {
                "https://example.com/welcome-return.mp4": {
                    "title": "Earlier playback", "position": 30.0, "duration": 180.0,
                    "finished": False, "updated_at_unix": 1700000000,
                },
            },
        }), encoding="utf-8")
    if scenario.stem == UPDATES:
        return run_update_appearance(executable, scenario, video, profile, output, folder, appearance, negative_control)
    if scenario.stem != SETTINGS:
        return run_process(executable, scenario, video, profile, output, folder, appearance, negative_control)

    path = profile / "config/settings.json"
    path.write_text(json.dumps(LEGACY_SETTINGS), encoding="utf-8")
    phases = []
    for phase in SETTINGS_PHASES:
        phase_dir = folder / phase
        phase_dir.mkdir()
        result = run_process(executable, scenario, video, profile, output, phase_dir,
                             appearance, negative_control, phase)
        if result["status"] not in {"passed", "failed"}:
            return result
        phases.append(result)

    last = phases[-1]
    with path.open(encoding="utf-8") as settings:
        profile_after = json.load(settings)
    preserved = (profile_after["playback"] == LEGACY_SETTINGS["playback"]
                 and profile_after["profile_extension"] == LEGACY_SETTINGS["profile_extension"]
                 and profile_after["appearance"]["accent_source"] == LEGACY_SETTINGS["appearance"]["accent_source"]
                 and profile_after["appearance"]["theme"] == "Auto")
    if not all(case.get("source_sha") == last.get("source_sha") and case.get("control") == last.get("control")
               for case in phases):
        return {**last, "status": "error", "error": "Settings relaunches used different builds or controls"}
    failures = [failure for case in phases for failure in case["assertion_failures"]]
    if not preserved:
        failures.append("settings_shell_appearance:unrelated_profile_fields")
    return {
        **last, "status": "failed" if failures else "passed",
        "exit_code": int(bool(failures)),
        "processes": [{"pid": case["pid"], "phase": case["phase"], "status": case["status"],
                       "exit_code": case["exit_code"]} for case in phases],
        "profile_preserved": preserved,
        "assertion_failures": failures,
        "checkpoints": [checkpoint for case in phases for checkpoint in case["checkpoints"]],
    }


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
            if spec["id"] == "about-identity" and not args.negative_control:
                receipt["about_expected"] = about_expected(executable, receipt)
            video = fixture(args.ffmpeg, temporary, spec, receipt)
            if spec["id"] == "open-files-folders":
                command("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework"
                        "/Support/lsregister", "-f", executable.parents[2])
            for appearance in spec["appearances"]:
                result = run_appearance(executable, scenario, video, temporary, output, appearance, args.negative_control)
                if result["status"] in {"passed", "failed"}:
                    if result.get("source_sha") != receipt["bundle_source_sha"]:
                        result.update(status="error", error="Running bundle SHA differed from the relocated package")
                    elif result.get("control") != control:
                        result.update(status="error", error="App did not report the requested control mode")
                    elif "about_expected" in receipt:
                        check_about_copy(result, receipt["about_expected"])
                receipt["appearances"].append(result)
            states = {result["status"] for result in receipt["appearances"]}
            if spec["id"] == UPDATES and not args.negative_control and states == {"passed"}:
                receipt["additional_controls"] = []
                for case in ("wrong-certificate", "rollback"):
                    profile, folder = temporary / case, output / case
                    folder.mkdir()
                    for directory in ("data", "config", "cache", "pictures"):
                        (profile / directory).mkdir(parents=True)
                    result = run_update_appearance(executable, scenario, video, profile, output, folder,
                                                   "light", False, control=case)
                    if result.get("source_sha") != receipt["bundle_source_sha"]:
                        result.update(status="error", error="Update control ran a different source SHA")
                    result["update_control"] = case
                    receipt["additional_controls"].append(result)
                    states.add(result["status"])
            receipt["status"] = next((state for state in ("error", "blocked_environment", "failed") if state in states), "passed")
            if args.negative_control and spec["id"] != UPDATES and receipt["status"] in {"passed", "failed"}:
                if spec["id"] == "subtitle-lift":
                    expected_failures = {
                        f"subtitle_clearance:{step['name']}" for step in spec["steps"]
                        if step.get("expect") == "clearance"
                    }
                elif spec["id"] in TRACK_CONTROLS:
                    expected_failures = {"track_switcher:pin"}
                elif spec["id"] in ACTION_CONTROLS:
                    expected_failures = ACTION_CONTROLS[spec["id"]][1]
                elif spec["id"] == SETTINGS:
                    expected_failures = {"settings_shell_appearance:light-relaunch"}
                else:
                    checkpoints = {
                        "window-fullscreen": ["fullscreen", "restored"],
                        "window-fit": ["fit-4k", "fit-portrait"],
                        "window-float": ["pinned", "pinned-after-fullscreen"],
                    }
                    prefix = spec["id"].replace("-", "_")
                    expected_failures = {f"{prefix}:{name}" for name in checkpoints[spec["id"]]}
                if not all(expected_failures <= set(result["assertion_failures"])
                           for result in receipt["appearances"]):
                    receipt.update(status="error", error="Negative control did not expose all expected failures in both themes")
                if spec["id"] in {SETTINGS, *ABOUT_CONTROLS, *QUEUE_CONTROLS} and any(
                    set(result["assertion_failures"]) != expected_failures for result in receipt["appearances"]
                ):
                    receipt.update(status="error", error="Negative control failed unrelated assertions")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        receipt["error"] = str(error)
    finally:
        verdict = output / "result.json"
        verdict.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        print(f"{receipt['status']}: {verdict}", flush=True)
    return {"passed": 0, "failed": 1}.get(receipt["status"], 2)


if __name__ == "__main__":
    raise SystemExit(main())
