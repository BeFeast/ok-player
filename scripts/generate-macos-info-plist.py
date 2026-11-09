#!/usr/bin/env python3
"""Stamp the macOS app's Info.plist with a version derived from git and its document types.

Usage: generate-macos-info-plist.py TEMPLATE OUTPUT --repo DIR --document-types PLIST [--build-number N]

CFBundleShortVersionString is MAJOR.MINOR.PATCH of the nearest `v<semver>` tag.
macOS reads three integers there, so a prerelease suffix is dropped: a build on the
way to 0.11.0 reports 0.11.0, the release it leads to, as the Linux and Windows
builds name theirs.

CFBundleVersion is COMMITS.BUILD: the number of commits reachable from HEAD, then the
CI run number (0 for a local build). Both only grow, so a newer build carries a higher
CFBundleVersion, which is what LaunchServices compares when two copies exist.

OKPSourceRevision keeps the full `git describe` output (tag, distance, abbreviated
commit), so a copy on disk names the commit it was built from.

A shallow clone has neither the tags nor the true commit count. It is refused rather
than stamped with a wrong version.

CFBundleDocumentTypes and UTImportedTypeDeclarations come from the property list that
okp-core's `okp-macos-document-types` generator writes from `media_formats`. They
replace whatever the template holds under those keys (a PlistBuddy merge would keep the
old arrays next to the new ones); the template itself carries neither.
"""
import argparse
import plistlib
import re
import subprocess
import sys

TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$")
DESCRIBE = re.compile(r"^(?P<tag>.+)-(?P<distance>\d+)-g(?P<commit>[0-9a-f]+)$")
BUILD_NUMBER = re.compile(r"^\d+$")
DOCUMENT_TYPE_KEYS = ("CFBundleDocumentTypes", "UTImportedTypeDeclarations")


class VersionError(Exception):
    pass


class DocumentTypesError(Exception):
    pass


def document_types(path):
    """Return the two document-type keys of the generated property list at PATH."""
    try:
        with open(path, "rb") as handle:
            fragment = plistlib.load(handle)
    except (OSError, plistlib.InvalidFileException) as error:
        raise DocumentTypesError(f"cannot read the document types {path}: {error}") from None
    if not isinstance(fragment, dict):
        raise DocumentTypesError(f"{path} is not a dictionary")
    unexpected = sorted(set(fragment) - set(DOCUMENT_TYPE_KEYS))
    if unexpected:
        raise DocumentTypesError(f"{path} carries keys other than the document types: {', '.join(unexpected)}")
    for key in DOCUMENT_TYPE_KEYS:
        if not isinstance(fragment.get(key), list) or not fragment[key]:
            raise DocumentTypesError(f"{path} has no {key}")
    return {key: fragment[key] for key in DOCUMENT_TYPE_KEYS}


def git(repo, *args):
    result = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    if result.returncode != 0:
        raise VersionError(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def bundle_version(repo, build_number):
    """Return the Info.plist keys that carry the version of the commit at HEAD."""
    if not BUILD_NUMBER.match(str(build_number)):
        raise VersionError(f"build number {build_number!r} is not a non-negative integer")
    if git(repo, "rev-parse", "--is-shallow-repository") == "true":
        raise VersionError("the clone is shallow; fetch the full history and tags to derive the version")
    try:
        describe = git(repo, "describe", "--tags", "--long", "--match", "v[0-9]*", "HEAD")
    except VersionError:
        raise VersionError("no v<major>.<minor>.<patch> tag is reachable from HEAD") from None
    parsed = DESCRIBE.match(describe)
    tag = TAG.match(parsed.group("tag")) if parsed else None
    if not tag:
        raise VersionError(f"nearest tag in {describe!r} is not v<major>.<minor>.<patch>[-prerelease]")
    commits = git(repo, "rev-list", "--count", "HEAD")
    return {
        "CFBundleShortVersionString": ".".join(str(int(part)) for part in tag.groups()),
        "CFBundleVersion": f"{commits}.{int(build_number)}",
        "OKPSourceRevision": describe,
    }


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    parser.add_argument("template")
    parser.add_argument("output")
    parser.add_argument("--repo", required=True, help="git work tree the app is built from")
    parser.add_argument("--document-types", required=True,
                        help="property list written by okp-core's okp-macos-document-types")
    parser.add_argument("--build-number", default="0", help="CI run number; 0 for a local build")
    args = parser.parse_args(argv[1:])
    try:
        with open(args.template, "rb") as handle:
            info = plistlib.load(handle)
        version = bundle_version(args.repo, args.build_number)
        types = document_types(args.document_types)
    except (OSError, plistlib.InvalidFileException, VersionError, DocumentTypesError) as error:
        print(f"generate-macos-info-plist: {error}", file=sys.stderr)
        return 1
    info.update(version)
    # Replace, never merge: the generated arrays are the whole of both keys.
    info.update(types)
    with open(args.output, "wb") as handle:
        plistlib.dump(info, handle, fmt=plistlib.FMT_XML)
    print(" ".join(f"{key}={value}" for key, value in version.items()))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
