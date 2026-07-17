//! Fedora release-acceptance contract for Linux packages.
//!
//! Fedora validation is more than an RPM build: stock Fedora ships a restricted
//! codec surface, keeps SELinux enforcing, and exposes different portal, Mesa,
//! and PipeWire versions across GNOME (Workstation) and KDE (Plasma) Wayland
//! sessions. This module is the pure, machine-readable acceptance contract the
//! Fedora VM harness fills in and validates. It deliberately encodes the release
//! rules as data so the same manifest can later be attached to a public beta
//! acceptance record without re-running the harness.
//!
//! The contract keeps four things honest that a naive "it launched" check would
//! silently paper over:
//!
//! 1. SELinux must stay **enforcing**; a permissive or disabled guest fails, and
//!    every recorded AVC denial must carry an explanation or it blocks release.
//! 2. Stock-repo and RPM Fusion codec results are recorded on **distinct**
//!    sources, and a stock codec the player cannot decode must surface an honest
//!    diagnostic — never a silent playback "success".
//! 3. A missing Flatpak/RPM artifact is a **blocked precondition**, not a false
//!    pass.
//! 4. GPU-specific gates (4K60, HDR, VA-API) are **skipped with capability
//!    evidence** in a virtual-GPU guest rather than passed or failed.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const FEDORA_ACCEPTANCE_SCHEMA_VERSION: u32 = 1;

/// Coverage areas the Fedora acceptance run must account for. Each area carries a
/// status; areas that are live-desktop only may remain `NotRun` for operator QA,
/// but they must be present so the bundle is complete rather than silently short.
pub const REQUIRED_COVERAGE_AREAS: &[FedoraCoverageArea] = &[
    FedoraCoverageArea::Install,
    FedoraCoverageArea::Update,
    FedoraCoverageArea::Removal,
    FedoraCoverageArea::DesktopEntry,
    FedoraCoverageArea::MimeAssociations,
    FedoraCoverageArea::AppStream,
    FedoraCoverageArea::FilePortals,
    FedoraCoverageArea::DragAndDrop,
    FedoraCoverageArea::Screenshots,
    FedoraCoverageArea::AudioPipeWire,
    FedoraCoverageArea::Mpris,
    FedoraCoverageArea::Subtitles,
    FedoraCoverageArea::StereoDownmix,
    FedoraCoverageArea::SettingsSizing,
    FedoraCoverageArea::Menus,
    FedoraCoverageArea::WindowDragging,
    FedoraCoverageArea::WindowGeometry,
];

/// Media capability profiles every acceptance run must account for exactly once.
/// The low-resource profile must actually run; the real-hardware profile is
/// skipped-with-evidence on a virtual GPU. A manifest that omits either profile
/// is structurally incomplete rather than an implicit pass.
pub const REQUIRED_MEDIA_PROFILES: &[MediaProfile] =
    &[MediaProfile::LowResource, MediaProfile::RealHardware];

/// Coverage areas that can only be attested in a live desktop session (portals,
/// drag/drop, window dragging via the compositor). A headless collector leaves
/// these `NotRun` for operator QA; the contract never lets them be reported
/// `Pass` from a non-live run.
pub const OPERATOR_ONLY_COVERAGE_AREAS: &[FedoraCoverageArea] = &[
    FedoraCoverageArea::FilePortals,
    FedoraCoverageArea::DragAndDrop,
    FedoraCoverageArea::WindowDragging,
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FedoraDesktop {
    WorkstationGnome,
    KdePlasma,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionType {
    Wayland,
    X11,
}

/// The SELinux runtime mode reported by `getenforce`. Acceptance requires
/// [`SelinuxMode::Enforcing`]; anything else fails rather than being "fixed" by
/// relaxing policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SelinuxMode {
    Enforcing,
    Permissive,
    Disabled,
}

/// One of the four Fedora test states. The state selects which repositories are
/// enabled and which package delivery is under test; it also decides whether an
/// artifact hash is a hard precondition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FedoraTestState {
    /// Stock Fedora repositories only (restricted codec surface).
    StockRepos,
    /// Stock Fedora plus RPM Fusion for a codec-complete surface.
    RpmFusionCodecs,
    /// The Flatpak package (requires a Flatpak artifact).
    FlatpakPackage,
    /// The native RPM / COPR package (requires an RPM or COPR artifact).
    NativeRpmCopr,
}

impl FedoraTestState {
    /// Whether this state can only be validated once a package artifact hash is
    /// present. The repo-configuration states exercise the workspace binary and
    /// do not gate on a package file; the packaged states do.
    pub fn requires_artifact(self) -> bool {
        matches!(self, Self::FlatpakPackage | Self::NativeRpmCopr)
    }

    fn accepts_artifact_kind(self, kind: FedoraArtifactKind) -> bool {
        match self {
            Self::FlatpakPackage => kind == FedoraArtifactKind::Flatpak,
            Self::NativeRpmCopr => {
                matches!(kind, FedoraArtifactKind::Rpm | FedoraArtifactKind::Copr)
            }
            Self::StockRepos | Self::RpmFusionCodecs => false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FedoraArtifactKind {
    Flatpak,
    Rpm,
    Copr,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FedoraArtifact {
    pub kind: FedoraArtifactKind,
    pub file_name: String,
    pub sha256: String,
}

/// Which repository provided the codec under test. Stock and RPM Fusion results
/// are always recorded on distinct sources so a codec-complete pass can never be
/// confused with a stock pass.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodecSource {
    StockFedora,
    RpmFusion,
}

/// Outcome of decoding one codec with one repository configuration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodecOutcome {
    /// The stream decoded and played.
    Decoded,
    /// The codec is unavailable and the player surfaced an honest diagnostic.
    /// This is the expected stock-Fedora path for patent-encumbered codecs.
    DiagnosedUnsupported,
    /// Playback reported success or hung while producing no frames and no
    /// diagnostic — the silent failure the contract exists to reject.
    SilentFailure,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodecCheck {
    /// Short codec label, e.g. `h264`, `hevc`, `aac`, `ac3`, `vp9`.
    pub codec: String,
    pub source: CodecSource,
    pub outcome: CodecOutcome,
    /// The honest diagnostic the shell surfaced, when the codec is unsupported.
    #[serde(default)]
    pub diagnostic: String,
}

/// Media capability profile. The low-resource profile (UI responsiveness plus
/// 1080p H.264) is capability-aware and must run even in a virtual-GPU guest;
/// the real-hardware profile (4K60, HDR, VA-API) is GPU-specific and is skipped
/// with capability evidence when no real GPU is present.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaProfile {
    LowResource,
    RealHardware,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileStatus {
    Pass,
    Fail,
    /// Deliberately skipped with capability evidence (virtual GPU).
    Skipped,
    NotRun,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaProfileResult {
    pub profile: MediaProfile,
    pub status: ProfileStatus,
    /// Free-form capability/measurement evidence, e.g. the skip reason or the
    /// observed responsiveness figures.
    #[serde(default)]
    pub evidence: String,
}

/// Renderer and hardware-decode capability of the guest. In a virtual-GPU guest
/// the renderer is a software or paravirtual string (e.g. `llvmpipe`, `virgl`),
/// `virtual_gpu` is true, and the real-hardware profile is skipped.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GpuCapability {
    pub renderer: String,
    pub virtual_gpu: bool,
    pub vaapi_available: bool,
}

/// One recorded AVC denial. Enforcing SELinux is expected to be quiet; any
/// denial that appears must carry a `justification` (a known, benign,
/// documented cause) or it blocks release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AvcDenial {
    /// A deduplicated denial signature (comm/scontext/tcontext/tclass), never a
    /// raw log line that could leak host paths.
    pub signature: String,
    #[serde(default = "one")]
    pub count: u32,
    /// `None` for an unexplained denial (blocks); `Some(reason)` when the denial
    /// is a known, accepted cause.
    #[serde(default)]
    pub justification: Option<String>,
}

fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SelinuxReport {
    pub mode: SelinuxMode,
    #[serde(default)]
    pub denials: Vec<AvcDenial>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FedoraEnvironment {
    /// Fedora release, e.g. `42`.
    pub fedora_version: String,
    pub desktop: FedoraDesktop,
    pub session: SessionType,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FedoraCoverageArea {
    Install,
    Update,
    Removal,
    DesktopEntry,
    MimeAssociations,
    AppStream,
    FilePortals,
    DragAndDrop,
    Screenshots,
    AudioPipeWire,
    Mpris,
    Subtitles,
    StereoDownmix,
    SettingsSizing,
    Menus,
    WindowDragging,
    WindowGeometry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CoverageStatus {
    Pass,
    Fail,
    /// A precondition was missing (e.g. the package is not installed yet).
    Blocked,
    NotRun,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CoverageCheck {
    pub area: FedoraCoverageArea,
    pub status: CoverageStatus,
    #[serde(default)]
    pub evidence: String,
}

/// The overall verdict for a Fedora acceptance run. `Blocked` is a first-class
/// result distinct from `Fail`: it means a precondition (usually a missing
/// artifact) prevented the run, not that the player misbehaved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AcceptanceVerdict {
    Pass,
    Blocked,
    Fail,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptanceOutcome {
    pub verdict: AcceptanceVerdict,
    /// Reasons the run failed. Non-empty only when `verdict == Fail`.
    pub failures: Vec<String>,
    /// Reasons the run is blocked on a precondition. Non-empty only when
    /// `verdict == Blocked`.
    pub blockers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FedoraAcceptanceManifest {
    pub schema_version: u32,
    pub environment: FedoraEnvironment,
    pub test_state: FedoraTestState,
    /// The package under test, when the state delivers one. Absent for the
    /// repo-configuration states, and a blocked precondition for packaged states.
    #[serde(default)]
    pub artifact: Option<FedoraArtifact>,
    pub selinux: SelinuxReport,
    pub gpu: GpuCapability,
    #[serde(default)]
    pub codec_checks: Vec<CodecCheck>,
    #[serde(default)]
    pub media_profiles: Vec<MediaProfileResult>,
    #[serde(default)]
    pub coverage: Vec<CoverageCheck>,
}

impl FedoraAcceptanceManifest {
    /// Build a deliberately-incomplete template for a state and environment. The
    /// harness fills in the collected facts; every required coverage area is
    /// pre-seeded `NotRun` so an incomplete run is visible rather than silently
    /// short.
    pub fn template(
        environment: FedoraEnvironment,
        test_state: FedoraTestState,
        gpu: GpuCapability,
    ) -> Self {
        let coverage = REQUIRED_COVERAGE_AREAS
            .iter()
            .map(|area| CoverageCheck {
                area: *area,
                status: CoverageStatus::NotRun,
                evidence: String::new(),
            })
            .collect();
        let media_profiles = vec![
            MediaProfileResult {
                profile: MediaProfile::LowResource,
                status: ProfileStatus::NotRun,
                evidence: String::new(),
            },
            MediaProfileResult {
                profile: MediaProfile::RealHardware,
                status: ProfileStatus::NotRun,
                evidence: String::new(),
            },
        ];
        Self {
            schema_version: FEDORA_ACCEPTANCE_SCHEMA_VERSION,
            environment,
            test_state,
            artifact: None,
            selinux: SelinuxReport {
                mode: SelinuxMode::Enforcing,
                denials: Vec::new(),
            },
            gpu,
            codec_checks: Vec::new(),
            media_profiles,
            coverage,
        }
    }

    /// Validate the structural integrity of the manifest independent of the
    /// pass/fail verdict: schema version, artifact shape, codec labels, and
    /// coverage completeness. These are authoring errors, not release outcomes.
    pub fn validate_structure(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.schema_version != FEDORA_ACCEPTANCE_SCHEMA_VERSION {
            errors.push(format!(
                "unsupported Fedora acceptance schema {}, expected {}",
                self.schema_version, FEDORA_ACCEPTANCE_SCHEMA_VERSION
            ));
        }
        if self.environment.fedora_version.trim().is_empty() {
            errors.push("environment.fedora_version is empty".to_owned());
        }

        if let Some(artifact) = &self.artifact {
            if !self.test_state.accepts_artifact_kind(artifact.kind) {
                errors.push(format!(
                    "{:?} artifact is not valid for the {:?} state",
                    artifact.kind, self.test_state
                ));
            }
            if artifact.file_name.trim().is_empty() {
                errors.push("artifact file_name is empty".to_owned());
            }
            if artifact.sha256.len() != 64
                || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                errors.push("artifact sha256 must be 64 hex characters".to_owned());
            }
        }

        for check in &self.codec_checks {
            if check.codec.trim().is_empty() {
                errors.push("codec check has an empty codec label".to_owned());
            }
            if check.source == CodecSource::RpmFusion
                && self.test_state == FedoraTestState::StockRepos
            {
                errors.push(format!(
                    "codec {}: RPM Fusion source is not valid in the stock-repos state",
                    check.codec
                ));
            }
        }

        // Media profiles must name every required profile exactly once, so an
        // empty `media_profiles` can never skip the low-resource gate or the
        // real-hardware skip check and slip through as a pass.
        for profile in REQUIRED_MEDIA_PROFILES {
            let count = self
                .media_profiles
                .iter()
                .filter(|result| result.profile == *profile)
                .count();
            match count {
                1 => {}
                0 => errors.push(format!("missing required media profile {profile:?}")),
                _ => errors.push(format!("duplicate media profile {profile:?}")),
            }
        }

        // Coverage must name every required area exactly once.
        let mut seen = BTreeSet::new();
        for check in &self.coverage {
            if !seen.insert(check.area) {
                errors.push(format!("duplicate coverage area {:?}", check.area));
            }
        }
        for area in REQUIRED_COVERAGE_AREAS {
            if !seen.contains(area) {
                errors.push(format!("missing required coverage area {area:?}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Evaluate the release verdict. Structural errors are hard failures; on top
    /// of those the enforcing-SELinux gate, AVC explanations, codec honesty, GPU
    /// skip evidence, and coverage results decide `Pass` / `Blocked` / `Fail`.
    pub fn evaluate(&self) -> AcceptanceOutcome {
        let mut failures = self.validate_structure().err().unwrap_or_default();
        let mut blockers = Vec::new();

        // SELinux must stay enforcing; a relaxed guest is not an acceptance guest.
        if self.selinux.mode != SelinuxMode::Enforcing {
            failures.push(format!(
                "SELinux is {:?}; acceptance requires enforcing",
                self.selinux.mode
            ));
        }
        for denial in &self.selinux.denials {
            if denial
                .justification
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                failures.push(format!("unexplained AVC denial: {}", denial.signature));
            }
        }

        // A packaged state with no artifact is a blocked precondition, not a
        // pass and not a failure.
        if self.test_state.requires_artifact() && self.artifact.is_none() {
            blockers.push(format!(
                "{:?} state requires a package artifact, none provided",
                self.test_state
            ));
        }

        // Codec honesty: a silent failure is always a failure. A diagnosed
        // unsupported codec must actually carry its diagnostic text.
        for check in &self.codec_checks {
            match check.outcome {
                CodecOutcome::SilentFailure => failures.push(format!(
                    "codec {} ({:?}) failed silently with no diagnostic",
                    check.codec, check.source
                )),
                CodecOutcome::DiagnosedUnsupported if check.diagnostic.trim().is_empty() => {
                    failures.push(format!(
                        "codec {} ({:?}) is unsupported but recorded no diagnostic",
                        check.codec, check.source
                    ));
                }
                _ => {}
            }
        }

        // Media profiles. The low-resource profile must run; the real-hardware
        // profile must be skipped-with-evidence on a virtual GPU and must never
        // be reported as a pass there (a false pass).
        for result in &self.media_profiles {
            match (result.profile, result.status) {
                (MediaProfile::LowResource, ProfileStatus::Fail) => {
                    failures.push("low-resource media profile failed".to_owned());
                }
                (MediaProfile::LowResource, ProfileStatus::Skipped | ProfileStatus::NotRun) => {
                    failures.push(
                        "low-resource media profile must run; it is capability-aware".to_owned(),
                    );
                }
                (MediaProfile::RealHardware, ProfileStatus::Pass) if self.gpu.virtual_gpu => {
                    failures.push(
                        "real-hardware media profile cannot pass on a virtual GPU".to_owned(),
                    );
                }
                (MediaProfile::RealHardware, ProfileStatus::Skipped) => {
                    if !self.gpu.virtual_gpu {
                        failures.push(
                            "real-hardware media profile was skipped without a virtual GPU"
                                .to_owned(),
                        );
                    } else if result.evidence.trim().is_empty()
                        || self.gpu.renderer.trim().is_empty()
                    {
                        failures.push(
                            "real-hardware skip needs renderer capability evidence".to_owned(),
                        );
                    }
                }
                (MediaProfile::RealHardware, ProfileStatus::NotRun) if self.gpu.virtual_gpu => {
                    failures.push(
                        "real-hardware media profile must be explicitly skipped on a virtual GPU"
                            .to_owned(),
                    );
                }
                _ => {}
            }
        }

        // Coverage: a failing area fails the run; a blocked non-operator area
        // blocks it; operator-only areas may stay NotRun for live QA.
        for check in &self.coverage {
            match check.status {
                CoverageStatus::Fail => {
                    failures.push(format!("coverage area {:?} failed", check.area));
                }
                CoverageStatus::Blocked => {
                    blockers.push(format!("coverage area {:?} is blocked", check.area));
                }
                CoverageStatus::NotRun if !OPERATOR_ONLY_COVERAGE_AREAS.contains(&check.area) => {
                    blockers.push(format!("coverage area {:?} was not run", check.area));
                }
                _ => {}
            }
        }

        let verdict = if !failures.is_empty() {
            AcceptanceVerdict::Fail
        } else if !blockers.is_empty() {
            AcceptanceVerdict::Blocked
        } else {
            AcceptanceVerdict::Pass
        };

        AcceptanceOutcome {
            verdict,
            failures,
            blockers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment() -> FedoraEnvironment {
        FedoraEnvironment {
            fedora_version: "42".to_owned(),
            desktop: FedoraDesktop::KdePlasma,
            session: SessionType::Wayland,
        }
    }

    fn virtual_gpu() -> GpuCapability {
        GpuCapability {
            renderer: "llvmpipe (LLVM 18, 256 bits)".to_owned(),
            virtual_gpu: true,
            vaapi_available: false,
        }
    }

    /// A fully passing stock-repos run in a virtual guest: enforcing SELinux, no
    /// unexplained denials, honest codec diagnostics, low-resource profile run,
    /// real-hardware skipped with evidence, every non-operator coverage area run.
    fn passing_stock_manifest() -> FedoraAcceptanceManifest {
        let mut manifest = FedoraAcceptanceManifest::template(
            environment(),
            FedoraTestState::StockRepos,
            virtual_gpu(),
        );
        manifest.codec_checks = vec![
            CodecCheck {
                codec: "vp9".to_owned(),
                source: CodecSource::StockFedora,
                outcome: CodecOutcome::Decoded,
                diagnostic: String::new(),
            },
            CodecCheck {
                codec: "hevc".to_owned(),
                source: CodecSource::StockFedora,
                outcome: CodecOutcome::DiagnosedUnsupported,
                diagnostic: "HEVC decoder unavailable in stock Fedora; install RPM Fusion"
                    .to_owned(),
            },
        ];
        manifest.media_profiles = vec![
            MediaProfileResult {
                profile: MediaProfile::LowResource,
                status: ProfileStatus::Pass,
                evidence: "1080p H.264 played, OSC responsive".to_owned(),
            },
            MediaProfileResult {
                profile: MediaProfile::RealHardware,
                status: ProfileStatus::Skipped,
                evidence: "no VA-API device on virtual GPU".to_owned(),
            },
        ];
        for check in &mut manifest.coverage {
            check.status = if OPERATOR_ONLY_COVERAGE_AREAS.contains(&check.area) {
                CoverageStatus::NotRun
            } else {
                CoverageStatus::Pass
            };
        }
        manifest
    }

    fn packaged_flatpak_manifest() -> FedoraAcceptanceManifest {
        let mut manifest = passing_stock_manifest();
        manifest.test_state = FedoraTestState::FlatpakPackage;
        manifest.artifact = Some(FedoraArtifact {
            kind: FedoraArtifactKind::Flatpak,
            file_name: "org.okplayer.OkPlayer.flatpak".to_owned(),
            sha256: "a".repeat(64),
        });
        manifest
    }

    #[test]
    fn passing_stock_run_is_a_pass() {
        let outcome = passing_stock_manifest().evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Pass, "{outcome:?}");
        assert!(outcome.failures.is_empty());
        assert!(outcome.blockers.is_empty());
    }

    #[test]
    fn permissive_selinux_fails() {
        let mut manifest = passing_stock_manifest();
        manifest.selinux.mode = SelinuxMode::Permissive;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("enforcing")));
    }

    #[test]
    fn disabled_selinux_fails() {
        let mut manifest = passing_stock_manifest();
        manifest.selinux.mode = SelinuxMode::Disabled;
        assert_eq!(manifest.evaluate().verdict, AcceptanceVerdict::Fail);
    }

    #[test]
    fn unexplained_avc_denial_fails_but_justified_one_passes() {
        let mut manifest = passing_stock_manifest();
        manifest.selinux.denials = vec![AvcDenial {
            signature: "comm=ok-player tclass=file".to_owned(),
            count: 2,
            justification: None,
        }];
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("AVC")));

        manifest.selinux.denials[0].justification =
            Some("known benign portal probe, tracked upstream".to_owned());
        assert_eq!(manifest.evaluate().verdict, AcceptanceVerdict::Pass);
    }

    #[test]
    fn silent_codec_failure_is_rejected() {
        let mut manifest = passing_stock_manifest();
        manifest.codec_checks[1].outcome = CodecOutcome::SilentFailure;
        manifest.codec_checks[1].diagnostic = String::new();
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("silently")));
    }

    #[test]
    fn diagnosed_unsupported_codec_needs_diagnostic_text() {
        let mut manifest = passing_stock_manifest();
        manifest.codec_checks[1].diagnostic = "   ".to_owned();
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("no diagnostic")));
    }

    #[test]
    fn rpm_fusion_codec_source_is_invalid_in_stock_state() {
        let mut manifest = passing_stock_manifest();
        manifest.codec_checks[0].source = CodecSource::RpmFusion;
        let errors = manifest.validate_structure().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("RPM Fusion source is not valid"))
        );
        // Structural error also fails the verdict.
        assert_eq!(manifest.evaluate().verdict, AcceptanceVerdict::Fail);
    }

    #[test]
    fn rpm_fusion_state_records_distinct_source() {
        let mut manifest = passing_stock_manifest();
        manifest.test_state = FedoraTestState::RpmFusionCodecs;
        manifest.codec_checks = vec![CodecCheck {
            codec: "hevc".to_owned(),
            source: CodecSource::RpmFusion,
            outcome: CodecOutcome::Decoded,
            diagnostic: String::new(),
        }];
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Pass, "{outcome:?}");
    }

    #[test]
    fn flatpak_state_without_artifact_is_blocked_not_failed() {
        let mut manifest = passing_stock_manifest();
        manifest.test_state = FedoraTestState::FlatpakPackage;
        manifest.artifact = None;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Blocked, "{outcome:?}");
        assert!(outcome.failures.is_empty());
        assert!(outcome.blockers.iter().any(|b| b.contains("artifact")));
    }

    #[test]
    fn packaged_state_with_artifact_passes() {
        assert_eq!(
            packaged_flatpak_manifest().evaluate().verdict,
            AcceptanceVerdict::Pass
        );
    }

    #[test]
    fn wrong_artifact_kind_for_state_is_a_structural_error() {
        let mut manifest = packaged_flatpak_manifest();
        manifest.artifact.as_mut().unwrap().kind = FedoraArtifactKind::Rpm;
        let errors = manifest.validate_structure().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("not valid for")));
    }

    #[test]
    fn bad_artifact_hash_is_rejected() {
        let mut manifest = packaged_flatpak_manifest();
        manifest.artifact.as_mut().unwrap().sha256 = "short".to_owned();
        let errors = manifest.validate_structure().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("64 hex")));
    }

    #[test]
    fn real_hardware_profile_cannot_pass_on_virtual_gpu() {
        let mut manifest = passing_stock_manifest();
        manifest.media_profiles[1].status = ProfileStatus::Pass;
        manifest.media_profiles[1].evidence = "impossible".to_owned();
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("virtual GPU")));
    }

    #[test]
    fn real_hardware_skip_needs_capability_evidence() {
        let mut manifest = passing_stock_manifest();
        manifest.media_profiles[1].evidence = String::new();
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(
            outcome
                .failures
                .iter()
                .any(|f| f.contains("capability evidence"))
        );
    }

    #[test]
    fn real_hardware_skip_without_virtual_gpu_is_a_failure() {
        let mut manifest = passing_stock_manifest();
        manifest.gpu.virtual_gpu = false;
        manifest.gpu.vaapi_available = true;
        // Real hardware present but the profile was skipped anyway.
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(
            outcome
                .failures
                .iter()
                .any(|f| f.contains("without a virtual GPU"))
        );
    }

    #[test]
    fn real_hardware_not_run_on_virtual_gpu_must_be_explicit_skip() {
        let mut manifest = passing_stock_manifest();
        manifest.media_profiles[1].status = ProfileStatus::NotRun;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(
            outcome
                .failures
                .iter()
                .any(|f| f.contains("explicitly skipped"))
        );
    }

    #[test]
    fn low_resource_profile_must_run() {
        let mut manifest = passing_stock_manifest();
        manifest.media_profiles[0].status = ProfileStatus::Skipped;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("low-resource")));
    }

    #[test]
    fn unrun_non_operator_coverage_blocks() {
        let mut manifest = passing_stock_manifest();
        let install = manifest
            .coverage
            .iter_mut()
            .find(|c| c.area == FedoraCoverageArea::Install)
            .unwrap();
        install.status = CoverageStatus::NotRun;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Blocked);
        assert!(outcome.blockers.iter().any(|b| b.contains("Install")));
    }

    #[test]
    fn failing_coverage_area_fails_the_run() {
        let mut manifest = passing_stock_manifest();
        let mpris = manifest
            .coverage
            .iter_mut()
            .find(|c| c.area == FedoraCoverageArea::Mpris)
            .unwrap();
        mpris.status = CoverageStatus::Fail;
        let outcome = manifest.evaluate();
        assert_eq!(outcome.verdict, AcceptanceVerdict::Fail);
        assert!(outcome.failures.iter().any(|f| f.contains("Mpris")));
    }

    #[test]
    fn operator_only_areas_may_stay_not_run() {
        // The passing template leaves portals/drag-drop/window-dragging NotRun.
        let manifest = passing_stock_manifest();
        assert_eq!(manifest.evaluate().verdict, AcceptanceVerdict::Pass);
    }

    #[test]
    fn missing_coverage_area_is_a_structural_error() {
        let mut manifest = passing_stock_manifest();
        manifest
            .coverage
            .retain(|c| c.area != FedoraCoverageArea::AppStream);
        let errors = manifest.validate_structure().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("AppStream")));
    }

    #[test]
    fn empty_media_profiles_is_a_structural_error_not_a_pass() {
        let mut manifest = passing_stock_manifest();
        manifest.media_profiles.clear();
        let errors = manifest.validate_structure().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("LowResource")));
        assert!(errors.iter().any(|e| e.contains("RealHardware")));
        // The structural gap must fail the verdict rather than pass silently.
        assert_eq!(manifest.evaluate().verdict, AcceptanceVerdict::Fail);
    }

    #[test]
    fn missing_low_resource_profile_is_a_structural_error() {
        let mut manifest = passing_stock_manifest();
        manifest
            .media_profiles
            .retain(|result| result.profile != MediaProfile::LowResource);
        let errors = manifest.validate_structure().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("missing required media profile LowResource"))
        );
    }

    #[test]
    fn duplicate_media_profile_is_a_structural_error() {
        let mut manifest = passing_stock_manifest();
        let low = manifest.media_profiles[0].clone();
        manifest.media_profiles.push(low);
        let errors = manifest.validate_structure().unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.contains("duplicate media profile LowResource"))
        );
    }

    #[test]
    fn schema_mismatch_is_rejected() {
        let mut manifest = passing_stock_manifest();
        manifest.schema_version = 999;
        let errors = manifest.validate_structure().unwrap_err();
        assert!(errors.iter().any(|e| e.contains("schema")));
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let manifest = packaged_flatpak_manifest();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let parsed: FedoraAcceptanceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn template_seeds_every_required_coverage_area_not_run() {
        let manifest = FedoraAcceptanceManifest::template(
            environment(),
            FedoraTestState::StockRepos,
            virtual_gpu(),
        );
        assert_eq!(manifest.coverage.len(), REQUIRED_COVERAGE_AREAS.len());
        assert!(
            manifest
                .coverage
                .iter()
                .all(|c| c.status == CoverageStatus::NotRun)
        );
        // A bare template is not release-ready: the low-resource profile has not
        // run (a hard failure) and non-operator coverage areas are unrun.
        assert_ne!(manifest.evaluate().verdict, AcceptanceVerdict::Pass);
    }
}
