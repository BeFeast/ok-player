#!/usr/bin/env python3
"""Test the release admission checks before the operator's signing key is loaded."""
import hashlib
import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / "publish-macos-beta.py"
SPEC = importlib.util.spec_from_file_location("macos_beta_publisher", SCRIPT)
publisher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publisher)
SOURCE = "1" * 40


class MacReleaseAdmission(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(prefix="okp-publisher-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)

    def make_receipt(self, *, certificate=publisher.SIGNING_CERTIFICATE, source=SOURCE,
                     build="10.9", helper=True):
        (self.root / "source-sha.txt").write_text(SOURCE + "\n")
        requirement = f'identifier "{publisher.BUNDLE_ID}" and certificate leaf = H"{certificate.lower()}"'
        (self.root / "signing.txt").write_text(
            f"mode=self-signed identity={certificate}\ndesignated_requirement={requirement}\n"
        )
        archive = self.root / publisher.FILENAME
        with zipfile.ZipFile(archive, "w") as bundle:
            bundle.writestr("OK Player.app/Contents/Info.plist", plistlib.dumps({
                "CFBundleIdentifier": publisher.BUNDLE_ID,
                "CFBundleVersion": build,
                "CFBundleShortVersionString": "0.11.0",
                "OKPSourceSHA": source,
                "OKPUpdateSigningMode": "self-signed",
            }))
            bundle.writestr("OK Player.app/Contents/MacOS/ok-player", b"player fixture" * 80)
            if helper:
                bundle.writestr("OK Player.app/Contents/MacOS/okp-updater", b"updater fixture")
        image = archive.read_bytes()
        (self.root / "SHA256SUMS.txt").write_text(
            f"{hashlib.sha256(image).hexdigest()}  {publisher.FILENAME}\n"
        )
        return image

    def test_exact_receipt_supplies_the_release_identity(self):
        image = self.make_receipt()
        data, release = publisher.describe(self.root, SOURCE)
        self.assertEqual(data, image)
        self.assertEqual(release, {
            "source": SOURCE, "build": "10.9", "version": "0.11.0", "filename": publisher.FILENAME,
            "sha256": hashlib.sha256(image).hexdigest(), "size": len(image),
        })

    def test_changed_zip_bytes_fail_admission(self):
        self.make_receipt()
        with (self.root / publisher.FILENAME).open("ab") as archive:
            archive.write(b"modified")
        with self.assertRaisesRegex(ValueError, "checksum"):
            publisher.describe(self.root, SOURCE)

    def test_another_self_signed_certificate_is_refused(self):
        self.make_receipt(certificate="A" * 40)
        with self.assertRaisesRegex(ValueError, "persistent macOS signing identity"):
            publisher.describe(self.root, SOURCE)

    def test_receipt_and_bundle_source_must_both_match(self):
        self.make_receipt(source="2" * 40)
        with self.assertRaisesRegex(ValueError, "different source"):
            publisher.describe(self.root, "2" * 40)
        with self.assertRaisesRegex(ValueError, "expected update-capable bundle"):
            publisher.describe(self.root, SOURCE)

    def test_legacy_bundle_without_helper_cannot_publish(self):
        self.make_receipt(helper=False)
        with self.assertRaisesRegex(ValueError, "expected update-capable bundle"):
            publisher.describe(self.root, SOURCE)

    def test_dotted_builds_order_by_both_numeric_components(self):
        self.assertGreater(publisher.build_order("10.10"), publisher.build_order("10.9"))
        self.assertGreater(publisher.build_order("11.0"), publisher.build_order("10.120"))
        self.make_receipt(build="10.01")
        with self.assertRaisesRegex(ValueError, "invalid build"):
            publisher.describe(self.root, SOURCE)


if __name__ == "__main__":
    unittest.main()
