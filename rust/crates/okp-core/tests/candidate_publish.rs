#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use okp_test_fixtures::unique_temp_dir;

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const VERSION: &str = "0.11.0-beta.0.42";
const DEB: &str = "ok-player_0.11.0-beta.0.42_amd64.deb";
const APPIMAGE: &str = "OK-Player-0.11.0-beta.0.42-x86_64.AppImage";
const FULL: &str = "com.befeast.okplayer-0.11.0-beta.0.42-linux-candidate-full.nupkg";
const SUMS: &str = "SHA256SUMS-42.txt";

#[test]
fn every_upload_failure_preserves_the_feed_and_assets() {
    for failed_asset in [DEB, APPIMAGE, FULL, SUMS, "candidate.linux.json"] {
        let root = unique_temp_dir("okp-candidate-publish-failure");
        let fixture = PublishFixture::new(root.path());
        fs::write(
            fixture.assets.join("candidate.linux.json"),
            b"old pointer\n",
        )
        .expect("old pointer should be written");
        fs::write(fixture.assets.join("unrelated.txt"), b"keep me\n")
            .expect("unrelated asset should be written");
        let before = fixture.asset_snapshot();

        let failed = fixture.run(Some(failed_asset), None);
        assert!(
            !failed.status.success(),
            "injected failure for {failed_asset} must fail"
        );
        assert_eq!(fixture.asset_snapshot(), before);
        assert!(!fixture.state.join("last-promoted.sha").exists());
    }
}

#[test]
fn existing_pointer_download_failure_stops_before_mutation() {
    let root = unique_temp_dir("okp-candidate-publish-download-failure");
    let fixture = PublishFixture::new(root.path());
    fs::write(
        fixture.assets.join("candidate.linux.json"),
        b"old pointer\n",
    )
    .expect("old pointer should be written");
    let before = fixture.asset_snapshot();

    let failed = fixture.run(None, Some("candidate.linux.json"));
    assert!(!failed.status.success(), "pointer download must fail");
    assert_eq!(fixture.asset_snapshot(), before);
    assert!(!fixture.state.join("last-promoted.sha").exists());
}

#[test]
fn successful_retry_reuses_the_exact_verified_bundle() {
    let root = unique_temp_dir("okp-candidate-publish-retry");
    let fixture = PublishFixture::new(root.path());
    fs::write(
        fixture.assets.join("candidate.linux.json"),
        b"old pointer\n",
    )
    .expect("old pointer should be written");

    let published = fixture.run(None, None);
    assert!(
        published.status.success(),
        "publish should succeed: {}",
        String::from_utf8_lossy(&published.stderr)
    );
    assert_eq!(
        fixture.asset_names(),
        [APPIMAGE, SUMS, "candidate.linux.json", FULL, DEB]
    );
    assert_eq!(
        fs::read_to_string(fixture.state.join("last-promoted.sha"))
            .expect("promoted marker should be written")
            .trim(),
        SOURCE_SHA
    );

    // The fake GitHub rejects replacing an existing versioned asset. This
    // retry can pass only by downloading and byte-comparing all four assets.
    let retry = fixture.run(None, None);
    assert!(
        retry.status.success(),
        "exact-bundle retry should reuse assets: {}",
        String::from_utf8_lossy(&retry.stderr)
    );

    fs::write(fixture.assets.join(DEB), b"tampered remote bytes")
        .expect("remote fixture should be tampered");
    let assets_before = fixture.asset_snapshot();
    let mismatch = fixture.run(None, None);
    assert!(
        !mismatch.status.success(),
        "mismatched remote asset must fail"
    );
    assert_eq!(fixture.asset_snapshot(), assets_before);
}

struct PublishFixture {
    repo_root: PathBuf,
    state: PathBuf,
    assets: PathBuf,
    fake_bin: PathBuf,
    bundle: PathBuf,
    feed: PathBuf,
    cli: PathBuf,
}

impl PublishFixture {
    fn new(root: &Path) -> Self {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .expect("repository root should resolve");
        let state = root.join("state");
        let assets = root.join("remote-assets");
        let fake_bin = root.join("bin");
        let bundle = root.join("bundle");
        let feed = root.join("feed.json");
        let cli = fake_bin.join("okp-candidate");
        for directory in [
            &state,
            &assets,
            &fake_bin,
            &bundle.join("artifacts/deb"),
            &bundle.join("artifacts/velopack"),
        ] {
            fs::create_dir_all(directory).expect("fixture directory should be created");
        }

        fs::write(bundle.join("artifacts/deb").join(DEB), b"verified deb")
            .expect("deb fixture should be written");
        fs::write(
            bundle.join("artifacts/velopack").join(APPIMAGE),
            b"verified appimage",
        )
        .expect("AppImage fixture should be written");
        fs::write(
            bundle.join("artifacts/velopack").join(FULL),
            b"verified full package",
        )
        .expect("Full package fixture should be written");
        fs::write(
            bundle.join("artifacts/SHA256SUMS"),
            format!(
                "{}  {DEB}\n{}  {APPIMAGE}\n",
                "a".repeat(64),
                "b".repeat(64)
            ),
        )
        .expect("checksum fixture should be written");
        fs::write(
            bundle.join("candidate-build.json"),
            format!(
                r#"{{
  "version": "{VERSION}",
  "build_number": 42,
  "source_sha": "{SOURCE_SHA}",
  "package": {{
    "artifacts": [
      {{"kind": "debian", "file_name": "{DEB}"}},
      {{"kind": "app-image", "file_name": "{APPIMAGE}"}}
    ]
  }}
}}
"#,
            ),
        )
        .expect("build record fixture should be written");
        fs::write(
            &feed,
            format!(
                r#"{{
  "version": "{VERSION}",
  "build": 42,
  "source_sha": "{SOURCE_SHA}",
  "appimage": {{"name": "{FULL}"}}
}}
"#,
            ),
        )
        .expect("feed fixture should be written");

        write_executable(&cli, FAKE_CANDIDATE_CLI);
        write_executable(&fake_bin.join("gh"), FAKE_GH);

        Self {
            repo_root,
            state,
            assets,
            fake_bin,
            bundle,
            feed,
            cli,
        }
    }

    fn run(&self, fail_upload: Option<&str>, fail_download: Option<&str>) -> Output {
        let mut command = Command::new("bash");
        command
            .arg(self.repo_root.join("scripts/publish-linux-candidate.sh"))
            .arg(&self.bundle)
            .arg("BeFeast/ok-player")
            .arg("accepted")
            .env("OKP_CANDIDATE_LOCK_HELD", "1")
            .env("OKP_CANDIDATE_STATE_DIR", &self.state)
            .env("OKP_CANDIDATE_CLI", &self.cli)
            .env("FAKE_CANDIDATE_FEED", &self.feed)
            .env("FAKE_GH_ASSETS", &self.assets)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            );
        if let Some(name) = fail_upload {
            command.env("FAKE_GH_FAIL_UPLOAD", name);
        }
        if let Some(name) = fail_download {
            command.env("FAKE_GH_FAIL_DOWNLOAD", name);
        }
        command.output().expect("publisher fixture should run")
    }

    fn asset_names(&self) -> Vec<String> {
        let mut names = fs::read_dir(&self.assets)
            .expect("assets should be readable")
            .map(|entry| {
                entry
                    .expect("asset entry should be readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn asset_snapshot(&self) -> Vec<(String, Vec<u8>)> {
        self.asset_names()
            .into_iter()
            .map(|name| {
                let bytes = fs::read(self.assets.join(&name)).expect("asset should be readable");
                (name, bytes)
            })
            .collect()
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("fake executable should be written");
    let mut permissions = fs::metadata(path)
        .expect("fake executable metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("fake executable should be executable");
}

const FAKE_CANDIDATE_CLI: &str = r#"#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  verify-bundle)
    ;;
  feed)
    output=""
    while [[ "$#" -gt 0 ]]; do
      if [[ "$1" == "--output" ]]; then output="$2"; shift 2; else shift; fi
    done
    cp "$FAKE_CANDIDATE_FEED" "$output"
    ;;
  prune-plan)
    ;;
  *)
    echo "unexpected fake candidate command: ${1:-}" >&2
    exit 2
    ;;
esac
"#;

const FAKE_GH: &str = r#"#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "release" ]] || exit 2
action="$2"
shift 2
case "$action" in
  view)
    if [[ " $* " == *" --json assets "* ]]; then
      for path in "$FAKE_GH_ASSETS"/*; do
        [[ -e "$path" ]] && basename -- "$path"
      done | jq -Rsc 'split("\n") | map(select(length > 0))'
    fi
    ;;
  create)
    mkdir -p "$FAKE_GH_ASSETS"
    ;;
  download)
    pattern=""
    directory=""
    while [[ "$#" -gt 0 ]]; do
      case "$1" in
        --pattern) pattern="$2"; shift 2 ;;
        --dir) directory="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    if [[ "${FAKE_GH_FAIL_DOWNLOAD:-}" == "$pattern" ]]; then exit 43; fi
    [[ -f "$FAKE_GH_ASSETS/$pattern" ]] || exit 1
    cp "$FAKE_GH_ASSETS/$pattern" "$directory/$pattern"
    ;;
  upload)
    source=""
    for argument in "$@"; do
      [[ -f "$argument" ]] && source="$argument"
    done
    [[ -n "$source" ]] || exit 2
    name="$(basename -- "$source")"
    clobber=false
    [[ " $* " == *" --clobber "* ]] && clobber=true
    if [[ "${FAKE_GH_FAIL_UPLOAD:-}" == "$name" ]]; then exit 42; fi
    if [[ -e "$FAKE_GH_ASSETS/$name" && "$clobber" != "true" ]]; then exit 3; fi
    cp "$source" "$FAKE_GH_ASSETS/$name"
    ;;
  delete-asset)
    rm -f -- "$FAKE_GH_ASSETS/$2"
    ;;
  *)
    exit 2
    ;;
esac
"#;
