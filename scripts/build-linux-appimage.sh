#!/usr/bin/env bash
set -euo pipefail

source_tree="$(realpath "${1:?usage: build-linux-appimage.sh <checkout> <output-dir>}")"
output_dir="$(realpath -m "${2:?usage: build-linux-appimage.sh <checkout> <output-dir>}")"
source_sha="$(git -C "$source_tree" rev-parse HEAD)"
if [[ "$(git -C "$source_tree" rev-parse --is-shallow-repository)" != false ]]; then
  echo 'AppImage build numbering requires complete first-parent history' >&2
  exit 1
fi
build="$(git -C "$source_tree" rev-list --first-parent --count "$source_sha")"
version="0.11.0-beta.0.$build"
image="OK-Player-$version-x86_64.AppImage"

"$source_tree/scripts/build-linux-portable-package.sh" appimage "$version"
install -Dm755 "$source_tree/artifacts/linux/velopack/$image" "$output_dir/$image"
printf '%s\n' "$source_sha" >"$output_dir/source.txt"
printf '%s\n' "$build" >"$output_dir/build.txt"
(cd "$output_dir" && sha256sum "$image" >SHA256SUMS)
