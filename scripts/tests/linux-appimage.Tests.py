#!/usr/bin/env python3
"""Behavioural tests for the AppImage archive, channel and managed installation."""

import fcntl
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "linux-appimage.py"
SPEC = importlib.util.spec_from_file_location("linux_appimage", SCRIPT)
appimage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(appimage)


class MemoryStore:
    def __init__(self):
        self.objects = {}
        self.writes = []
        self.interrupt_at = None

    def read(self, key, limit=None, missing=False):
        if missing:
            return self.objects.get(key)
        return self.objects[key]

    def put(self, key, data, immutable=False):
        if key == self.interrupt_at:
            raise OSError("Interrupted channel publication")
        if immutable and key in self.objects:
            raise AssertionError(f"Attempt to replace {key}")
        self.objects[key] = data
        self.writes.append(key)


class DigestSigner:
    def sign(self, data):
        return appimage.digest(data).encode()

    def verify(self, data, signature):
        if signature != self.sign(data):
            raise ValueError("Invalid test signature")


def fixture(seed):
    image = bytearray(bytes([seed]) * 2048)
    image[:4] = b"\x7fELF"
    image[8:11] = b"AI\x02"
    image[18:20] = b"\x3e\x00"
    return bytes(image)


class AppImageBuildTests(unittest.TestCase):
    def test_build_handoff_binds_image_to_first_parent_source(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            source = work / "source"
            source.mkdir()
            output = work / "output"

            def git(*arguments):
                return subprocess.check_output(
                    ["git", "-C", source, *arguments], text=True
                ).strip()

            git("init", "--quiet", "--initial-branch=main")
            git("config", "user.name", "AppImage test")
            git("config", "user.email", "appimage@example.invalid")
            git("commit", "--quiet", "--allow-empty", "-m", "Root")
            git("switch", "--quiet", "-c", "side")
            git("commit", "--quiet", "--allow-empty", "-m", "Side")
            git("switch", "--quiet", "main")
            git("merge", "--quiet", "--no-ff", "-m", "Merge side", "side")

            scripts = source / "scripts"
            scripts.mkdir()
            builder = scripts / "build-linux-portable-package.sh"
            builder.write_text(
                '#!/usr/bin/env bash\n'
                'set -eu\n'
                '[[ "$#" == 2 && "$1" == appimage ]]\n'
                'cd "$(dirname "$0")/.."\n'
                'mkdir -p artifacts/linux/velopack\n'
                'printf "%s" "$2" >"artifacts/linux/velopack/OK-Player-$2-x86_64.AppImage"\n'
            )
            builder.chmod(0o755)
            subprocess.run(
                [SCRIPT.with_name("build-linux-appimage.sh"), source, output], check=True
            )
            version = "0.11.0-beta.0.2"
            image = output / f"OK-Player-{version}-x86_64.AppImage"
            self.assertEqual(image.read_text(), version)
            self.assertTrue(image.stat().st_mode & 0o100)
            self.assertEqual((output / "source.txt").read_text().strip(), git("rev-parse", "HEAD"))
            self.assertEqual((output / "build.txt").read_text().strip(), "2")
            self.assertEqual(
                (output / "SHA256SUMS").read_text().split(),
                [appimage.digest(image.read_bytes()), image.name],
            )


class AppImageTests(unittest.TestCase):
    def setUp(self):
        self.data = fixture(1)
        self.metadata = appimage.identity("1" * 40, 11, self.data)
        self.store = MemoryStore()
        self.signer = DigestSigner()

    def archive(self):
        return appimage.publish_archive(self.store, self.metadata, self.data,
                                        self.signer.sign, self.signer)

    def test_archive_is_immutable_and_retry_reuses_exact_bytes(self):
        release = self.archive()
        first_write_count = len(self.store.writes)
        self.assertEqual(self.archive(), release)
        self.assertEqual(len(self.store.writes), first_write_count)
        other = fixture(2)
        with self.assertRaisesRegex(ValueError, "Conflicting immutable"):
            appimage.publish_archive(self.store, appimage.identity("1" * 40, 11, other), other,
                                     self.signer.sign, self.signer)
        self.assertEqual(len(self.store.writes), first_write_count)

    def test_promoted_channel_points_at_the_same_signed_image(self):
        beta = self.archive()
        stable = {**beta, "channel": "stable"}
        appimage.publish_channel(self.store, stable, self.signer.sign, self.signer)
        pointer = f"{appimage.PREFIX}/stable/latest.json"
        self.assertEqual(self.store.writes[-2:], [f"{pointer}.sig", pointer])
        fetched = appimage.read_channel(self.store.read, "stable", self.signer)
        self.assertEqual(appimage.payload(self.store.read, fetched, "stable", self.signer), self.data)
        older = appimage.descriptor(appimage.identity("2" * 40, 10, fixture(2)), "stable")
        with self.assertRaisesRegex(ValueError, "backwards"):
            appimage.publish_channel(self.store, older, self.signer.sign, self.signer)
        self.assertEqual(appimage.read_channel(self.store.read, "stable", self.signer), fetched)

    def test_mac_prefix_orders_dotted_builds_and_retains_the_latest_pointer(self):
        prefix = "ok-player/macos"
        old = {"channel": "beta", "build": "10.9"}
        new = {"channel": "beta", "build": "10.10"}

        def order(value):
            return tuple(map(int, value.split(".")))

        for release in (old, new):
            appimage.publish_channel(self.store, release, self.signer.sign, self.signer,
                                     prefix=prefix, order=order)
        pointer = f"{prefix}/beta/latest.json"
        actual = self.store.read(pointer)
        self.assertEqual(actual, appimage.encode(new))
        sig_key = appimage.channel_signature_key("beta", actual, prefix)
        self.signer.verify(actual, self.store.read(sig_key))
        with self.assertRaisesRegex(ValueError, "backwards"):
            appimage.publish_channel(self.store, old, self.signer.sign, self.signer,
                                     prefix=prefix, order=order)
        self.assertEqual(self.store.read(pointer), actual)

    def test_current_channel_stays_verifiable_when_publication_is_interrupted(self):
        current = self.archive()
        appimage.publish_channel(self.store, current, self.signer.sign, self.signer)
        next_data = fixture(2)
        next_meta = appimage.identity("2" * 40, 12, next_data)
        newer = appimage.publish_archive(self.store, next_meta, next_data, self.signer.sign, self.signer)
        self.store.interrupt_at = appimage.channel_key("beta")
        with self.assertRaisesRegex(OSError, "Interrupted channel publication"):
            appimage.publish_channel(self.store, newer, self.signer.sign, self.signer)
        self.assertEqual(appimage.read_channel(self.store.read, "beta", self.signer), current)
        self.store.interrupt_at = None
        appimage.publish_channel(self.store, newer, self.signer.sign, self.signer)
        self.assertEqual(appimage.read_channel(self.store.read, "beta", self.signer), newer)

    def test_payload_and_descriptors_fail_closed_on_tampering(self):
        beta = self.archive()
        release_path = f"{appimage.archive(beta['build'])}/release.json"
        self.store.objects[release_path] += b" "
        with self.assertRaisesRegex(ValueError, "signature"):
            appimage.read_signed(self.store.read, release_path, self.signer)
        with self.assertRaisesRegex(ValueError, "descriptor and provenance"):
            appimage.payload(self.store.read, {**beta, "key": "somewhere-else"}, "beta", self.signer)
        self.store.objects[beta["key"]] = fixture(2)
        with self.assertRaisesRegex(ValueError, "bytes and provenance"):
            appimage.payload(self.store.read, beta, "beta", self.signer)

    def test_install_serializes_with_launch_and_preserves_rollback(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            managed = root / "OK-Player.AppImage"
            appimage.install(self.data, self.metadata, root)
            original = managed.resolve()
            newer_data = fixture(2)
            newer = appimage.identity("2" * 40, 12, newer_data)
            with (root / ".appimage.lock").open("a") as launcher:
                fcntl.flock(launcher, fcntl.LOCK_SH)
                with self.assertRaisesRegex(ValueError, "Close OK Player"):
                    appimage.install(newer_data, newer, root)
            self.assertEqual(managed.resolve(), original)
            appimage.install(newer_data, newer, root)
            appimage.install(self.data, self.metadata, root)
            self.assertEqual(managed.read_bytes(), newer_data)
            self.assertEqual(original.read_bytes(), self.data)


if __name__ == "__main__":
    unittest.main()
