#!/usr/bin/env python3
"""Exercise the real signed updater and atomic AppImage pointer on Linux."""
import hashlib
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from threading import Thread
import unittest

UPDATER = Path(sys.argv.pop(1)).resolve()
SOURCE = "1" * 40
OLD_BUILD = 100
NEW_BUILD = 101


def encode(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha(data):
    return hashlib.sha256(data).hexdigest()


def name(build):
    return f"OK-Player-0.11.0-beta.0.{build}-x86_64.AppImage"


class SignedAppImage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="okp-signed-updater-test-")
        cls.addClassCleanup(cls.temp.cleanup)
        cls.root = Path(cls.temp.name)
        cls.gpg_home = cls.root / "gpg"
        cls.gpg_home.mkdir(mode=0o700)
        cls.gpg = ("gpg", "--homedir", cls.gpg_home, "--batch", "--pinentry-mode", "loopback", "--passphrase", "")
        cls.addClassCleanup(subprocess.run, ["gpgconf", "--homedir", cls.gpg_home, "--kill", "all"])
        subprocess.run([*cls.gpg, "--quick-generate-key", "OK Player fixture <test@example.invalid>",
                        "rsa2048", "sign", "1d"], check=True, capture_output=True)
        fingerprints = subprocess.check_output([*cls.gpg, "--with-colons", "--fingerprint"], text=True)
        cls.fingerprint = next(line.split(":")[9] for line in fingerprints.splitlines() if line.startswith("fpr:"))
        cls.key = subprocess.check_output([*cls.gpg, "--export", cls.fingerprint])
        cls.player = cls.root / "player.c"
        cls.player.write_text("""
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const char *receipt = getenv("OKP_UPDATE_RESTART_RECEIPT");
    if (!receipt) return 0;
    if (!PUBLISH_RECEIPT) return 1;
    FILE *file = fopen(receipt, "wx");
    if (!file) return 2;
    fprintf(file, "%d\\n%s\\n", BUILD, SOURCE);
    return fclose(file);
}
""")

    def setUp(self):
        self.work = Path(tempfile.mkdtemp(prefix="test-", dir=self.root))
        self.target = self.work / "OK-Player.AppImage"
        self.pubkey = self.work / "fixture.gpg"
        self.pubkey.write_bytes(self.key)
        (self.work / "builds").mkdir(mode=0o700)
        self.previous = Path(f"builds/{OLD_BUILD}/{name(OLD_BUILD)}")
        self.image(OLD_BUILD, self.work / self.previous)
        self.target.symlink_to(self.previous)
        self.feed = self.work / "feed"
        self.feed.mkdir()
        handler = partial(SimpleHTTPRequestHandler, directory=self.feed)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.worker = Thread(target=self.server.serve_forever, daemon=True)
        self.worker.start()
        self.addCleanup(self.worker.join)
        self.addCleanup(self.server.server_close)
        self.addCleanup(self.server.shutdown)

    def image(self, build, path, publish_startup=True):
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["cc", f"-DBUILD={build}", f'-DSOURCE="{SOURCE}"',
                        f"-DPUBLISH_RECEIPT={int(publish_startup)}", self.player, "-o", path],
                       check=True, capture_output=True)
        path.chmod(0o755)
        with path.open("r+b") as exe:
            exe.seek(8)
            exe.write(b"AI\x02")

    def release(self, *, bad_manifest=False, bad_package=False, publish_startup=True, channel="beta"):
        key = f"ok-player/linux/builds/{NEW_BUILD}/{name(NEW_BUILD)}"
        package = self.feed / key
        self.image(NEW_BUILD, package, publish_startup)
        image = package.read_bytes()
        provenance = {"source": SOURCE, "build": NEW_BUILD, "version": f"0.11.0-beta.0.{NEW_BUILD}",
                      "filename": package.name, "sha256": sha(image), "size": len(image)}
        wire = encode(provenance)
        package.with_name("provenance.json").write_bytes(wire)
        manifest = self.feed / "ok-player/linux/beta/latest.json"
        signature_dir = manifest.parent / "manifests"
        signature_dir.mkdir(parents=True)
        release = {**provenance, "provenance_sha256": sha(wire), "schema": 1, "channel": channel,
                   "key": key, "fingerprint": self.fingerprint, "platform": "linux-appimage"}
        manifest.write_bytes(encode(release))
        self.sign(manifest, signature_dir / (sha(manifest.read_bytes()) + ".sig"), bad_manifest)
        self.sign(package, package.with_suffix(".AppImage.sig"), bad_package)
        return release

    def sign(self, path, sig, corrupt):
        subprocess.run([*self.gpg, "--output", sig, "--detach-sign", path], check=True, capture_output=True)
        if corrupt:
            data = bytearray(sig.read_bytes())
            data[-1] ^= 1
            sig.write_bytes(data)

    def command(self, action, *extra):
        return [UPDATER, f"fixture-{action}", "beta", str(OLD_BUILD), SOURCE, self.target,
                str(self.server.server_port), self.pubkey, *extra]

    def apply(self, release):
        with subprocess.Popen(["sleep", "60"]) as player:
            try:
                with subprocess.Popen(self.command("install", release["sha256"], str(player.pid)),
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      text=True) as helper:
                    line = helper.stdout.readline()
                    if line == "ready\n":
                        helper.stdin.write("relaunch\n")
                        helper.stdin.flush()
                        player.terminate()
                        player.wait(timeout=5)
                    _, error = helper.communicate(timeout=35)
                    return helper.returncode, line, error
            finally:
                if player.poll() is None:
                    player.terminate()

    def test_checked_verified_and_restarted(self):
        release = self.release()
        result = subprocess.run(self.command("check"), capture_output=True, text=True, check=True)
        self.assertEqual(json.loads(result.stdout), release)
        status, phase, error = self.apply(release)
        self.assertEqual((status, phase), (0, "ready\n"), error)
        self.assertEqual(self.target.readlink(), Path(f"builds/{NEW_BUILD}/{name(NEW_BUILD)}"))
        self.assertEqual(sha(self.target.read_bytes()), release["sha256"])
        self.assertEqual(json.loads((self.target.resolve().parent / "release.json").read_text()), release)
        self.assertTrue((self.work / self.previous).is_file())

    def test_invalid_manifest_stops_check(self):
        self.release(bad_manifest=True)
        result = subprocess.run(self.command("check"), capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("signature could not be verified", result.stderr)
        self.assertEqual(self.target.readlink(), self.previous)

    def test_signed_manifest_from_other_channel_is_rejected(self):
        self.release(channel="stable")
        result = subprocess.run(self.command("check"), capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid release identity", result.stderr)

    def test_current_build_is_not_offered_again(self):
        self.release()
        argv = self.command("check")
        argv[3] = str(NEW_BUILD)
        result = subprocess.run(argv, capture_output=True, text=True, check=True)
        self.assertIsNone(json.loads(result.stdout))

    def test_invalid_package_stops_before_restart(self):
        release = self.release(bad_package=True)
        status, phase, error = self.apply(release)
        self.assertNotEqual(status, 0)
        self.assertEqual(phase, "")
        self.assertIn("signature could not be verified", error)
        self.assertEqual(self.target.readlink(), self.previous)

    def test_wrong_package_digest_stops_before_restart(self):
        release = self.release()
        package = self.feed / release["key"]
        payload = bytearray(package.read_bytes())
        payload[-1] ^= 1
        package.write_bytes(payload)
        status, phase, error = self.apply(release)
        self.assertNotEqual(status, 0)
        self.assertEqual(phase, "")
        self.assertIn("wrong SHA-256", error)
        self.assertEqual(self.target.readlink(), self.previous)

    def test_failed_startup_rolls_back(self):
        release = self.release(publish_startup=False)
        status, phase, error = self.apply(release)
        self.assertNotEqual(status, 0)
        self.assertEqual(phase, "ready\n")
        self.assertIn("exited before confirming startup", error)
        self.assertEqual(self.target.readlink(), self.previous)


if __name__ == "__main__":
    unittest.main(verbosity=2)
