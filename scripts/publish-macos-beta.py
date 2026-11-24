#!/usr/bin/env python3
"""Publish a verified hosted macOS ZIP using OK Player's existing R2 authority."""
import argparse
import importlib.util
import os
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import tempfile
import zipfile

PREFIX = "ok-player/macos"
FILENAME = "ok-player-macos-arm64.zip"
BUNDLE_ID = "uk.oklabs.ok-player"


def appimage_publisher():
    source = Path(__file__).with_name("linux-appimage.py")
    spec = importlib.util.spec_from_file_location("ok_player_release", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def describe(directory, source):
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("Expected the exact Forgejo source SHA")
    if (directory / "source-sha.txt").read_text().strip() != source:
        raise ValueError("The hosted receipt names a different source")
    signing = (directory / "signing.txt").read_text().splitlines()
    identity = next((match[1] for line in signing
                     if (match := re.fullmatch(r"mode=self-signed identity=([A-Fa-f0-9]{40})", line))), None)
    expected_requirement = f'identifier "{BUNDLE_ID}" and certificate leaf = H"{identity.lower()}"' if identity else None
    if not identity or f"designated_requirement={expected_requirement}" not in signing:
        raise ValueError("The update ZIP requires OK Player's persistent macOS signing identity")
    image = (directory / FILENAME).read_bytes()
    with zipfile.ZipFile(directory / FILENAME) as bundle:
        info = plistlib.loads(bundle.read("OK Player.app/Contents/Info.plist"))
        if ("OK Player.app/Contents/MacOS/okp-updater" not in bundle.namelist()
                or info["OKPUpdateSigningMode"] != "self-signed"
                or info["CFBundleIdentifier"] != BUNDLE_ID or info["OKPSourceSHA"] != source):
            raise ValueError("The ZIP does not contain the expected update-capable bundle")
    build, version = info["CFBundleVersion"], info["CFBundleShortVersionString"]
    if (not re.fullmatch(r"[1-9][0-9]*\.(?:0|[1-9][0-9]*)", build)
            or not re.fullmatch(r"(?:0|[1-9][0-9]*)(?:\.(?:0|[1-9][0-9]*)){2}", version)):
        raise ValueError("The ZIP has an invalid build or version number")
    return image, {"source": source, "build": build, "version": version, "filename": FILENAME}


def publish(directory, source):
    common = appimage_publisher()
    image, metadata = describe(directory, source)
    metadata.update(sha256=common.digest(image), size=len(image))
    if not 1024 <= metadata["size"] <= 512 * 1024 * 1024:
        raise ValueError("The ZIP is outside the supported update size")
    if (directory / "SHA256SUMS.txt").read_text().split() != [metadata["sha256"], FILENAME]:
        raise ValueError("The ZIP differs from the hosted runner's checksum")

    provenance = common.encode(metadata)
    archive = f"{PREFIX}/builds/{metadata['build']}"
    key = f"{archive}/{FILENAME}"
    release = {**metadata, "schema": 1, "platform": "macos-arm64", "channel": "beta",
               "fingerprint": common.FINGERPRINT, "key": key, "provenance_sha256": common.digest(provenance)}

    with tempfile.TemporaryDirectory(prefix="okp-macos-beta-") as work:
        verification = Path(work) / "verify"
        signing = Path(work) / "signing"
        for home in (verification, signing):
            home.mkdir(mode=0o700)
        keyring = Path(__file__).resolve().parents[1] / "rust/packaging/linux/ok-player-archive-keyring.asc"
        verifier = common.Verifier(verification, keyring)
        try:
            subprocess.run(["gpg", "--homedir", signing, "--batch", "--import"], check=True,
                           input=os.environ["OKP_GPG_PRIVATE_KEY"].encode(), capture_output=True)
            store = common.Store()

            def signer(data):
                return common.sign(data, signing, verifier)

            common.publish_signed_archive(store, archive, metadata["filename"], image, provenance,
                                          common.encode(release), signer, verifier)
            common.publish_channel(store, release, signer, verifier, prefix=PREFIX,
                                   order=lambda build: tuple(map(int, build.split("."))))
        finally:
            if shutil.which("gpgconf"):
                subprocess.run(["gpgconf", "--homedir", signing, "--kill", "all"], check=False)

    print(f"{common.ORIGIN}/{key} SHA-256 {metadata['sha256']}")
    print(f"Version {metadata['version']} · Build {metadata['build']} · Source {source}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--source", required=True)
    args = parser.parse_args()
    try:
        publish(args.artifact, args.source)
    except (KeyError, OSError, ValueError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        raise SystemExit(f"macOS beta publication failed: {error}") from None
