from __future__ import annotations

import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
INSTALLER = ROOT / "install.py"
HOST_NAME = "org.ok_player.browser"


class InstallerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.home = Path(self.temporary.name) / "user-home"
        self.home.mkdir()
        self.player = self.home / ".local/bin/ok-player"
        self.player.parent.mkdir(parents=True)
        self.player.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        self.player.chmod(0o755)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_installer(self, *arguments: str, expect_success: bool = True):
        completed = subprocess.run(
            [
                sys.executable,
                os.fspath(INSTALLER),
                "--user-home",
                os.fspath(self.home),
                *arguments,
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
            timeout=10,
        )
        if expect_success and completed.returncode != 0:
            self.fail(f"installer failed: {completed.stderr}")
        return completed

    def test_dry_run_reports_absolute_targets_without_writing(self) -> None:
        config = self.home / "custom-helium"
        completed = self.run_installer(
            "--browser",
            "helium",
            "--browser-config-dir",
            os.fspath(config),
            "--player",
            os.fspath(self.player),
            "--dry-run",
        )
        self.assertIn("Dry run only", completed.stdout)
        self.assertIn(os.fspath(config / "NativeMessagingHosts"), completed.stdout)
        self.assertFalse(config.exists())
        self.assertFalse(
            (self.home / ".local/state/ok-player/browser-extension/helium.json").exists()
        )

    def test_apply_installs_fixed_origin_and_uninstall_restores_files_and_flags(self) -> None:
        config = self.home / "custom-helium"
        native_manifest = config / "NativeMessagingHosts" / f"{HOST_NAME}.json"
        native_manifest.parent.mkdir(parents=True)
        prior_manifest = b'{"name":"preexisting-host"}\n'
        native_manifest.write_bytes(prior_manifest)
        flags = self.home / ".config/helium-browser-flags.conf"
        flags.parent.mkdir(parents=True)
        original_flags = "--enable-features=UseOzonePlatform\n# operator setting\n"
        flags.write_text(original_flags, encoding="utf-8")

        applied = self.run_installer(
            "--browser",
            "helium",
            "--browser-config-dir",
            os.fspath(config),
            "--player",
            os.fspath(self.player),
            "--load-extension",
            "--apply",
        )
        self.assertIn("no browser was started", applied.stdout)

        extension_dir = (
            self.home / ".local/share/ok-player/browser-extension/helium/extension"
        )
        manifest = json.loads((extension_dir / "manifest.json").read_text(encoding="utf-8"))
        digest = hashlib.sha256(base64.b64decode(manifest["key"])).hexdigest()[:32]
        expected_id = "".join(chr(ord("a") + int(nibble, 16)) for nibble in digest)
        installed_native = json.loads(native_manifest.read_text(encoding="utf-8"))
        self.assertEqual(
            installed_native["allowed_origins"], [f"chrome-extension://{expected_id}/"]
        )
        self.assertTrue(Path(installed_native["path"]).is_absolute())
        host_config = Path(installed_native["path"]).with_name("host-config.json")
        self.assertEqual(
            json.loads(host_config.read_text(encoding="utf-8"))["player_path"],
            os.fspath(self.player),
        )
        self.assertTrue(os.access(installed_native["path"], os.X_OK))
        managed_flag = f"--load-extension={extension_dir}"
        self.assertEqual(flags.read_text(encoding="utf-8"), original_flags + managed_flag + "\n")

        with flags.open("a", encoding="utf-8") as stream:
            stream.write("--operator-added-later\n")
        removed = self.run_installer("--browser", "helium", "--uninstall")
        self.assertIn("unrelated files and flags were preserved", removed.stdout)
        self.assertEqual(native_manifest.read_bytes(), prior_manifest)
        self.assertEqual(
            flags.read_text(encoding="utf-8"), original_flags + "--operator-added-later\n"
        )
        self.assertFalse((extension_dir / "manifest.json").exists())
        self.assertFalse(
            (self.home / ".local/state/ok-player/browser-extension/helium.json").exists()
        )

    def test_rollback_alias_restores_a_preexisting_chromium_manifest(self) -> None:
        config = self.home / ".config/chromium"
        native_manifest = config / "NativeMessagingHosts" / f"{HOST_NAME}.json"
        native_manifest.parent.mkdir(parents=True)
        native_manifest.write_text("preexisting\n", encoding="utf-8")

        self.run_installer(
            "--browser",
            "chromium",
            "--player",
            os.fspath(self.player),
            "--apply",
        )
        self.assertNotEqual(native_manifest.read_text(encoding="utf-8"), "preexisting\n")
        self.run_installer("--browser", "chromium", "--rollback")
        self.assertEqual(native_manifest.read_text(encoding="utf-8"), "preexisting\n")

    def test_apply_rejects_a_missing_player_without_installing_anything(self) -> None:
        missing = self.home / ".local/bin/missing-player"
        completed = self.run_installer(
            "--browser",
            "chromium",
            "--player",
            os.fspath(missing),
            "--apply",
            expect_success=False,
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("Pass --player", completed.stderr)
        self.assertFalse((self.home / ".config/chromium").exists())
        self.assertFalse(
            (self.home / ".local/state/ok-player/browser-extension/chromium.json").exists()
        )

    def test_uninstall_refuses_to_overwrite_a_managed_file_changed_later(self) -> None:
        self.run_installer(
            "--browser",
            "chromium",
            "--player",
            os.fspath(self.player),
            "--apply",
        )
        background = (
            self.home
            / ".local/share/ok-player/browser-extension/chromium/extension/background.js"
        )
        background.write_text("operator replacement\n", encoding="utf-8")

        completed = self.run_installer(
            "--browser", "chromium", "--uninstall", expect_success=False
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("changed after installation", completed.stderr)
        self.assertEqual(background.read_text(encoding="utf-8"), "operator replacement\n")
        self.assertTrue(
            (self.home / ".local/state/ok-player/browser-extension/chromium.json").exists()
        )


if __name__ == "__main__":
    unittest.main()
