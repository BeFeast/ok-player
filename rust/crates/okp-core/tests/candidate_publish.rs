#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use okp_core::candidate_channel::{CandidateFeed, select_candidate_update_from_feed};
use okp_core::sha256sums::{Sha256Sums, sha256_hex};
use okp_test_fixtures::unique_temp_dir;

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const STALE_REQUEST_SHA: &str = "89abcdef0123456789abcdef0123456789abcdef";
const PREVIOUS_SHA: &str = "fedcba9876543210fedcba9876543210fedcba98";
const VERSION: &str = "0.11.0-beta.0.42";
const DEB: &str = "ok-player_0.11.0-beta.0.42_amd64.deb";
const APPIMAGE: &str = "OK-Player-0.11.0-beta.0.42-x86_64.AppImage";
const FULL: &str = "com.befeast.okplayer-0.11.0-beta.0.42-linux-candidate-full.nupkg";
const SUMS: &str = "SHA256SUMS-42.txt";
const STALE_VERSION: &str = "0.11.0-beta.0.41";
const STALE_DEB: &str = "ok-player_0.11.0-beta.0.41_amd64.deb";
const STALE_APPIMAGE: &str = "OK-Player-0.11.0-beta.0.41-x86_64.AppImage";
const STALE_FULL: &str = "com.befeast.okplayer-0.11.0-beta.0.41-linux-candidate-full.nupkg";
const TEST_TAG: &str = "linux-candidate-overlap-fixture";

#[test]
fn every_upload_failure_preserves_the_feed_and_assets() {
    for failed_asset in [DEB, APPIMAGE, FULL, SUMS, "candidate.linux.json"] {
        let root = unique_temp_dir("okp-candidate-publish-failure");
        let fixture = PublishFixture::new(root.path());
        fixture.write_previous_pointer();
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
    fixture.write_previous_pointer();
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
    fixture.write_previous_pointer();

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

#[test]
fn coalesced_build_is_stale_when_it_no_longer_matches_the_requested_sha() {
    let root = unique_temp_dir("okp-candidate-coalesced-generation");
    let fixture = PublishFixture::new(root.path());
    fixture.write_previous_pointer();
    fixture.set_current_sha(SOURCE_SHA);
    let before = fixture.asset_snapshot();

    let stale = fixture.run_generation_with_evidence(
        &fixture.bundle,
        &fixture.feed,
        STALE_REQUEST_SHA,
        42,
        &fixture.stale_decision,
        "coalesced-run-a",
    );
    assert!(
        stale.status.success(),
        "coalesced run must be a successful no-op: {}",
        String::from_utf8_lossy(&stale.stderr)
    );
    assert_eq!(fixture.asset_snapshot(), before);
    assert!(fixture.mutation_log().is_empty());
    assert!(!fixture.state.join("last-promoted.sha").exists());

    let evidence: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&fixture.stale_decision)
            .expect("coalesced decision evidence should be written"),
    )
    .expect("coalesced decision evidence should be JSON");
    assert_eq!(evidence["outcome"], "stale_generation");
    assert_eq!(evidence["requested_sha"], STALE_REQUEST_SHA);
    assert_eq!(evidence["build_sha"], SOURCE_SHA);
    assert_eq!(evidence["current_sha"], SOURCE_SHA);
    assert_eq!(
        evidence["stale_reasons"],
        serde_json::json!(["requested_head_changed", "build_does_not_match_request"])
    );
}

#[test]
fn overlapping_stale_run_cannot_mutate_the_generation_published_while_it_waits() {
    let root = unique_temp_dir("okp-candidate-stale-generation");
    let fixture = PublishFixture::new(root.path());
    fixture.write_previous_pointer();
    fs::write(fixture.assets.join("unrelated.txt"), b"keep me\n")
        .expect("unrelated asset should be written");
    fixture.set_current_sha(STALE_REQUEST_SHA);

    let mut stale_run = fixture.spawn_held_stale_generation();
    wait_for_file(&fixture.pause_ready, &mut stale_run);
    let admission: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&fixture.admission).expect("run A admission evidence should exist"),
    )
    .expect("run A admission evidence should be JSON");
    assert_eq!(admission["requested_sha"], STALE_REQUEST_SHA);
    assert_eq!(admission["build_sha"], STALE_REQUEST_SHA);
    assert_eq!(admission["build_number"], 41);
    assert!(
        fixture.mutation_log().is_empty(),
        "run A must pause before any release mutation"
    );

    fixture.set_current_sha(SOURCE_SHA);
    let published = fixture.run_generation_with_evidence(
        &fixture.bundle,
        &fixture.feed,
        SOURCE_SHA,
        42,
        &fixture.current_decision,
        "run-b",
    );
    assert!(
        published.status.success(),
        "run B should publish while run A is held: {}",
        String::from_utf8_lossy(&published.stderr)
    );

    let current_evidence: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&fixture.current_decision)
            .expect("run B decision evidence should be written"),
    )
    .expect("run B decision evidence should be JSON");
    assert_eq!(current_evidence["outcome"], "eligible");
    assert_eq!(current_evidence["requested_sha"], SOURCE_SHA);
    assert_eq!(current_evidence["build_sha"], SOURCE_SHA);
    assert_eq!(current_evidence["current_sha"], SOURCE_SHA);
    assert_eq!(current_evidence["build_number"], 42);
    assert_eq!(current_evidence["allocated_build"], 42);

    let pointer: CandidateFeed = serde_json::from_str(
        &fs::read_to_string(fixture.assets.join("candidate.linux.json"))
            .expect("published pointer should exist"),
    )
    .expect("published pointer should be JSON");
    assert_eq!(pointer.build, 42);
    assert_eq!(pointer.commit_sha, SOURCE_SHA);
    assert_eq!(pointer.package.name, DEB);
    assert_eq!(pointer.appimage.name, FULL);

    let deb_bytes = fs::read(fixture.assets.join(DEB)).expect("published deb should exist");
    let appimage_bytes =
        fs::read(fixture.assets.join(APPIMAGE)).expect("published AppImage should exist");
    let full_bytes =
        fs::read(fixture.assets.join(FULL)).expect("published full package should exist");
    assert_eq!(
        deb_bytes,
        fs::read(fixture.bundle.join("artifacts/deb").join(DEB))
            .expect("verified run B deb should exist")
    );
    assert_eq!(
        appimage_bytes,
        fs::read(fixture.bundle.join("artifacts/velopack").join(APPIMAGE))
            .expect("verified run B AppImage should exist")
    );
    assert_eq!(
        full_bytes,
        fs::read(fixture.bundle.join("artifacts/velopack").join(FULL))
            .expect("verified run B full package should exist")
    );
    assert_eq!(pointer.package.sha256, sha256_hex(&deb_bytes));
    assert_eq!(pointer.appimage.sha256, sha256_hex(&full_bytes));

    let sums_bytes = fs::read(fixture.assets.join(SUMS)).expect("published sums should exist");
    let sums = Sha256Sums::parse(
        std::str::from_utf8(&sums_bytes).expect("published sums should be UTF-8"),
    )
    .expect("published sums should parse");
    assert_eq!(
        sums.expected_hex(DEB),
        Some(sha256_hex(&deb_bytes).as_str())
    );
    assert_eq!(
        sums.expected_hex(APPIMAGE),
        Some(sha256_hex(&appimage_bytes).as_str())
    );

    let selected = select_candidate_update_from_feed(pointer.clone(), "0.11.0-beta.0.40")
        .expect("an enrolled updater should select run B");
    assert_eq!(selected.build, 42);
    assert_eq!(selected.commit_sha, SOURCE_SHA);
    assert_eq!(selected.package.name, DEB);
    assert_eq!(selected.appimage.name, FULL);
    assert!(
        fixture
            .asset_names()
            .iter()
            .all(|name| ![STALE_DEB, STALE_APPIMAGE, STALE_FULL].contains(&name.as_str())),
        "the rolling surface must expose no run A artifacts"
    );

    let mutations_after_b = fixture.mutation_log();
    assert_eq!(mutations_after_b.len(), 5);
    assert!(
        mutations_after_b
            .iter()
            .all(|line| line.starts_with("run-b\t")),
        "only run B may mutate the isolated release surface: {mutations_after_b:?}"
    );
    assert_eq!(
        mutations_after_b
            .iter()
            .filter(|line| line.contains("\tupload\t") && line.contains("candidate.linux.json"))
            .count(),
        1,
        "run B must replace the rolling pointer exactly once"
    );

    let assets_after_b = fixture.asset_snapshot();
    let hashes_after_b = fixture.asset_hashes();
    let promoted_after_b = fs::read(fixture.state.join("last-promoted.sha"))
        .expect("run B promoted marker should be written");
    assert_eq!(promoted_after_b, format!("{SOURCE_SHA}\n").as_bytes());
    let current_decision_after_b =
        fs::read(&fixture.current_decision).expect("run B decision should remain readable");

    fs::write(&fixture.pause_resume, b"resume\n").expect("run A should be resumed");
    let stale = stale_run
        .wait_with_output()
        .expect("run A publisher process should finish");
    assert!(
        stale.status.success(),
        "run A must exit successfully as stale_generation: {}",
        String::from_utf8_lossy(&stale.stderr)
    );
    assert!(
        String::from_utf8_lossy(&stale.stdout).contains("stale_generation"),
        "run A should report stale_generation"
    );

    let stale_evidence: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&fixture.stale_decision)
            .expect("run A decision evidence should be written"),
    )
    .expect("run A decision evidence should be JSON");
    assert_eq!(stale_evidence["outcome"], "stale_generation");
    assert_eq!(stale_evidence["requested_sha"], STALE_REQUEST_SHA);
    assert_eq!(stale_evidence["build_sha"], STALE_REQUEST_SHA);
    assert_eq!(stale_evidence["current_sha"], SOURCE_SHA);
    assert_eq!(stale_evidence["build_number"], 41);
    assert_eq!(stale_evidence["allocated_build"], 42);
    assert_eq!(stale_evidence["published_build"], 42);
    assert_eq!(stale_evidence["published_sha"], SOURCE_SHA);
    assert_eq!(
        stale_evidence["stale_reasons"],
        serde_json::json!([
            "requested_head_changed",
            "newer_generation_allocated",
            "newer_generation_published"
        ])
    );

    assert_eq!(fixture.asset_snapshot(), assets_after_b);
    assert_eq!(fixture.asset_hashes(), hashes_after_b);
    assert_eq!(fixture.mutation_log(), mutations_after_b);
    assert_eq!(
        fs::read(fixture.state.join("last-promoted.sha"))
            .expect("promoted marker should remain readable"),
        promoted_after_b
    );
    assert_eq!(
        fs::read(&fixture.current_decision).expect("run B decision should remain readable"),
        current_decision_after_b
    );
}

struct PublishFixture {
    repo_root: PathBuf,
    state: PathBuf,
    assets: PathBuf,
    fake_bin: PathBuf,
    bundle: PathBuf,
    feed: PathBuf,
    stale_bundle: PathBuf,
    stale_feed: PathBuf,
    cli: PathBuf,
    current_sha_file: PathBuf,
    mutations: PathBuf,
    admission: PathBuf,
    pause_ready: PathBuf,
    pause_resume: PathBuf,
    current_decision: PathBuf,
    stale_decision: PathBuf,
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
        let stale_bundle = root.join("stale-bundle");
        let stale_feed = root.join("stale-feed.json");
        let cli = fake_bin.join("okp-candidate");
        let current_sha_file = root.join("current.sha");
        let mutations = root.join("mutations.log");
        let admission = root.join("run-a-admission.json");
        let pause_ready = root.join("run-a-paused");
        let pause_resume = root.join("run-a-resume");
        let current_decision = root.join("run-b-decision.json");
        let stale_decision = root.join("run-a-decision.json");
        for directory in [&state, &assets, &fake_bin] {
            fs::create_dir_all(directory).expect("fixture directory should be created");
        }

        write_generation(
            &bundle,
            &feed,
            GenerationSpec {
                version: VERSION,
                build: 42,
                source_sha: SOURCE_SHA,
                deb: DEB,
                appimage: APPIMAGE,
                full: FULL,
            },
        );
        write_generation(
            &stale_bundle,
            &stale_feed,
            GenerationSpec {
                version: STALE_VERSION,
                build: 41,
                source_sha: STALE_REQUEST_SHA,
                deb: STALE_DEB,
                appimage: STALE_APPIMAGE,
                full: STALE_FULL,
            },
        );
        fs::write(state.join("build-number"), b"42\n")
            .expect("allocated generation should be written");
        fs::write(&current_sha_file, format!("{SOURCE_SHA}\n"))
            .expect("current SHA fixture should be written");

        write_executable(&cli, FAKE_CANDIDATE_CLI);
        write_executable(&fake_bin.join("gh"), FAKE_GH);

        Self {
            repo_root,
            state,
            assets,
            fake_bin,
            bundle,
            feed,
            stale_bundle,
            stale_feed,
            cli,
            current_sha_file,
            mutations,
            admission,
            pause_ready,
            pause_resume,
            current_decision,
            stale_decision,
        }
    }

    fn run(&self, fail_upload: Option<&str>, fail_download: Option<&str>) -> Output {
        self.set_current_sha(SOURCE_SHA);
        self.write_allocated_build(42);
        let mut command = self.publisher_command(
            &self.bundle,
            &self.feed,
            SOURCE_SHA,
            &self.state.join("last-publish-decision.json"),
            "fixture-run",
        );
        if let Some(name) = fail_upload {
            command.env("FAKE_GH_FAIL_UPLOAD", name);
        }
        if let Some(name) = fail_download {
            command.env("FAKE_GH_FAIL_DOWNLOAD", name);
        }
        command.output().expect("publisher fixture should run")
    }

    fn run_generation_with_evidence(
        &self,
        bundle: &Path,
        feed: &Path,
        requested_sha: &str,
        allocated_build: u64,
        decision: &Path,
        run_id: &str,
    ) -> Output {
        self.write_allocated_build(allocated_build);
        self.publisher_command(bundle, feed, requested_sha, decision, run_id)
            .output()
            .expect("publisher fixture should run")
    }

    fn spawn_held_stale_generation(&self) -> Child {
        self.write_allocated_build(41);
        let mut command = self.publisher_command(
            &self.stale_bundle,
            &self.stale_feed,
            STALE_REQUEST_SHA,
            &self.stale_decision,
            "run-a",
        );
        command
            .env("FAKE_CANDIDATE_ADMISSION", &self.admission)
            .env("FAKE_CANDIDATE_PAUSE_READY", &self.pause_ready)
            .env("FAKE_CANDIDATE_PAUSE_RESUME", &self.pause_resume)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("run A publisher fixture should start")
    }

    fn publisher_command(
        &self,
        bundle: &Path,
        feed: &Path,
        requested_sha: &str,
        decision: &Path,
        run_id: &str,
    ) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(self.repo_root.join("scripts/publish-linux-candidate.sh"))
            .arg(bundle)
            .arg("BeFeast/ok-player")
            .arg("accepted")
            .env("OKP_CANDIDATE_LOCK_HELD", "1")
            .env("OKP_CANDIDATE_STATE_DIR", &self.state)
            .env("OKP_CANDIDATE_CLI", &self.cli)
            .env("OKP_CANDIDATE_REQUESTED_SHA", requested_sha)
            .env("OKP_CANDIDATE_PUBLISH_DECISION", decision)
            .env("OKP_CANDIDATE_TAG", TEST_TAG)
            .env("FAKE_CANDIDATE_FEED", feed)
            .env("FAKE_GH_ASSETS", &self.assets)
            .env("FAKE_GH_CURRENT_SHA_FILE", &self.current_sha_file)
            .env("FAKE_GH_MUTATIONS", &self.mutations)
            .env("FAKE_GH_RUN_ID", run_id)
            .env("REAL_CANDIDATE_CLI", env!("CARGO_BIN_EXE_okp-candidate"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").expect("PATH should be set")
                ),
            );
        command
    }

    fn set_current_sha(&self, sha: &str) {
        fs::write(&self.current_sha_file, format!("{sha}\n"))
            .expect("current SHA fixture should be updated");
    }

    fn write_allocated_build(&self, build: u64) {
        fs::write(self.state.join("build-number"), format!("{build}\n"))
            .expect("allocated generation should be updated");
    }

    fn write_previous_pointer(&self) {
        fs::write(
            self.assets.join("candidate.linux.json"),
            format!(
                r#"{{
  "channel": "candidate",
  "version": "0.11.0-beta.0.40",
  "build": 40,
  "commit_sha": "{PREVIOUS_SHA}",
  "timestamp_utc": "2026-07-17T23:00:00Z",
  "acceptance": "accepted",
  "package": {{
    "name": "ok-player_0.11.0-beta.0.40_amd64.deb",
    "url": "https://example.invalid/old.deb",
    "size": 10,
    "sha256": "{}"
  }},
  "appimage": {{
    "package_id": "com.befeast.okplayer",
    "name": "com.befeast.okplayer-0.11.0-beta.0.40-linux-candidate-full.nupkg",
    "url": "https://example.invalid/old.nupkg",
    "size": 20,
    "sha256": "{}",
    "sha1": "{}"
  }},
  "sha256sums_url": "https://example.invalid/SHA256SUMS-40.txt",
  "history": []
}}
"#,
                "d".repeat(64),
                "e".repeat(64),
                "f".repeat(40),
            ),
        )
        .expect("previous pointer should be written");
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

    fn asset_hashes(&self) -> Vec<(String, String)> {
        self.asset_snapshot()
            .into_iter()
            .map(|(name, bytes)| (name, sha256_hex(&bytes)))
            .collect()
    }

    fn mutation_log(&self) -> Vec<String> {
        fs::read_to_string(&self.mutations)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

struct GenerationSpec<'a> {
    version: &'a str,
    build: u64,
    source_sha: &'a str,
    deb: &'a str,
    appimage: &'a str,
    full: &'a str,
}

fn write_generation(bundle: &Path, feed: &Path, generation: GenerationSpec<'_>) {
    fs::create_dir_all(bundle.join("artifacts/deb"))
        .expect("deb fixture directory should be created");
    fs::create_dir_all(bundle.join("artifacts/velopack"))
        .expect("Velopack fixture directory should be created");

    let deb_bytes = format!(
        "verified deb for {} build {}\n",
        generation.source_sha, generation.build
    )
    .into_bytes();
    let appimage_bytes = format!(
        "verified AppImage for {} build {}\n",
        generation.source_sha, generation.build
    )
    .into_bytes();
    let full_bytes = format!(
        "verified full package for {} build {}\n",
        generation.source_sha, generation.build
    )
    .into_bytes();
    let deb_sha256 = sha256_hex(&deb_bytes);
    let appimage_sha256 = sha256_hex(&appimage_bytes);
    let full_sha256 = sha256_hex(&full_bytes);

    fs::write(
        bundle.join("artifacts/deb").join(generation.deb),
        &deb_bytes,
    )
    .expect("deb fixture should be written");
    fs::write(
        bundle.join("artifacts/velopack").join(generation.appimage),
        &appimage_bytes,
    )
    .expect("AppImage fixture should be written");
    fs::write(
        bundle.join("artifacts/velopack").join(generation.full),
        &full_bytes,
    )
    .expect("Full package fixture should be written");
    fs::write(
        bundle.join("artifacts/SHA256SUMS"),
        format!(
            "{deb_sha256}  {}\n{appimage_sha256}  {}\n",
            generation.deb, generation.appimage
        ),
    )
    .expect("checksum fixture should be written");
    fs::write(
        bundle.join("candidate-build.json"),
        format!(
            r#"{{
  "version": "{}",
  "build_number": {},
  "source_sha": "{}",
  "package": {{
    "artifacts": [
      {{"kind": "debian", "file_name": "{}"}},
      {{"kind": "app-image", "file_name": "{}"}}
    ]
  }}
}}
"#,
            generation.version,
            generation.build,
            generation.source_sha,
            generation.deb,
            generation.appimage,
        ),
    )
    .expect("build record fixture should be written");
    fs::write(
        feed,
        format!(
            r#"{{
  "channel": "candidate",
  "version": "{}",
  "build": {},
  "commit_sha": "{}",
  "timestamp_utc": "2026-07-18T00:00:00Z",
  "acceptance": "accepted",
  "package": {{
    "name": "{}",
    "url": "https://example.invalid/{}",
    "size": {},
    "sha256": "{deb_sha256}"
  }},
  "appimage": {{
    "package_id": "com.befeast.okplayer",
    "name": "{}",
    "url": "https://example.invalid/{}",
    "size": {},
    "sha256": "{full_sha256}",
    "sha1": "{}"
  }},
  "sha256sums_url": "https://example.invalid/SHA256SUMS-{}.txt",
  "history": []
}}
"#,
            generation.version,
            generation.build,
            generation.source_sha,
            generation.deb,
            generation.deb,
            deb_bytes.len(),
            generation.full,
            generation.full,
            full_bytes.len(),
            "c".repeat(40),
            generation.build,
        ),
    )
    .expect("feed fixture should be written");
}

fn wait_for_file(path: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if let Some(status) = child
            .try_wait()
            .expect("held publisher status should be readable")
        {
            panic!("held publisher exited before reaching the barrier: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "held publisher did not reach the barrier in time"
        );
        thread::sleep(Duration::from_millis(10));
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
    if [[ -n "${FAKE_CANDIDATE_PAUSE_READY:-}" ]]; then
      bundle=""
      while [[ "$#" -gt 0 ]]; do
        if [[ "$1" == "--bundle" ]]; then bundle="$2"; shift 2; else shift; fi
      done
      [[ -n "$bundle" ]] || exit 2
      admission_tmp="${FAKE_CANDIDATE_ADMISSION}.tmp"
      jq -n \
        --arg requested_sha "$OKP_CANDIDATE_REQUESTED_SHA" \
        --arg build_sha "$(jq -r '.source_sha' "$bundle/candidate-build.json")" \
        --argjson build_number "$(jq -r '.build_number' "$bundle/candidate-build.json")" \
        '{requested_sha: $requested_sha, build_sha: $build_sha, build_number: $build_number}' \
        >"$admission_tmp"
      mv -f -- "$admission_tmp" "$FAKE_CANDIDATE_ADMISSION"
      : >"$FAKE_CANDIDATE_PAUSE_READY"
      for _ in {1..1000}; do
        [[ -e "$FAKE_CANDIDATE_PAUSE_RESUME" ]] && break
        sleep 0.01
      done
      [[ -e "$FAKE_CANDIDATE_PAUSE_RESUME" ]] || {
        echo "timed out waiting to resume held candidate publisher" >&2
        exit 124
      }
    fi
    ;;
  feed)
    output=""
    while [[ "$#" -gt 0 ]]; do
      if [[ "$1" == "--output" ]]; then output="$2"; shift 2; else shift; fi
    done
    cp "$FAKE_CANDIDATE_FEED" "$output"
    ;;
  publish-decision)
    exec "$REAL_CANDIDATE_CLI" "$@"
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
if [[ "${1:-}" == "api" ]]; then
  cat "$FAKE_GH_CURRENT_SHA_FILE"
  exit 0
fi
[[ "${1:-}" == "release" ]] || exit 2
action="$2"
shift 2
tag="${1:-}"
record_mutation() {
  local operation="$1" asset="$2" digest="$3"
  printf '%s\t%s\t%s\t%s\t%s\n' \
    "$FAKE_GH_RUN_ID" "$operation" "$tag" "$asset" "$digest" \
    >>"$FAKE_GH_MUTATIONS"
}
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
    record_mutation create - -
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
    record_mutation upload "$name" "$(sha256sum "$source" | awk '{print $1}')"
    ;;
  delete-asset)
    name="$2"
    rm -f -- "$FAKE_GH_ASSETS/$name"
    record_mutation delete-asset "$name" -
    ;;
  *)
    exit 2
    ;;
esac
"#;
