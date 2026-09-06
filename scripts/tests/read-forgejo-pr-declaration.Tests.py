#!/usr/bin/env python3
"""Exercise live declaration writes and fail-closed errors without network."""
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError

spec = importlib.util.spec_from_file_location(
    "declaration", Path(__file__).resolve().parents[1] / "read-forgejo-pr-declaration.py"
)
reader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reader)


class DeclarationTests(unittest.TestCase):
    def call(self, output, payload=None, error=None):
        with patch.dict(os.environ, {
            "FORGEJO_API_URL": "https://forge.example/api/v1",
            "GITHUB_REPOSITORY": "owner/player",
            "FORGEJO_TOKEN": "test-token",
        }), patch("sys.argv", ["reader", "12", output]), patch.object(
            reader, "urlopen", side_effect=error,
            return_value=io.BytesIO(json.dumps(payload).encode()),
        ) as request:
            reader.main()
            self.assertEqual(request.call_args.args[0].full_url,
                             "https://forge.example/api/v1/repos/owner/player/pulls/12")

    def test_reread_replaces_stale_declaration(self):
        with tempfile.TemporaryDirectory() as output:
            self.call(output, {"title": "WIP previous", "body": "old"})
            self.call(output, {"title": "Ready", "body": "Current results"})
            self.assertEqual(Path(output, "pr-title.txt").read_text(), "Ready\n")
            self.assertEqual(Path(output, "pr-body.txt").read_text(), "Current results\n")

    def test_null_body_is_empty_for_the_unfinished_gate(self):
        with tempfile.TemporaryDirectory() as output:
            self.call(output, {"title": "Change", "body": None})
            self.assertEqual(Path(output, "pr-body.txt").read_text(), "\n")

    def test_api_failure_does_not_fabricate_declaration(self):
        with tempfile.TemporaryDirectory() as output:
            with self.assertRaises(HTTPError):
                self.call(output, error=HTTPError("https://forge.example", 403, "Forbidden", {}, None))
            self.assertFalse(Path(output, "pr-title.txt").exists())

    def test_non_object_payload_fails_closed(self):
        with tempfile.TemporaryDirectory() as output:
            with self.assertRaises(SystemExit):
                self.call(output, ["unexpected"])
            self.assertFalse(Path(output, "pr-title.txt").exists())

    def test_malformed_payload_fails_closed(self):
        with tempfile.TemporaryDirectory() as output:
            with self.assertRaises(SystemExit):
                self.call(output, {"message": "error"})
            self.assertFalse(Path(output, "pr-title.txt").exists())


if __name__ == "__main__":
    unittest.main()
