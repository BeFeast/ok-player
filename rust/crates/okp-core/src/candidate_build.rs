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

use serde::{Deserialize, Serialize};

use crate::acceptance_evidence::{ArtifactKind, PackageIdentity};

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
