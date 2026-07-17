//! Coalesced Linux candidate-builder contract and decision logic (issue #340).
//!
//! A self-hosted x86_64 Ubuntu builder polls `origin/main` and, when it has
//! advanced past the last successfully built SHA, produces an installable
//! candidate bundle for the candidate channel (promotion itself is owned by
//! #339). This module owns the *decidable* parts of that pipeline so no
//! build/promotion state machine lives in a shell script:
//!
//! * [`BuildDecision::resolve`] — build the current `origin/main` HEAD or skip
//!   an unchanged SHA. Because the decision always targets HEAD, every merge
//!   landed since the last build is coalesced into one candidate.
//! * [`CandidateBuild`] — the stable artifact contract emitted as
//!   `candidate-build.json`, plus [`CandidateBuild::promotable`] which decides
//!   whether a build may move the feed. Build and promotion are separated: a
//!   build that fails any required gate is never promotable, so the last
//!   promoted SHA and feed stay untouched.
//! * [`Heartbeat`] / [`classify_activity`] — let an external watchdog tell an
//!   active build from a stalled build from an idle unchanged `main`.
//!
//! The module is pure and timestamp-agnostic: callers supply RFC3339 strings
//! and monotonic ages so the logic stays deterministic and fully unit-tested.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::acceptance_evidence::{ArtifactKind, PackageIdentity};
use crate::candidate_channel::{
    AcceptanceStatus, CandidateAppImage, CandidateFeed, CandidateHistoryEntry, CandidatePackage,
};
use crate::sha256sums::{Sha256Sums, sha256_hex};
use crate::update_selection::compare_versions;
use crate::velopack_artifacts::LINUX_VELOPACK_PACKAGE_ID;

/// Schema version of the `candidate-build.json` artifact contract.
pub const CANDIDATE_SCHEMA_VERSION: u32 = 1;

/// Update channel a candidate build targets. Deliberately distinct from the
/// public `linux` release channel so a candidate can never be mistaken for a
/// published release.
pub const CANDIDATE_CHANNEL: &str = "linux-candidate";

/// Gates that must all pass before a build is promotable. Order matches the
/// pipeline the builder runs (fast, cheap gates first).
pub const REQUIRED_CANDIDATE_GATES: &[&str] = &[
    "fmt",
    "clippy",
    "workspace-tests",
    "deb-package",
    "appimage-package",
    "package-identity",
    "install-upgrade-uninstall-smoke",
    "headless-launch-smoke",
];

/// Optional gate name. Its evidence is only enforced when the operator marks a
/// build as requiring native-hardware acceptance.
pub const NATIVE_HARDWARE_GATE: &str = "native-hardware-smoke";

/// Default idle window before a `Building` heartbeat is treated as stalled.
pub const DEFAULT_STALL_AFTER_SECONDS: u64 = 900;

/// Maximum previous accepted candidates retained on the rolling surface.
pub const MAX_RETAINED_PREVIOUS: usize = 5;

/// Build the issue #339 SemVer identity for the current public-beta phase.
pub fn candidate_version(version_base: &str, build_number: u64) -> Result<String, String> {
    let base = version_base.trim().trim_end_matches('.');
    if base.is_empty() {
        return Err("candidate version base is empty".to_owned());
    }
    if build_number == 0 {
        return Err("candidate build number must be greater than zero".to_owned());
    }
    Ok(format!("{base}.{build_number}"))
}

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeed {
    #[serde(rename = "Assets")]
    assets: Vec<VelopackFeedAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct VelopackFeedAsset {
    #[serde(rename = "PackageId")]
    package_id: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FileName")]
    file_name: String,
    #[serde(rename = "SHA1", default)]
    sha1: String,
    #[serde(rename = "SHA256")]
    sha256: String,
    #[serde(rename = "Size")]
    size: u64,
}

/// Exact bundle material needed to assemble the rolling feed after all hashes
/// and channel metadata have been re-verified from disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCandidateBundle {
    pub record: CandidateBuild,
    pub deb_path: PathBuf,
    pub appimage_path: PathBuf,
    pub velopack_path: PathBuf,
    pub sums_path: PathBuf,
    pub velopack: CandidateAppImage,
}

/// Recompute every candidate artifact identity from the staged native-builder
/// bundle. Promotion must call this immediately before moving the feed pointer;
/// trusting only the JSON record would allow post-build byte replacement.
pub fn verify_candidate_bundle(bundle: &Path) -> Result<VerifiedCandidateBundle, Vec<String>> {
    let mut errors = Vec::new();
    let record_path = bundle.join("candidate-build.json");
    let record: CandidateBuild = match read_json(&record_path) {
        Ok(record) => record,
        Err(error) => return Err(vec![error]),
    };
    if let Err(mut record_errors) = record.promotable() {
        errors.append(&mut record_errors);
    }

    let identity_path = bundle.join("artifacts/package-identity.json");
    match read_json::<PackageIdentity>(&identity_path) {
        Ok(identity) if identity != record.package => {
            errors.push("package-identity.json does not match candidate-build.json".to_owned())
        }
        Ok(_) => {}
        Err(error) => errors.push(error),
    }

    let deb_identity = record
        .package
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Debian);
    let appimage_identity = record
        .package
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::AppImage);
    let deb_path = identity_path_for(bundle, "deb", deb_identity, &mut errors);
    let appimage_path = identity_path_for(bundle, "velopack", appimage_identity, &mut errors);

    let sums_path = bundle.join("artifacts/SHA256SUMS");
    let sums = match fs::read_to_string(&sums_path) {
        Ok(text) => match Sha256Sums::parse(&text) {
            Ok(sums) => Some(sums),
            Err(error) => {
                errors.push(format!("{}: {error}", sums_path.display()));
                None
            }
        },
        Err(error) => {
            errors.push(format!("{}: {error}", sums_path.display()));
            None
        }
    };
    if let Some(sums) = &sums {
        for identity in [deb_identity, appimage_identity].into_iter().flatten() {
            match sums.expected_hex(&identity.file_name) {
                Some(digest) if digest.eq_ignore_ascii_case(&identity.sha256) => {}
                Some(digest) => errors.push(format!(
                    "SHA256SUMS digest for {} is {}, expected {}",
                    identity.file_name, digest, identity.sha256
                )),
                None => errors.push(format!(
                    "SHA256SUMS has no entry for {}",
                    identity.file_name
                )),
            }
        }
    }

    let velopack_feed_path = bundle
        .join("artifacts/velopack")
        .join(format!("releases.{CANDIDATE_CHANNEL}.json"));
    let velopack_feed: Option<VelopackFeed> = match read_json(&velopack_feed_path) {
        Ok(feed) => Some(feed),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    let mut velopack_path = PathBuf::new();
    let mut velopack = None;
    if let Some(feed) = velopack_feed {
        let matches = feed
            .assets
            .into_iter()
            .filter(|asset| {
                asset.package_id == LINUX_VELOPACK_PACKAGE_ID
                    && asset.version == record.version
                    && asset.kind.eq_ignore_ascii_case("full")
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            errors.push(format!(
                "{} must contain exactly one Full asset for package {} version {}",
                velopack_feed_path.display(),
                LINUX_VELOPACK_PACKAGE_ID,
                record.version
            ));
        } else if let Some(asset) = matches.into_iter().next() {
            velopack_path = bundle.join("artifacts/velopack").join(&asset.file_name);
            match hash_file(&velopack_path) {
                Ok((digest, size)) => {
                    if !digest.eq_ignore_ascii_case(&asset.sha256) {
                        errors.push(format!(
                            "{} SHA256 is {}, feed declares {}",
                            asset.file_name, digest, asset.sha256
                        ));
                    }
                    if size != asset.size {
                        errors.push(format!(
                            "{} size is {}, feed declares {}",
                            asset.file_name, size, asset.size
                        ));
                    }
                    velopack = Some(CandidateAppImage {
                        package_id: asset.package_id,
                        name: asset.file_name,
                        url: String::new(),
                        size,
                        sha256: digest,
                        sha1: asset.sha1,
                    });
                }
                Err(error) => errors.push(error),
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(VerifiedCandidateBundle {
        record,
        deb_path: deb_path.expect("validated Debian identity has a path"),
        appimage_path: appimage_path.expect("validated AppImage identity has a path"),
        velopack_path,
        sums_path,
        velopack: velopack.expect("validated Velopack feed has a full package"),
    })
}

/// Assemble the single candidate pointer from a verified native bundle.
pub fn assemble_candidate_feed(
    verified: &VerifiedCandidateBundle,
    base_url: &str,
    acceptance: AcceptanceStatus,
    previous: Option<&CandidateFeed>,
) -> Result<CandidateFeed, String> {
    let record = &verified.record;
    if let Some(previous) = previous {
        if record.build_number == previous.build
            && record.version == previous.version
            && record.source_sha == previous.commit_sha
        {
            let mut idempotent = previous.clone();
            idempotent.acceptance = acceptance;
            return Ok(idempotent);
        }
        if record.build_number <= previous.build {
            return Err(format!(
                "candidate build {} must be newer than published build {}",
                record.build_number, previous.build
            ));
        }
        if compare_versions(&record.version, &previous.version) != std::cmp::Ordering::Greater {
            return Err(format!(
                "candidate version {} must sort after published version {}",
                record.version, previous.version
            ));
        }
    }
    let base_url = base_url.trim_end_matches('/');
    let deb_identity = record
        .package
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == ArtifactKind::Debian)
        .ok_or_else(|| "candidate record has no Debian artifact".to_owned())?;
    let mut appimage = verified.velopack.clone();
    appimage.url = format!("{base_url}/{}", appimage.name);
    let package = CandidatePackage {
        name: deb_identity.file_name.clone(),
        url: format!("{base_url}/{}", deb_identity.file_name),
        size: Some(
            fs::metadata(&verified.deb_path)
                .map_err(|error| format!("{}: {error}", verified.deb_path.display()))?
                .len(),
        ),
        sha256: deb_identity.sha256.clone(),
    };
    let sums_url = format!("{base_url}/SHA256SUMS-{}.txt", record.build_number);
    let mut history = Vec::new();
    if let Some(previous) = previous {
        if previous.acceptance == AcceptanceStatus::Accepted {
            history.push(CandidateHistoryEntry {
                version: previous.version.clone(),
                build: previous.build,
                package: previous.package.clone(),
                appimage: previous.appimage.clone(),
                sha256sums_url: previous.sha256sums_url.clone().unwrap_or_default(),
            });
        }
        history.extend(previous.history.iter().cloned());
    }
    history.retain(|entry| entry.build != record.build_number && entry.version != record.version);
    history.truncate(MAX_RETAINED_PREVIOUS);

    Ok(CandidateFeed {
        channel: crate::candidate_channel::CANDIDATE_CHANNEL.to_owned(),
        version: record.version.clone(),
        build: record.build_number,
        commit_sha: record.source_sha.clone(),
        timestamp_utc: record.finished_at.clone(),
        acceptance,
        package,
        appimage,
        sha256sums_url: Some(sums_url),
        history,
    })
}

/// Candidate assets safe to prune after the new pointer is published.
pub fn candidate_prune_plan(feed: &CandidateFeed, release_assets: &[String]) -> Vec<String> {
    let mut keep = vec![
        "candidate.linux.json".to_owned(),
        feed.package.name.clone(),
        feed.appimage.name.clone(),
        format!("OK-Player-{}-x86_64.AppImage", feed.version),
        format!("SHA256SUMS-{}.txt", feed.build),
    ];
    for entry in &feed.history {
        keep.push(entry.package.name.clone());
        keep.push(entry.appimage.name.clone());
        keep.push(format!("OK-Player-{}-x86_64.AppImage", entry.version));
        keep.push(format!("SHA256SUMS-{}.txt", entry.build));
    }
    release_assets
        .iter()
        .filter(|asset| {
            is_candidate_package_asset(asset) && !keep.iter().any(|item| item == *asset)
        })
        .cloned()
        .collect()
}

fn is_candidate_package_asset(name: &str) -> bool {
    (name.starts_with("ok-player_") && name.ends_with("_amd64.deb"))
        || (name.starts_with("OK-Player-") && name.ends_with("-x86_64.AppImage"))
        || (name.starts_with("com.befeast.okplayer-")
            && (name.ends_with("-linux-candidate-full.nupkg")
                || name.ends_with("-linux-full.nupkg")))
        || (name.starts_with("SHA256SUMS-") && name.ends_with(".txt"))
}

fn identity_path_for(
    bundle: &Path,
    lane: &str,
    identity: Option<&crate::acceptance_evidence::PackageArtifact>,
    errors: &mut Vec<String>,
) -> Option<PathBuf> {
    let identity = identity?;
    let path = bundle
        .join("artifacts")
        .join(lane)
        .join(&identity.file_name);
    match hash_file(&path) {
        Ok((digest, _)) if digest.eq_ignore_ascii_case(&identity.sha256) => Some(path),
        Ok((digest, _)) => {
            errors.push(format!(
                "{} SHA256 is {}, record declares {}",
                path.display(),
                digest,
                identity.sha256
            ));
            None
        }
        Err(error) => {
            errors.push(error);
            None
        }
    }
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok((sha256_hex(&bytes), bytes.len() as u64))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Normalize a candidate SHA: trim, lowercase, require a full 40-char hex SHA.
fn normalize_sha(sha: &str) -> Option<String> {
    let trimmed = sha.trim().to_ascii_lowercase();
    if trimmed.len() == 40 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(trimmed)
    } else {
        None
    }
}

/// Whether the builder should build the current `origin/main` HEAD or skip it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "kebab-case")]
pub enum BuildDecision {
    /// HEAD advanced past the last built SHA (or nothing was ever built).
    Build { head_sha: String },
    /// HEAD equals the last successfully built SHA; nothing to do.
    SkipUnchanged { sha: String },
}

impl BuildDecision {
    /// Resolve the decision from the current HEAD and the last successfully
    /// built SHA. A missing or blank marker means "never built" and always
    /// builds. Both SHAs must be full 40-char hex.
    pub fn resolve(head_sha: &str, last_built_sha: Option<&str>) -> Result<Self, String> {
        let head =
            normalize_sha(head_sha).ok_or_else(|| format!("invalid head SHA: {head_sha:?}"))?;
        // A missing or blank marker means "never built"; only a valid, equal
        // marker skips.
        if let Some(last) = last_built_sha
            .map(str::trim)
            .filter(|last| !last.is_empty())
        {
            let last =
                normalize_sha(last).ok_or_else(|| format!("invalid last-built SHA: {last:?}"))?;
            if last == head {
                return Ok(Self::SkipUnchanged { sha: head });
            }
        }
        Ok(Self::Build { head_sha: head })
    }

    /// True when a build should run.
    #[must_use]
    pub fn should_build(&self) -> bool {
        matches!(self, Self::Build { .. })
    }

    /// The resolved SHA, whichever variant this is.
    #[must_use]
    pub fn sha(&self) -> &str {
        match self {
            Self::Build { head_sha } => head_sha,
            Self::SkipUnchanged { sha } => sha,
        }
    }
}

/// Outcome of a single bounded gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateStatus {
    Passed,
    Failed,
    Skipped,
}

/// One gate result recorded into the artifact contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub name: String,
    pub status: GateStatus,
    #[serde(default)]
    pub detail: String,
}

/// The phase a heartbeat reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildPhase {
    /// `main` has not advanced; the builder is idle by design.
    Idle,
    /// A build is running.
    Building,
}

/// What a watchdog concludes from the newest heartbeat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildActivity {
    Idle,
    Building,
    Stalled,
}

/// Classify builder activity from the newest heartbeat's phase and age.
///
/// An idle heartbeat is always idle (an unchanged `main` is expected, not a
/// fault). A building heartbeat older than `stall_after_seconds` is stalled;
/// otherwise the build is progressing.
#[must_use]
pub fn classify_activity(
    phase: BuildPhase,
    seconds_since_heartbeat: u64,
    stall_after_seconds: u64,
) -> BuildActivity {
    match phase {
        BuildPhase::Idle => BuildActivity::Idle,
        BuildPhase::Building if seconds_since_heartbeat > stall_after_seconds => {
            BuildActivity::Stalled
        }
        BuildPhase::Building => BuildActivity::Building,
    }
}

/// A single heartbeat line the builder appends and a watchdog tails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub phase: BuildPhase,
    pub unix_seconds: u64,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub source_sha: String,
}

impl Heartbeat {
    /// Classify this heartbeat as of `now_unix_seconds`.
    #[must_use]
    pub fn classify(&self, now_unix_seconds: u64, stall_after_seconds: u64) -> BuildActivity {
        let age = now_unix_seconds.saturating_sub(self.unix_seconds);
        classify_activity(self.phase, age, stall_after_seconds)
    }
}

/// The stable artifact contract emitted as `candidate-build.json`. It records
/// exactly what was built, from which source, and whether every gate passed —
/// enough for the candidate channel (#339) to promote without rebuilding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateBuild {
    pub schema_version: u32,
    pub channel: String,
    pub source_sha: String,
    pub build_number: u64,
    pub version: String,
    pub started_at: String,
    pub finished_at: String,
    /// When set, the optional native-hardware smoke must have passed too.
    #[serde(default)]
    pub require_native_hardware: bool,
    pub gates: Vec<GateResult>,
    pub package: PackageIdentity,
}

impl CandidateBuild {
    /// Assemble a build record. `source_sha` must match the package identity's
    /// commit SHA (they describe the same clean checkout).
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        source_sha: String,
        build_number: u64,
        version: String,
        started_at: String,
        finished_at: String,
        require_native_hardware: bool,
        gates: Vec<GateResult>,
        package: PackageIdentity,
    ) -> Self {
        Self {
            schema_version: CANDIDATE_SCHEMA_VERSION,
            channel: CANDIDATE_CHANNEL.to_owned(),
            source_sha,
            build_number,
            version,
            started_at,
            finished_at,
            require_native_hardware,
            gates,
            package,
        }
    }

    fn gate(&self, name: &str) -> Option<&GateResult> {
        self.gates.iter().find(|gate| gate.name == name)
    }

    /// Decide whether this build may be promoted to the candidate feed.
    /// Returns every reason it is not, so a builder can log all of them at
    /// once. A build that is not promotable must never move the feed.
    pub fn promotable(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.schema_version != CANDIDATE_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported candidate schema {}, expected {}",
                self.schema_version, CANDIDATE_SCHEMA_VERSION
            ));
        }
        if self.channel != CANDIDATE_CHANNEL {
            errors.push(format!(
                "candidate channel must be {CANDIDATE_CHANNEL}, got {:?}",
                self.channel
            ));
        }
        if normalize_sha(&self.source_sha).is_none() {
            errors.push("source_sha must be a full 40-character hex SHA".to_owned());
        }
        if self.source_sha != self.package.commit_sha {
            errors.push("source_sha does not match the package commit_sha".to_owned());
        }
        if self.build_number == 0 {
            errors.push("build_number must be greater than zero".to_owned());
        }
        if self.started_at.trim().is_empty() {
            errors.push("started_at is empty".to_owned());
        }
        if self.finished_at.trim().is_empty() {
            errors.push("finished_at is empty".to_owned());
        }

        // The version must remain traceable to the build number so an
        // installed candidate can be tied back to this exact build.
        if self.version.trim().is_empty() {
            errors.push("version is empty".to_owned());
        } else if !version_carries_build_number(&self.version, self.build_number) {
            errors.push(format!(
                "version {:?} does not carry build number {}",
                self.version, self.build_number
            ));
        }
        if self.version != self.package.version {
            errors.push("version does not match the package version".to_owned());
        }

        validate_candidate_identity(&self.package, &self.version, &mut errors);

        for name in REQUIRED_CANDIDATE_GATES {
            match self.gate(name) {
                None => errors.push(format!("missing required gate: {name}")),
                Some(gate) if gate.status != GateStatus::Passed => {
                    errors.push(format!(
                        "required gate {name} did not pass: {:?}",
                        gate.status
                    ));
                }
                Some(_) => {}
            }
        }

        if self.require_native_hardware {
            match self.gate(NATIVE_HARDWARE_GATE) {
                None => errors.push(format!(
                    "native-hardware evidence required but gate {NATIVE_HARDWARE_GATE} is absent"
                )),
                Some(gate) if gate.status != GateStatus::Passed => errors.push(format!(
                    "required gate {NATIVE_HARDWARE_GATE} did not pass: {:?}",
                    gate.status
                )),
                Some(_) => {}
            }
        }

        // A failed gate anywhere in the record blocks promotion even if it is
        // not in the required set.
        for gate in &self.gates {
            if gate.status == GateStatus::Failed {
                errors.push(format!("gate {} failed", gate.name));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// True when the build number appears as a standalone numeric token in the
/// version string (e.g. `0.1.0-linux-candidate.42`).
fn version_carries_build_number(version: &str, build_number: u64) -> bool {
    let wanted = build_number.to_string();
    version
        .split(|character: char| !character.is_ascii_digit())
        .any(|token| token == wanted)
}

fn validate_candidate_identity(package: &PackageIdentity, version: &str, errors: &mut Vec<String>) {
    for kind in [ArtifactKind::Debian, ArtifactKind::AppImage] {
        let matching = package
            .artifacts
            .iter()
            .filter(|artifact| artifact.kind == kind)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            errors.push(format!(
                "package must contain exactly one {kind:?} artifact"
            ));
        }
    }
    for artifact in &package.artifacts {
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            errors.push(format!(
                "{}: artifact sha256 must be 64 hex characters",
                artifact.file_name
            ));
        }
        if !version.trim().is_empty() && !artifact.file_name.contains(version) {
            errors.push(format!(
                "artifact {} does not carry version {version}",
                artifact.file_name
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acceptance_evidence::PackageArtifact;
    use crate::candidate_channel::select_candidate_update_from_feed;
    use okp_test_fixtures::unique_temp_dir;

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
    const OTHER: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn build_when_nothing_was_ever_built() {
        assert_eq!(
            BuildDecision::resolve(HEAD, None).unwrap(),
            BuildDecision::Build {
                head_sha: HEAD.to_owned()
            }
        );
        assert_eq!(
            BuildDecision::resolve(HEAD, Some("")).unwrap(),
            BuildDecision::Build {
                head_sha: HEAD.to_owned()
            }
        );
    }

    #[test]
    fn skip_the_same_sha_and_build_a_new_one() {
        assert_eq!(
            BuildDecision::resolve(HEAD, Some(HEAD)).unwrap(),
            BuildDecision::SkipUnchanged {
                sha: HEAD.to_owned()
            }
        );
        assert!(
            BuildDecision::resolve(HEAD, Some(OTHER))
                .unwrap()
                .should_build()
        );
    }

    #[test]
    fn decision_normalizes_case_and_whitespace() {
        let decision =
            BuildDecision::resolve(&format!("  {}  ", HEAD.to_uppercase()), Some(HEAD)).unwrap();
        assert_eq!(
            decision,
            BuildDecision::SkipUnchanged {
                sha: HEAD.to_owned()
            }
        );
    }

    #[test]
    fn malformed_shas_are_rejected() {
        assert!(BuildDecision::resolve("not-a-sha", None).is_err());
        assert!(BuildDecision::resolve(HEAD, Some("deadbeef")).is_err());
    }

    fn package(version: &str) -> PackageIdentity {
        PackageIdentity {
            version: version.to_owned(),
            commit_sha: HEAD.to_owned(),
            artifacts: vec![
                PackageArtifact {
                    kind: ArtifactKind::Debian,
                    file_name: format!("ok-player_{version}_amd64.deb"),
                    sha256: "a".repeat(64),
                },
                PackageArtifact {
                    kind: ArtifactKind::AppImage,
                    file_name: format!("OK-Player-{version}-x86_64.AppImage"),
                    sha256: "b".repeat(64),
                },
            ],
        }
    }

    fn passing_gates() -> Vec<GateResult> {
        REQUIRED_CANDIDATE_GATES
            .iter()
            .map(|name| GateResult {
                name: (*name).to_owned(),
                status: GateStatus::Passed,
                detail: String::new(),
            })
            .collect()
    }

    fn candidate(build_number: u64) -> CandidateBuild {
        let version = format!("0.1.0-linux-candidate.{build_number}");
        CandidateBuild::new(
            HEAD.to_owned(),
            build_number,
            version.clone(),
            "2026-07-17T10:00:00Z".to_owned(),
            "2026-07-17T11:00:00Z".to_owned(),
            false,
            passing_gates(),
            package(&version),
        )
    }

    #[test]
    fn a_fully_passing_build_is_promotable() {
        assert_eq!(candidate(42).promotable(), Ok(()));
    }

    #[test]
    fn candidate_versions_follow_the_public_beta_identity_ladder() {
        assert_eq!(
            candidate_version("0.11.0-beta.0", 108).unwrap(),
            "0.11.0-beta.0.108"
        );
        assert_eq!(
            candidate_version("0.11.0-beta.1", 3).unwrap(),
            "0.11.0-beta.1.3"
        );
        assert!(candidate_version("0.11.0-beta.1", 0).is_err());
    }

    #[test]
    fn native_bundle_to_enrolled_selection_preserves_exact_identity_and_public_feed() {
        let root = unique_temp_dir("okp-candidate-contract");
        let bundle = root.join("bundle");
        let deb_dir = bundle.join("artifacts/deb");
        let velopack_dir = bundle.join("artifacts/velopack");
        fs::create_dir_all(&deb_dir).unwrap();
        fs::create_dir_all(&velopack_dir).unwrap();
        let public_feed = root.join("public-feed.json");
        fs::write(&public_feed, b"public-feed-byte-for-byte\n").unwrap();
        let public_before = fs::read(&public_feed).unwrap();

        let build_number = 42;
        let version = candidate_version("0.11.0-beta.1", build_number).unwrap();
        let deb_name = format!("ok-player_{version}_amd64.deb");
        let appimage_name = format!("OK-Player-{version}-x86_64.AppImage");
        let nupkg_name = format!("com.befeast.okplayer-{version}-linux-candidate-full.nupkg");
        let deb_bytes = b"native deb bytes";
        let appimage_bytes = b"native appimage bytes";
        let nupkg_bytes = b"native velopack bytes";
        fs::write(deb_dir.join(&deb_name), deb_bytes).unwrap();
        fs::write(velopack_dir.join(&appimage_name), appimage_bytes).unwrap();
        fs::write(velopack_dir.join(&nupkg_name), nupkg_bytes).unwrap();

        let identity = PackageIdentity {
            version: version.clone(),
            commit_sha: HEAD.to_owned(),
            artifacts: vec![
                PackageArtifact {
                    kind: ArtifactKind::Debian,
                    file_name: deb_name.clone(),
                    sha256: sha256_hex(deb_bytes),
                },
                PackageArtifact {
                    kind: ArtifactKind::AppImage,
                    file_name: appimage_name.clone(),
                    sha256: sha256_hex(appimage_bytes),
                },
            ],
        };
        let record = CandidateBuild::new(
            HEAD.to_owned(),
            build_number,
            version.clone(),
            "2026-07-17T10:00:00Z".to_owned(),
            "2026-07-17T10:45:00Z".to_owned(),
            false,
            passing_gates(),
            identity.clone(),
        );
        fs::write(
            bundle.join("candidate-build.json"),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        fs::write(
            bundle.join("artifacts/package-identity.json"),
            serde_json::to_vec_pretty(&identity).unwrap(),
        )
        .unwrap();
        fs::write(
            bundle.join("artifacts/SHA256SUMS"),
            format!(
                "{}  {deb_name}\n{}  {appimage_name}\n",
                sha256_hex(deb_bytes),
                sha256_hex(appimage_bytes)
            ),
        )
        .unwrap();
        fs::write(
            velopack_dir.join(format!("releases.{CANDIDATE_CHANNEL}.json")),
            serde_json::to_vec_pretty(&serde_json::json!({
                "Assets": [{
                    "PackageId": "com.befeast.okplayer",
                    "Version": version,
                    "Type": "Full",
                    "FileName": nupkg_name,
                    "SHA1": "1".repeat(40),
                    "SHA256": sha256_hex(nupkg_bytes),
                    "Size": nupkg_bytes.len(),
                    "NotesMarkdown": "",
                    "NotesHtml": ""
                }]
            }))
            .unwrap(),
        )
        .unwrap();

        let verified = verify_candidate_bundle(&bundle).expect("native bundle should verify");
        assert_eq!(verified.record.source_sha, HEAD);
        let feed = assemble_candidate_feed(
            &verified,
            "https://example.invalid/linux-candidate",
            AcceptanceStatus::Accepted,
            None,
        )
        .expect("verified bundle should assemble");
        let update = select_candidate_update_from_feed(feed, "0.11.0-beta.1.41")
            .expect("enrolled install should select the exact native bundle");
        assert_eq!(update.commit_sha, HEAD);
        assert_eq!(update.package.sha256, sha256_hex(deb_bytes));
        assert_eq!(update.appimage.sha256, sha256_hex(nupkg_bytes));
        assert_eq!(fs::read(&public_feed).unwrap(), public_before);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn feed_assembly_retains_complete_previous_recovery_points_and_prunes_older_assets() {
        let root = unique_temp_dir("okp-candidate-retention");
        fs::create_dir_all(&root).unwrap();
        let deb_path = root.join("ok-player_0.11.0-beta.1.42_amd64.deb");
        fs::write(&deb_path, b"deb").unwrap();
        let version = "0.11.0-beta.1.42";
        let record = CandidateBuild::new(
            HEAD.to_owned(),
            42,
            version.to_owned(),
            "2026-07-17T10:00:00Z".to_owned(),
            "2026-07-17T10:45:00Z".to_owned(),
            false,
            passing_gates(),
            package(version),
        );
        let appimage = |version: &str| CandidateAppImage {
            package_id: "com.befeast.okplayer".to_owned(),
            name: format!("com.befeast.okplayer-{version}-linux-candidate-full.nupkg"),
            url: format!("https://example.invalid/{version}.nupkg"),
            size: 100,
            sha256: "d".repeat(64),
            sha1: "e".repeat(40),
        };
        let deb = |version: &str| CandidatePackage {
            name: format!("ok-player_{version}_amd64.deb"),
            url: format!("https://example.invalid/{version}.deb"),
            size: Some(100),
            sha256: "c".repeat(64),
        };
        let history = |version: &str, build: u64| CandidateHistoryEntry {
            version: version.to_owned(),
            build,
            package: deb(version),
            appimage: appimage(version),
            sha256sums_url: format!("https://example.invalid/SHA256SUMS-{build}.txt"),
        };
        let previous = CandidateFeed {
            channel: crate::candidate_channel::CANDIDATE_CHANNEL.to_owned(),
            version: "0.11.0-beta.1.41".to_owned(),
            build: 41,
            commit_sha: OTHER.to_owned(),
            timestamp_utc: "2026-07-17T09:45:00Z".to_owned(),
            acceptance: AcceptanceStatus::Accepted,
            package: deb("0.11.0-beta.1.41"),
            appimage: appimage("0.11.0-beta.1.41"),
            sha256sums_url: Some("https://example.invalid/SHA256SUMS-41.txt".to_owned()),
            history: vec![
                history("0.11.0-beta.1.40", 40),
                history("0.11.0-beta.1.39", 39),
            ],
        };
        let verified = VerifiedCandidateBundle {
            record,
            deb_path,
            appimage_path: PathBuf::new(),
            velopack_path: PathBuf::new(),
            sums_path: PathBuf::new(),
            velopack: appimage(version),
        };

        let feed = assemble_candidate_feed(
            &verified,
            "https://example.invalid/linux-candidate",
            AcceptanceStatus::Accepted,
            Some(&previous),
        )
        .unwrap();
        assert_eq!(feed.history.len(), 3);
        assert_eq!(feed.history[0].build, 41);
        assert!(feed.has_sufficient_recovery());

        let obsolete = "ok-player_0.11.0-beta.1.10_amd64.deb".to_owned();
        let obsolete_full =
            "com.befeast.okplayer-0.11.0-beta.1.10-linux-candidate-full.nupkg".to_owned();
        let unknown = "operator-note.txt".to_owned();
        let plan = candidate_prune_plan(
            &feed,
            &[
                feed.package.name.clone(),
                obsolete.clone(),
                obsolete_full.clone(),
                unknown,
            ],
        );
        assert_eq!(plan, vec![obsolete, obsolete_full]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_required_gate_blocks_promotion() {
        let mut build = candidate(7);
        build
            .gates
            .iter_mut()
            .find(|gate| gate.name == "clippy")
            .unwrap()
            .status = GateStatus::Failed;
        let errors = build.promotable().unwrap_err();
        assert!(errors.iter().any(|error| error.contains("clippy")));
    }

    #[test]
    fn a_missing_required_gate_blocks_promotion() {
        let mut build = candidate(7);
        build.gates.retain(|gate| gate.name != "workspace-tests");
        let errors = build.promotable().unwrap_err();
        assert!(errors.iter().any(|error| error.contains("workspace-tests")));
    }

    #[test]
    fn native_hardware_evidence_is_enforced_only_when_required() {
        let mut build = candidate(9);
        build.require_native_hardware = true;
        let errors = build.promotable().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains(NATIVE_HARDWARE_GATE))
        );

        build.gates.push(GateResult {
            name: NATIVE_HARDWARE_GATE.to_owned(),
            status: GateStatus::Passed,
            detail: String::new(),
        });
        assert_eq!(build.promotable(), Ok(()));
    }

    #[test]
    fn version_must_carry_the_build_number() {
        let mut build = candidate(12);
        build.version = "0.1.0-linux-candidate.999".to_owned();
        build.package.version = build.version.clone();
        for artifact in &mut build.package.artifacts {
            artifact.file_name = artifact.file_name.replace("12", "999");
        }
        let errors = build.promotable().unwrap_err();
        assert!(errors.iter().any(|error| error.contains("build number 12")));
    }

    #[test]
    fn source_sha_must_match_the_package_commit() {
        let mut build = candidate(3);
        build.source_sha = OTHER.to_owned();
        let errors = build.promotable().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("does not match the package commit"))
        );
    }

    #[test]
    fn heartbeat_classification_separates_idle_building_and_stalled() {
        assert_eq!(
            classify_activity(BuildPhase::Idle, 100_000, DEFAULT_STALL_AFTER_SECONDS),
            BuildActivity::Idle
        );
        assert_eq!(
            classify_activity(BuildPhase::Building, 10, DEFAULT_STALL_AFTER_SECONDS),
            BuildActivity::Building
        );
        assert_eq!(
            classify_activity(
                BuildPhase::Building,
                DEFAULT_STALL_AFTER_SECONDS + 1,
                DEFAULT_STALL_AFTER_SECONDS
            ),
            BuildActivity::Stalled
        );
    }

    #[test]
    fn heartbeat_age_is_saturating() {
        let beat = Heartbeat {
            phase: BuildPhase::Building,
            unix_seconds: 1000,
            note: "packaging".to_owned(),
            source_sha: HEAD.to_owned(),
        };
        // A clock that appears to move backwards must not underflow into a huge
        // age that falsely reports a stall.
        assert_eq!(
            beat.classify(500, DEFAULT_STALL_AFTER_SECONDS),
            BuildActivity::Building
        );
    }
}
