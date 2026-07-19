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
