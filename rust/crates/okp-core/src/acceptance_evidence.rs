//! Release-acceptance evidence contract for Linux packages.
//!
//! The contract deliberately separates deterministic model/render checks from
//! operator-only GNOME/Wayland behavior. Xvfb can prove pixels and scripted
//! application state, but it can never attest portal, compositor, clipboard,
//! drag/drop, chooser, or focus behavior.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub const EVIDENCE_SCHEMA_VERSION: u32 = 1;

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
    "wayland-always-on-top-unavailable",
    "keyboard-focus-navigation",
];

fn xvfb_viewport(state: &str) -> Viewport {
    match state {
        "narrow-layout" => Viewport {
            width: 480,
            height: 540,
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
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EvidenceManifest {
    pub schema_version: u32,
    pub package: PackageIdentity,
    pub rows: Vec<EvidenceRow>,
}

impl EvidenceManifest {
    /// Build a deliberately incomplete operator template for an exact package.
    pub fn template(package: PackageIdentity) -> Self {
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
        }));

        Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            package,
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
        let mut manifest = EvidenceManifest::template(package());
        for row in &mut manifest.rows {
            row.reference = "canonical-redline".to_owned();
            match row.level {
                EvidenceLevel::GnomeWaylandOperator => {
                    row.operator_status = EvidenceStatus::Pass;
                }
                _ => {
                    row.measurement_result = EvidenceStatus::Pass;
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

    #[test]
    fn complete_exact_manifest_is_release_ready() {
        let manifest = passing_manifest();
        assert_eq!(manifest.validate_release_ready(&package()), Ok(()));
    }

    #[test]
    fn template_records_fullscreen_and_wayland_always_on_top_boundaries() {
        let manifest = EvidenceManifest::template(package());
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
