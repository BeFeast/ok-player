#!/usr/bin/env python3
"""Publish and install signed OK Player AppImage releases."""

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.request

ORIGIN = "https://updates.befeast.com"
PREFIX = "ok-player/linux"
BUCKET = "befeast-updates"
FINGERPRINT = "77D0FCDEB0D594E13E50F43A9337815EB0F78C63"
CHANNELS = ("beta", "stable")


def encode(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def identity(source, build, image):
    if not re.fullmatch(r"[a-f0-9]{40}", source) or type(build) is not int or build < 1:
        raise ValueError("Expected a full Git SHA and positive build number")
    if len(image) < 1024 or image[:4] != b"\x7fELF" or image[8:11] != b"AI\x02" or image[18:20] != b"\x3e\x00":
        raise ValueError("Expected an x86_64 type-2 AppImage")
    version = f"0.11.0-beta.0.{build}"
    filename = f"OK-Player-{version}-x86_64.AppImage"
    return {"source": source, "build": build, "version": version, "filename": filename,
            "sha256": digest(image), "size": len(image)}


def archive(build):
    return f"{PREFIX}/builds/{build}"


def descriptor(provenance, channel):
    return {**provenance, "schema": 1, "platform": "linux-appimage", "channel": channel,
            "fingerprint": FINGERPRINT, "provenance_sha256": digest(encode(provenance)),
            "key": f"{archive(provenance['build'])}/{provenance['filename']}"}


class Verifier:
    def __init__(self, home, key):
        self.home = home
        self.keyring = home / "trusted.gpg"
        subprocess.run(["gpg", "--homedir", home, "--batch", "--dearmor", "--output",
                        self.keyring, key], check=True)

    def verify(self, data, signature):
        path = self.home / "payload"
        sig = self.home / "payload.sig"
        path.write_bytes(data)
        sig.write_bytes(signature)
        result = subprocess.run(["gpgv", "--homedir", self.home, "--keyring", self.keyring,
                                 "--status-fd", "1", sig, path], check=True, capture_output=True,
                                text=True)
        signatures = [line.split() for line in result.stdout.splitlines()
                      if line.startswith("[GNUPG:] VALIDSIG ")]
        if len(signatures) != 1 or signatures[0][-1] != FINGERPRINT:
            raise ValueError("AppImage signature uses an unexpected authority")


def read_signed(read, key, verifier):
    data, signature = read(key), read(f"{key}.sig")
    verifier.verify(data, signature)
    return json.loads(data)


def channel_key(channel, prefix=PREFIX):
    return f"{prefix}/{channel}/latest.json"


def channel_signature_key(channel, data, prefix=PREFIX):
    return f"{prefix}/{channel}/manifests/{digest(data)}.sig"


def read_channel(read, channel, verifier):
    data = read(channel_key(channel))
    verifier.verify(data, read(channel_signature_key(channel, data)))
    return json.loads(data)


def payload(read, release, channel, verifier):
    build = release["build"]
    if type(build) is not int or build < 1:
        raise ValueError("Invalid AppImage build number")
    prefix = archive(build)
    provenance = read(f"{prefix}/provenance.json", limit=65536)
    metadata = json.loads(provenance)
    if provenance != encode(metadata) or release != descriptor(metadata, channel):
        raise ValueError("AppImage descriptor and provenance disagree")
    filename = f"OK-Player-0.11.0-beta.0.{build}-x86_64.AppImage"
    image = read(f"{prefix}/{filename}", limit=release["size"])
    if metadata != identity(release["source"], build, image):
        raise ValueError("AppImage bytes and provenance disagree")
    verifier.verify(image, read(f"{prefix}/{filename}.sig"))
    return image


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def fetch(key, limit=65536):
    if type(limit) is not int or not 0 < limit <= 512 * 1024 * 1024:
        raise ValueError("Invalid AppImage download size")
    request = urllib.request.Request(f"{ORIGIN}/{key}", headers={
        "Cache-Control": "no-cache", "User-Agent": "OK Player AppImage Updater/1",
    })
    with urllib.request.build_opener(NoRedirect()).open(request, timeout=60) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError("AppImage download exceeds the expected size")
    return data


class Store:
    def __init__(self):
        import boto3

        endpoint = os.environ["R2_ENDPOINT"]
        if not re.fullmatch(r"https://[a-f0-9]{32}\.r2\.cloudflarestorage\.com/?", endpoint):
            raise ValueError("Expected an HTTPS Cloudflare R2 endpoint")
        self.client = boto3.client("s3", endpoint_url=endpoint.rstrip("/"), region_name="auto",
                                   aws_access_key_id=os.environ["R2_ACCESS_KEY_ID"],
                                   aws_secret_access_key=os.environ["R2_SECRET_ACCESS_KEY"])

    def read(self, key, limit=512 * 1024 * 1024, missing=False):
        from botocore.exceptions import ClientError

        try:
            response = self.client.get_object(Bucket=BUCKET, Key=key)
        except ClientError as error:
            if missing and error.response["Error"]["Code"] == "NoSuchKey":
                return None
            raise
        with response["Body"] as body:
            data = body.read(limit + 1)
        if len(data) > limit:
            raise ValueError(f"Published object exceeds its expected size: {key}")
        return data

    def put(self, key, data, immutable=False):
        condition = {"IfNoneMatch": "*"} if immutable else {}
        cache = "public, max-age=31536000, immutable" if immutable else "no-cache"
        content = "application/json" if key.endswith(".json") else "application/octet-stream"
        self.client.put_object(Bucket=BUCKET, Key=key, Body=data, ContentType=content,
                               CacheControl=cache, **condition)
        if self.read(key) != data:
            raise ValueError(f"Publication readback failed: {key}")


def sign(data, home, verifier):
    path = home / "signing-payload"
    sig = home / "signing-payload.sig"
    path.write_bytes(data)
    subprocess.run(["gpg", "--homedir", home, "--batch", "--yes", "--pinentry-mode", "loopback",
                    "--passphrase-fd", "0", "--local-user", FINGERPRINT, "--output", sig,
                    "--detach-sign", path], check=True,
                   input=(os.environ.get("OKP_GPG_PASSPHRASE", "") + "\n").encode())
    signature = sig.read_bytes()
    verifier.verify(data, signature)
    return signature


def publish_archive(store, metadata, image, signer, verifier):
    release = encode(descriptor(metadata, "beta"))
    publish_signed_archive(store, archive(metadata["build"]), metadata["filename"], image,
                           encode(metadata), release, signer, verifier)
    return json.loads(release)


def publish_signed_archive(store, prefix, filename, image, provenance, release, signer, verifier):
    files = {filename: image, "provenance.json": provenance, "release.json": release}
    existing = {name: store.read(f"{prefix}/{name}", missing=True) for name in files}
    for name, data in files.items():
        if existing[name] is not None and existing[name] != data:
            raise ValueError(f"Conflicting immutable release object: {name}")
    signatures = {}
    for name in (filename, "release.json"):
        key = f"{name}.sig"
        previous = store.read(f"{prefix}/{key}", missing=True)
        if previous is not None:
            verifier.verify(files[name], previous)
        else:
            signatures[key] = signer(files[name])
    for name, data in {**files, **signatures}.items():
        if existing.get(name) is None:
            store.put(f"{prefix}/{name}", data, immutable=True)


def publish_channel(store, release, signer, verifier, *, prefix=PREFIX, order=int):
    channel = release["channel"]
    key = channel_key(channel, prefix)
    data = encode(release)
    current = store.read(key, missing=True)
    if current is not None:
        active = json.loads(current)
        if order(active["build"]) > order(release["build"]):
            raise ValueError("Refusing to move a release channel backwards")
        if active["build"] == release["build"] and current != data:
            raise ValueError("Conflicting release channel descriptor for the same build")
    signature_key = channel_signature_key(channel, data, prefix)
    signature = store.read(signature_key, missing=True)
    if signature is None:
        signature = signer(data)
        store.put(signature_key, signature, immutable=True)
    else:
        verifier.verify(data, signature)
    # Clients use the immutable signature for the exact bytes they fetched.
    # The moving signature remains available for manual verification.
    store.put(f"{key}.sig", signature)
    store.put(key, data)


def installed_build(managed):
    if managed.is_symlink():
        relative = managed.readlink()
        old_build = relative.parent.name
        if (not re.fullmatch(r"[1-9][0-9]*", old_build) or
                relative != Path("builds", old_build, f"OK-Player-0.11.0-beta.0.{old_build}-x86_64.AppImage")):
            raise ValueError("Unrecognized managed AppImage link")
        if not managed.is_file():
            raise ValueError("Managed AppImage is missing from its build directory")
        return int(old_build)
    if managed.exists():
        raise ValueError("Move the unmanaged AppImage out of the installation path first")
    return 0


def install(data, metadata, root):
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    managed = root / "OK-Player.AppImage"
    with (root / ".appimage.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("Close OK Player before updating") from error
        current = installed_build(managed)
        if current >= metadata["build"]:
            print(f"Build {current} is already installed")
            return
        builds = root / "builds"
        builds.mkdir(mode=0o700, exist_ok=True)
        dest = builds / str(metadata["build"])
        image = dest / metadata["filename"]
        with tempfile.TemporaryDirectory(prefix=".appimage-", dir=root) as staging:
            stage = Path(staging)
            if dest.exists():
                if digest(image.read_bytes()) != metadata["sha256"]:
                    raise ValueError("Installed archive contains different bytes for this build")
            else:
                copy = stage / "build"
                copy.mkdir(mode=0o700)
                (copy / image.name).write_bytes(data)
                (copy / image.name).chmod(0o755)
                (copy / "release.json").write_bytes(encode(metadata))
                copy.rename(dest)
            link = stage / "current"
            link.symlink_to(image.relative_to(root))
            link.replace(managed)
    print(f"Installed build {metadata['build']} at {managed}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    installed_key = Path(__file__).with_name("ok-player-archive-keyring.asc")
    repository_key = Path(__file__).resolve().parents[1] / "rust/packaging/linux/ok-player-archive-keyring.asc"
    parser.add_argument("--keyring", type=Path, default=installed_key if installed_key.is_file() else repository_key)
    commands = parser.add_subparsers(dest="command", required=True)
    beta = commands.add_parser("publish-beta")
    beta.add_argument("--source", required=True)
    beta.add_argument("--build", type=int, required=True)
    beta.add_argument("--image", type=Path, required=True)
    beta.add_argument("--archive-only", action="store_true")
    stable = commands.add_parser("promote-stable")
    stable.add_argument("--build", type=int, required=True)
    update = commands.add_parser("update")
    update.add_argument("channel", choices=CHANNELS)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="okp-appimage-") as temporary:
        home = Path(temporary)
        verification = home / "verify"
        verification.mkdir(mode=0o700)
        verifier = Verifier(verification, args.keyring)
        if args.command == "update":
            release = read_channel(fetch, args.channel, verifier)
            root = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share") / "ok-player"
            current = installed_build(root / "OK-Player.AppImage")
            if current and current >= release["build"]:
                print(f"Build {current} is already installed")
            else:
                install(payload(fetch, release, args.channel, verifier), release, root)
            with tempfile.TemporaryDirectory(prefix=".channel-", dir=root) as work:
                preference = Path(work) / "update-channel"
                preference.write_text(args.channel + "\n")
                preference.replace(root / "update-channel")
            return

        signing = home / "signing"
        signing.mkdir(mode=0o700)
        subprocess.run(["gpg", "--homedir", signing, "--batch", "--import"], check=True,
                       input=os.environ["OKP_GPG_PRIVATE_KEY"].encode(), capture_output=True)
        store = Store()

        def signer(data):
            return sign(data, signing, verifier)

        try:
            if args.command == "publish-beta":
                image = args.image.read_bytes()
                metadata = identity(args.source, args.build, image)
                if args.image.name != metadata["filename"]:
                    raise ValueError("AppImage filename does not match the build identity")
                release = publish_archive(store, metadata, image, signer, verifier)
                if not args.archive_only:
                    publish_channel(store, release, signer, verifier)
            else:
                beta = read_signed(store.read, f"{archive(args.build)}/release.json", verifier)
                payload(store.read, beta, "beta", verifier)
                if beta["build"] != args.build:
                    raise ValueError("Archive points to a different AppImage build")
                release = {**beta, "channel": "stable"}
                publish_channel(store, release, signer, verifier)
            print(f"{ORIGIN}/{release['key']}  SHA-256 {release['sha256']}")
        finally:
            if shutil.which("gpgconf"):
                subprocess.run(["gpgconf", "--homedir", signing, "--kill", "all"], check=False)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"AppImage operation failed: {error}") from None
