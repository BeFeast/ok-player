#!/usr/bin/env python3
"""The macOS bundle: version from git, final identity in Info.plist, and the app icon."""
import importlib.util
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
PLIST_SCRIPT = ROOT / "scripts" / "generate-macos-info-plist.py"
ICON_SCRIPT = ROOT / "scripts" / "generate-macos-icon.py"
TEMPLATE = ROOT / "macos" / "Info.plist"
ICON = ROOT / "macos" / "Resources" / "AppIcon.icns"

spec = importlib.util.spec_from_file_location("generate_macos_icon", ICON_SCRIPT)
icon = importlib.util.module_from_spec(spec)
spec.loader.exec_module(icon)

# The ten images of a macOS iconset, by ICNS element type.
ICONSET = {"icp4": 16, "ic11": 32, "icp5": 32, "ic12": 64, "ic07": 128,
           "ic13": 256, "ic08": 256, "ic14": 512, "ic09": 512, "ic10": 1024}


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True).stdout.strip()


def commit(repo, message):
    git(repo, "-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q",
        "--allow-empty", "-m", message)
    return git(repo, "rev-parse", "HEAD")


def stamp(repo, build_number="0", template=TEMPLATE):
    """Run the generator as the build script does; return (exit code, plist or None, stderr)."""
    with tempfile.TemporaryDirectory() as scratch:
        output = Path(scratch, "Info.plist")
        result = subprocess.run([sys.executable, str(PLIST_SCRIPT), str(template), str(output),
                                 "--repo", str(repo), "--build-number", build_number],
                                capture_output=True, text=True)
        info = plistlib.loads(output.read_bytes()) if output.exists() else None
    return result.returncode, info, result.stderr


class BundleVersionTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.repo = Path(self.scratch.name, "repo")
        self.repo.mkdir()
        git(self.repo, "init", "-q")

    def tearDown(self):
        self.scratch.cleanup()

    def test_version_comes_from_the_nearest_release_tag_and_the_build_number(self):
        commit(self.repo, "one")
        git(self.repo, "tag", "v0.10.14")
        commit(self.repo, "two")
        git(self.repo, "tag", "v0.11.0-beta.1")
        commit(self.repo, "three")
        head = commit(self.repo, "four")
        # Tags of other lanes are not product versions.
        git(self.repo, "tag", "linux-v9.9.9-linux-alpha.1")
        git(self.repo, "tag", "windows-candidate")
        code, info, stderr = stamp(self.repo, "57")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(info["CFBundleShortVersionString"], "0.11.0")
        self.assertEqual(info["CFBundleVersion"], "4.57")
        self.assertEqual(info["OKPSourceRevision"], f"v0.11.0-beta.1-2-g{head[:7]}")

    def test_a_later_commit_gets_a_higher_bundle_version(self):
        commit(self.repo, "one")
        git(self.repo, "tag", "v1.2.3")
        _, earlier, _ = stamp(self.repo, "9")
        commit(self.repo, "two")
        _, later, _ = stamp(self.repo, "9")
        self.assertEqual(earlier["CFBundleVersion"], "1.9")
        self.assertEqual(later["CFBundleVersion"], "2.9")
        self.assertEqual(later["CFBundleShortVersionString"], "1.2.3")

    def test_without_a_release_tag_the_build_fails_instead_of_inventing_a_version(self):
        commit(self.repo, "one")
        git(self.repo, "tag", "linux-v0.1.0")
        code, info, stderr = stamp(self.repo)
        self.assertNotEqual(code, 0)
        self.assertIsNone(info)
        self.assertIn("no v<major>.<minor>.<patch> tag", stderr)

    def test_a_shallow_clone_is_refused(self):
        commit(self.repo, "one")
        git(self.repo, "tag", "v1.0.0")
        commit(self.repo, "two")
        shallow = Path(self.scratch.name, "shallow")
        subprocess.run(["git", "clone", "-q", "--depth=1", f"file://{self.repo}", str(shallow)],
                       check=True, capture_output=True)
        code, _, stderr = stamp(shallow)
        self.assertNotEqual(code, 0)
        self.assertIn("shallow", stderr)

    def test_a_build_number_that_is_not_a_number_is_refused(self):
        commit(self.repo, "one")
        git(self.repo, "tag", "v1.0.0")
        code, _, stderr = stamp(self.repo, "12a")
        self.assertNotEqual(code, 0)
        self.assertIn("build number", stderr)


class InfoPlistTests(unittest.TestCase):
    def test_built_plist_carries_the_final_identity_icon_and_network_description(self):
        with tempfile.TemporaryDirectory() as scratch:
            repo = Path(scratch)
            git(repo, "init", "-q")
            commit(repo, "one")
            git(repo, "tag", "v2.0.0")
            code, info, stderr = stamp(repo, "3")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(info["CFBundleIdentifier"], "uk.oklabs.ok-player")
        self.assertEqual(info["CFBundleExecutable"], "ok-player")
        self.assertEqual(info["CFBundleIconFile"], "AppIcon")
        self.assertEqual(info["LSMinimumSystemVersion"], "14.0")
        self.assertTrue(info["NSLocalNetworkUsageDescription"].strip())
        self.assertEqual((info["CFBundleShortVersionString"], info["CFBundleVersion"]), ("2.0.0", "1.3"))
        self.assertEqual(info["CFBundleDocumentTypes"][0]["CFBundleTypeRole"], "Viewer")


class IconTests(unittest.TestCase):
    def assert_iconset(self, data):
        elements = icon.read_icns(data)
        self.assertEqual([kind for kind, _ in elements], list(ICONSET))
        for kind, png in elements:
            self.assertEqual(icon.png_size(png), (ICONSET[kind], ICONSET[kind]), kind)

    def test_container_round_trips_and_states_each_length(self):
        pngs = [(kind, icon.PNG_SIGNATURE + bytes([index]) * index) for index, kind in enumerate(["icp4", "ic10"], 1)]
        data = icon.pack_icns(pngs)
        self.assertEqual(data[:4], b"icns")
        self.assertEqual(int.from_bytes(data[4:8], "big"), len(data))
        self.assertEqual(icon.read_icns(data), pngs)

    def test_container_refuses_data_that_is_not_png(self):
        with self.assertRaises(ValueError):
            icon.pack_icns([("ic10", b"not a png")])
        with self.assertRaises(ValueError):
            icon.read_icns(b"icns\x00\x00\x00\x09x")

    def test_checked_in_icon_holds_every_size_of_the_iconset(self):
        self.assert_iconset(ICON.read_bytes())

    @unittest.skipUnless(shutil.which("rsvg-convert"), "rsvg-convert (librsvg) is not installed")
    def test_generator_renders_every_size_from_the_linux_icon_sources(self):
        with tempfile.TemporaryDirectory() as scratch:
            output = Path(scratch, "AppIcon.icns")
            result = subprocess.run([sys.executable, str(ICON_SCRIPT), str(output)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_iconset(output.read_bytes())


if __name__ == "__main__":
    unittest.main()
