//! Uniform update lifecycle shared by every install kind (issue #680).
//!
//! Today each install kind carries its own ad-hoc update behaviour, which is
//! how #660 could report "updated" while the old binary kept running, and why
//! the RPM install has no update story at all. This module owns the whole
//! model portably: which kind of install is running, what that install is
//! allowed to do about updates, the single state machine every check/apply
//! walks, and the projection that turns a state into what the surfaces show.
//!
//! Two rules keep the model portable and testable:
//!
//! * **Detection is pure.** The shell gathers facts about the process
//!   (environment variables, the executable path, whether a package manager
//!   claims that path, whether the Velopack layout is next to it) into
//!   [`InstallEvidence`]; [`detect_install_kind`] decides. Core never touches
//!   the filesystem or the environment, so the same detector runs headless in
//!   tests and inside the Windows shell through `okp-ffi`.
//! * **Shells never write update strings.** [`UpdateLifecycle::describe`] is
//!   the only source of user-facing update text and of the action a surface may
//!   offer, so an Updates panel cannot render a state as a different one and
//!   the About surface cannot disagree with it.
//!
//! Network access, package downloads, process restarts and every other side
//! effect stay in the shells; this module only decides.

use std::cmp::Ordering;
use std::fmt;

use crate::update_selection::compare_versions;

/// How the running copy of OK Player was installed.
///
/// Decided by [`detect_install_kind`] from shell-gathered [`InstallEvidence`],
/// never by probing the machine from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InstallKind {
    /// Windows install laid out and updated by Velopack.
    WindowsVelopack,
    /// Linux AppImage, updated by replacing the image in place.
    AppImage,
    /// `.deb` install owned by dpkg/apt.
    Deb,
    /// `.rpm` install owned by rpm/dnf.
    Rpm,
    /// Flatpak install, updated by the Flatpak runtime.
    Flatpak,
    /// A build that no packaging owns: `cargo run`, a build tree, an unpacked
    /// tarball. Updates are disabled rather than pretended.
    DevBuild,
}

impl InstallKind {
    /// What this install kind is allowed to do about updates.
    pub const fn capability(self) -> UpdateCapability {
        match self {
            Self::WindowsVelopack | Self::AppImage => UpdateCapability::SelfApply,
            Self::Deb | Self::Rpm | Self::Flatpak => UpdateCapability::SystemManaged,
            Self::DevBuild => UpdateCapability::Unmanaged,
        }
    }

    /// Whether the app discovers versions for this install kind at all. The
    /// rpm and flatpak lanes never ask — they report who updates them — while
    /// every other kind, the system-managed `.deb` lane included, polls a feed
    /// of its own.
    pub const fn discovers_versions(self) -> bool {
        !matches!(self, Self::Rpm | Self::Flatpak)
    }

    /// Whether taking a discovered offer downloads *and* applies it in one
    /// step, relaunching the process. The AppImage lane does: one click
    /// downloads, applies and restarts, with no separate apply the user
    /// confirms. Velopack downloads in the background and applies later, so
    /// its download does not close anything.
    pub const fn applies_while_downloading(self) -> bool {
        matches!(self, Self::AppImage)
    }

    /// Stable identifier for logs and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsVelopack => "windows-velopack",
            Self::AppImage => "appimage",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Flatpak => "flatpak",
            Self::DevBuild => "dev-build",
        }
    }

    /// How the user updates a [`UpdateCapability::SystemManaged`] install. The
    /// hint travels inside [`UpdateState::AvailableExternally`] so no shell has
    /// to invent one.
    const fn system_update_hint_text(self) -> &'static str {
        match self {
            Self::Deb => "Update OK Player with your package manager (apt).",
            Self::Rpm => "Update OK Player with your package manager (dnf).",
            Self::Flatpak => "Update OK Player with Flatpak (flatpak update).",
            Self::WindowsVelopack | Self::AppImage | Self::DevBuild => {
                "Update OK Player with your system update tool."
            }
        }
    }
}

impl fmt::Display for InstallKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What an install may do about an available update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateCapability {
    /// The app downloads and applies the update itself.
    SelfApply,
    /// A system update tool owns the payload; the app may only report.
    SystemManaged,
    /// Nothing updates this install; the app says so instead of claiming to be
    /// current.
    Unmanaged,
}

/// The shell's answer to "does a package manager own the running executable?".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PackageOwnership {
    /// The shell has not asked, or the query failed. Treated exactly like
    /// [`Self::Unowned`]: an install is only reported as packaged on a positive
    /// answer.
    #[default]
    Unknown,
    /// The shell asked and no package manager claims the path.
    Unowned,
    /// dpkg claims the executable path.
    Dpkg,
    /// rpm claims the executable path.
    Rpm,
}

/// Facts about the running process that the shell collects for
/// [`detect_install_kind`]. Everything is optional: a shell that cannot answer
/// a question leaves the field empty and detection degrades to the next signal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallEvidence {
    /// Value of `$FLATPAK_ID`, when set.
    pub flatpak_id: Option<String>,
    /// Whether `/.flatpak-info` exists (the sandbox marker survives an unset
    /// `$FLATPAK_ID`).
    pub flatpak_info_present: bool,
    /// Value of `$APPIMAGE`, the absolute path of the running image, when set.
    pub appimage_path: Option<String>,
    /// Absolute path of the running executable as the shell resolved it.
    pub executable_path: Option<String>,
    /// Whether a package manager claims [`Self::executable_path`].
    pub package_ownership: PackageOwnership,
    /// Whether the Velopack layout (a `current` directory beside an
    /// `Update.exe`) surrounds the executable.
    pub velopack_layout_present: bool,
}

impl InstallEvidence {
    fn is_flatpak(&self) -> bool {
        self.flatpak_id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
            || self.flatpak_info_present
    }

    /// The executable is running out of an AppImage's own mount, which nothing
    /// else produces and which no inherited variable can fake.
    fn runs_from_appimage_mount(&self) -> bool {
        self.executable_path
            .as_deref()
            .is_some_and(|path| path.contains("/.mount_"))
    }

    /// `$APPIMAGE` is set. On its own this only says *some* AppImage is
    /// involved: the variable is inherited, so a packaged OK Player launched by
    /// an unrelated AppImage sees it too.
    fn appimage_variable_set(&self) -> bool {
        self.appimage_path
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty())
    }

    /// The install kind implied by a positive package-ownership answer.
    fn owning_package(&self) -> Option<InstallKind> {
        match self.package_ownership {
            PackageOwnership::Dpkg => Some(InstallKind::Deb),
            PackageOwnership::Rpm => Some(InstallKind::Rpm),
            PackageOwnership::Unowned | PackageOwnership::Unknown => None,
        }
    }
}

/// Decides the install kind from shell-gathered evidence. Pure: no filesystem,
/// no environment, no process inspection.
///
/// Precedence follows how strongly a signal binds the *running* process to an
/// update mechanism:
///
/// 1. Flatpak, because inside the sandbox a `dpkg`/`rpm` answer describes the
///    runtime rather than OK Player.
/// 2. An executable running out of an AppImage mount — the image is
///    self-contained wherever it is parked, including `/tmp`, where nothing
///    owns the path.
/// 3. The Velopack layout, which only a Velopack install has.
/// 4. A positive package-ownership answer, which separates deb from rpm. It
///    outranks a bare `$APPIMAGE` because that variable is *inherited*: an
///    AppImage-packaged launcher passes it to the packaged OK Player it starts,
///    and only the executable's own ownership describes that child.
/// 5. `$APPIMAGE` when the shell has *explicitly* established that no package
///    owns the executable — an extract-and-run AppImage
///    (`APPIMAGE_EXTRACT_AND_RUN`) has no mount path to corroborate it. An
///    ownership query that failed ([`PackageOwnership::Unknown`]) is not that
///    evidence: the executable may well be packaged, and the variable may well
///    belong to some other image that launched it.
/// 6. Otherwise a dev build: unowned is not the same as up to date.
pub fn detect_install_kind(evidence: &InstallEvidence) -> InstallKind {
    if evidence.is_flatpak() {
        return InstallKind::Flatpak;
    }
    if evidence.runs_from_appimage_mount() {
        return InstallKind::AppImage;
    }
    if evidence.velopack_layout_present {
        return InstallKind::WindowsVelopack;
    }
    if let Some(packaged) = evidence.owning_package() {
        return packaged;
    }
    if evidence.appimage_variable_set() && evidence.package_ownership == PackageOwnership::Unowned {
        return InstallKind::AppImage;
    }
    InstallKind::DevBuild
}

/// Orders two build versions for the restart check, where a release must
/// outrank the prereleases that led to it.
///
/// [`compare_versions`] compares numeric runs, which is right *within* a lane —
/// `alpha.109` after `alpha.108` — but reads `1.0.0` as older than
/// `1.0.0-beta.1`, because the missing fourth run defaults to zero. A
/// prerelease-to-stable upgrade would then look like a failed restart. So the
/// numeric core is compared first, a version with no prerelease tail wins a tie
/// against one that has it, and two prereleases of the same core fall back to
/// the natural comparison.
fn compare_build_order(left: &str, right: &str) -> Ordering {
    let (left_core, left_pre) = split_prerelease(left);
    let (right_core, right_pre) = split_prerelease(right);
    match compare_versions(left_core, right_core) {
        Ordering::Equal => {}
        order => return order,
    }
    match (left_pre, right_pre) {
        (None, None) => Ordering::Equal,
        // A release outranks any prerelease of the same core.
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left_pre), Some(right_pre)) => compare_versions(left_pre, right_pre),
    }
}

/// Splits `1.0.0-beta.1` into its numeric core and its prerelease tail.
fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    }
}

/// One position in the update lifecycle.
///
/// `Idle → Checking → UpToDate | Available | AvailableExternally | Failed`, and
/// from `Available` a [`UpdateCapability::SelfApply`] install walks
/// `Downloading → ReadyToApply → Applying → RestartPending → Running`. A
/// [`UpdateCapability::SystemManaged`] install stops at `AvailableExternally`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing has been checked yet in this session.
    Idle,
    /// A check is in flight. `carried` is the offer that was already on screen
    /// when the user asked to re-check; a failed refresh puts it back exactly
    /// as it was instead of dropping a known update or demoting it.
    Checking { carried: Option<CarriedOffer> },
    /// The check completed and this build is the newest one.
    UpToDate,
    /// A newer version exists and this install can apply it itself.
    Available { version: String },
    /// A newer version exists but a system update tool owns it; `hint` says
    /// which one.
    AvailableExternally { version: String, hint: String },
    /// A discovered version the user chose to skip. The offer is remembered in
    /// full — nothing prompts for it, but everything needed to act on it later
    /// survives: `hint` keeps telling a system-managed install how to get the
    /// release anyway, and `staged` keeps a verified payload from being thrown
    /// away and downloaded again.
    Skipped {
        version: String,
        hint: Option<String>,
        staged: bool,
    },
    /// A system update tool owns this install and the app does not discover
    /// versions for it at all — the rpm and flatpak lanes, which report who
    /// updates them and never run a check. Distinct from [`Self::Idle`], whose
    /// surface still offers one.
    ManagedExternally { hint: String },
    /// The payload for `version` is being fetched.
    Downloading { version: String },
    /// The payload is staged and verified; applying is a user decision.
    ReadyToApply { version: String },
    /// The payload is being applied.
    Applying { version: String },
    /// `version` is installed on disk but this process still runs the old
    /// binary. Never a "you are on `version`" state — that conflation is #660.
    RestartPending { version: String },
    /// The process restarted and is running `version`.
    Running { version: String },
    /// A fallible step gave up. Reachable from `Checking`, `Downloading`,
    /// `Applying` and `RestartPending`. `target` is the version the failed
    /// attempt was for, retained whenever the failure happened after discovery
    /// so the same offer stays known and retryable; a check that failed before
    /// finding anything has none.
    Failed {
        reason: String,
        target: Option<String>,
        /// Whether the payload for `target` is still downloaded and verified.
        /// An apply that failed leaves one behind, so retrying re-applies it
        /// rather than fetching it all over again.
        staged: bool,
    },
}

impl UpdateState {
    /// The version this state is about, when it is about one. Always the update
    /// *target* — never a claim about the binary currently executing.
    pub fn target_version(&self) -> Option<&str> {
        match self {
            Self::Available { version }
            | Self::AvailableExternally { version, .. }
            | Self::Downloading { version }
            | Self::ReadyToApply { version }
            | Self::Applying { version }
            | Self::RestartPending { version }
            | Self::Skipped { version, .. } => Some(version),
            Self::Failed { target, .. } => target.as_deref(),
            Self::Checking { carried } => carried.as_ref().map(CarriedOffer::version),
            Self::Idle | Self::UpToDate | Self::ManagedExternally { .. } => None,
            // `Running` carries the version that is executing, which is the
            // running version rather than a target still to be reached.
            Self::Running { .. } => None,
        }
    }
}

/// The offer a refresh carries through [`UpdateState::Checking`], restored
/// intact when the refresh fails. Keeping the whole offer — not just its
/// version — is what lets a failed re-check leave an available update available
/// and a skipped one skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarriedOffer {
    Available {
        version: String,
    },
    AvailableExternally {
        version: String,
        hint: String,
    },
    /// A payload already downloaded and verified, waiting to be applied.
    ReadyToApply {
        version: String,
    },
    Skipped {
        version: String,
        hint: Option<String>,
        staged: bool,
    },
    /// An offer that had already failed. A refresh over it must not quietly
    /// promote it back into a clean offer and drop the error that explains
    /// what the user is looking at.
    Failed {
        version: String,
        reason: String,
        staged: bool,
    },
}

impl CarriedOffer {
    pub fn version(&self) -> &str {
        match self {
            Self::Available { version }
            | Self::AvailableExternally { version, .. }
            | Self::ReadyToApply { version }
            | Self::Skipped { version, .. }
            | Self::Failed { version, .. } => version,
        }
    }

    fn into_state(self) -> UpdateState {
        match self {
            Self::Available { version } => UpdateState::Available { version },
            Self::AvailableExternally { version, hint } => {
                UpdateState::AvailableExternally { version, hint }
            }
            Self::ReadyToApply { version } => UpdateState::ReadyToApply { version },
            Self::Skipped {
                version,
                hint,
                staged,
            } => UpdateState::Skipped {
                version,
                hint,
                staged,
            },
            Self::Failed {
                version,
                reason,
                staged,
            } => UpdateState::Failed {
                reason,
                target: Some(version),
                staged,
            },
        }
    }
}

/// Why a transition was refused. The lifecycle never changes state on a
/// refusal, so a shell may retry or ignore it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateTransitionError {
    /// The transition is not defined for the current state.
    NotAllowedFrom(UpdateState),
    /// The install kind's capability forbids the transition anywhere.
    CapabilityForbids(UpdateCapability),
    /// The transition belongs to a lane this install kind is not on — the
    /// combined download-and-apply step exists only where accepting an offer
    /// does both.
    NotThisLane(InstallKind),
}

/// What a surface may tell the user about the binary that is executing right
/// now — the field #660 got wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionClaim {
    /// The running binary is the newest version this install knows about.
    Current,
    /// A newer version is known; the running binary is not it. Covers every
    /// state between discovery and the restart that actually swaps the binary,
    /// `RestartPending` included.
    Superseded { newer: String },
    /// Nothing is known yet: no check has run, one is in flight, or one failed.
    Unknown,
    /// This install is not updated at all.
    NotApplicable,
}

/// The single action a surface may offer for the current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateAction {
    /// Run an update check.
    CheckNow,
    /// Start downloading the discovered update.
    DownloadUpdate,
    /// Apply the staged payload. Both self-applying lanes relaunch the process
    /// as part of applying, so the label says so.
    ApplyAndRestart,
    /// Restart to start running the applied version.
    RestartToFinish,
    /// Retry after a failure.
    Retry,
    /// Suppress this exact version so nothing prompts for it again.
    SkipVersion,
    /// Install a version the user previously skipped.
    InstallAnyway,
}

impl UpdateAction {
    /// Label a shell renders on the action control.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CheckNow => "Check for updates",
            Self::DownloadUpdate => "Update now",
            Self::ApplyAndRestart => "Install and restart",
            Self::RestartToFinish => "Restart now",
            Self::Retry => "Try again",
            Self::SkipVersion => "Skip this version",
            Self::InstallAnyway => "Install anyway",
        }
    }

    /// Whether the action makes the app itself change the installed bits. Only
    /// a [`UpdateCapability::SelfApply`] install ever reaches a state that
    /// offers one.
    pub const fn applies_update_in_app(self) -> bool {
        matches!(
            self,
            Self::DownloadUpdate
                | Self::ApplyAndRestart
                | Self::RestartToFinish
                | Self::InstallAnyway
        )
    }

    /// Whether taking this action shuts the running process down. Both ways of
    /// getting onto a new build do — applying a staged payload relaunches the
    /// app, and so does finishing an applied one — so a surface can warn before
    /// it happens instead of closing the player out from under the user.
    pub const fn closes_the_app(self) -> bool {
        matches!(self, Self::ApplyAndRestart | Self::RestartToFinish)
    }
}

/// Everything the Updates surface and the About surface are allowed to show,
/// derived from one state by [`UpdateLifecycle::describe`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdatePresentation {
    pub install_kind: InstallKind,
    pub capability: UpdateCapability,
    /// The version this process is executing. Unchanged by anything short of a
    /// restart into a new build.
    pub version_in_use: String,
    /// The update target, when a state is about one.
    pub target_version: Option<String>,
    /// What may be said about the running binary.
    pub claim: VersionClaim,
    /// Message for the Updates surface.
    pub updates_message: String,
    /// Message for the About surface, derived from the same state so the two
    /// surfaces cannot disagree.
    pub about_message: String,
    /// The primary action the surface may offer, if any.
    pub action: Option<UpdateAction>,
    /// Whether taking [`Self::action`] shuts the running player down, for this
    /// install kind. Beyond the actions that always do, this covers the
    /// AppImage lane, where accepting the offer downloads, applies and
    /// relaunches in one step — a surface must be able to warn before that,
    /// whatever the action is called.
    pub action_closes_the_app: bool,
    /// The secondary action offered beside the primary one — today only
    /// "Skip this version" on a live offer. Kept in the projection so a shell
    /// never has to maintain its own parallel offer model.
    pub secondary_action: Option<UpdateAction>,
    /// What went wrong with the last attempt while the state carried on — a
    /// refresh that failed over a standing offer. Already folded into
    /// [`Self::updates_message`]; exposed separately for surfaces that render
    /// the offer and its status line as two things, as the GTK one does.
    pub notice: Option<String>,
}

/// The update state machine for one install.
///
/// Constructed with the install kind and the version this process is running;
/// every transition is a method that either advances the state or returns an
/// [`UpdateTransitionError`] and leaves it untouched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateLifecycle {
    install_kind: InstallKind,
    running_version: String,
    state: UpdateState,
    /// What went wrong with the last attempt, when the state itself carried on
    /// regardless — a failed refresh that restored the offer it was
    /// refreshing. Cleared by every transition that succeeds.
    notice: Option<String>,
}

impl UpdateLifecycle {
    /// Starts at [`UpdateState::Idle`] for `install_kind`, running
    /// `running_version`.
    pub fn new(install_kind: InstallKind, running_version: impl Into<String>) -> Self {
        Self {
            install_kind,
            running_version: running_version.into(),
            state: UpdateState::Idle,
            notice: None,
        }
    }

    /// Starts an install whose updates a system tool owns outright and which
    /// the app never queries for versions — the rpm and flatpak lanes, which
    /// today answer a check with "managed by DNF/Flatpak" rather than a
    /// version. The lifecycle reports who updates it and offers nothing, which
    /// [`UpdateState::Idle`] cannot do: an idle install still offers a check.
    /// Only a [`UpdateCapability::SystemManaged`] install can be in this state.
    pub fn managed_externally(
        install_kind: InstallKind,
        running_version: impl Into<String>,
    ) -> Result<Self, UpdateTransitionError> {
        if install_kind.capability() != UpdateCapability::SystemManaged {
            return Err(UpdateTransitionError::CapabilityForbids(
                install_kind.capability(),
            ));
        }
        if install_kind.discovers_versions() {
            // The `.deb` lane is system-managed but still discovers versions
            // from its own feed, so it keeps its check and its announcement;
            // only lanes that never ask are owned outright.
            return Err(UpdateTransitionError::NotThisLane(install_kind));
        }
        Ok(Self {
            install_kind,
            running_version: running_version.into(),
            state: UpdateState::ManagedExternally {
                hint: install_kind.system_update_hint_text().to_owned(),
            },
            notice: None,
        })
    }

    /// Rebuilds the lifecycle in a process that started with an update already
    /// downloaded and staged but not yet applied — the user downloaded it,
    /// closed the app, and came back. The payload is still on disk (Velopack
    /// keeps a pending release; the Linux lane keeps the staged asset), so the
    /// lifecycle returns to [`UpdateState::ReadyToApply`] instead of pretending
    /// nothing was found and re-downloading it. Only a
    /// [`UpdateCapability::SelfApply`] install stages payloads of its own.
    pub fn resumed_with_staged_update(
        install_kind: InstallKind,
        running_version: impl Into<String>,
        staged_version: impl Into<String>,
    ) -> Result<Self, UpdateTransitionError> {
        if install_kind.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(
                install_kind.capability(),
            ));
        }
        let running_version = running_version.into();
        let staged_version = staged_version.into();
        // A record left behind by an earlier run can be stale — the user may
        // have replaced the install by hand since. Applying it would be a
        // downgrade, so a record that is not newer than what is running is
        // discarded rather than offered.
        let state = if compare_build_order(&staged_version, &running_version) == Ordering::Greater {
            UpdateState::ReadyToApply {
                version: staged_version,
            }
        } else {
            UpdateState::Idle
        };
        Ok(Self {
            install_kind,
            running_version,
            state,
            notice: None,
        })
    }

    /// Rebuilds the lifecycle in the process that came up after a self-applied
    /// update, and settles the restart in one step.
    ///
    /// A self-apply replaces the process, so the `RestartPending` lifecycle
    /// that staged the update dies with the old one; the shell persists the
    /// pending target across the restart (on Windows Velopack keeps its own
    /// staged-release record) and passes it here together with the version the
    /// new process actually came up as. The comparison is
    /// [`Self::restarted_into`]'s, so a restart that silently came back on the
    /// old binary is caught across the process boundary too (#660) instead of
    /// being lost with the old lifecycle. Only a
    /// [`UpdateCapability::SelfApply`] install can have a pending restart.
    pub fn resumed_after_restart(
        install_kind: InstallKind,
        running_version: impl Into<String>,
        pending_version: impl Into<String>,
    ) -> Result<Self, UpdateTransitionError> {
        if install_kind.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(
                install_kind.capability(),
            ));
        }
        let running_version = running_version.into();
        let mut lifecycle = Self {
            install_kind,
            running_version: running_version.clone(),
            state: UpdateState::RestartPending {
                version: pending_version.into(),
            },
            notice: None,
        };
        lifecycle.restarted_into(running_version)?;
        Ok(lifecycle)
    }

    pub const fn install_kind(&self) -> InstallKind {
        self.install_kind
    }

    pub const fn capability(&self) -> UpdateCapability {
        self.install_kind.capability()
    }

    pub const fn state(&self) -> &UpdateState {
        &self.state
    }

    /// The version this process is executing — not the version an applied but
    /// unrestarted update would run.
    pub fn running_version(&self) -> &str {
        &self.running_version
    }

    /// Begins a check. Allowed from every settled state; refused outright for
    /// an [`UpdateCapability::Unmanaged`] install, which has nothing to check.
    pub fn start_check(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() == UpdateCapability::Unmanaged {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        match self.state {
            UpdateState::Idle
            | UpdateState::UpToDate
            | UpdateState::Available { .. }
            | UpdateState::AvailableExternally { .. }
            | UpdateState::Skipped { .. }
            | UpdateState::Running { .. }
            | UpdateState::Failed { .. } => {
                // A re-check must not lose the offer already on screen: if it
                // fails, that offer comes back exactly as it was.
                let carried = self.carried_offer();
                Ok(self.enter(UpdateState::Checking { carried }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The offer currently on screen, in the form a failed refresh would put
    /// back. A failure that retained a target comes back as the offer a retry
    /// would restore, which is what the surface was showing.
    fn carried_offer(&self) -> Option<CarriedOffer> {
        let version = match &self.state {
            // A failure stays a failure across a refresh: its reason is what
            // the surface is showing, and promoting it to a clean offer would
            // erase it.
            UpdateState::Failed {
                reason,
                target: Some(version),
                staged,
            } => {
                return Some(CarriedOffer::Failed {
                    version: version.clone(),
                    reason: reason.clone(),
                    staged: *staged,
                });
            }
            UpdateState::Available { version }
            | UpdateState::AvailableExternally { version, .. } => version.clone(),
            UpdateState::Skipped {
                version,
                hint,
                staged,
            } => {
                return Some(CarriedOffer::Skipped {
                    version: version.clone(),
                    hint: hint.clone(),
                    staged: *staged,
                });
            }
            _ => return None,
        };
        Some(if self.capability() == UpdateCapability::SelfApply {
            CarriedOffer::Available { version }
        } else {
            CarriedOffer::AvailableExternally {
                version,
                hint: self.install_kind.system_update_hint_text().to_owned(),
            }
        })
    }

    /// The check completed and found nothing newer.
    pub fn check_found_none(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match self.state {
            UpdateState::Checking { .. } => Ok(self.enter(UpdateState::UpToDate)),
            _ => Err(self.rejected()),
        }
    }

    /// The check found `version`. A [`UpdateCapability::SelfApply`] install
    /// gets an actionable [`UpdateState::Available`]; a
    /// [`UpdateCapability::SystemManaged`] install gets
    /// [`UpdateState::AvailableExternally`] with the hint for its packaging and
    /// goes no further.
    pub fn check_found(
        &mut self,
        version: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() == UpdateCapability::Unmanaged {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        if !matches!(self.state, UpdateState::Checking { .. }) {
            return Err(self.rejected());
        }
        let version = version.into();
        let next = if self.capability() == UpdateCapability::SelfApply {
            UpdateState::Available { version }
        } else {
            UpdateState::AvailableExternally {
                version,
                hint: self.install_kind.system_update_hint_text().to_owned(),
            }
        };
        Ok(self.enter(next))
    }

    /// The check itself failed (no network, bad manifest, …).
    pub fn check_failed(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        let UpdateState::Checking { carried } = &self.state else {
            return Err(self.rejected());
        };
        let reason = reason.into();
        // The offer that was on screen before the refresh comes back intact,
        // with the error carried beside it as a notice. Only a check that had
        // no offer to protect ends in `Failed`.
        match carried.clone() {
            Some(offer) => {
                let restored = offer.into_state();
                Ok(self.enter_with_notice(restored, format!("Update check failed: {reason}")))
            }
            None => Ok(self.enter(UpdateState::Failed {
                reason,
                target: None,
                staged: false,
            })),
        }
    }

    /// Suppresses the discovered version: nothing prompts for it again, but it
    /// stays known so the user can still install it on demand. Mirrors the
    /// per-channel skip the settings already persist.
    pub fn skip_offer(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        let (version, staged) = match &self.state {
            UpdateState::Available { version }
            | UpdateState::AvailableExternally { version, .. } => (version.clone(), false),
            UpdateState::ReadyToApply { version } => (version.clone(), true),
            UpdateState::Failed {
                target: Some(version),
                staged,
                ..
            } => (version.clone(), *staged),
            _ => return Err(self.rejected()),
        };
        // Skipping silences the prompt, not the instructions: a system-managed
        // install must keep being told how to get the release it skipped.
        let hint = (self.capability() == UpdateCapability::SystemManaged)
            .then(|| self.install_kind.system_update_hint_text().to_owned());
        Ok(self.enter(UpdateState::Skipped {
            version,
            hint,
            staged,
        }))
    }

    /// Installs a version the user had skipped. Only a
    /// [`UpdateCapability::SelfApply`] install can act on it in the app; a
    /// system-managed one keeps pointing at its package manager.
    pub fn install_anyway(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        match &self.state {
            UpdateState::Skipped {
                version, staged, ..
            } => {
                let version = version.clone();
                // A payload kept through the skip is applied, not fetched again.
                let next = if *staged {
                    UpdateState::ReadyToApply { version }
                } else {
                    UpdateState::Downloading { version }
                };
                Ok(self.enter(next))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Starts fetching the discovered payload. Only a
    /// [`UpdateCapability::SelfApply`] install downloads anything.
    pub fn start_download(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        match &self.state {
            UpdateState::Available { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::Downloading { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The payload is downloaded and verified.
    pub fn download_finished(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Downloading { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::ReadyToApply { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The one-call download-and-apply step succeeded and the new build is on
    /// disk, awaiting the relaunch.
    ///
    /// The AppImage lane downloads and applies in a single call, so there is no
    /// separate `ReadyToApply` for the user to act on; this reports the whole
    /// step at once. Lanes that stage first must go through
    /// [`Self::download_finished`] and [`Self::start_apply`].
    pub fn download_and_apply_needs_restart(
        &mut self,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        if !self.install_kind.applies_while_downloading() {
            return Err(UpdateTransitionError::NotThisLane(self.install_kind));
        }
        match &self.state {
            UpdateState::Downloading { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::RestartPending { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The one-call download-and-apply step failed *after* the payload landed.
    ///
    /// That lane reports one error for both halves, so the shell says which
    /// half it was; when the download succeeded the verified payload is still
    /// on disk and the retry re-applies it instead of fetching it again.
    pub fn download_and_apply_failed(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        if !self.install_kind.applies_while_downloading() {
            return Err(UpdateTransitionError::NotThisLane(self.install_kind));
        }
        match &self.state {
            UpdateState::Downloading { version } => {
                let target = Some(version.clone());
                Ok(self.enter(UpdateState::Failed {
                    reason: reason.into(),
                    target,
                    staged: true,
                }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The download or its verification failed.
    pub fn download_failed(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Downloading { version } => {
                let target = Some(version.clone());
                Ok(self.enter(UpdateState::Failed {
                    reason: reason.into(),
                    target,
                    // The download is what failed; there is nothing staged.
                    staged: false,
                }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Starts applying the staged payload.
    pub fn start_apply(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::ReadyToApply { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::Applying { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Applying succeeded: the new version is on disk, this process still runs
    /// the old one. The only success exit from
    /// [`UpdateState::Applying`] besides [`Self::apply_finished_running`].
    pub fn apply_needs_restart(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Applying { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::RestartPending { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Applying succeeded and the new bits are already the ones executing (an
    /// installer that re-execs the process before handing control back).
    /// Advances [`Self::running_version`].
    pub fn apply_finished_running(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Applying { version } => {
                let version = version.clone();
                self.running_version = version.clone();
                Ok(self.enter(UpdateState::Running { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Applying failed.
    pub fn apply_failed(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Applying { version } => {
                let target = Some(version.clone());
                Ok(self.enter(UpdateState::Failed {
                    reason: reason.into(),
                    target,
                    // `Applying` is only reachable from `ReadyToApply`, so the
                    // verified payload is still on disk: a retry applies it.
                    staged: true,
                }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The process restarted and reports the version it came back as.
    ///
    /// Coming back on the pending target — or on anything newer, which a
    /// manual upgrade or a stale pending marker can produce — completes the
    /// update, and the state records the version actually running. Coming back
    /// *older* is #660 itself: the restart ran the old binary, so it becomes
    /// [`UpdateState::Failed`] instead of a silent success.
    pub fn restarted_into(
        &mut self,
        running_version: impl Into<String>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        let UpdateState::RestartPending { version } = &self.state else {
            return Err(self.rejected());
        };
        let pending = version.clone();
        let running_version = running_version.into();
        if compare_build_order(&running_version, &pending) != Ordering::Less {
            self.running_version = running_version.clone();
            return Ok(self.enter(UpdateState::Running {
                version: running_version,
            }));
        }
        self.running_version = running_version.clone();
        Ok(self.enter(UpdateState::Failed {
            reason: format!(
                "restart still runs {running_version}; the update to {pending} did not take effect"
            ),
            target: Some(pending),
            // The apply already consumed the payload; recovering starts over.
            staged: false,
        }))
    }

    /// Restores the offer a failure interrupted, without a fresh discovery
    /// round: a download, an apply, or a re-check that failed after discovery
    /// kept its target, so the same version becomes actionable again instead of
    /// vanishing until the next check. A check that failed before finding
    /// anything has nothing to restore and is refused —
    /// [`Self::start_check`] is its retry. Only a
    /// [`UpdateCapability::SelfApply`] install can reach a failure that holds a
    /// target: the download, apply and restart steps are its own, and a failed
    /// re-check restores the standing offer directly rather than failing.
    pub fn retry_failed_update(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        let UpdateState::Failed {
            target: Some(version),
            staged,
            ..
        } = &self.state
        else {
            return Err(self.rejected());
        };
        let version = version.clone();
        // A verified payload is not thrown away because applying it failed
        // once: the retry re-applies rather than re-downloading.
        let restored = if *staged {
            UpdateState::ReadyToApply { version }
        } else {
            UpdateState::Available { version }
        };
        Ok(self.enter(restored))
    }

    /// Everything the surfaces may show for the current state. The only place
    /// update text is produced.
    pub fn describe(&self) -> UpdatePresentation {
        let capability = self.capability();
        let target_version = self.state.target_version().map(str::to_owned);
        let claim = self.version_claim();
        let updates_message = match &self.notice {
            Some(notice) => format!("{} {notice}", self.updates_message(&claim)),
            None => self.updates_message(&claim),
        };
        let about_message = self.about_message(&claim);
        UpdatePresentation {
            install_kind: self.install_kind,
            capability,
            version_in_use: self.running_version.clone(),
            target_version,
            claim,
            updates_message,
            about_message,
            action: self.action(),
            action_closes_the_app: self.action_closes_the_app(),
            secondary_action: self.secondary_action(),
            notice: self.notice.clone(),
        }
    }

    fn version_claim(&self) -> VersionClaim {
        if self.capability() == UpdateCapability::Unmanaged {
            return VersionClaim::NotApplicable;
        }
        match &self.state {
            UpdateState::UpToDate | UpdateState::Running { .. } => VersionClaim::Current,
            UpdateState::Available { version }
            | UpdateState::AvailableExternally { version, .. }
            | UpdateState::Skipped { version, .. }
            | UpdateState::Downloading { version }
            | UpdateState::ReadyToApply { version }
            | UpdateState::Applying { version }
            // The applied bits are on disk, but this process is still the old
            // binary: superseded, not current (#660).
            | UpdateState::RestartPending { version } => VersionClaim::Superseded {
                newer: version.clone(),
            },
            // A failure that kept its target still knows a newer build exists
            // and that this process is not running it.
            UpdateState::Failed {
                target: Some(version),
                ..
            } => VersionClaim::Superseded {
                newer: version.clone(),
            },
            // A refresh does not un-know the offer it is refreshing.
            UpdateState::Checking {
                carried: Some(offer),
            } => VersionClaim::Superseded {
                newer: offer.version().to_owned(),
            },
            UpdateState::Idle
            | UpdateState::Checking { carried: None }
            | UpdateState::ManagedExternally { .. }
            | UpdateState::Failed { target: None, .. } => VersionClaim::Unknown,
        }
    }

    fn updates_message(&self, claim: &VersionClaim) -> String {
        if matches!(claim, VersionClaim::NotApplicable) {
            return "Updates are disabled for development builds.".to_owned();
        }
        match &self.state {
            UpdateState::Idle => "OK Player has not checked for updates yet.".to_owned(),
            UpdateState::Checking { .. } => "Checking for updates…".to_owned(),
            UpdateState::UpToDate => "OK Player is up to date.".to_owned(),
            UpdateState::Available { version } => format!("Version {version} is available."),
            UpdateState::AvailableExternally { version, hint } => {
                format!("Version {version} is available. {hint}")
            }
            UpdateState::Downloading { version } => format!("Downloading version {version}…"),
            UpdateState::ReadyToApply { version } => {
                format!("Version {version} is ready to install.")
            }
            UpdateState::Applying { version } => format!("Installing version {version}…"),
            UpdateState::RestartPending { version } => format!(
                "Version {version} is installed. Restart OK Player to start running it — this session is still on {}.",
                self.running_version
            ),
            UpdateState::Running { version } => {
                format!("OK Player is now running version {version}.")
            }
            UpdateState::Skipped {
                version,
                hint: Some(hint),
                ..
            } => format!("Version {version} was skipped. {hint}"),
            UpdateState::Skipped { version, .. } => {
                format!("Version {version} was skipped.")
            }
            UpdateState::ManagedExternally { hint } => hint.clone(),
            UpdateState::Failed {
                reason,
                target: Some(version),
                ..
            } => format!("The update to version {version} failed: {reason}"),
            UpdateState::Failed {
                reason,
                target: None,
                ..
            } => format!("Update failed: {reason}"),
        }
    }

    fn about_message(&self, claim: &VersionClaim) -> String {
        let running = &self.running_version;
        match claim {
            VersionClaim::Current => format!("OK Player {running} — up to date."),
            VersionClaim::Superseded { newer } => {
                if matches!(self.state, UpdateState::RestartPending { .. }) {
                    format!("OK Player {running} — restart to finish updating to {newer}.")
                } else if matches!(self.state, UpdateState::Failed { .. }) {
                    format!("OK Player {running} — updating to {newer} failed.")
                } else if matches!(self.state, UpdateState::Skipped { .. }) {
                    format!("OK Player {running} — version {newer} was skipped.")
                } else {
                    format!("OK Player {running} — version {newer} is available.")
                }
            }
            VersionClaim::Unknown => match &self.state {
                UpdateState::ManagedExternally { hint } => format!("OK Player {running} — {hint}"),
                _ => format!("OK Player {running}."),
            },
            VersionClaim::NotApplicable => {
                format!("OK Player {running} — development build; updates are disabled.")
            }
        }
    }

    fn action(&self) -> Option<UpdateAction> {
        if self.capability() == UpdateCapability::Unmanaged {
            return None;
        }
        match self.state {
            UpdateState::Idle | UpdateState::UpToDate | UpdateState::Running { .. } => {
                Some(UpdateAction::CheckNow)
            }
            UpdateState::Available { .. } => Some(UpdateAction::DownloadUpdate),
            UpdateState::ReadyToApply { .. } => Some(UpdateAction::ApplyAndRestart),
            UpdateState::RestartPending { .. } => Some(UpdateAction::RestartToFinish),
            UpdateState::Failed { .. } => Some(UpdateAction::Retry),
            // A skipped version stays installable on demand, but only where the
            // app installs anything at all.
            UpdateState::Skipped { .. } => match self.capability() {
                UpdateCapability::SelfApply => Some(UpdateAction::InstallAnyway),
                UpdateCapability::SystemManaged | UpdateCapability::Unmanaged => None,
            },
            // A system-managed update is announced, never actioned in-app; an
            // install the system owns outright offers not even a check; a
            // check, download or apply in flight offers nothing.
            UpdateState::AvailableExternally { .. }
            | UpdateState::ManagedExternally { .. }
            | UpdateState::Checking { .. }
            | UpdateState::Downloading { .. }
            | UpdateState::Applying { .. } => None,
        }
    }

    /// Whether the offered action ends the session. An action that always
    /// restarts does; so does accepting an offer on a lane that applies while
    /// it downloads.
    fn action_closes_the_app(&self) -> bool {
        let Some(action) = self.action() else {
            return false;
        };
        if action.closes_the_app() {
            return true;
        }
        matches!(
            action,
            UpdateAction::DownloadUpdate | UpdateAction::InstallAnyway
        ) && self.install_kind.applies_while_downloading()
    }

    /// The secondary action beside [`Self::action`]. A live offer — discovered,
    /// or discovered and then failed — can be skipped; nothing else can.
    fn secondary_action(&self) -> Option<UpdateAction> {
        if self.capability() == UpdateCapability::Unmanaged {
            return None;
        }
        match &self.state {
            UpdateState::Available { .. }
            | UpdateState::AvailableExternally { .. }
            | UpdateState::Failed {
                target: Some(_), ..
            } => Some(UpdateAction::SkipVersion),
            _ => None,
        }
    }

    fn enter(&mut self, next: UpdateState) -> &UpdateState {
        self.state = next;
        self.notice = None;
        &self.state
    }

    fn enter_with_notice(&mut self, next: UpdateState, notice: String) -> &UpdateState {
        self.state = next;
        self.notice = Some(notice);
        &self.state
    }

    fn rejected(&self) -> UpdateTransitionError {
        UpdateTransitionError::NotAllowedFrom(self.state.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transition attempt applied to a lifecycle, used to drive the machine
    /// generically in the sweep-based invariant tests.
    type Drive = Box<dyn Fn(&mut UpdateLifecycle)>;
    /// A transition attempt whose refusal the caller inspects.
    type Attempt = Box<dyn Fn(&mut UpdateLifecycle) -> Result<(), UpdateTransitionError>>;

    const SELF_APPLY_KINDS: [InstallKind; 2] =
        [InstallKind::WindowsVelopack, InstallKind::AppImage];
    const SYSTEM_MANAGED_KINDS: [InstallKind; 3] =
        [InstallKind::Deb, InstallKind::Rpm, InstallKind::Flatpak];

    fn evidence() -> InstallEvidence {
        InstallEvidence::default()
    }

    fn checking(carried: Option<CarriedOffer>) -> UpdateState {
        UpdateState::Checking { carried }
    }

    fn carried_available(version: &str) -> Option<CarriedOffer> {
        Some(CarriedOffer::Available {
            version: version.to_owned(),
        })
    }

    /// Drives a lifecycle as far as the install kind allows, calling `observe`
    /// after every attempted transition so an invariant can be checked over the
    /// whole reachable state space instead of one hand-picked state.
    fn sweep_reachable_states(kind: InstallKind, mut observe: impl FnMut(&UpdateLifecycle)) {
        let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
        observe(&lifecycle);

        let attempts: Vec<Drive> = vec![
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_check();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.check_found("2.0.0");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_download();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.download_finished();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_apply();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.apply_needs_restart();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.restarted_into("2.0.0");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_check();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.check_found_none();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_check();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.check_failed("network unreachable");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.retry_failed_update();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_check();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.check_found("3.0.0");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.skip_offer();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.install_anyway();
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.download_failed("checksum mismatch");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.retry_failed_update();
            }),
        ];
        for attempt in attempts {
            attempt(&mut lifecycle);
            observe(&lifecycle);
        }
    }

    // ---------------------------------------------------------------- detection

    #[test]
    fn install_kind_detection_is_table_driven_over_evidence() {
        let cases: Vec<(&str, InstallEvidence, InstallKind)> = vec![
            (
                "flatpak via FLATPAK_ID",
                InstallEvidence {
                    flatpak_id: Some("uk.oklabs.OkPlayer".to_owned()),
                    executable_path: Some("/app/bin/ok-player".to_owned()),
                    ..evidence()
                },
                InstallKind::Flatpak,
            ),
            (
                "flatpak via sandbox marker only",
                InstallEvidence {
                    flatpak_info_present: true,
                    executable_path: Some("/app/bin/ok-player".to_owned()),
                    ..evidence()
                },
                InstallKind::Flatpak,
            ),
            (
                "flatpak sandbox wins over a runtime dpkg answer",
                InstallEvidence {
                    flatpak_info_present: true,
                    executable_path: Some("/app/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Dpkg,
                    ..evidence()
                },
                InstallKind::Flatpak,
            ),
            (
                "empty FLATPAK_ID is not a flatpak",
                InstallEvidence {
                    flatpak_id: Some(String::new()),
                    executable_path: Some("/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Dpkg,
                    ..evidence()
                },
                InstallKind::Deb,
            ),
            (
                "appimage parked in /tmp with nobody owning the path",
                InstallEvidence {
                    appimage_path: Some("/tmp/OK_Player-x86_64.AppImage".to_owned()),
                    executable_path: Some("/tmp/.mount_OKPlay1/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Unowned,
                    ..evidence()
                },
                InstallKind::AppImage,
            ),
            (
                "an inherited APPIMAGE loses to the executable's own package",
                InstallEvidence {
                    appimage_path: Some("/opt/launcher/Launcher-x86_64.AppImage".to_owned()),
                    executable_path: Some("/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Dpkg,
                    ..evidence()
                },
                InstallKind::Deb,
            ),
            (
                "extract-and-run AppImage has no mount path to corroborate it",
                InstallEvidence {
                    appimage_path: Some("/home/u/OK_Player-x86_64.AppImage".to_owned()),
                    executable_path: Some(
                        "/tmp/appimage_extracted_9f1/usr/bin/ok-player".to_owned(),
                    ),
                    package_ownership: PackageOwnership::Unowned,
                    ..evidence()
                },
                InstallKind::AppImage,
            ),
            (
                "an inherited APPIMAGE is not trusted when ownership is unknown",
                InstallEvidence {
                    appimage_path: Some("/opt/launcher/Launcher-x86_64.AppImage".to_owned()),
                    executable_path: Some("/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Unknown,
                    ..evidence()
                },
                InstallKind::DevBuild,
            ),
            (
                "appimage mount path without APPIMAGE set",
                InstallEvidence {
                    executable_path: Some("/tmp/.mount_OKPlayXy/usr/bin/ok-player".to_owned()),
                    ..evidence()
                },
                InstallKind::AppImage,
            ),
            (
                "deb running from /usr",
                InstallEvidence {
                    executable_path: Some("/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Dpkg,
                    ..evidence()
                },
                InstallKind::Deb,
            ),
            (
                "rpm running from /usr",
                InstallEvidence {
                    executable_path: Some("/usr/bin/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Rpm,
                    ..evidence()
                },
                InstallKind::Rpm,
            ),
            (
                "velopack layout on windows",
                InstallEvidence {
                    executable_path: Some(
                        r"C:\Users\u\AppData\Local\OkPlayer\current\OkPlayer.exe".to_owned(),
                    ),
                    velopack_layout_present: true,
                    ..evidence()
                },
                InstallKind::WindowsVelopack,
            ),
            (
                "unowned build tree is a dev build",
                InstallEvidence {
                    executable_path: Some("/home/dev/ok-player/target/debug/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Unowned,
                    ..evidence()
                },
                InstallKind::DevBuild,
            ),
            (
                "unanswerable ownership is a dev build, not a package",
                InstallEvidence {
                    executable_path: Some("/opt/ok-player/ok-player".to_owned()),
                    package_ownership: PackageOwnership::Unknown,
                    ..evidence()
                },
                InstallKind::DevBuild,
            ),
            (
                "no evidence at all is a dev build",
                evidence(),
                InstallKind::DevBuild,
            ),
        ];

        for (name, given, expected) in cases {
            assert_eq!(detect_install_kind(&given), expected, "case: {name}");
        }
    }

    #[test]
    fn capabilities_follow_the_install_kind() {
        for kind in SELF_APPLY_KINDS {
            assert_eq!(kind.capability(), UpdateCapability::SelfApply, "{kind}");
        }
        for kind in SYSTEM_MANAGED_KINDS {
            assert_eq!(kind.capability(), UpdateCapability::SystemManaged, "{kind}");
        }
        assert_eq!(
            InstallKind::DevBuild.capability(),
            UpdateCapability::Unmanaged
        );
    }

    // --------------------------------------------------------- happy lifecycle

    #[test]
    fn self_apply_walks_from_check_to_running_the_new_version() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");

        assert_eq!(lifecycle.start_check().unwrap(), &checking(None));
        assert_eq!(
            lifecycle.check_found("2.0.0").unwrap(),
            &UpdateState::Available {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(
            lifecycle.start_download().unwrap(),
            &UpdateState::Downloading {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(
            lifecycle.download_finished().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(
            lifecycle.start_apply().unwrap(),
            &UpdateState::Applying {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(
            lifecycle.apply_needs_restart().unwrap(),
            &UpdateState::RestartPending {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(lifecycle.running_version(), "1.0.0");

        assert_eq!(
            lifecycle.restarted_into("2.0.0").unwrap(),
            &UpdateState::Running {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(lifecycle.running_version(), "2.0.0");
        assert_eq!(lifecycle.describe().claim, VersionClaim::Current);
    }

    #[test]
    fn a_check_that_finds_nothing_reports_up_to_date() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::WindowsVelopack, "1.0.0");

        lifecycle.start_check().unwrap();
        assert_eq!(
            lifecycle.check_found_none().unwrap(),
            &UpdateState::UpToDate
        );

        let presentation = lifecycle.describe();
        assert_eq!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.target_version, None);
        assert_eq!(presentation.action, Some(UpdateAction::CheckNow));
    }

    #[test]
    fn every_fallible_step_can_reach_failed_and_retry_from_there() {
        for (name, drive) in [
            (
                "check",
                Box::new(|life: &mut UpdateLifecycle| {
                    life.start_check().unwrap();
                    life.check_failed("network unreachable").unwrap();
                }) as Drive,
            ),
            (
                "download",
                Box::new(|life: &mut UpdateLifecycle| {
                    life.start_check().unwrap();
                    life.check_found("2.0.0").unwrap();
                    life.start_download().unwrap();
                    life.download_failed("checksum mismatch").unwrap();
                }),
            ),
            (
                "apply",
                Box::new(|life: &mut UpdateLifecycle| {
                    life.start_check().unwrap();
                    life.check_found("2.0.0").unwrap();
                    life.start_download().unwrap();
                    life.download_finished().unwrap();
                    life.start_apply().unwrap();
                    life.apply_failed("permission denied").unwrap();
                }),
            ),
        ] {
            let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            drive(&mut lifecycle);
            assert!(
                matches!(lifecycle.state(), UpdateState::Failed { .. }),
                "{name} should be able to fail, got {:?}",
                lifecycle.state()
            );
            assert_eq!(
                lifecycle.describe().action,
                Some(UpdateAction::Retry),
                "{name} failure should be retryable"
            );
            assert!(
                matches!(
                    lifecycle.start_check().unwrap(),
                    UpdateState::Checking { .. }
                ),
                "{name} failure should allow another check"
            );
        }
    }

    // ------------------------------------------------------------- invariants

    /// Invariant: `RestartPending` must never render as "you are on <new
    /// version>" — the #660 regression.
    #[test]
    fn restart_pending_never_claims_the_running_build_is_the_new_version() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_needs_restart().unwrap();

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.claim,
            VersionClaim::Superseded {
                newer: "2.0.0".to_owned()
            }
        );
        assert_ne!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.version_in_use, "1.0.0");
        assert_eq!(lifecycle.running_version(), "1.0.0");
        assert_eq!(presentation.action, Some(UpdateAction::RestartToFinish));
    }

    /// Invariant: the restart is what makes the new version real. A restart
    /// that comes back on the old binary is a failure, not a success.
    #[test]
    fn restart_that_still_runs_the_old_binary_is_a_failure() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_needs_restart().unwrap();

        lifecycle.restarted_into("1.0.0").unwrap();

        assert!(
            matches!(lifecycle.state(), UpdateState::Failed { .. }),
            "a restart onto the old binary must fail, got {:?}",
            lifecycle.state()
        );
        let presentation = lifecycle.describe();
        assert_ne!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.version_in_use, "1.0.0");
    }

    /// Invariant: `Applying` cannot succeed without moving to `RestartPending`
    /// or `Running(new)`. Every other transition attempted from `Applying` is
    /// refused and leaves the state untouched.
    #[test]
    fn applying_can_only_succeed_into_restart_pending_or_running() {
        fn applying() -> UpdateLifecycle {
            let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            lifecycle.start_check().unwrap();
            lifecycle.check_found("2.0.0").unwrap();
            lifecycle.start_download().unwrap();
            lifecycle.download_finished().unwrap();
            lifecycle.start_apply().unwrap();
            lifecycle
        }

        let refusals: Vec<(&str, Attempt)> = vec![
            (
                "start_check",
                Box::new(|life: &mut UpdateLifecycle| life.start_check().map(|_| ())),
            ),
            (
                "check_found_none",
                Box::new(|life: &mut UpdateLifecycle| life.check_found_none().map(|_| ())),
            ),
            (
                "check_found",
                Box::new(|life: &mut UpdateLifecycle| life.check_found("2.0.0").map(|_| ())),
            ),
            (
                "check_failed",
                Box::new(|life: &mut UpdateLifecycle| life.check_failed("nope").map(|_| ())),
            ),
            (
                "start_download",
                Box::new(|life: &mut UpdateLifecycle| life.start_download().map(|_| ())),
            ),
            (
                "download_finished",
                Box::new(|life: &mut UpdateLifecycle| life.download_finished().map(|_| ())),
            ),
            (
                "download_failed",
                Box::new(|life: &mut UpdateLifecycle| life.download_failed("nope").map(|_| ())),
            ),
            (
                "start_apply",
                Box::new(|life: &mut UpdateLifecycle| life.start_apply().map(|_| ())),
            ),
            (
                "restarted_into",
                Box::new(|life: &mut UpdateLifecycle| life.restarted_into("2.0.0").map(|_| ())),
            ),
        ];

        let applying_state = UpdateState::Applying {
            version: "2.0.0".to_owned(),
        };
        for (name, attempt) in refusals {
            let mut lifecycle = applying();
            let result = attempt(&mut lifecycle);
            assert!(
                result.is_err(),
                "{name} must not be a way out of Applying, it produced {result:?}"
            );
            assert_eq!(
                lifecycle.state(),
                &applying_state,
                "{name} must leave Applying untouched"
            );
        }

        let mut restarting = applying();
        assert_eq!(
            restarting.apply_needs_restart().unwrap(),
            &UpdateState::RestartPending {
                version: "2.0.0".to_owned()
            }
        );

        let mut in_place = applying();
        assert_eq!(
            in_place.apply_finished_running().unwrap(),
            &UpdateState::Running {
                version: "2.0.0".to_owned()
            }
        );

        let mut failing = applying();
        assert_eq!(
            failing.apply_failed("disk full").unwrap(),
            &UpdateState::Failed {
                reason: "disk full".to_owned(),
                target: Some("2.0.0".to_owned()),
                staged: true,
            }
        );
    }

    /// Invariant: a `SystemManaged` install never offers an in-app "Update
    /// now"; it announces the update and hands the user to their package
    /// manager.
    #[test]
    fn system_managed_never_offers_an_in_app_update_action() {
        for kind in SYSTEM_MANAGED_KINDS {
            let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
            lifecycle.start_check().unwrap();
            let state = lifecycle.check_found("2.0.0").unwrap().clone();
            assert!(
                matches!(state, UpdateState::AvailableExternally { .. }),
                "{kind} should stop at AvailableExternally, got {state:?}"
            );

            let presentation = lifecycle.describe();
            assert_eq!(
                presentation.action, None,
                "{kind} must offer no in-app apply"
            );
            assert!(
                presentation.updates_message.contains("2.0.0"),
                "{kind} should still announce the version: {}",
                presentation.updates_message
            );

            // Downloading is refused by the capability itself, not merely by
            // the state it would have to start from.
            let mut probe = lifecycle.clone();
            assert_eq!(
                probe.start_download(),
                Err(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} must refuse an in-app download on capability grounds"
            );
            let mut probe = lifecycle.clone();
            assert!(
                probe.download_finished().is_err(),
                "{kind} must refuse an in-app download completion"
            );
            let mut probe = lifecycle.clone();
            assert!(
                probe.start_apply().is_err(),
                "{kind} must refuse an in-app apply"
            );
            let mut probe = lifecycle.clone();
            assert!(
                probe.apply_needs_restart().is_err(),
                "{kind} must refuse an in-app restart handoff"
            );

            let mut failed_recheck = UpdateLifecycle::new(kind, "1.0.0");
            failed_recheck.start_check().unwrap();
            failed_recheck.check_found("2.0.0").unwrap();
            failed_recheck.start_check().unwrap();
            failed_recheck.check_failed("feed unavailable").unwrap();
            assert!(
                matches!(
                    failed_recheck.state(),
                    UpdateState::AvailableExternally { .. }
                ),
                "{kind} must come back as an announcement, never an in-app offer"
            );
            assert_eq!(failed_recheck.describe().action, None);

            let mut failed_check = UpdateLifecycle::new(kind, "1.0.0");
            failed_check.start_check().unwrap();
            failed_check.check_failed("network unreachable").unwrap();
            assert_eq!(
                failed_check.retry_failed_update(),
                Err(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} resumes no in-app offer of its own"
            );

            let mut restored = UpdateLifecycle::new(kind, "1.0.0");
            restored.start_check().unwrap();
            restored.check_found("2.0.0").unwrap();
            restored.start_check().unwrap();
            restored.check_failed("feed unavailable").unwrap();
            restored.skip_offer().unwrap();
            assert!(
                matches!(restored.state(), UpdateState::Skipped { .. }),
                "{kind} can still skip a restored announcement"
            );
            assert_eq!(restored.describe().action, None);

            let mut skipped = UpdateLifecycle::new(kind, "1.0.0");
            skipped.start_check().unwrap();
            skipped.check_found("2.0.0").unwrap();
            skipped.skip_offer().unwrap();
            assert_eq!(
                skipped.describe().action,
                None,
                "{kind} cannot install a skipped version in app"
            );
            assert_eq!(
                skipped.install_anyway(),
                Err(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                ))
            );

            sweep_reachable_states(kind, |life| {
                let action = life.describe().action;
                assert!(
                    !action.is_some_and(UpdateAction::applies_update_in_app),
                    "{kind} offered {action:?} in state {:?}",
                    life.state()
                );
            });
        }
    }

    #[test]
    fn system_managed_hint_names_the_packaging_tool() {
        for (kind, expected) in [
            (InstallKind::Deb, "apt"),
            (InstallKind::Rpm, "dnf"),
            (InstallKind::Flatpak, "flatpak"),
        ] {
            let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
            lifecycle.start_check().unwrap();
            lifecycle.check_found("2.0.0").unwrap();
            let UpdateState::AvailableExternally { hint, .. } = lifecycle.state() else {
                panic!("{kind} should stop at AvailableExternally");
            };
            assert!(
                hint.to_ascii_lowercase().contains(expected),
                "{kind} hint should name {expected}, got {hint}"
            );
            assert!(
                lifecycle.describe().updates_message.contains(hint.as_str()),
                "{kind} should surface its hint"
            );
        }
    }

    /// Invariant: an `Unmanaged` (dev) build reports updates as disabled, never
    /// as up to date.
    #[test]
    fn dev_build_reports_updates_disabled_not_up_to_date() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::DevBuild, "0.0.0-dev");

        assert_eq!(
            lifecycle.start_check(),
            Err(UpdateTransitionError::CapabilityForbids(
                UpdateCapability::Unmanaged
            ))
        );
        assert_eq!(lifecycle.state(), &UpdateState::Idle);

        let presentation = lifecycle.describe();
        assert_eq!(presentation.claim, VersionClaim::NotApplicable);
        assert_ne!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.action, None);

        let up_to_date = UpdateLifecycle::new(InstallKind::AppImage, "0.0.0-dev");
        let mut checked = up_to_date;
        checked.start_check().unwrap();
        checked.check_found_none().unwrap();
        assert_ne!(
            presentation.updates_message,
            checked.describe().updates_message,
            "a dev build must not read as an up-to-date install"
        );
        assert_ne!(presentation.about_message, checked.describe().about_message);
        assert!(
            presentation
                .updates_message
                .to_lowercase()
                .contains("disabled"),
            "a dev build must say updates are disabled, got {}",
            presentation.updates_message
        );
        assert!(
            presentation
                .about_message
                .to_lowercase()
                .contains("disabled"),
            "About must say the same, got {}",
            presentation.about_message
        );

        sweep_reachable_states(InstallKind::DevBuild, |life| {
            assert_eq!(
                life.state(),
                &UpdateState::Idle,
                "an unmanaged install has no lifecycle to walk"
            );
            assert_eq!(life.describe().claim, VersionClaim::NotApplicable);
        });
    }

    /// Invariant: the About surface and the Updates surface read one state, so
    /// they cannot disagree about which version is running.
    #[test]
    fn about_and_updates_surfaces_agree_on_one_state() {
        for kind in [
            InstallKind::WindowsVelopack,
            InstallKind::AppImage,
            InstallKind::Deb,
            InstallKind::Rpm,
            InstallKind::Flatpak,
            InstallKind::DevBuild,
        ] {
            sweep_reachable_states(kind, |life| {
                let presentation = life.describe();
                assert_eq!(
                    presentation.version_in_use,
                    life.running_version(),
                    "{kind} About must report the executing build in {:?}",
                    life.state()
                );
                assert!(
                    presentation
                        .about_message
                        .contains(&presentation.version_in_use),
                    "{kind} About message must name the executing build, got {}",
                    presentation.about_message
                );
                match &presentation.claim {
                    VersionClaim::Current => {}
                    _ => assert!(
                        !presentation.about_message.contains("up to date"),
                        "{kind} About claimed up to date in {:?}: {}",
                        life.state(),
                        presentation.about_message
                    ),
                }
                if let VersionClaim::Superseded { newer } = &presentation.claim {
                    assert_ne!(
                        &presentation.version_in_use, newer,
                        "{kind} cannot be superseded by the build it is running"
                    );
                    assert!(
                        presentation.about_message.contains(newer),
                        "{kind} About must name the newer build in {:?}",
                        life.state()
                    );
                    assert_eq!(presentation.target_version.as_deref(), Some(newer.as_str()));
                }
            });
        }
    }

    /// Invariant: the restart check survives the process boundary. A self-apply
    /// replaces the process, so the lifecycle that reached `RestartPending` is
    /// gone by the time the new binary starts; the pending target is handed to
    /// the new process, which settles the same comparison.
    #[test]
    fn a_resumed_process_settles_the_restart_against_the_pending_target() {
        let landed =
            UpdateLifecycle::resumed_after_restart(InstallKind::WindowsVelopack, "2.0.0", "2.0.0")
                .expect("a self-applying install can resume after a restart");
        assert_eq!(
            landed.state(),
            &UpdateState::Running {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(landed.running_version(), "2.0.0");
        let presentation = landed.describe();
        assert_eq!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.version_in_use, "2.0.0");

        let stalled =
            UpdateLifecycle::resumed_after_restart(InstallKind::AppImage, "1.0.0", "2.0.0")
                .expect("a self-applying install can resume after a restart");
        assert!(
            matches!(stalled.state(), UpdateState::Failed { .. }),
            "coming back on the old binary must fail, got {:?}",
            stalled.state()
        );
        let presentation = stalled.describe();
        assert_ne!(presentation.claim, VersionClaim::Current);
        assert_eq!(presentation.version_in_use, "1.0.0");
        assert_eq!(presentation.action, Some(UpdateAction::Retry));
    }

    #[test]
    fn only_a_self_applying_install_can_resume_a_pending_restart() {
        for kind in SYSTEM_MANAGED_KINDS {
            assert_eq!(
                UpdateLifecycle::resumed_after_restart(kind, "1.0.0", "2.0.0").err(),
                Some(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} never stages a restart of its own"
            );
        }
        assert_eq!(
            UpdateLifecycle::resumed_after_restart(InstallKind::DevBuild, "1.0.0", "2.0.0").err(),
            Some(UpdateTransitionError::CapabilityForbids(
                UpdateCapability::Unmanaged
            ))
        );
    }

    /// Invariant: an install a system tool owns outright reports who updates
    /// it and offers nothing — not even the check an `Idle` surface offers.
    #[test]
    fn a_system_owned_install_reports_its_manager_instead_of_offering_a_check() {
        for (kind, tool) in [(InstallKind::Rpm, "dnf"), (InstallKind::Flatpak, "flatpak")] {
            let mut lifecycle = UpdateLifecycle::managed_externally(kind, "1.0.0")
                .expect("a system-managed install can be owned outright");

            let presentation = lifecycle.describe();
            assert_eq!(
                presentation.action, None,
                "{kind} must not offer a check it never runs"
            );
            assert!(
                presentation.updates_message.to_lowercase().contains(tool),
                "{kind} should name its update tool, got {}",
                presentation.updates_message
            );
            assert!(
                presentation.about_message.to_lowercase().contains(tool),
                "{kind} About should say the same, got {}",
                presentation.about_message
            );
            assert_ne!(presentation.claim, VersionClaim::Current);
            assert_eq!(presentation.target_version, None);

            assert!(
                lifecycle.start_check().is_err(),
                "{kind} must refuse a check while the system owns updates"
            );

            let idle = UpdateLifecycle::new(kind, "1.0.0");
            assert_eq!(
                idle.describe().action,
                Some(UpdateAction::CheckNow),
                "{kind} that does discover versions still offers a check"
            );
            assert_ne!(
                presentation.updates_message,
                idle.describe().updates_message,
                "{kind} owned outright must not read like an install that has simply not checked yet"
            );
        }

        for kind in SELF_APPLY_KINDS {
            assert_eq!(
                UpdateLifecycle::managed_externally(kind, "1.0.0").err(),
                Some(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SelfApply
                )),
                "{kind} applies its own updates"
            );
        }
        assert_eq!(
            UpdateLifecycle::managed_externally(InstallKind::DevBuild, "1.0.0").err(),
            Some(UpdateTransitionError::CapabilityForbids(
                UpdateCapability::Unmanaged
            ))
        );

        // The `.deb` lane is system-managed but discovers versions itself, so
        // it must keep its check rather than be declared system-owned.
        assert_eq!(
            UpdateLifecycle::managed_externally(InstallKind::Deb, "1.0.0").err(),
            Some(UpdateTransitionError::NotThisLane(InstallKind::Deb)),
            "the deb lane polls its own feed"
        );
        let deb = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        assert_eq!(deb.describe().action, Some(UpdateAction::CheckNow));
    }

    /// Invariant: a failure after discovery keeps the offer. The version stays
    /// known and the same update can be retried without waiting for another
    /// check to rediscover it.
    #[test]
    fn a_failure_after_discovery_keeps_its_target_retryable() {
        for (name, fail) in [
            (
                "download",
                Box::new(|life: &mut UpdateLifecycle| {
                    life.start_download().unwrap();
                    life.download_failed("checksum mismatch").unwrap();
                }) as Drive,
            ),
            (
                "apply",
                Box::new(|life: &mut UpdateLifecycle| {
                    life.start_download().unwrap();
                    life.download_finished().unwrap();
                    life.start_apply().unwrap();
                    life.apply_failed("permission denied").unwrap();
                }),
            ),
        ] {
            let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            lifecycle.start_check().unwrap();
            lifecycle.check_found("2.0.0").unwrap();
            fail(&mut lifecycle);

            assert_eq!(
                lifecycle.state().target_version(),
                Some("2.0.0"),
                "{name} failure must keep the discovered version"
            );
            let presentation = lifecycle.describe();
            assert_eq!(presentation.target_version.as_deref(), Some("2.0.0"));
            assert_eq!(
                presentation.claim,
                VersionClaim::Superseded {
                    newer: "2.0.0".to_owned()
                },
                "{name} failure still knows the running build is not the newest"
            );
            assert_eq!(presentation.action, Some(UpdateAction::Retry));

            let resumed = lifecycle.retry_failed_update().unwrap().clone();
            assert!(
                matches!(
                    resumed,
                    UpdateState::Available { .. } | UpdateState::ReadyToApply { .. }
                ),
                "{name} failure must be retryable without a fresh check, got {resumed:?}"
            );
        }
    }

    #[test]
    fn a_check_that_failed_before_finding_anything_has_no_offer_to_retry() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_failed("network unreachable").unwrap();

        assert_eq!(lifecycle.state().target_version(), None);
        assert_eq!(lifecycle.describe().claim, VersionClaim::Unknown);
        assert_eq!(
            lifecycle.retry_failed_update(),
            Err(UpdateTransitionError::NotAllowedFrom(UpdateState::Failed {
                reason: "network unreachable".to_owned(),
                target: None,
                staged: false,
            }))
        );
        assert_eq!(lifecycle.start_check().unwrap(), &checking(None));
    }

    /// Invariant: a re-check never destroys the offer it is refreshing. The
    /// shell's `Checking(Some(previous))` behaviour, in the model.
    #[test]
    fn a_failed_recheck_restores_the_offer_it_was_refreshing() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();

        assert_eq!(
            lifecycle.start_check().unwrap(),
            &checking(carried_available("2.0.0")),
            "a refresh must carry the offer it is refreshing"
        );
        assert_eq!(
            lifecycle.describe().claim,
            VersionClaim::Superseded {
                newer: "2.0.0".to_owned()
            },
            "rechecking does not un-know the discovered version"
        );

        lifecycle.check_failed("feed unavailable").unwrap();
        assert_eq!(
            lifecycle.state(),
            &UpdateState::Available {
                version: "2.0.0".to_owned()
            },
            "the previous offer must come back intact, not demoted to a failure"
        );

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.action,
            Some(UpdateAction::DownloadUpdate),
            "the offer keeps its own action instead of turning into a retry"
        );
        assert_eq!(
            presentation.secondary_action,
            Some(UpdateAction::SkipVersion)
        );
        let notice = presentation
            .notice
            .as_deref()
            .expect("a failed refresh must still say it failed");
        assert!(notice.contains("feed unavailable"), "notice was {notice}");
        assert!(
            presentation.updates_message.contains("feed unavailable"),
            "the one-line message must carry the failure too: {}",
            presentation.updates_message
        );
    }

    #[test]
    fn a_failed_recheck_leaves_a_skipped_offer_skipped() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.skip_offer().unwrap();

        lifecycle.start_check().unwrap();
        lifecycle.check_failed("feed unavailable").unwrap();

        assert_eq!(
            lifecycle.state(),
            &UpdateState::Skipped {
                version: "2.0.0".to_owned(),
                hint: None,
                staged: false,
            },
            "a failed refresh must not un-skip an offer"
        );
        assert_eq!(
            lifecycle.describe().action,
            Some(UpdateAction::InstallAnyway)
        );
    }

    #[test]
    fn a_check_with_no_offer_to_protect_still_fails() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_failed("network unreachable").unwrap();

        assert_eq!(
            lifecycle.state(),
            &UpdateState::Failed {
                reason: "network unreachable".to_owned(),
                target: None,
                staged: false,
            }
        );
        assert_eq!(lifecycle.describe().action, Some(UpdateAction::Retry));
    }

    #[test]
    fn a_notice_does_not_outlive_the_next_successful_transition() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_check().unwrap();
        lifecycle.check_failed("feed unavailable").unwrap();
        assert!(lifecycle.describe().notice.is_some());

        lifecycle.start_download().unwrap();
        assert_eq!(
            lifecycle.describe().notice,
            None,
            "a stale failure must not follow the user into the next step"
        );
    }

    #[test]
    fn a_successful_recheck_that_finds_nothing_clears_the_old_offer() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_check().unwrap();

        assert_eq!(
            lifecycle.check_found_none().unwrap(),
            &UpdateState::UpToDate
        );
        assert_eq!(lifecycle.state().target_version(), None);
        assert_eq!(lifecycle.describe().claim, VersionClaim::Current);
    }

    /// Invariant: the projection can express the whole live offer — the primary
    /// action and the skip beside it — so a shell never needs a second offer
    /// model of its own.
    #[test]
    fn a_live_offer_projects_both_its_primary_action_and_the_skip() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();

        let presentation = lifecycle.describe();
        assert_eq!(presentation.action, Some(UpdateAction::DownloadUpdate));
        assert_eq!(
            presentation.secondary_action,
            Some(UpdateAction::SkipVersion)
        );

        lifecycle.skip_offer().unwrap();
        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.action,
            Some(UpdateAction::InstallAnyway),
            "a skipped version stays installable on demand"
        );
        assert_eq!(
            presentation.secondary_action, None,
            "an already skipped offer has nothing left to skip"
        );
        assert_eq!(presentation.target_version.as_deref(), Some("2.0.0"));
        assert_eq!(
            presentation.claim,
            VersionClaim::Superseded {
                newer: "2.0.0".to_owned()
            },
            "skipping does not make the running build current"
        );

        assert_eq!(
            lifecycle.install_anyway().unwrap(),
            &UpdateState::Downloading {
                version: "2.0.0".to_owned()
            }
        );
    }

    #[test]
    fn a_failed_offer_can_still_be_skipped() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_failed("checksum mismatch").unwrap();

        assert_eq!(
            lifecycle.describe().secondary_action,
            Some(UpdateAction::SkipVersion)
        );
        assert_eq!(
            lifecycle.skip_offer().unwrap(),
            &UpdateState::Skipped {
                version: "2.0.0".to_owned(),
                hint: None,
                staged: false,
            }
        );
    }

    /// Invariant: an update downloaded in an earlier session comes back staged.
    /// The user downloaded it, quit before applying, and must not have to
    /// download it again — least of all offline.
    #[test]
    fn a_staged_update_survives_an_ordinary_relaunch() {
        for kind in SELF_APPLY_KINDS {
            let mut lifecycle = UpdateLifecycle::resumed_with_staged_update(kind, "1.0.0", "2.0.0")
                .expect("a self-applying install can resume a staged payload");

            assert_eq!(
                lifecycle.state(),
                &UpdateState::ReadyToApply {
                    version: "2.0.0".to_owned()
                },
                "{kind} must come back ready to apply, not idle"
            );
            let presentation = lifecycle.describe();
            assert_eq!(presentation.action, Some(UpdateAction::ApplyAndRestart));
            assert_eq!(presentation.version_in_use, "1.0.0");
            assert_eq!(
                presentation.claim,
                VersionClaim::Superseded {
                    newer: "2.0.0".to_owned()
                }
            );

            assert_eq!(
                lifecycle.start_apply().unwrap(),
                &UpdateState::Applying {
                    version: "2.0.0".to_owned()
                },
                "{kind} must apply the staged payload without downloading again"
            );
        }

        for kind in SYSTEM_MANAGED_KINDS {
            assert_eq!(
                UpdateLifecycle::resumed_with_staged_update(kind, "1.0.0", "2.0.0").err(),
                Some(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} stages nothing of its own"
            );
        }
        assert_eq!(
            UpdateLifecycle::resumed_with_staged_update(InstallKind::DevBuild, "1.0.0", "2.0.0")
                .err(),
            Some(UpdateTransitionError::CapabilityForbids(
                UpdateCapability::Unmanaged
            ))
        );
    }

    /// Invariant: an apply that failed keeps the verified payload. `Applying`
    /// is only reachable from `ReadyToApply`, so retrying re-applies what is
    /// already on disk instead of demanding another download — which, offline,
    /// would mean no retry at all.
    #[test]
    fn a_failed_apply_retries_the_staged_payload_instead_of_downloading_again() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_failed("permission denied").unwrap();

        assert_eq!(
            lifecycle.retry_failed_update().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "a staged payload must come back ready to apply"
        );
        assert_eq!(
            lifecycle.describe().action,
            Some(UpdateAction::ApplyAndRestart)
        );
        assert_eq!(
            lifecycle.start_apply().unwrap(),
            &UpdateState::Applying {
                version: "2.0.0".to_owned()
            }
        );
    }

    #[test]
    fn a_failed_download_retries_from_the_download() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_failed("checksum mismatch").unwrap();

        assert_eq!(
            lifecycle.retry_failed_update().unwrap(),
            &UpdateState::Available {
                version: "2.0.0".to_owned()
            },
            "a payload that never landed has nothing staged to re-apply"
        );
        assert_eq!(
            lifecycle.describe().action,
            Some(UpdateAction::DownloadUpdate)
        );
    }

    #[test]
    fn a_failed_refresh_over_a_staged_failure_keeps_the_payload_staged() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_failed("permission denied").unwrap();

        lifecycle.start_check().unwrap();
        lifecycle.check_failed("feed unavailable").unwrap();

        assert_eq!(
            lifecycle.state().target_version(),
            Some("2.0.0"),
            "a failed refresh must not discard a staged payload"
        );
        assert_eq!(
            lifecycle.retry_failed_update().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "and the retry still applies it rather than downloading again"
        );
    }

    /// Invariant: applying a staged update relaunches the app, and the action
    /// says so — both self-applying lanes call apply-and-restart.
    #[test]
    fn applying_a_staged_update_announces_that_it_restarts_the_app() {
        for kind in SELF_APPLY_KINDS {
            let lifecycle = UpdateLifecycle::resumed_with_staged_update(kind, "1.0.0", "2.0.0")
                .expect("a self-applying install can stage a payload");
            let action = lifecycle
                .describe()
                .action
                .expect("a staged payload is actionable");

            assert!(
                action.closes_the_app(),
                "{kind} must warn that applying closes the player, got {action:?}"
            );
            assert!(
                action.label().to_lowercase().contains("restart"),
                "{kind} label must mention the restart, got {}",
                action.label()
            );
        }

        assert!(UpdateAction::RestartToFinish.closes_the_app());
        for action in [
            UpdateAction::CheckNow,
            UpdateAction::DownloadUpdate,
            UpdateAction::Retry,
            UpdateAction::SkipVersion,
        ] {
            assert!(
                !action.closes_the_app(),
                "{action:?} does not close the player"
            );
        }
    }

    /// Invariant: skipping a system-managed offer silences the prompt without
    /// silencing the instructions — the user must still be told how to get that
    /// release from their package manager.
    #[test]
    fn a_skipped_system_managed_offer_still_says_how_to_get_it() {
        for (kind, tool) in [
            (InstallKind::Deb, "apt"),
            (InstallKind::Rpm, "dnf"),
            (InstallKind::Flatpak, "flatpak"),
        ] {
            let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
            lifecycle.start_check().unwrap();
            lifecycle.check_found("2.0.0").unwrap();
            lifecycle.skip_offer().unwrap();

            let presentation = lifecycle.describe();
            assert!(
                presentation.updates_message.to_lowercase().contains(tool),
                "{kind} must keep naming its update tool after a skip, got {}",
                presentation.updates_message
            );
            assert!(
                presentation.updates_message.contains("2.0.0"),
                "{kind} must still name the skipped version"
            );
            assert_eq!(
                presentation.action, None,
                "{kind} still installs nothing in app"
            );
        }

        let mut self_apply = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        self_apply.start_check().unwrap();
        self_apply.check_found("2.0.0").unwrap();
        self_apply.skip_offer().unwrap();
        assert_eq!(
            self_apply.describe().action,
            Some(UpdateAction::InstallAnyway),
            "a self-applying install needs no hint — it has an action"
        );
    }

    /// Invariant: skipping does not throw away a payload that survived a failed
    /// apply. Installing it anyway applies what is on disk.
    #[test]
    fn installing_anyway_applies_a_payload_the_skip_kept() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_failed("permission denied").unwrap();
        lifecycle.skip_offer().unwrap();

        assert_eq!(
            lifecycle.install_anyway().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "a skipped staged payload must be applied, not downloaded again"
        );

        let mut never_downloaded = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        never_downloaded.start_check().unwrap();
        never_downloaded.check_found("2.0.0").unwrap();
        never_downloaded.skip_offer().unwrap();
        assert_eq!(
            never_downloaded.install_anyway().unwrap(),
            &UpdateState::Downloading {
                version: "2.0.0".to_owned()
            },
            "a skip with nothing staged still has to fetch the payload"
        );
    }

    /// Invariant: only coming back *older* is a failed restart. A process that
    /// relaunches on the pending build, or on something newer still, has
    /// finished the update.
    #[test]
    fn a_restart_onto_a_newer_build_than_the_target_still_completes() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_needs_restart().unwrap();

        assert_eq!(
            lifecycle.restarted_into("3.0.0").unwrap(),
            &UpdateState::Running {
                version: "3.0.0".to_owned()
            },
            "a newer build than the target is not a failed restart"
        );
        assert_eq!(
            lifecycle.running_version(),
            "3.0.0",
            "the state must record what is actually running"
        );
        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.claim,
            VersionClaim::Current,
            "3.0.0 cannot be superseded by the 2.0.0 it overtook"
        );
        assert_eq!(presentation.target_version, None);
        assert_eq!(presentation.action, Some(UpdateAction::CheckNow));

        let resumed =
            UpdateLifecycle::resumed_after_restart(InstallKind::WindowsVelopack, "3.0.0", "2.0.0")
                .expect("a self-applying install can resume");
        assert_eq!(
            resumed.state(),
            &UpdateState::Running {
                version: "3.0.0".to_owned()
            },
            "a stale pending marker must not fail a process that is already newer"
        );
        assert_eq!(resumed.describe().claim, VersionClaim::Current);

        let mut older = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        older.start_check().unwrap();
        older.check_found("2.0.0").unwrap();
        older.start_download().unwrap();
        older.download_finished().unwrap();
        older.start_apply().unwrap();
        older.apply_needs_restart().unwrap();
        older.restarted_into("1.0.0").unwrap();
        assert!(
            matches!(older.state(), UpdateState::Failed { .. }),
            "only an older build is a failed restart"
        );
    }

    /// Invariant: a release outranks the prereleases that led to it, so moving
    /// from a beta to the stable build it became is a completed update, not a
    /// failed restart.
    #[test]
    fn a_stable_build_counts_as_newer_than_the_prerelease_it_replaced() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0-beta.1");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("1.0.0-beta.2").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_needs_restart().unwrap();

        assert_eq!(
            lifecycle.restarted_into("1.0.0").unwrap(),
            &UpdateState::Running {
                version: "1.0.0".to_owned()
            },
            "the stable 1.0.0 is not older than the 1.0.0-beta.2 it replaced"
        );
        assert_eq!(lifecycle.describe().claim, VersionClaim::Current);

        // …and the ordering inside a prerelease lane is unchanged.
        let mut lane = UpdateLifecycle::new(InstallKind::AppImage, "0.1.0-alpha.108");
        lane.start_check().unwrap();
        lane.check_found("0.1.0-alpha.109").unwrap();
        lane.start_download().unwrap();
        lane.download_finished().unwrap();
        lane.start_apply().unwrap();
        lane.apply_needs_restart().unwrap();
        lane.restarted_into("0.1.0-alpha.108").unwrap();
        assert!(
            matches!(lane.state(), UpdateState::Failed { .. }),
            "alpha.108 is still older than alpha.109"
        );

        let mut stable_to_pre = UpdateLifecycle::new(InstallKind::AppImage, "0.9.0");
        stable_to_pre.start_check().unwrap();
        stable_to_pre.check_found("1.0.0").unwrap();
        stable_to_pre.start_download().unwrap();
        stable_to_pre.download_finished().unwrap();
        stable_to_pre.start_apply().unwrap();
        stable_to_pre.apply_needs_restart().unwrap();
        stable_to_pre.restarted_into("1.0.0-beta.1").unwrap();
        assert!(
            matches!(stable_to_pre.state(), UpdateState::Failed { .. }),
            "a prerelease of 1.0.0 is not the 1.0.0 that was applied"
        );
    }

    /// Invariant: a surface can always tell whether the action it offers will
    /// close the player — including on the AppImage lane, where accepting the
    /// offer downloads, applies and relaunches in one step.
    #[test]
    fn an_offer_that_applies_while_downloading_says_it_closes_the_app() {
        let mut appimage = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        appimage.start_check().unwrap();
        appimage.check_found("2.0.0").unwrap();
        let presentation = appimage.describe();
        assert_eq!(presentation.action, Some(UpdateAction::DownloadUpdate));
        assert!(
            presentation.action_closes_the_app,
            "accepting an AppImage offer relaunches the player"
        );

        appimage.skip_offer().unwrap();
        let presentation = appimage.describe();
        assert_eq!(presentation.action, Some(UpdateAction::InstallAnyway));
        assert!(
            presentation.action_closes_the_app,
            "installing a skipped AppImage version relaunches it too"
        );

        let mut velopack = UpdateLifecycle::new(InstallKind::WindowsVelopack, "1.0.0");
        velopack.start_check().unwrap();
        velopack.check_found("2.0.0").unwrap();
        let presentation = velopack.describe();
        assert_eq!(presentation.action, Some(UpdateAction::DownloadUpdate));
        assert!(
            !presentation.action_closes_the_app,
            "Velopack downloads in the background and applies later"
        );

        velopack.start_download().unwrap();
        velopack.download_finished().unwrap();
        let presentation = velopack.describe();
        assert_eq!(presentation.action, Some(UpdateAction::ApplyAndRestart));
        assert!(
            presentation.action_closes_the_app,
            "applying the staged payload does close it"
        );

        let idle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        assert_eq!(idle.describe().action, Some(UpdateAction::CheckNow));
        assert!(
            !idle.describe().action_closes_the_app,
            "checking for updates closes nothing"
        );
    }

    /// Invariant: a refresh over a failed offer restores the failure, not a
    /// tidied-up version of it. The reason is what the surface is showing.
    #[test]
    fn a_failed_refresh_over_a_failed_offer_keeps_the_original_error() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_failed("permission denied").unwrap();

        lifecycle.start_check().unwrap();
        lifecycle.check_failed("feed unavailable").unwrap();

        assert_eq!(
            lifecycle.state(),
            &UpdateState::Failed {
                reason: "permission denied".to_owned(),
                target: Some("2.0.0".to_owned()),
                staged: true,
            },
            "the apply failure must survive the refresh intact"
        );
        assert_eq!(
            lifecycle.retry_failed_update().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "and the payload it kept is still staged"
        );
    }

    /// Invariant: the lane that downloads and applies in one call can report
    /// either half. When the apply half fails the payload has already landed,
    /// so the retry applies it rather than downloading it again.
    #[test]
    fn the_one_call_lane_can_report_an_apply_failure_over_a_landed_payload() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_download().unwrap();

        lifecycle
            .download_and_apply_failed("could not replace the image")
            .unwrap();
        assert_eq!(
            lifecycle.state(),
            &UpdateState::Failed {
                reason: "could not replace the image".to_owned(),
                target: Some("2.0.0".to_owned()),
                staged: true,
            }
        );
        assert_eq!(
            lifecycle.retry_failed_update().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "the payload that landed is applied, not fetched again"
        );

        // The download half failing still stages nothing.
        let mut lost = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lost.start_check().unwrap();
        lost.check_found("2.0.0").unwrap();
        lost.start_download().unwrap();
        lost.download_failed("connection reset").unwrap();
        assert_eq!(
            lost.retry_failed_update().unwrap(),
            &UpdateState::Available {
                version: "2.0.0".to_owned()
            }
        );

        // And the success side reports the whole step at once.
        let mut applied = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        applied.start_check().unwrap();
        applied.check_found("2.0.0").unwrap();
        applied.start_download().unwrap();
        assert_eq!(
            applied.download_and_apply_needs_restart().unwrap(),
            &UpdateState::RestartPending {
                version: "2.0.0".to_owned()
            }
        );
        assert_eq!(applied.running_version(), "1.0.0");
    }

    #[test]
    fn a_staging_lane_has_no_combined_download_and_apply_step() {
        let mut velopack = UpdateLifecycle::new(InstallKind::WindowsVelopack, "1.0.0");
        velopack.start_check().unwrap();
        velopack.check_found("2.0.0").unwrap();
        velopack.start_download().unwrap();

        assert_eq!(
            velopack.download_and_apply_failed("nope"),
            Err(UpdateTransitionError::NotThisLane(
                InstallKind::WindowsVelopack
            )),
            "Velopack stages first; its apply is a separate step"
        );
        assert_eq!(
            velopack.download_and_apply_needs_restart(),
            Err(UpdateTransitionError::NotThisLane(
                InstallKind::WindowsVelopack
            ))
        );
        assert_eq!(
            velopack.download_finished().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            }
        );
    }

    /// Invariant: a staged record left by an earlier run is only offered when
    /// it is actually newer than what is running. A user who replaced the
    /// install by hand must not be offered a downgrade.
    #[test]
    fn a_stale_staged_record_is_discarded_rather_than_offered() {
        for staged in ["1.0.0", "0.9.0"] {
            let lifecycle =
                UpdateLifecycle::resumed_with_staged_update(InstallKind::AppImage, "1.0.0", staged)
                    .expect("a self-applying install can resume");

            assert_eq!(
                lifecycle.state(),
                &UpdateState::Idle,
                "a {staged} record must not be offered to a 1.0.0 install"
            );
            let presentation = lifecycle.describe();
            assert_eq!(presentation.target_version, None);
            assert_eq!(presentation.claim, VersionClaim::Unknown);
            assert_eq!(
                presentation.action,
                Some(UpdateAction::CheckNow),
                "the user can still check for a real update"
            );
            assert!(
                !presentation.action_closes_the_app,
                "and nothing offered here restarts the player"
            );
        }

        let genuine =
            UpdateLifecycle::resumed_with_staged_update(InstallKind::AppImage, "1.0.0", "2.0.0")
                .expect("a self-applying install can resume");
        assert_eq!(
            genuine.state(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "a genuinely newer staged payload is still offered"
        );
    }

    #[test]
    fn a_refused_transition_never_changes_the_state() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        assert_eq!(
            lifecycle.download_finished(),
            Err(UpdateTransitionError::NotAllowedFrom(UpdateState::Idle))
        );
        assert_eq!(lifecycle.state(), &UpdateState::Idle);
    }
}
