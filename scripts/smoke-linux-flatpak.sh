#!/usr/bin/env bash
# Root-free static validation for the Flatpak manifest and its offline source lock.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
CARGO_SOURCES="$ROOT/rust/packaging/flatpak/cargo-sources.json"

for tool in python3 flatpak-builder desktop-file-validate appstreamcli; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

python3 - "$MANIFEST" "$CARGO_SOURCES" <<'PY'
import json
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
cargo_sources_path = Path(sys.argv[2])
manifest = json.loads(manifest_path.read_text())
cargo_sources = json.loads(cargo_sources_path.read_text())

assert manifest["app-id"] == "com.befeast.okplayer"
assert manifest["runtime"] == "org.gnome.Platform"
assert manifest["runtime-version"] == "50"
assert "org.freedesktop.Sdk.Extension.rust-stable" in manifest["sdk-extensions"]

extensions = manifest["add-extensions"]
codecs = extensions["org.freedesktop.Platform.codecs-extra"]
assert codecs["version"] == "25.08-extra"
assert codecs["directory"] == "lib/codecs-extra"
assert codecs["add-ld-path"] == "."

permissions = set(manifest["finish-args"])
required = {
    "--socket=fallback-x11",
    "--socket=wayland",
    "--socket=pulseaudio",
    "--device=dri",
    "--filesystem=xdg-pictures:rw",
    "--own-name=org.mpris.MediaPlayer2.okplayer",
}
assert required <= permissions
assert "--device=all" not in permissions
assert "--filesystem=host" not in permissions
assert "--filesystem=home" not in permissions

app = manifest["modules"][0]
assert "cargo --offline build --locked" in app["build-commands"][0]
assert app["build-options"]["env"]["CARGO_NET_OFFLINE"] == "true"
assert "cargo-sources.json" in app["sources"]

native_sources = []
def collect(value):
    if isinstance(value, dict):
        if value.get("type") in {"archive", "git"} and value.get("url"):
            native_sources.append(value)
        for child in value.values():
            collect(child)
    elif isinstance(value, list):
        for child in value:
            collect(child)
collect(app.get("modules", []))
for source in native_sources:
    if source["type"] == "archive":
        assert source.get("sha256"), source
    else:
        assert source.get("commit"), source

archives = [source for source in cargo_sources if source.get("type") == "archive"]
assert archives, "Cargo source lock contains no crates"
assert all(source.get("sha256") for source in archives)
assert cargo_sources[-1].get("dest-filename") == "config"
assert "replace-with = \"vendored-sources\"" in cargo_sources[-1].get("contents", "")
PY

flatpak-builder --show-manifest "$MANIFEST" >/dev/null
desktop-file-validate "$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
appstreamcli validate --pedantic --no-color \
  "$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

echo "Flatpak manifest smoke passed: pinned native sources, offline Cargo lock, and sandbox permissions are valid"
