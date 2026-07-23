//! Release-acceptance evidence contract for Linux packages.
//!
//! The contract deliberately separates deterministic model/render checks from
//! operator-only GNOME/Wayland behavior. Xvfb can prove pixels and scripted
//! application state, but it can never attest portal, compositor, clipboard,
//! drag/drop, chooser, or focus behavior.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::update_selection::compare_versions;

pub const EVIDENCE_SCHEMA_VERSION: u32 = 2;
pub const CANDIDATE_UPGRADE_EVIDENCE_SCHEMA_VERSION: u32 = 1;
pub const FLATPAK_BETA_ARTIFACT_SCHEMA_VERSION: u32 = 1;
pub const FLATPAK_LIFECYCLE_EVIDENCE_SCHEMA_VERSION: u32 = 2;

pub const REQUIRED_FLATPAK_LIFECYCLE_STEPS: &[&str] = &[
    "install-baseline",
    "launch-baseline",
    "update-current",
    "launch-current",
    "rollback-baseline",
    "launch-rollback",
    "restore-current",
    "uninstall",
    "remote-cleanup",
];

pub const REQUIRED_CANDIDATE_UPGRADE_CHECKS: &[&str] = &[
    "interrupted-download-rejected",
    "corrupt-checksum-rejected",
    "feed-identity-mismatch-rejected",
    "unavailable-feed-reported-failed",
    "pkexec-insufficient-privilege-recovery",
    "pkexec-cancelled-recovery",
    "rollback-reinstall-recovery",
    "non-enrolled-install-isolated",
    "public-feed-unchanged",
    "no-update-distinct-from-check-failure",
];

pub const REQUIRED_MODEL_CHECKS: &[&str] = &[
    "rust-workspace-gates",
    "natural-folder-queue",
    "acceptance-contract",
];

pub const REQUIRED_XVFB_STATES: &[&str] = &[
    "first-run",
    "continue-watching",
    "history",
    "loaded-paused-osc",
    "paused",
    "buffering-loading",
    "playback-error",
    "playing-idle",
    "osd",
    "buffered-timeline",
    "chapter-context",
    "chapters",
    "up-next",
    "settings-about",
    "narrow-layout",
    "bright-video-background",
    "dark-video-background",
    "fullscreen",
    "always-on-top",
];

pub const REQUIRED_INSTALLED_CHECKS: &[&str] = &["installed-launch", "installed-package-version"];

pub const REQUIRED_LIVE_CHECKS: &[&str] = &[
    "gnome-file-chooser",
    "gnome-folder-chooser",
    "wayland-drag-drop",
    "wayland-clipboard",
    "desktop-portal",
    "wayland-compositor-fullscreen",
    "wayland-double-click-fullscreen",
    "wayland-always-on-top-unavailable",
    "keyboard-focus-navigation",
];

fn xvfb_viewport(state: &str) -> Viewport {
    match state {
        "narrow-layout" => Viewport {
            width: 480,
            height: 540,
        },
        "settings-about" => Viewport {
            width: 760,
            height: 560,
        },
        "fullscreen" => Viewport {
            width: 1280,
            height: 900,
        },
        _ => Viewport {
            width: 1120,
            height: 680,
        },
    }
}

fn xvfb_theme(state: &str) -> EvidenceTheme {
    if matches!(state, "continue-watching" | "history" | "settings-about") {
        EvidenceTheme::Light
    } else {
        EvidenceTheme::Dark
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Debian,
    AppImage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PackageArtifact {
    pub kind: ArtifactKind,
    pub file_name: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PackageIdentity {
    pub version: String,
    pub commit_sha: String,
    pub artifacts: Vec<PackageArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlatpakBundleArtifact {
    pub file_name: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlatpakRevisionIdentity {
    pub version: String,
    pub ostree_commit: String,
    pub bundle: FlatpakBundleArtifact,
}

/// Portable identity manifest emitted beside the two-version Flatpak beta
/// repositories. It intentionally carries only public names and digests: no
/// host path, hostname, repository URL, or credential field exists.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlatpakBetaArtifact {
    pub schema_version: u32,
    pub source_commit: String,
    pub app_id: String,
    pub arch: String,
    pub branch: String,
    pub baseline_repository: String,
    pub update_repository: String,
    pub baseline: FlatpakRevisionIdentity,
    pub update: FlatpakRevisionIdentity,
    pub update_parent_commit: String,
}

impl FlatpakBetaArtifact {
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != FLATPAK_BETA_ARTIFACT_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported Flatpak beta artifact schema {}, expected {}",
                self.schema_version, FLATPAK_BETA_ARTIFACT_SCHEMA_VERSION
            ));
        }
        validate_git_commit(
            "Flatpak beta artifact source_commit",
            &self.source_commit,
            &mut errors,
        );
        for (name, value) in [
            ("app_id", self.app_id.as_str()),
            ("arch", self.arch.as_str()),
            ("branch", self.branch.as_str()),
        ] {
            if value.trim().is_empty() {
                errors.push(format!("Flatpak beta artifact {name} is empty"));
            }
        }
        for (name, value) in [
            ("baseline_repository", self.baseline_repository.as_str()),
            ("update_repository", self.update_repository.as_str()),
        ] {
            validate_portable_artifact_name(name, value, &mut errors);
        }
        validate_flatpak_revision("baseline", &self.baseline, &mut errors);
        validate_flatpak_revision("update", &self.update, &mut errors);
        validate_ostree_commit(
            "update_parent_commit",
            &self.update_parent_commit,
            &mut errors,
        );
        if compare_versions(&self.update.version, &self.baseline.version)
            != std::cmp::Ordering::Greater
        {
            errors.push("Flatpak update version is not newer than the baseline version".to_owned());
        }
        if self.update.ostree_commit == self.baseline.ostree_commit {
            errors.push("Flatpak baseline and update commits are identical".to_owned());
        }
        if self.update_parent_commit != self.baseline.ostree_commit {
            errors.push("Flatpak update parent is not the baseline commit".to_owned());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Source identity embedded in the packaged mapped-window renderer evidence.
/// Other renderer metrics remain collector-owned, while this shared contract
/// prevents an uploaded result from being detached from its artifact and PR.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct FlatpakSoftwareRendererEvidence {
    pub source_commit: String,
}

impl FlatpakSoftwareRendererEvidence {
    pub fn validate_source_binding(
        &self,
        artifact: &FlatpakBetaArtifact,
        expected_source_commit: &str,
    ) -> Result<(), Vec<String>> {
        let mut errors = artifact.validate().err().unwrap_or_default();
        validate_git_commit(
            "Flatpak software renderer source_commit",
            &self.source_commit,
            &mut errors,
        );
        validate_git_commit(
            "expected Flatpak acceptance source commit",
            expected_source_commit,
            &mut errors,
        );
        if self.source_commit != artifact.source_commit {
            errors.push(
                "Flatpak software renderer source_commit does not match the artifact source_commit"
                    .to_owned(),
            );
        }
        if self.source_commit != expected_source_commit {
            errors.push(
                "Flatpak software renderer source_commit does not match the current pull request head"
                    .to_owned(),
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlatpakAcceptanceDesktop {
    Gnome,
    Kde,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FlatpakAcceptanceSession {
    Wayland,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlatpakLifecycleStep {
    pub id: String,
    pub deployed_commit: Option<String>,
    pub status: EvidenceStatus,
}

/// Exact-head real-machine evidence for the Flatpak repository lifecycle.
/// The fixed enum fields and absence of free-form machine identity fields keep
/// the public record from accidentally capturing private lab topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FlatpakLifecycleEvidence {
    pub schema_version: u32,
    pub pull_request_head: String,
    pub downloaded_artifact_sha256: String,
    pub desktop: FlatpakAcceptanceDesktop,
    pub session: FlatpakAcceptanceSession,
    pub artifact: FlatpakBetaArtifact,
    pub steps: Vec<FlatpakLifecycleStep>,
}

impl FlatpakLifecycleEvidence {
    pub fn template(
        pull_request_head: String,
        downloaded_artifact_sha256: String,
        desktop: FlatpakAcceptanceDesktop,
        artifact: FlatpakBetaArtifact,
    ) -> Self {
        let steps = REQUIRED_FLATPAK_LIFECYCLE_STEPS
            .iter()
            .map(|id| FlatpakLifecycleStep {
                id: (*id).to_owned(),
                deployed_commit: expected_flatpak_step_commit(id, &artifact).map(str::to_owned),
                status: EvidenceStatus::NotRun,
            })
            .collect();
        Self {
            schema_version: FLATPAK_LIFECYCLE_EVIDENCE_SCHEMA_VERSION,
            pull_request_head,
            downloaded_artifact_sha256,
            desktop,
            session: FlatpakAcceptanceSession::Wayland,
            artifact,
            steps,
        }
    }

    pub fn validate_ready(&self) -> Result<(), Vec<String>> {
        let mut errors = self.artifact.validate().err().unwrap_or_default();
        if self.schema_version != FLATPAK_LIFECYCLE_EVIDENCE_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported Flatpak lifecycle evidence schema {}, expected {}",
                self.schema_version, FLATPAK_LIFECYCLE_EVIDENCE_SCHEMA_VERSION
            ));
        }
        validate_git_commit(
            "Flatpak lifecycle pull_request_head",
            &self.pull_request_head,
            &mut errors,
        );
        validate_sha256(
            "Flatpak lifecycle downloaded_artifact_sha256",
            &self.downloaded_artifact_sha256,
            &mut errors,
        );
        if self.pull_request_head != self.artifact.source_commit {
            errors.push(
                "Flatpak lifecycle pull_request_head does not match the artifact source_commit"
                    .to_owned(),
            );
        }

        let step_order = self
            .steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();
        if step_order != REQUIRED_FLATPAK_LIFECYCLE_STEPS {
            errors.push(format!(
                "Flatpak lifecycle steps must appear in this order: {}",
                REQUIRED_FLATPAK_LIFECYCLE_STEPS.join(", ")
            ));
        }

        let mut step_ids = BTreeSet::new();
        for step in &self.steps {
            if !step_ids.insert(step.id.as_str()) {
                errors.push(format!("duplicate Flatpak lifecycle step: {}", step.id));
            }
            if let Some(deployed_commit) = &step.deployed_commit {
                validate_ostree_commit(
                    &format!("Flatpak lifecycle step {} deployed_commit", step.id),
                    deployed_commit,
                    &mut errors,
                );
            }
        }
        for id in REQUIRED_FLATPAK_LIFECYCLE_STEPS {
            let matching = self
                .steps
                .iter()
                .filter(|step| step.id == *id)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                errors.push(format!(
                    "Flatpak lifecycle step {id} must appear exactly once"
                ));
                continue;
            }
            let step = matching[0];
            if step.status != EvidenceStatus::Pass {
                errors.push(format!("Flatpak lifecycle step {id} is not PASS"));
            }
            let expected = expected_flatpak_step_commit(id, &self.artifact);
            match (step.deployed_commit.as_deref(), expected) {
                (Some(deployed), Some(expected)) if deployed != expected => {
                    errors.push(format!(
                        "Flatpak lifecycle step {id} deployed {deployed}, expected {expected}"
                    ));
                }
                (None, Some(expected)) => errors.push(format!(
                    "Flatpak lifecycle step {id} has no deployed commit, expected {expected}"
                )),
                (Some(deployed), None) => errors.push(format!(
                    "Flatpak lifecycle cleanup step {id} must not record a deployed commit, found {deployed}"
                )),
                _ => {}
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn expected_flatpak_step_commit<'a>(
    id: &str,
    artifact: &'a FlatpakBetaArtifact,
) -> Option<&'a str> {
    match id {
        "install-baseline" | "launch-baseline" | "rollback-baseline" | "launch-rollback" => {
            Some(&artifact.baseline.ostree_commit)
        }
        "update-current" | "launch-current" | "restore-current" => {
            Some(&artifact.update.ostree_commit)
        }
        "uninstall" | "remote-cleanup" => None,
        _ => None,
    }
}

fn validate_flatpak_revision(
    name: &str,
    revision: &FlatpakRevisionIdentity,
    errors: &mut Vec<String>,
) {
    if revision.version.trim().is_empty() {
        errors.push(format!("Flatpak {name} version is empty"));
    }
    validate_ostree_commit(
        &format!("Flatpak {name} ostree_commit"),
        &revision.ostree_commit,
        errors,
    );
    validate_portable_artifact_name(
        &format!("Flatpak {name} bundle file_name"),
        &revision.bundle.file_name,
        errors,
    );
    validate_sha256(
        &format!("Flatpak {name} bundle sha256"),
        &revision.bundle.sha256,
        errors,
    );
}

fn validate_portable_artifact_name(name: &str, value: &str, errors: &mut Vec<String>) {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
    {
        errors.push(format!("{name} must be one portable relative file name"));
    }
}

fn validate_git_commit(name: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        errors.push(format!("{name} must be a full 40-character hex SHA"));
    }
}

fn validate_ostree_commit(name: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        errors.push(format!(
            "{name} must be a full 64-character hex OSTree commit"
        ));
    }
}

/// Exact identity observed at one point in an installed candidate upgrade
/// chain. Feed and artifact digests bind the operator result to published bytes
/// rather than a verbal version claim.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateUpgradeStage {
    pub version: String,
    pub commit_sha: String,
    pub feed_sha256: String,
    /// SHA-256 of the user-facing `.deb` or AppImage installed at this stage.
    pub installable_sha256: String,
    /// SHA-256 of the updater payload selected by the feed. This equals the
    /// `.deb` digest on Debian and the full `.nupkg` digest on AppImage.
    pub update_payload_sha256: String,
}

/// How an unchanged install on the defective candidate reached the first build
/// containing the updater fix.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateRecoveryPath {
    /// A feed or artifact correction let the unchanged defective install apply
    /// the fixed candidate through its built-in updater.
    FeedSideBuiltIn,
    /// The old updater could not be repaired remotely, so the operator applied
    /// one explicitly documented bootstrap package.
    ExplicitBootstrap,
}

/// Public predecessor -> defective candidate -> fixed candidate N -> candidate
/// N+1 evidence for one package lane.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateUpgradeLaneEvidence {
    pub kind: ArtifactKind,
    pub public_predecessor: CandidateUpgradeStage,
    pub defective_candidate: CandidateUpgradeStage,
    pub recovery_path: CandidateRecoveryPath,
    pub candidate_n: CandidateUpgradeStage,
    pub candidate_n_plus_one: CandidateUpgradeStage,
    pub candidate_n_plus_one_applied_via_settings: bool,
    pub restarted_version: String,
    pub restarted_commit_sha: String,
    pub settings_probe_sha256_before: String,
    pub settings_probe_sha256_after: String,
    pub history_probe_sha256_before: String,
    pub history_probe_sha256_after: String,
    pub status: EvidenceStatus,
    #[serde(default)]
    pub notes: String,
}

/// One required negative/recovery scenario in the installed updater matrix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateUpgradeCheck {
    pub id: String,
    pub status: EvidenceStatus,
    #[serde(default)]
    pub notes: String,
}

/// Machine-readable gate for deleting historical Linux migration anchors.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CandidateUpgradeEvidence {
    pub schema_version: u32,
    pub migration_anchors: Vec<PackageIdentity>,
    pub public_feed_sha256_before: String,
    pub public_feed_sha256_after: String,
    pub lanes: Vec<CandidateUpgradeLaneEvidence>,
    pub checks: Vec<CandidateUpgradeCheck>,
}

impl CandidateUpgradeEvidence {
    /// Validate both installed lanes, exact restart/state identity, public-feed
    /// isolation, and every required failure/recovery scenario. Only a complete
    /// PASS is sufficient to remove the migration anchor.
    pub fn validate_cleanup_ready(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != CANDIDATE_UPGRADE_EVIDENCE_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported candidate upgrade evidence schema {}, expected {}",
                self.schema_version, CANDIDATE_UPGRADE_EVIDENCE_SCHEMA_VERSION
            ));
        }
        if self.migration_anchors.len() != 2 {
            errors.push(
                "candidate upgrade evidence must contain exactly the public and defective migration anchors"
                    .to_owned(),
            );
        }
        let mut anchor_versions = BTreeSet::new();
        for anchor in &self.migration_anchors {
            validate_package_identity(anchor, &mut errors);
            if !anchor_versions.insert(anchor.version.as_str()) {
                errors.push(format!(
                    "duplicate migration anchor version: {}",
                    anchor.version
                ));
            }
        }
        validate_sha256(
            "public_feed_sha256_before",
            &self.public_feed_sha256_before,
            &mut errors,
        );
        validate_sha256(
            "public_feed_sha256_after",
            &self.public_feed_sha256_after,
            &mut errors,
        );
        if !self
            .public_feed_sha256_before
            .eq_ignore_ascii_case(&self.public_feed_sha256_after)
        {
            errors.push("public feed changed during candidate acceptance".to_owned());
        }

        for kind in [ArtifactKind::Debian, ArtifactKind::AppImage] {
            let matching = self
                .lanes
                .iter()
                .filter(|lane| lane.kind == kind)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                errors.push(format!(
                    "candidate upgrade evidence must contain exactly one {kind:?} lane"
                ));
                continue;
            }
            let lane = matching[0];
            validate_candidate_upgrade_lane(lane, &mut errors);
            validate_candidate_migration_anchor(
                &self.migration_anchors,
                kind,
                "public predecessor",
                &lane.public_predecessor,
                &mut errors,
            );
            validate_candidate_migration_anchor(
                &self.migration_anchors,
                kind,
                "defective candidate",
                &lane.defective_candidate,
                &mut errors,
            );
        }

        let mut check_ids = BTreeSet::new();
        for check in &self.checks {
            if !check_ids.insert(check.id.as_str()) {
                errors.push(format!("duplicate candidate upgrade check: {}", check.id));
            }
        }
        for required in REQUIRED_CANDIDATE_UPGRADE_CHECKS {
            let matching = self
                .checks
                .iter()
                .filter(|check| check.id == *required)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                errors.push(format!(
                    "candidate upgrade check {required} must appear exactly once"
                ));
            } else if matching[0].status != EvidenceStatus::Pass {
                errors.push(format!("candidate upgrade check {required} is not PASS"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceLevel {
    ModelUnit,
    XvfbRender,
    InstalledPackage,
    GnomeWaylandOperator,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceStatus {
    NotRun,
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceTheme {
    Light,
    Dark,
    Auto,
    NotApplicable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Measurement {
    pub name: String,
    pub expected: f64,
    pub actual: f64,
    pub tolerance: f64,
    pub unit: String,
    pub status: EvidenceStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EvidenceRow {
    pub id: String,
    pub level: EvidenceLevel,
    pub viewport: Option<Viewport>,
    pub theme: EvidenceTheme,
    pub state: String,
    pub reference: String,
    pub measurement_result: EvidenceStatus,
    pub operator_status: EvidenceStatus,
    #[serde(default)]
    pub measurements: Vec<Measurement>,
    #[serde(default)]
    pub notes: String,
    /// Privacy-preserving SHA-256 for the execution context that produced this
    /// row. Installed-package PASS rows must identify a context distinct from
    /// the artifact build execution.
    #[serde(default)]
    pub execution_environment_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EvidenceManifest {
    pub schema_version: u32,
    pub package: PackageIdentity,
    pub build_environment_sha256: String,
    pub rows: Vec<EvidenceRow>,
}

impl EvidenceManifest {
    /// Build a deliberately incomplete operator template for an exact package.
    pub fn template(package: PackageIdentity, build_environment_sha256: String) -> Self {
        let mut rows = Vec::new();
        rows.extend(REQUIRED_MODEL_CHECKS.iter().map(|state| EvidenceRow {
            id: (*state).to_owned(),
            level: EvidenceLevel::ModelUnit,
            viewport: None,
            theme: EvidenceTheme::NotApplicable,
            state: (*state).to_owned(),
            reference: "rust-test-suite".to_owned(),
            measurement_result: EvidenceStatus::NotRun,
            operator_status: EvidenceStatus::NotRun,
            measurements: Vec::new(),
            notes: String::new(),
            execution_environment_sha256: None,
        }));
        rows.extend(REQUIRED_XVFB_STATES.iter().map(|state| EvidenceRow {
            id: format!("xvfb-{state}"),
            level: EvidenceLevel::XvfbRender,
            viewport: Some(xvfb_viewport(state)),
            theme: xvfb_theme(state),
            state: (*state).to_owned(),
            reference: String::new(),
            measurement_result: EvidenceStatus::NotRun,
            operator_status: EvidenceStatus::NotRun,
            measurements: Vec::new(),
            notes: String::new(),
            execution_environment_sha256: None,
        }));
        rows.extend(REQUIRED_INSTALLED_CHECKS.iter().map(|state| EvidenceRow {
            id: (*state).to_owned(),
            level: EvidenceLevel::InstalledPackage,
            viewport: None,
            theme: EvidenceTheme::NotApplicable,
            state: (*state).to_owned(),
            reference: "packaging-contract".to_owned(),
            measurement_result: EvidenceStatus::NotRun,
            operator_status: EvidenceStatus::NotRun,
            measurements: Vec::new(),
            notes: String::new(),
            execution_environment_sha256: None,
        }));
        rows.extend(REQUIRED_LIVE_CHECKS.iter().map(|state| EvidenceRow {
            id: (*state).to_owned(),
            level: EvidenceLevel::GnomeWaylandOperator,
            viewport: Some(Viewport {
                width: 1120,
                height: 680,
            }),
            theme: EvidenceTheme::Auto,
            state: (*state).to_owned(),
            reference: "operator-acceptance".to_owned(),
            measurement_result: EvidenceStatus::NotRun,
            operator_status: EvidenceStatus::NotRun,
            measurements: Vec::new(),
            notes: String::new(),
            execution_environment_sha256: None,
        }));

        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            package,
            build_environment_sha256,
            rows,
        }
    }

    /// Validate schema integrity and the non-negotiable evidence-level boundary.
    pub fn validate_contract(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.schema_version != EVIDENCE_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported evidence schema {}, expected {}",
                self.schema_version, EVIDENCE_SCHEMA_VERSION
            ));
        }
        validate_package_identity(&self.package, &mut errors);
        validate_sha256(
            "build_environment_sha256",
            &self.build_environment_sha256,
            &mut errors,
        );

        let mut ids = BTreeSet::new();
        for row in &self.rows {
            if row.id.trim().is_empty() {
                errors.push("evidence row id is empty".to_owned());
            } else if !ids.insert(row.id.as_str()) {
                errors.push(format!("duplicate evidence row id: {}", row.id));
            }
            if row.state.trim().is_empty() {
                errors.push(format!("{}: state is empty", row.id));
            }
            if row.reference.trim().is_empty() {
                errors.push(format!("{}: canonical reference is empty", row.id));
            }
            if row.measurement_result == EvidenceStatus::Pass && row.measurements.is_empty() {
                errors.push(format!(
                    "{}: measurement result is pass but carries no measurements",
                    row.id
                ));
            }
            if row.measurement_result == EvidenceStatus::Pass
                && row
                    .measurements
                    .iter()
                    .any(|measurement| measurement.status != EvidenceStatus::Pass)
            {
                errors.push(format!(
                    "{}: measurement result is pass but a measurement is not PASS",
                    row.id
                ));
            }
            if row.measurements.iter().any(|measurement| {
                measurement.status == EvidenceStatus::Pass
                    && (measurement.actual - measurement.expected).abs() > measurement.tolerance
            }) {
                errors.push(format!(
                    "{}: a passing measurement is outside its tolerance",
                    row.id
                ));
            }
            if row.level != EvidenceLevel::GnomeWaylandOperator
                && REQUIRED_LIVE_CHECKS.contains(&row.state.as_str())
            {
                errors.push(format!(
                    "{}: live desktop behavior cannot be recorded at {:?} level",
                    row.id, row.level
                ));
            }
            if row.level != EvidenceLevel::GnomeWaylandOperator
                && row.operator_status == EvidenceStatus::Pass
            {
                errors.push(format!(
                    "{}: operator PASS requires gnome-wayland-operator evidence",
                    row.id
                ));
            }
            if row.level == EvidenceLevel::XvfbRender && row.viewport.is_none() {
                errors.push(format!("{}: Xvfb evidence requires a viewport", row.id));
            }
            if row.level == EvidenceLevel::InstalledPackage
                && row.measurement_result == EvidenceStatus::Pass
            {
                match row.execution_environment_sha256.as_deref() {
                    Some(fingerprint) => {
                        validate_sha256(
                            &format!("{} execution_environment_sha256", row.id),
                            fingerprint,
                            &mut errors,
                        );
                        if fingerprint.eq_ignore_ascii_case(&self.build_environment_sha256) {
                            errors.push(format!(
                                "{}: installed-package evidence ran in the artifact build execution context",
                                row.id
                            ));
                        }
                    }
                    None => errors.push(format!(
                        "{}: installed-package PASS requires an execution environment fingerprint",
                        row.id
                    )),
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate the exact candidate identity and every release-blocking row.
    pub fn validate_release_ready(
        &self,
        expected_package: &PackageIdentity,
    ) -> Result<(), Vec<String>> {
        let mut errors = self.validate_contract().err().unwrap_or_default();
        if &self.package != expected_package {
            errors
                .push("evidence package identity does not match the candidate package".to_owned());
        }

        let states = rows_by_state(&self.rows);
        for state in REQUIRED_MODEL_CHECKS {
            require_pass(&states, state, EvidenceLevel::ModelUnit, false, &mut errors);
        }
        for state in REQUIRED_XVFB_STATES {
            require_pass(
                &states,
                state,
                EvidenceLevel::XvfbRender,
                false,
                &mut errors,
            );
            require_viewport_and_theme(
                &states,
                state,
                xvfb_viewport(state),
                xvfb_theme(state),
                &mut errors,
            );
        }
        for state in REQUIRED_INSTALLED_CHECKS {
            require_pass(
                &states,
                state,
                EvidenceLevel::InstalledPackage,
                false,
                &mut errors,
            );
        }
        for state in REQUIRED_LIVE_CHECKS {
            require_pass(
                &states,
                state,
                EvidenceLevel::GnomeWaylandOperator,
                true,
                &mut errors,
            );
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn validate_package_identity(package: &PackageIdentity, errors: &mut Vec<String>) {
    if package.version.trim().is_empty() {
        errors.push("package version is empty".to_owned());
    }
    if package.commit_sha.len() != 40
        || !package
            .commit_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        errors.push("package commit_sha must be a full 40-character hex SHA".to_owned());
    }

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
        if artifact.file_name.trim().is_empty() {
            errors.push(format!("{:?} artifact file name is empty", artifact.kind));
        }
        if artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            errors.push(format!(
                "{}: artifact sha256 must be 64 hex characters",
                artifact.file_name
            ));
        }
    }
}

fn validate_candidate_upgrade_lane(lane: &CandidateUpgradeLaneEvidence, errors: &mut Vec<String>) {
    for (name, stage) in [
        ("public_predecessor", &lane.public_predecessor),
        ("defective_candidate", &lane.defective_candidate),
        ("candidate_n", &lane.candidate_n),
        ("candidate_n_plus_one", &lane.candidate_n_plus_one),
    ] {
        if stage.version.trim().is_empty() {
            errors.push(format!("{:?} {name} version is empty", lane.kind));
        }
        if stage.commit_sha.len() != 40
            || !stage
                .commit_sha
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            errors.push(format!(
                "{:?} {name} commit_sha must be a full 40-character hex SHA",
                lane.kind
            ));
        }
        validate_sha256(
            &format!("{:?} {name} feed_sha256", lane.kind),
            &stage.feed_sha256,
            errors,
        );
        validate_sha256(
            &format!("{:?} {name} installable_sha256", lane.kind),
            &stage.installable_sha256,
            errors,
        );
        validate_sha256(
            &format!("{:?} {name} update_payload_sha256", lane.kind),
            &stage.update_payload_sha256,
            errors,
        );
        match lane.kind {
            ArtifactKind::Debian
                if !stage
                    .installable_sha256
                    .eq_ignore_ascii_case(&stage.update_payload_sha256) =>
            {
                errors.push(format!(
                    "Debian {name} update payload does not match its installable .deb"
                ));
            }
            ArtifactKind::AppImage
                if stage
                    .installable_sha256
                    .eq_ignore_ascii_case(&stage.update_payload_sha256) =>
            {
                errors.push(format!(
                    "AppImage {name} update payload must identify the distinct Velopack full package"
                ));
            }
            _ => {}
        }
    }

    if compare_versions(
        &lane.defective_candidate.version,
        &lane.public_predecessor.version,
    ) != std::cmp::Ordering::Greater
    {
        errors.push(format!(
            "{:?} defective candidate is not newer than its public predecessor",
            lane.kind
        ));
    }
    if compare_versions(&lane.candidate_n.version, &lane.defective_candidate.version)
        != std::cmp::Ordering::Greater
    {
        errors.push(format!(
            "{:?} candidate N is not newer than the defective candidate",
            lane.kind
        ));
    }
    if compare_versions(
        &lane.candidate_n_plus_one.version,
        &lane.candidate_n.version,
    ) != std::cmp::Ordering::Greater
    {
        errors.push(format!(
            "{:?} candidate N+1 is not newer than candidate N",
            lane.kind
        ));
    }
    if !lane.candidate_n_plus_one_applied_via_settings {
        errors.push(format!(
            "{:?} candidate N+1 was not applied through Settings",
            lane.kind
        ));
    }
    if lane.restarted_version != lane.candidate_n_plus_one.version
        || lane.restarted_commit_sha != lane.candidate_n_plus_one.commit_sha
    {
        errors.push(format!(
            "{:?} restart identity does not match candidate N+1",
            lane.kind
        ));
    }
    for (name, value) in [
        (
            "settings_probe_sha256_before",
            &lane.settings_probe_sha256_before,
        ),
        (
            "settings_probe_sha256_after",
            &lane.settings_probe_sha256_after,
        ),
        (
            "history_probe_sha256_before",
            &lane.history_probe_sha256_before,
        ),
        (
            "history_probe_sha256_after",
            &lane.history_probe_sha256_after,
        ),
    ] {
        validate_sha256(&format!("{:?} {name}", lane.kind), value, errors);
    }
    if !lane
        .settings_probe_sha256_before
        .eq_ignore_ascii_case(&lane.settings_probe_sha256_after)
    {
        errors.push(format!("{:?} settings changed across upgrades", lane.kind));
    }
    if !lane
        .history_probe_sha256_before
        .eq_ignore_ascii_case(&lane.history_probe_sha256_after)
    {
        errors.push(format!("{:?} history changed across upgrades", lane.kind));
    }
    if lane.status != EvidenceStatus::Pass {
        errors.push(format!(
            "{:?} candidate upgrade lane is not PASS",
            lane.kind
        ));
    }
    if lane.notes.trim().is_empty() {
        errors.push(format!(
            "{:?} candidate upgrade lane must document its {:?} recovery path",
            lane.kind, lane.recovery_path
        ));
    }
}

fn validate_candidate_migration_anchor(
    anchors: &[PackageIdentity],
    kind: ArtifactKind,
    label: &str,
    stage: &CandidateUpgradeStage,
    errors: &mut Vec<String>,
) {
    let matching = anchors
        .iter()
        .filter(|anchor| anchor.version == stage.version && anchor.commit_sha == stage.commit_sha)
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        errors.push(format!(
            "{kind:?} {label} does not match exactly one retained migration anchor"
        ));
        return;
    }
    let artifacts = matching[0]
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == kind)
        .collect::<Vec<_>>();
    if artifacts.len() == 1
        && !artifacts[0]
            .sha256
            .eq_ignore_ascii_case(&stage.installable_sha256)
    {
        errors.push(format!(
            "{kind:?} {label} artifact does not match its retained migration anchor"
        ));
    }
}

fn validate_sha256(name: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        errors.push(format!("{name} must be 64 hex characters"));
    }
}

fn rows_by_state(rows: &[EvidenceRow]) -> BTreeMap<&str, Vec<&EvidenceRow>> {
    let mut states = BTreeMap::<&str, Vec<&EvidenceRow>>::new();
    for row in rows {
        states.entry(row.state.as_str()).or_default().push(row);
    }
    states
}

fn require_pass(
    states: &BTreeMap<&str, Vec<&EvidenceRow>>,
    state: &str,
    expected_level: EvidenceLevel,
    operator_required: bool,
    errors: &mut Vec<String>,
) {
    let Some(rows) = states.get(state) else {
        errors.push(format!("missing required evidence row: {state}"));
        return;
    };
    if rows.len() != 1 {
        errors.push(format!(
            "required evidence state {state} must have exactly one row"
        ));
        return;
    }
    let row = rows[0];
    if row.level != expected_level {
        errors.push(format!(
            "{state}: expected {expected_level:?} evidence, got {:?}",
            row.level
        ));
    }
    if operator_required {
        if row.operator_status != EvidenceStatus::Pass {
            errors.push(format!("{state}: live operator status is not PASS"));
        }
    } else if row.measurement_result != EvidenceStatus::Pass {
        errors.push(format!("{state}: measurement result is not PASS"));
    }
}

fn require_viewport_and_theme(
    states: &BTreeMap<&str, Vec<&EvidenceRow>>,
    state: &str,
    expected_viewport: Viewport,
    expected_theme: EvidenceTheme,
    errors: &mut Vec<String>,
) {
    let Some(row) = states.get(state).and_then(|rows| rows.first()).copied() else {
        return;
    };
    if row.viewport != Some(expected_viewport) {
        errors.push(format!(
            "{state}: expected viewport {}x{}",
            expected_viewport.width, expected_viewport.height
        ));
    }
    if row.theme != expected_theme {
        errors.push(format!(
            "{state}: expected {expected_theme:?} theme, got {:?}",
            row.theme
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> PackageIdentity {
        PackageIdentity {
            version: "0.1.0-linux-alpha.113".to_owned(),
            commit_sha: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            artifacts: vec![
                PackageArtifact {
                    kind: ArtifactKind::Debian,
                    file_name: "ok-player_0.1.0-linux-alpha.113_amd64.deb".to_owned(),
                    sha256: "a".repeat(64),
                },
                PackageArtifact {
                    kind: ArtifactKind::AppImage,
                    file_name: "OK-Player-0.1.0-linux-alpha.113-x86_64.AppImage".to_owned(),
                    sha256: "b".repeat(64),
                },
            ],
        }
    }

    fn passing_manifest() -> EvidenceManifest {
        let mut manifest = EvidenceManifest::template(package(), "a".repeat(64));
        for row in &mut manifest.rows {
            row.reference = "canonical-redline".to_owned();
            match row.level {
                EvidenceLevel::GnomeWaylandOperator => {
                    row.operator_status = EvidenceStatus::Pass;
                }
                _ => {
                    row.measurement_result = EvidenceStatus::Pass;
                    if row.level == EvidenceLevel::InstalledPackage {
                        row.execution_environment_sha256 = Some("b".repeat(64));
                    }
                    row.measurements.push(Measurement {
                        name: "acceptance".to_owned(),
                        expected: 1.0,
                        actual: 1.0,
                        tolerance: 0.0,
                        unit: "boolean".to_owned(),
                        status: EvidenceStatus::Pass,
                    });
                }
            }
        }
        manifest
    }

    fn passing_candidate_upgrade_evidence() -> CandidateUpgradeEvidence {
        let stage =
            |version: &str, digest: char, update_payload_digest: char| CandidateUpgradeStage {
                version: version.to_owned(),
                commit_sha: digest.to_string().repeat(40),
                feed_sha256: digest.to_string().repeat(64),
                installable_sha256: digest.to_string().repeat(64),
                update_payload_sha256: update_payload_digest.to_string().repeat(64),
            };
        let lane = |kind| {
            let payload_digests = match kind {
                ArtifactKind::Debian => ['a', 'b', 'c', 'f'],
                ArtifactKind::AppImage => ['1', '2', '3', '4'],
            };
            CandidateUpgradeLaneEvidence {
            kind,
            public_predecessor: stage("0.1.0-linux-alpha.112", 'a', payload_digests[0]),
            defective_candidate: stage("0.11.0-beta.0.10", 'b', payload_digests[1]),
            recovery_path: CandidateRecoveryPath::ExplicitBootstrap,
            candidate_n: stage("0.11.0-beta.0.12", 'c', payload_digests[2]),
            candidate_n_plus_one: stage("0.11.0-beta.0.13", 'f', payload_digests[3]),
            candidate_n_plus_one_applied_via_settings: true,
            restarted_version: "0.11.0-beta.0.13".to_owned(),
            restarted_commit_sha: "f".repeat(40),
            settings_probe_sha256_before: "d".repeat(64),
            settings_probe_sha256_after: "d".repeat(64),
            history_probe_sha256_before: "e".repeat(64),
            history_probe_sha256_after: "e".repeat(64),
            status: EvidenceStatus::Pass,
            notes: "Installed candidate N once as the documented recovery bootstrap, then applied candidate N+1 through Settings."
                .to_owned(),
        }
        };
        CandidateUpgradeEvidence {
            schema_version: CANDIDATE_UPGRADE_EVIDENCE_SCHEMA_VERSION,
            migration_anchors: {
                let mut public = package();
                public.version = "0.1.0-linux-alpha.112".to_owned();
                public.commit_sha = "a".repeat(40);
                for artifact in &mut public.artifacts {
                    artifact.sha256 = "a".repeat(64);
                }
                let mut private = package();
                private.version = "0.11.0-beta.0.10".to_owned();
                private.commit_sha = "b".repeat(40);
                for artifact in &mut private.artifacts {
                    artifact.sha256 = "b".repeat(64);
                }
                vec![public, private]
            },
            public_feed_sha256_before: "f".repeat(64),
            public_feed_sha256_after: "f".repeat(64),
            lanes: vec![lane(ArtifactKind::Debian), lane(ArtifactKind::AppImage)],
            checks: REQUIRED_CANDIDATE_UPGRADE_CHECKS
                .iter()
                .map(|id| CandidateUpgradeCheck {
                    id: (*id).to_owned(),
                    status: EvidenceStatus::Pass,
                    notes: String::new(),
                })
                .collect(),
        }
    }

    fn flatpak_artifact() -> FlatpakBetaArtifact {
        FlatpakBetaArtifact {
            schema_version: FLATPAK_BETA_ARTIFACT_SCHEMA_VERSION,
            source_commit: "1".repeat(40),
            app_id: "com.befeast.okplayer".to_owned(),
            arch: "x86_64".to_owned(),
            branch: "beta".to_owned(),
            baseline_repository: "repo-baseline".to_owned(),
            update_repository: "repo".to_owned(),
            baseline: FlatpakRevisionIdentity {
                version: "0.11.0-beta.0".to_owned(),
                ostree_commit: "a".repeat(64),
                bundle: FlatpakBundleArtifact {
                    file_name: "OK-Player-0.11.0-beta.0.flatpak".to_owned(),
                    sha256: "b".repeat(64),
                },
            },
            update: FlatpakRevisionIdentity {
                version: "0.11.0-beta.1".to_owned(),
                ostree_commit: "c".repeat(64),
                bundle: FlatpakBundleArtifact {
                    file_name: "OK-Player-0.11.0-beta.1.flatpak".to_owned(),
                    sha256: "d".repeat(64),
                },
            },
            update_parent_commit: "a".repeat(64),
        }
    }

    #[test]
    fn complete_exact_manifest_is_release_ready() {
        let manifest = passing_manifest();
        assert_eq!(manifest.validate_release_ready(&package()), Ok(()));
    }

    #[test]
    fn installed_acceptance_must_run_outside_the_build_execution() {
        let mut manifest = passing_manifest();
        let build_environment_sha256 = manifest.build_environment_sha256.clone();
        for row in &mut manifest.rows {
            if row.level == EvidenceLevel::InstalledPackage {
                row.execution_environment_sha256 = Some(build_environment_sha256.clone());
            }
        }

        let errors = manifest.validate_release_ready(&package()).unwrap_err();
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.contains("artifact build execution context"))
                .count(),
            REQUIRED_INSTALLED_CHECKS.len()
        );
    }

    #[test]
    fn complete_candidate_upgrade_evidence_unblocks_anchor_cleanup() {
        assert_eq!(
            passing_candidate_upgrade_evidence().validate_cleanup_ready(),
            Ok(())
        );
    }

    #[test]
    fn flatpak_beta_artifact_requires_a_direct_two_version_history() {
        assert_eq!(flatpak_artifact().validate(), Ok(()));

        let mut artifact = flatpak_artifact();
        artifact.update.version = artifact.baseline.version.clone();
        artifact.update_parent_commit = "e".repeat(64);
        artifact.baseline_repository = "/tmp/private-repo".to_owned();

        let errors = artifact.validate().unwrap_err();
        assert!(errors.iter().any(|error| error.contains("not newer")));
        assert!(errors.iter().any(|error| error.contains("parent")));
        assert!(
            errors
                .iter()
                .any(|error| error.contains("portable relative file name"))
        );
    }

    #[test]
    fn flatpak_lifecycle_template_stays_blocked_until_every_real_step_passes() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        let errors = evidence.validate_ready().unwrap_err();
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.contains("is not PASS"))
                .count(),
            REQUIRED_FLATPAK_LIFECYCLE_STEPS.len()
        );

        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        assert_eq!(evidence.validate_ready(), Ok(()));
    }

    #[test]
    fn flatpak_lifecycle_rejects_a_false_rollback_commit() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        evidence
            .steps
            .iter_mut()
            .find(|step| step.id == "launch-rollback")
            .expect("rollback launch step")
            .deployed_commit = Some(evidence.artifact.update.ostree_commit.clone());

        let errors = evidence.validate_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("launch-rollback deployed"))
        );
    }

    #[test]
    fn flatpak_lifecycle_rejects_missing_cleanup_steps() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        evidence
            .steps
            .retain(|step| !matches!(step.id.as_str(), "uninstall" | "remote-cleanup"));

        let errors = evidence.validate_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("step uninstall must appear exactly once"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("step remote-cleanup must appear exactly once"))
        );
    }

    #[test]
    fn flatpak_lifecycle_requires_restore_before_cleanup() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        evidence.steps.swap(6, 7);

        let errors = evidence.validate_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("steps must appear in this order"))
        );
    }

    #[test]
    fn flatpak_lifecycle_rejects_false_restore_and_cleanup_commits() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        evidence
            .steps
            .iter_mut()
            .find(|step| step.id == "restore-current")
            .expect("restore step")
            .deployed_commit = Some(evidence.artifact.baseline.ostree_commit.clone());
        evidence
            .steps
            .iter_mut()
            .find(|step| step.id == "uninstall")
            .expect("uninstall step")
            .deployed_commit = Some(evidence.artifact.update.ostree_commit.clone());

        let errors = evidence.validate_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("restore-current deployed"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("cleanup step uninstall must not record"))
        );
    }

    #[test]
    fn flatpak_lifecycle_must_match_the_artifact_source_commit() {
        let mut evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Gnome,
            flatpak_artifact(),
        );
        for step in &mut evidence.steps {
            step.status = EvidenceStatus::Pass;
        }
        evidence.pull_request_head = "3".repeat(40);

        let errors = evidence.validate_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("does not match the artifact source_commit"))
        );
    }

    #[test]
    fn flatpak_software_renderer_evidence_binds_artifact_to_current_head() {
        let artifact = flatpak_artifact();
        let evidence = FlatpakSoftwareRendererEvidence {
            source_commit: artifact.source_commit.clone(),
        };

        assert_eq!(
            evidence.validate_source_binding(&artifact, &artifact.source_commit),
            Ok(())
        );
    }

    #[test]
    fn flatpak_software_renderer_evidence_rejects_detached_or_short_sources() {
        let artifact = flatpak_artifact();
        let evidence = FlatpakSoftwareRendererEvidence {
            source_commit: "short".to_owned(),
        };

        let errors = evidence
            .validate_source_binding(&artifact, &"2".repeat(40))
            .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("must be a full 40-character hex SHA"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("does not match the artifact source_commit"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("does not match the current pull request head"))
        );
    }

    #[test]
    fn flatpak_lifecycle_template_represents_kde_wayland() {
        let evidence = FlatpakLifecycleEvidence::template(
            "1".repeat(40),
            "2".repeat(64),
            FlatpakAcceptanceDesktop::Kde,
            flatpak_artifact(),
        );

        let json = serde_json::to_string(&evidence).expect("serialize KDE lifecycle evidence");
        assert!(json.contains(r#""desktop":"kde""#));
        let decoded: FlatpakLifecycleEvidence =
            serde_json::from_str(&json).expect("deserialize KDE lifecycle evidence");
        assert_eq!(decoded.desktop, FlatpakAcceptanceDesktop::Kde);
        assert_eq!(decoded.session, FlatpakAcceptanceSession::Wayland);
    }

    #[test]
    fn candidate_upgrade_evidence_rejects_state_loss_and_public_feed_drift() {
        let mut evidence = passing_candidate_upgrade_evidence();
        evidence.public_feed_sha256_after = "1".repeat(64);
        evidence.lanes[0].history_probe_sha256_after = "2".repeat(64);

        let errors = evidence.validate_cleanup_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("public feed changed"))
        );
        assert!(errors.iter().any(|error| error.contains("history changed")));
    }

    #[test]
    fn candidate_upgrade_evidence_requires_every_failure_scenario() {
        let mut evidence = passing_candidate_upgrade_evidence();
        evidence
            .checks
            .retain(|check| check.id != "pkexec-cancelled-recovery");

        let errors = evidence.validate_cleanup_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("pkexec-cancelled-recovery"))
        );
    }

    #[test]
    fn candidate_upgrade_evidence_requires_defective_install_recovery_accounting() {
        let mut evidence = passing_candidate_upgrade_evidence();
        evidence.lanes[0].notes.clear();
        evidence.lanes[1].candidate_n = evidence.lanes[1].defective_candidate.clone();
        evidence.lanes[1].candidate_n_plus_one_applied_via_settings = false;

        let errors = evidence.validate_cleanup_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("must document its ExplicitBootstrap recovery path"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("not newer than the defective candidate"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("candidate N+1 was not applied through Settings"))
        );
    }

    #[test]
    fn candidate_upgrade_evidence_binds_lane_stages_to_retained_anchor_bytes() {
        let mut evidence = passing_candidate_upgrade_evidence();
        evidence.migration_anchors[0].artifacts[0].sha256 = "0".repeat(64);
        evidence.lanes[1].defective_candidate.commit_sha = "1".repeat(40);

        let errors = evidence.validate_cleanup_ready().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("public predecessor artifact"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("defective candidate does not match"))
        );
    }

    #[test]
    fn candidate_upgrade_evidence_rejects_copied_appimage_installable_sha() {
        let mut evidence = passing_candidate_upgrade_evidence();
        let appimage = evidence
            .lanes
            .iter_mut()
            .find(|lane| lane.kind == ArtifactKind::AppImage)
            .expect("AppImage lane");
        for stage in [
            &mut appimage.public_predecessor,
            &mut appimage.defective_candidate,
            &mut appimage.candidate_n,
            &mut appimage.candidate_n_plus_one,
        ] {
            stage.update_payload_sha256 = stage.installable_sha256.clone();
        }

        let errors = evidence.validate_cleanup_ready().unwrap_err();
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.contains("distinct Velopack full package"))
                .count(),
            4
        );
    }

    #[test]
    fn template_records_special_viewports_and_wayland_boundaries() {
        let manifest = EvidenceManifest::template(package(), "a".repeat(64));
        let settings_about = manifest
            .rows
            .iter()
            .find(|row| row.state == "settings-about")
            .expect("Settings/About row");
        assert_eq!(
            settings_about.viewport,
            Some(Viewport {
                width: 760,
                height: 560
            })
        );

        let fullscreen = manifest
            .rows
            .iter()
            .find(|row| row.state == "fullscreen")
            .expect("fullscreen row");
        assert_eq!(
            fullscreen.viewport,
            Some(Viewport {
                width: 1280,
                height: 900
            })
        );
        assert_eq!(fullscreen.level, EvidenceLevel::XvfbRender);

        let unavailable = manifest
            .rows
            .iter()
            .find(|row| row.state == "wayland-always-on-top-unavailable")
            .expect("Wayland always-on-top row");
        assert_eq!(unavailable.level, EvidenceLevel::GnomeWaylandOperator);
        assert_eq!(unavailable.operator_status, EvidenceStatus::NotRun);

        // The repeated double-click fullscreen toggle (issue #330) is live GNOME/
        // Wayland behavior: deterministic tests and Xvfb cannot attest that 20
        // real double-clicks each land, so it is operator-required.
        let double_click = manifest
            .rows
            .iter()
            .find(|row| row.state == "wayland-double-click-fullscreen")
            .expect("Wayland double-click fullscreen row");
        assert_eq!(double_click.level, EvidenceLevel::GnomeWaylandOperator);
        assert_eq!(double_click.operator_status, EvidenceStatus::NotRun);
    }

    #[test]
    fn xvfb_cannot_attest_live_desktop_behavior() {
        let mut manifest = passing_manifest();
        let chooser = manifest
            .rows
            .iter_mut()
            .find(|row| row.state == "gnome-file-chooser")
            .expect("chooser row");
        chooser.level = EvidenceLevel::XvfbRender;
        chooser.measurement_result = EvidenceStatus::Pass;
        chooser.measurements.push(Measurement {
            name: "window-visible".to_owned(),
            expected: 1.0,
            actual: 1.0,
            tolerance: 0.0,
            unit: "boolean".to_owned(),
            status: EvidenceStatus::Pass,
        });

        let errors = manifest.validate_release_ready(&package()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("live desktop behavior"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("expected GnomeWaylandOperator"))
        );
    }

    #[test]
    fn evidence_for_a_different_package_is_rejected() {
        let manifest = passing_manifest();
        let mut other = package();
        other.artifacts[0].sha256 = "c".repeat(64);

        let errors = manifest.validate_release_ready(&other).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("does not match")));
    }

    #[test]
    fn alpha_112_known_idle_and_osc_failures_block_release() {
        let mut manifest = passing_manifest();
        for state in ["first-run", "playing-idle", "loaded-paused-osc"] {
            let row = manifest
                .rows
                .iter_mut()
                .find(|row| row.state == state)
                .expect("required alpha.112 row");
            row.measurement_result = EvidenceStatus::Fail;
            row.measurements[0].actual = 0.0;
            row.measurements[0].status = EvidenceStatus::Fail;
            row.notes = "alpha.112 known wrong idle/OSC/layout state".to_owned();
        }

        let errors = manifest.validate_release_ready(&package()).unwrap_err();
        for state in ["first-run", "playing-idle", "loaded-paused-osc"] {
            assert!(
                errors.iter().any(|error| error.contains(state)),
                "missing failure for {state}: {errors:?}"
            );
        }
    }

    #[test]
    fn passing_measurement_outside_tolerance_is_rejected() {
        let mut manifest = passing_manifest();
        manifest.rows[0].measurements[0].actual = 12.0;

        let errors = manifest.validate_contract().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("outside its tolerance"))
        );
    }

    #[test]
    fn release_rejects_changed_canonical_viewport_and_theme() {
        let mut manifest = passing_manifest();
        let first_run = manifest
            .rows
            .iter_mut()
            .find(|row| row.state == "first-run")
            .expect("first-run row");
        first_run.viewport = Some(Viewport {
            width: 1280,
            height: 720,
        });
        first_run.theme = EvidenceTheme::Light;

        let errors = manifest.validate_release_ready(&package()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("expected viewport"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains("expected Dark theme"))
        );
    }

    #[test]
    fn passing_row_cannot_hide_a_failed_measurement() {
        let mut manifest = passing_manifest();
        manifest.rows[0].measurements[0].status = EvidenceStatus::Fail;

        let errors = manifest.validate_contract().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.contains("measurement is not PASS"))
        );
    }
}
