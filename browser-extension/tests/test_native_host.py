from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[1]
HOST_SOURCE = ROOT / "host/ok_player_browser_host.py"


def frame(value: object) -> bytes:
    payload = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    return struct.pack("<I", len(payload)) + payload


def parse_single_frame(value: bytes) -> dict[str, object]:
    if len(value) < 4:
        raise AssertionError("native response had no complete header")
    (size,) = struct.unpack("<I", value[:4])
    if len(value) != size + 4:
        raise AssertionError("native response contained missing or extra stdout bytes")
    result = json.loads(value[4:].decode("utf-8"))
    if not isinstance(result, dict):
        raise AssertionError("native response was not an object")
    return result


class NativeHostTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.host = self.root / "ok_player_browser_host.py"
        shutil.copyfile(HOST_SOURCE, self.host)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_host(self, request: bytes, environment: dict[str, str] | None = None):
        return subprocess.run(
            [sys.executable, os.fspath(self.host)],
            input=request,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            check=False,
            timeout=5,
        )

    def configure(self, player: Path) -> None:
        (self.root / "host-config.json").write_text(
            json.dumps({"player_path": os.fspath(player)}), encoding="utf-8"
        )

    def test_unicode_query_and_fragment_reach_one_exact_player_argument(self) -> None:
        capture = self.root / "captured-argv.jsonl"
        player = self.root / "ok-player"
        player.write_text(
            "#!/usr/bin/python3\n"
            "import json, os, sys\n"
            "with open(os.environ['OKP_BROWSER_TEST_CAPTURE'], 'a', encoding='utf-8') as stream:\n"
            "    stream.write(json.dumps(sys.argv[1:], ensure_ascii=False) + '\\n')\n"
            "print('child output must not enter native messaging')\n",
            encoding="utf-8",
        )
        player.chmod(0o755)
        self.configure(player)
        url = "https://example.test/雪?q=a%26b&name=été#章-2"
        environment = dict(os.environ)
        environment["OKP_BROWSER_TEST_CAPTURE"] = os.fspath(capture)

        completed = self.run_host(frame({"url": url}), environment)
        self.assertEqual(completed.returncode, 0, completed.stderr.decode("utf-8"))
        response = parse_single_frame(completed.stdout)
        self.assertEqual(response["ok"], True)
        self.assertEqual(response["status"], "launch_requested")
        self.assertIn("not been confirmed", response["message"])

        deadline = time.monotonic() + 3
        while (not capture.exists() or capture.stat().st_size == 0) and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertTrue(capture.exists(), "the fake installed player was not invoked")
        invocations = [json.loads(line) for line in capture.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(invocations, [[url]])

    def test_missing_configuration_and_player_return_actionable_errors(self) -> None:
        missing_config = parse_single_frame(
            self.run_host(frame({"url": "https://example.test/video"})).stdout
        )
        self.assertEqual(missing_config["code"], "host_not_configured")
        self.assertIn("Reinstall", missing_config["message"])

        self.configure(self.root / "missing-ok-player")
        missing_player = parse_single_frame(
            self.run_host(frame({"url": "https://example.test/video"})).stdout
        )
        self.assertEqual(missing_player["code"], "player_not_found")
        self.assertIn("--player", missing_player["message"])

    def test_unsupported_and_option_looking_inputs_do_not_launch(self) -> None:
        player = self.root / "ok-player"
        player.write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")
        player.chmod(0o755)
        self.configure(player)
        for value in (
            "file:///tmp/movie.mp4",
            "javascript:alert(1)",
            "--fullscreen",
            "https://example.test/video\x00next",
            " https://example.test/video",
        ):
            with self.subTest(value=value):
                response = parse_single_frame(self.run_host(frame({"url": value})).stdout)
                self.assertEqual(response["ok"], False)
                self.assertIn(response["code"], {"unsafe_url", "unsupported_url"})

        surrogate_payload = b'{"url":"https://example.test/\\ud800"}'
        surrogate_response = parse_single_frame(
            self.run_host(struct.pack("<I", len(surrogate_payload)) + surrogate_payload).stdout
        )
        self.assertEqual(surrogate_response["code"], "unsafe_url")

    def test_request_shape_and_frame_size_are_bounded(self) -> None:
        wrong_shape = parse_single_frame(
            self.run_host(frame({"url": "https://example.test", "flags": ["--fullscreen"]})).stdout
        )
        self.assertEqual(wrong_shape["code"], "invalid_request")

        oversized = parse_single_frame(self.run_host(struct.pack("<I", 64 * 1024 + 1)).stdout)
        self.assertEqual(oversized["code"], "invalid_request")
        self.assertIn("too large", oversized["message"])


if __name__ == "__main__":
    unittest.main()
