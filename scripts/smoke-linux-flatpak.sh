#!/usr/bin/env bash
# Root-free static validation for the Flatpak manifest and its offline source lock.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="$ROOT/rust/packaging/flatpak/com.befeast.okplayer.json"
CARGO_SOURCES="$ROOT/rust/packaging/flatpak/cargo-sources.json"
APP_PATCH="$ROOT/rust/packaging/flatpak/ok-player-flatpak.patch"
BUILD_SCRIPT="$ROOT/scripts/build-flatpak-beta.sh"
SOFTWARE_RENDER_SCRIPT="$ROOT/scripts/smoke-linux-software-renderer.sh"
WORKFLOW="$ROOT/.github/workflows/flatpak.yml"
GITIGNORE="$ROOT/.gitignore"

for tool in bash python3 sed flatpak-builder desktop-file-validate appstreamcli; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "Missing required tool: $tool" >&2
    exit 127
  }
done

python3 - "$MANIFEST" "$CARGO_SOURCES" "$APP_PATCH" "$WORKFLOW" "$GITIGNORE" "$SOFTWARE_RENDER_SCRIPT" <<'PY'
import json
import re
import sys
from pathlib import Path

manifest_path = Path(sys.argv[1])
cargo_sources_path = Path(sys.argv[2])
app_patch_path = Path(sys.argv[3])
workflow_path = Path(sys.argv[4])
gitignore_path = Path(sys.argv[5])
software_render_script_path = Path(sys.argv[6])
manifest = json.loads(manifest_path.read_text())
cargo_sources = json.loads(cargo_sources_path.read_text())
workflow = workflow_path.read_text()
gitignore = gitignore_path.read_text().splitlines()
software_render_script = software_render_script_path.read_text()
app_patch = app_patch_path.read_text()

assert "ref: ${{ github.event.pull_request.head.sha || github.sha }}" in workflow
assert "OKP_ACCEPTANCE_SOURCE_COMMIT: ${{ github.event.pull_request.head.sha || github.sha }}" in workflow
assert "OKP_FLATPAK_ARTIFACT_MANIFEST: artifacts/linux/flatpak/flatpak-beta-artifact.json" in workflow
assert "Packaged no-DRI software renderer smoke" in workflow
assert "flatpak run --user --nodevice=dri" in workflow
assert 'xdg-user-dirs-update --set PICTURES "$HOME/Pictures"' in workflow
assert "artifacts/manual-ui/linux-software-renderer-smoke/**" in workflow
assert re.search(r"apt-get install -y [^\n]*\bripgrep\b", workflow)
assert "mapped_gtk_player_window=pass" in software_render_script
assert '[[ "$window_map_state" == "IsViewable" ]]' in software_render_script
assert "non_trivial_geometry=pass" in software_render_script
assert "visible_video_region=pass" in software_render_script
assert "backend=libmpv-software" in software_render_script
assert "gtk_scene_renderer=cairo" in software_render_script
assert "software_pixel_format=bgr0" in software_render_script
assert "dri_fd_count=" in software_render_script
assert "command -v magick" in software_render_script
assert "image_convert()" in software_render_script
assert "image_compare()" in software_render_script
assert "convert compare" in software_render_script
assert "flatpak ps --columns=child-pid,application" in software_render_script
assert 'child_process" == "ok-player"' in software_render_script
assert "xdg-user-dir PICTURES" in software_render_script
assert '"map_state": window_map_state' in software_render_script
assert '"screenshots": {' in software_render_script
assert '"source_commit": source_commit' in software_render_script
assert "screenshot_sha256=" in software_render_script
assert "later_screenshot_sha256=" in software_render_script
assert 'sys.argv[2]: "<repo>"' in software_render_script
assert 'sys.argv[3]: "<home>"' in software_render_script
assert "probe_backend=not-run" in software_render_script
assert "OKP_SOFTWARE_RENDER_PROBE" not in software_render_script
assert "Renderer policy: mode=software-no-dri" in software_render_script
assert "flatpak-software-renderer-validate" in software_render_script
assert '--source-commit "$SOURCE_COMMIT"' in software_render_script
assert "/.flatpak-builder/" in gitignore

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
assert "LIBGL_ALWAYS_SOFTWARE" not in json.dumps(manifest)

app = manifest["modules"][0]
assert "cargo --offline build --locked" in app["build-commands"][0]
assert app["build-options"]["env"]["CARGO_NET_OFFLINE"] == "true"
assert app["build-options"]["env"]["OKP_BUILD_VERSION"] == "0.11.0-beta.1"
assert app["build-options"]["env"]["OKP_BUILD_SHA"] == "flatpak-beta"
assert "cargo-sources.json" in app["sources"]
app_source = app["sources"][0]
assert app_source["type"] == "git"
assert app_source["url"] == "https://github.com/BeFeast/ok-player.git"
assert len(app_source.get("commit", "")) == 40
assert app["sources"][1] == {
    "type": "patch",
    "path": app_patch_path.name,
}
assert app_patch_path.is_file()
index_lines = [
    line for line in app_patch_path.read_text().splitlines() if line.startswith("index ")
]
assert index_lines
assert all(
    re.fullmatch(r"index [0-9a-f]{40}\.\.[0-9a-f]{40}(?: [0-7]{6})?", line)
    for line in index_lines
), "Flatpak patch must use full Git object IDs"
assert "new file mode 100644\nindex 0000000000000000000000000000000000000000" in app_patch_path.read_text()
assert "+++ b/rust/crates/okp-core/src/linux_renderer.rs" in app_patch_path.read_text()
assert "MPV_RENDER_PARAM_ADVANCED_CONTROL" in app_patch
assert "DecoderFailed" in app_patch
assert "diagnose_mpv_runtime" in app_patch
assert "mpv.stop()" in app_patch
assert "org.freedesktop.Platform.codecs-extra" in app_patch

def assert_portable_meson_libdir(module):
    if module.get("buildsystem") == "meson":
        assert "--libdir=lib" in module.get("config-opts", []), module["name"]
    for child in module.get("modules", []):
        assert_portable_meson_libdir(child)

for module in app.get("modules", []):
    assert_portable_meson_libdir(module)

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
collect(app["sources"][0])
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

app_commit="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["modules"][0]["sources"][0]["commit"])' "$MANIFEST")"
git -C "$ROOT" cat-file -e "${app_commit}^{tree}"

expected_patch="$(mktemp)"
trap 'rm -f "$expected_patch"' EXIT
git -C "$ROOT" diff --full-index --binary --no-ext-diff "$app_commit" -- \
  rust/crates/okp-core/src/lib.rs \
  rust/crates/okp-core/src/playback_failure.rs \
  rust/crates/okp-core/src/presentation_evidence.rs \
  rust/crates/okp-mpv/src/ffi.rs \
  rust/crates/okp-mpv/src/lib.rs \
  rust/crates/okp-mpv/src/player.rs \
  rust/crates/okp-mpv/src/pump.rs \
  rust/crates/okp-linux-gtk/build.rs \
  rust/crates/okp-linux-gtk/src/about.rs \
  rust/crates/okp-linux-gtk/src/main.rs \
  rust/crates/okp-linux-gtk/src/mpv_bridge.rs \
  rust/crates/okp-linux-gtk/src/playlist_ops.rs \
  rust/crates/okp-linux-gtk/src/screenshots.rs \
  rust/crates/okp-linux-gtk/src/tests.rs \
  rust/crates/okp-linux-gtk/src/track_popovers.rs \
  rust/crates/okp-linux-gtk/src/updates.rs \
  rust/crates/okp-linux-gtk/src/window.rs >"$expected_patch"
new_file_status=0
git -C "$ROOT" diff --no-index --full-index --binary \
  /dev/null rust/crates/okp-core/src/linux_renderer.rs >>"$expected_patch" || new_file_status=$?
[[ "$new_file_status" -eq 1 ]] || {
  echo "Failed to generate the Flatpak patch for linux_renderer.rs" >&2
  exit "$new_file_status"
}
sed -i 's/^ $//' "$expected_patch"
cmp "$expected_patch" "$APP_PATCH"
bash -n "$BUILD_SCRIPT"
bash -n "$SOFTWARE_RENDER_SCRIPT"

flatpak-builder --show-manifest "$MANIFEST" >/dev/null
desktop-file-validate "$ROOT/rust/packaging/linux/com.befeast.okplayer.desktop"
appstreamcli validate --pedantic --no-color \
  "$ROOT/rust/packaging/linux/com.befeast.okplayer.metainfo.xml"

echo "Flatpak manifest smoke passed: pinned native sources, offline Cargo lock, and sandbox permissions are valid"
