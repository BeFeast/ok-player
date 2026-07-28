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

use std::fmt;

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
/// 5. `$APPIMAGE` with nothing contradicting it — an extract-and-run AppImage
///    (`APPIMAGE_EXTRACT_AND_RUN`) has no mount path to corroborate it, and no
///    package owns it either.
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
    if evidence.appimage_variable_set() {
        return InstallKind::AppImage;
    }
    InstallKind::DevBuild
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
    /// A check is in flight.
    Checking,
    /// The check completed and this build is the newest one.
    UpToDate,
    /// A newer version exists and this install can apply it itself.
    Available { version: String },
    /// A newer version exists but a system update tool owns it; `hint` says
    /// which one.
    AvailableExternally { version: String, hint: String },
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
            | Self::RestartPending { version } => Some(version),
            Self::Failed { target, .. } => target.as_deref(),
            Self::Idle | Self::Checking | Self::UpToDate | Self::ManagedExternally { .. } => None,
            // `Running` carries the version that is executing, which is the
            // running version rather than a target still to be reached.
            Self::Running { .. } => None,
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
    /// Apply the staged payload.
    InstallUpdate,
    /// Restart to start running the applied version.
    RestartToFinish,
    /// Retry after a failure.
    Retry,
}

impl UpdateAction {
    /// Label a shell renders on the action control.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CheckNow => "Check for updates",
            Self::DownloadUpdate => "Update now",
            Self::InstallUpdate => "Install now",
            Self::RestartToFinish => "Restart now",
            Self::Retry => "Try again",
        }
    }

    /// Whether the action makes the app itself change the installed bits. Only
    /// a [`UpdateCapability::SelfApply`] install ever reaches a state that
    /// offers one.
    pub const fn applies_update_in_app(self) -> bool {
        matches!(
            self,
            Self::DownloadUpdate | Self::InstallUpdate | Self::RestartToFinish
        )
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
    /// The one action the surface may offer, if any.
    pub action: Option<UpdateAction>,
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
}

impl UpdateLifecycle {
    /// Starts at [`UpdateState::Idle`] for `install_kind`, running
    /// `running_version`.
    pub fn new(install_kind: InstallKind, running_version: impl Into<String>) -> Self {
        Self {
            install_kind,
            running_version: running_version.into(),
            state: UpdateState::Idle,
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
        Ok(Self {
            install_kind,
            running_version: running_version.into(),
            state: UpdateState::ManagedExternally {
                hint: install_kind.system_update_hint_text().to_owned(),
            },
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
            | UpdateState::Running { .. }
            | UpdateState::Failed { .. } => Ok(self.enter(UpdateState::Checking)),
            _ => Err(self.rejected()),
        }
    }

    /// The check completed and found nothing newer.
    pub fn check_found_none(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match self.state {
            UpdateState::Checking => Ok(self.enter(UpdateState::UpToDate)),
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
        if !matches!(self.state, UpdateState::Checking) {
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
        match self.state {
            UpdateState::Checking => Ok(self.enter(UpdateState::Failed {
                reason: reason.into(),
                // A check that never found anything has no offer to retain.
                target: None,
            })),
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
                }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// The process restarted and reports the version it came back as.
    ///
    /// Matching the pending target completes the update. A mismatch is #660
    /// itself — the restart ran the old binary — and becomes
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
        if running_version == pending {
            self.running_version = running_version;
            return Ok(self.enter(UpdateState::Running { version: pending }));
        }
        self.running_version = running_version.clone();
        Ok(self.enter(UpdateState::Failed {
            reason: format!(
                "restart still runs {running_version}; the update to {pending} did not take effect"
            ),
            target: Some(pending),
        }))
    }

    /// Retries the offer a failure interrupted, without a fresh discovery
    /// round: a download or apply that failed after discovery kept its target,
    /// so the same version becomes actionable again instead of vanishing until
    /// the next check. A check that failed before finding anything has nothing
    /// to retry and is refused — [`Self::start_check`] is its retry. Only a
    /// [`UpdateCapability::SelfApply`] install has an offer of its own to
    /// resume.
    pub fn retry_failed_update(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        match &self.state {
            UpdateState::Failed {
                target: Some(version),
                ..
            } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::Available { version }))
            }
            _ => Err(self.rejected()),
        }
    }

    /// Everything the surfaces may show for the current state. The only place
    /// update text is produced.
    pub fn describe(&self) -> UpdatePresentation {
        let capability = self.capability();
        let target_version = self.state.target_version().map(str::to_owned);
        let claim = self.version_claim();
        let updates_message = self.updates_message(&claim);
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
            UpdateState::Idle
            | UpdateState::Checking
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
            UpdateState::Checking => "Checking for updates…".to_owned(),
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
            UpdateState::ManagedExternally { hint } => hint.clone(),
            UpdateState::Failed {
                reason,
                target: Some(version),
            } => format!("The update to version {version} failed: {reason}"),
            UpdateState::Failed {
                reason,
                target: None,
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
            UpdateState::ReadyToApply { .. } => Some(UpdateAction::InstallUpdate),
            UpdateState::RestartPending { .. } => Some(UpdateAction::RestartToFinish),
            UpdateState::Failed { .. } => Some(UpdateAction::Retry),
            // A system-managed update is announced, never actioned in-app; an
            // install the system owns outright offers not even a check; a
            // check, download or apply in flight offers nothing.
            UpdateState::AvailableExternally { .. }
            | UpdateState::ManagedExternally { .. }
            | UpdateState::Checking
            | UpdateState::Downloading { .. }
            | UpdateState::Applying { .. } => None,
        }
    }

    fn enter(&mut self, next: UpdateState) -> &UpdateState {
        self.state = next;
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

        assert_eq!(lifecycle.start_check().unwrap(), &UpdateState::Checking);
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
            assert_eq!(
                lifecycle.start_check().unwrap(),
                &UpdateState::Checking,
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
                target: Some("2.0.0".to_owned())
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

            let mut failed_check = UpdateLifecycle::new(kind, "1.0.0");
            failed_check.start_check().unwrap();
            failed_check.check_failed("network unreachable").unwrap();
            assert_eq!(
                failed_check.retry_failed_update(),
                Err(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} has no in-app offer of its own to resume"
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

            assert_eq!(
                lifecycle.retry_failed_update().unwrap(),
                &UpdateState::Available {
                    version: "2.0.0".to_owned()
                },
                "{name} failure must be retryable without a fresh check"
            );
            assert_eq!(
                lifecycle.describe().action,
                Some(UpdateAction::DownloadUpdate)
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
            }))
        );
        assert_eq!(lifecycle.start_check().unwrap(), &UpdateState::Checking);
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
