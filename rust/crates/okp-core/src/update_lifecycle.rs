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
//! * **A version comes with what the shell knows about it.** Every version the
//!   shell reports is a [`ReportedVersion`]: the string it observed plus
//!   whether that string is the complete package version or a truncated form
//!   (#694 — the Windows `App.AppVersion` drops the prerelease tail, so
//!   `0.11.0-beta.0.14` and `.15` both read as a stable `0.11.0`). Ordering
//!   refuses to conclude anything a truncated string cannot support instead of
//!   guessing, because a real prerelease-to-stable upgrade and a truncated
//!   prerelease have exactly the same shape.
//!
//! * **The disk can move without us.** A package manager upgrades a
//!   system-managed install while the player runs, so "what is installed" is
//!   an observation the shell makes, not something the app did
//!   ([`UpdateLifecycle::installed_version_observed`]). Core decides whether
//!   that observation establishes anything.
//!
//! Network access, package downloads, process restarts and every other side
//! effect stay in the shells; this module only decides.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;

use crate::update_selection::compare_versions;

/// What the Updates surface says while a check is in flight. A refresh over a
/// standing offer appends it to that offer's own message instead of replacing
/// it, so the offer stays legible while it is being refreshed.
const CHECKING_MESSAGE: &str = "Checking for updates…";

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
    /// What this install kind's packaging *implies* about updates.
    ///
    /// Implies, not establishes: a `.deb` is updated by apt only if apt has a
    /// source that carries OK Player, which is a fact about the machine rather
    /// than about the packaging (#725). [`UpdateLifecycle::capability`] narrows
    /// this by what the shell observed; that is the one a surface must read.
    pub const fn capability(self) -> UpdateCapability {
        match self {
            Self::WindowsVelopack | Self::AppImage => UpdateCapability::SelfApply,
            Self::Deb | Self::Rpm | Self::Flatpak => UpdateCapability::SystemManaged,
            Self::DevBuild => UpdateCapability::Unmanaged,
        }
    }

    /// Whether this kind's update path is a repository the machine subscribes
    /// to — something that may not be configured at all, and that the app must
    /// therefore establish rather than assume (#725).
    ///
    /// Only the apt lane is. `dnf` and `flatpak` own the install *because* they
    /// performed it: a Flatpak came from a remote that is still configured, and
    /// an `.rpm` from `dnf` leaves its repository behind. A `.deb` is the one
    /// lane a user routinely installs by downloading a file, which leaves dpkg
    /// owning the package and apt with nothing to fetch it from — the #725
    /// machine exactly.
    pub const fn delivery_must_be_established(self) -> bool {
        matches!(self, Self::Deb)
    }

    /// The tool that owns updates for this kind, named as the user knows it.
    pub const fn system_update_tool(self) -> &'static str {
        match self {
            Self::Deb => "apt",
            Self::Rpm => "dnf",
            Self::Flatpak => "Flatpak",
            Self::WindowsVelopack | Self::AppImage | Self::DevBuild => "your system update tool",
        }
    }

    /// Whether the project publishes a repository a user of this kind could subscribe
    /// to. Only the apt lane does: an `.rpm` or a Flatpak install came from a repository
    /// the user already has, and the AppImage and Velopack lanes update themselves.
    ///
    /// What the instructions *say* is not decided here, because it cannot be: they name a
    /// suite, and the suite is a property of the build the user installed rather than of
    /// the packaging format (#726).
    pub const fn publishes_a_repository(self) -> bool {
        matches!(self, Self::Deb)
    }

    /// The command that makes this kind's packaging tool read a source it has been given
    /// but has not fetched — the whole remedy for
    /// [`UpdateState::AvailableButSourceUnread`].
    pub const fn refresh_command(self) -> Option<&'static str> {
        match self {
            Self::Deb => Some("sudo apt update"),
            Self::Rpm | Self::Flatpak | Self::WindowsVelopack | Self::AppImage | Self::DevBuild => {
                None
            }
        }
    }

    /// The command that upgrades **this package and nothing else on the machine** through the
    /// tool that owns it (#759).
    ///
    /// Not "this package alone": a new version may need a newer dependency, and apt resolves
    /// that as part of installing the package at all — no command that installs OK Player can
    /// avoid it. What it cannot do is reach *unrelated* packages, which is the whole of the
    /// defect this replaced.
    ///
    /// It names the package on purpose. What it replaced opened the desktop's *system-wide*
    /// updater, which builds a transaction over every upgradable package on the machine: on
    /// the reporting machine that pulled in `tzdata`, whose debconf question blocked
    /// `dpkg-preconfigure` behind a prompt nothing was answering, and the window sat at
    /// `Preconfiguring packages ...` indefinitely. None of it was OK Player's package, and
    /// that is the point — an application a packaging tool owns has one package's worth of
    /// business.
    ///
    /// **No suite and no channel appears here.** What this machine subscribes to was
    /// established by [`crate::apt_policy`] / [`crate::apt_sources`] (#725/#726); the command
    /// upgrades from whatever that is, so it cannot move anybody between channels the way a
    /// hard-coded stanza would.
    ///
    /// `--only-upgrade` narrows it further in the other direction: apt is forbidden to
    /// *install* anything that is not already on the machine, so the transaction cannot grow
    /// past the package and the dependencies it already has.
    pub const fn upgrade_command(self) -> Option<&'static str> {
        match self {
            Self::Deb => Some("sudo apt install --only-upgrade ok-player"),
            // Not "there is no such command" but "this app has no state in which to offer
            // one": the rpm and flatpak lanes never discover a version, so they never reach a
            // state that announces one. What they say is who updates them, unchanged.
            Self::Rpm | Self::Flatpak | Self::WindowsVelopack | Self::AppImage | Self::DevBuild => {
                None
            }
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

    /// How the user updates an install whose payload a system tool owns *and*
    /// can actually reach. The hint travels inside
    /// [`UpdateState::AvailableExternally`] so no shell has to invent one.
    ///
    /// `suite` is the source the machine was observed to subscribe to, when the
    /// shell established one. Naming it is what keeps the sentence true: it is
    /// the suite that carries the version being announced, and it is the answer
    /// to "your updater says there is nothing" — a `stable` machine is never
    /// offered a candidate, and now it can see which channel it is on (#725).
    fn system_update_hint_text(self, suite: Option<&str>) -> String {
        match (self, suite) {
            (Self::Deb, Some(suite)) => {
                format!("Update OK Player with apt — it is in the {suite} suite you subscribe to.")
            }
            (Self::Deb, None) => "Update OK Player with your package manager (apt).".to_owned(),
            (Self::Rpm, _) => "Update OK Player with your package manager (dnf).".to_owned(),
            (Self::Flatpak, _) => "Update OK Player with Flatpak (flatpak update).".to_owned(),
            (Self::WindowsVelopack | Self::AppImage | Self::DevBuild, _) => {
                "Update OK Player with your system update tool.".to_owned()
            }
        }
    }
}

/// What the shell observed when it asked the packaging tool what it can deliver
/// for OK Player (#725).
///
/// The same shape as [`InstallEvidence`], for the same reason: establishing it
/// means running `apt-cache policy`, which core must not do, so the shell
/// observes and core decides. [`crate::apt_policy`] turns the command's output
/// into one of these without touching a live system, so every rule below stays
/// testable against captured output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum PackageSourceEvidence {
    /// Nobody asked, or the question could not be answered — the tool is
    /// absent, or it failed. An honest state of its own: it is *not* evidence
    /// that a source exists, and the surface may not act as though it were.
    #[default]
    Unestablished,
    /// The tool answered and nothing this machine subscribes to carries OK
    /// Player. The #725 machine: a package installed from a downloaded file,
    /// known to apt only through dpkg's status.
    NoSource,
    /// A source for OK Player is configured, and the packaging tool has not read it yet
    /// (#725/#726).
    ///
    /// Every `.deb` install passes through this state. `postinst` writes
    /// `/etc/apt/sources.list.d/ok-player.sources` and stops there — running `apt update` is
    /// not a maintainer script's business — so on first launch the stanza is on disk and
    /// `apt-cache policy` still knows the package only through dpkg's status. Reading that
    /// as [`Self::NoSource`] made the app deny the existence of a file its own packaging had
    /// just written, and offer setup commands for a suite that might not be the user's.
    ///
    /// It is emphatically not a gap: the delivery path exists, it has simply not been read.
    /// The remedy is a refresh, and it is one command.
    ConfiguredButUnread { suite: Option<String> },
    /// A configured source carries OK Player. `suite` is what the machine
    /// subscribes to; `deliverable` is the version the tool would install now,
    /// or nothing when that suite has nothing beyond what is installed.
    Source {
        suite: String,
        deliverable: Option<String>,
    },
}

impl PackageSourceEvidence {
    /// Whether a delivery path was established at all — a source exists, whether or not the
    /// tool has read it yet.
    ///
    /// This is the condition the whole surface turns on, and the reason it is not
    /// [`Self::carries_the_package`]: a machine that has a source apt has not fetched has a
    /// delivery path, and telling it to add one would be false. An unanswered question is
    /// still not an answer, so [`Self::Unestablished`] is not one of these.
    pub fn source_established(&self) -> bool {
        matches!(self, Self::Source { .. } | Self::ConfiguredButUnread { .. })
    }

    /// Whether a source the tool has actually read can fetch OK Player. Narrower than
    /// [`Self::source_established`], and the one that decides what may be announced.
    pub fn carries_the_package(&self) -> bool {
        matches!(self, Self::Source { .. })
    }

    /// The suite the machine subscribes to, when one was established.
    pub fn suite(&self) -> Option<&str> {
        match self {
            Self::Source { suite, .. } => Some(suite.as_str()),
            Self::ConfiguredButUnread { suite } => suite.as_deref(),
            Self::Unestablished | Self::NoSource => None,
        }
    }

    /// The version the packaging tool would install now, when it has one.
    pub fn deliverable(&self) -> Option<&str> {
        match self {
            Self::Source { deliverable, .. } => deliverable.as_deref(),
            Self::Unestablished | Self::NoSource | Self::ConfiguredButUnread { .. } => None,
        }
    }

    /// Why an install a packaging tool owns has no delivery path, when it has
    /// none. A source that has merely not been read is not one of these.
    const fn gap(&self) -> Option<SourceGap> {
        match self {
            Self::Unestablished => Some(SourceGap::Unestablished),
            Self::NoSource => Some(SourceGap::NoSource),
            Self::Source { .. } | Self::ConfiguredButUnread { .. } => None,
        }
    }
}

/// Why a system-managed install cannot be updated by its packaging tool (#725).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceGap {
    /// The tool answered, and no configured source carries OK Player.
    NoSource,
    /// The tool could not be asked. Distinct from [`Self::NoSource`] because
    /// the remedy is the same but the claim is not: the app does not know that
    /// nothing is configured, only that it could not find out.
    Unestablished,
}

/// How a user puts a packaging tool's delivery path in place, for a surface
/// that has to show it (#700, #725).
///
/// The commands are the README's, verbatim, and they are known to work:
/// `scripts/verify-apt-source-instructions.sh` reads them out of the app, requires the README
/// to publish exactly them, and then follows them end to end in a clean Debian container.
/// Neither half is worth much alone — text that matches a document nobody ran, or a container
/// run of commands the user is never shown.
///
/// The app never runs them. Adding an archive and its signing key is a
/// system-wide, privileged change a user makes deliberately; the privileged
/// install path was removed in #698 and this does not bring it back.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RepositorySetup {
    /// The suite these commands subscribe to. Named because it is the one thing about them
    /// the user must be able to check before pasting: the channels are separate on purpose.
    pub suite: String,
    /// One line saying what taking these commands achieves.
    pub summary: String,
    /// The commands, as one copyable block.
    pub commands: String,
    /// The signing key's fingerprint, so the archive can be verified against
    /// something the app did not fetch.
    pub key_fingerprint: &'static str,
    /// Where a package can be downloaded instead, for a user who does not want
    /// a repository at all.
    pub downloads_url: &'static str,
}

/// The signing key fingerprint the README publishes, as apt prints it.
pub const APT_ARCHIVE_KEY_FINGERPRINT: &str = "77D0 FCDE B0D5 94E1 3E50  F43A 9337 815E B0F7 8C63";
/// Where a package can be downloaded instead of subscribing to anything.
pub const OK_PLAYER_DOWNLOADS_URL: &str = "https://github.com/BeFeast/ok-player/releases";

/// What it takes to subscribe to `suite` of the signed APT repository OK Player publishes.
///
/// **The suite is a parameter and never a constant.** The archive publishes two, deliberately
/// separate, and the app is shown these commands by a user who is on one of them: handing a
/// `candidate` tester the `stable` block moves them off the channel they installed for, which
/// is the failure #726 exists to prevent, done by the app's own advice. `None` for a suite the
/// archive does not publish — a build whose channel cannot be established is told so rather
/// than guessed at.
pub fn apt_repository_setup(suite: &str) -> Option<RepositorySetup> {
    let stanza = match suite {
        "stable" => "ok-player.sources",
        "candidate" => "ok-player-candidate.sources",
        _ => return None,
    };
    let qualifier = if suite == "stable" {
        "and apt updates OK Player with the rest of your system"
    } else {
        "and apt updates OK Player with the rest of your system — this is the QA channel, and its builds are not releases"
    };
    Some(RepositorySetup {
        suite: suite.to_owned(),
        summary: format!("Add the signed OK Player repository once, {qualifier}:"),
        commands: format!(
            "sudo install -d -m 0755 /usr/share/keyrings\n\
curl -fsSL https://befeast.github.io/ok-player/apt/ok-player-archive-keyring.gpg \\\n  \
| sudo tee /usr/share/keyrings/ok-player-archive-keyring.gpg >/dev/null\n\
curl -fsSL https://befeast.github.io/ok-player/apt/{stanza} \\\n  \
| sudo tee /etc/apt/sources.list.d/ok-player.sources >/dev/null\n\
sudo apt update\n\
sudo apt install ok-player"
        ),
        key_fingerprint: APT_ARCHIVE_KEY_FINGERPRINT,
        downloads_url: OK_PLAYER_DOWNLOADS_URL,
    })
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
    /// A system update tool owns the payload *and* was observed to be able to
    /// fetch it; the app may only report, and may point at that tool.
    SystemManaged,
    /// A system update tool owns the install, but nothing it can reach carries
    /// OK Player, so it can deliver nothing (#725).
    ///
    /// Distinct from [`Self::SystemManaged`] because the app must say something
    /// different: naming a version and sending the user to a tool that has
    /// never heard of it is the dead end #725 reported. Distinct from
    /// [`Self::Unmanaged`] too — the packaging is real, the payload is real,
    /// and the missing piece is a repository the user can add in one paste.
    SystemUnreachable,
    /// Nothing updates this install; the app says so instead of claiming to be
    /// current.
    Unmanaged,
}

impl UpdateCapability {
    /// Whether a packaging tool owns this install's updates, reachable or not.
    /// The lanes that never apply an update in the app.
    pub const fn is_system_owned(self) -> bool {
        matches!(self, Self::SystemManaged | Self::SystemUnreachable)
    }
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
    /// Value of `$APPDIR`, the mounted image root, when set. Paired with
    /// [`Self::executable_path`] it distinguishes *this* process running from
    /// an image from merely having inherited the variables of one.
    pub appdir_path: Option<String>,
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

    /// `$APPDIR` is set *and* contains the running executable. An AppImage
    /// launcher exports both variables to whatever it starts, so the variables
    /// alone would misread an unpackaged build launched from one; only the
    /// executable's own location settles it.
    fn executable_inside_appdir(&self) -> bool {
        let appdir = self
            .appdir_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        let executable = self
            .executable_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty());
        match (appdir, executable) {
            (Some(appdir), Some(executable)) => {
                let appdir = appdir.trim_end_matches('/');
                !appdir.is_empty() && executable.starts_with(&format!("{appdir}/"))
            }
            _ => false,
        }
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
    if evidence.appimage_variable_set()
        && evidence.executable_inside_appdir()
        && evidence.package_ownership == PackageOwnership::Unowned
    {
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
        (Some(left_pre), Some(right_pre)) => compare_prerelease(left_pre, right_pre),
    }
}

/// Orders two prerelease tails identifier by identifier, so the *stage* is
/// compared before its counter: `alpha.109` precedes `beta.1`, which whole-tail
/// numeric-run comparison gets backwards by looking at 109 against 1 first.
///
/// Within an identifier, two numbers compare numerically (`alpha.9` before
/// `alpha.10`) and anything else compares lexically; a number sorts before a
/// word, and a tail that is a prefix of another sorts before it (`beta.2`
/// before the `beta.2.41` candidate builds cut from it).
fn compare_prerelease(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_part), Some(right_part)) => {
                let order = match (left_part.parse::<u64>(), right_part.parse::<u64>()) {
                    (Ok(left_number), Ok(right_number)) => left_number.cmp(&right_number),
                    // A numeric identifier ranks below an alphanumeric one.
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => left_part.cmp(right_part),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// Splits `1.0.0-beta.1` into its numeric core and its prerelease tail.
fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    }
}

/// How completely a shell was able to state a version (#694).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VersionFidelity {
    /// The exact package version, prerelease tail included. Every conclusion
    /// the ordering rules can draw is available.
    #[default]
    Complete,
    /// A normalised form: the numeric core is the build's, but any prerelease
    /// tail was dropped on the way — the Windows `App.AppVersion`, which is
    /// `Major.Minor.Build` read off the assembly, reports `0.11.0-beta.0.14`
    /// and `0.11.0-beta.0.15` alike as `0.11.0`. The string looks exactly like
    /// a stable release, so the ordering rules must not read it as one.
    Truncated,
}

/// A version string together with what the shell knows about it.
///
/// Shells that can name the exact build (`0.11.0-beta.0.15` from the Velopack
/// release identity, from the informational assembly version, or from a Linux
/// package version) report [`VersionFidelity::Complete`] and get the full
/// ordering. A shell that can only observe a truncated form says so, and the
/// comparisons that the missing tail would decide return "unknown" rather than
/// a wrong answer.
///
/// `From<&str>` and `From<String>` produce a complete version, so a caller that
/// has the real thing — every Linux lane, and every test that is not about
/// truncation — just passes a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReportedVersion {
    text: String,
    fidelity: VersionFidelity,
}

impl ReportedVersion {
    /// The exact package version of the build.
    pub fn complete(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            fidelity: VersionFidelity::Complete,
        }
    }

    /// A version whose prerelease tail the shell could not observe.
    pub fn truncated(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            fidelity: VersionFidelity::Truncated,
        }
    }

    /// The string as the shell observed it. Always safe to display — only
    /// ordering conclusions are restricted.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn fidelity(&self) -> VersionFidelity {
        self.fidelity
    }

    pub const fn is_complete(&self) -> bool {
        matches!(self.fidelity, VersionFidelity::Complete)
    }
}

impl From<&str> for ReportedVersion {
    fn from(text: &str) -> Self {
        Self::complete(text)
    }
}

impl From<String> for ReportedVersion {
    fn from(text: String) -> Self {
        Self::complete(text)
    }
}

impl From<&String> for ReportedVersion {
    fn from(text: &String) -> Self {
        Self::complete(text.clone())
    }
}

impl fmt::Display for ReportedVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Orders two reported builds, or answers `None` when the strings on hand
/// cannot decide it (#694).
///
/// A truncated string carries the numeric core and nothing else, so:
///
/// * different cores still decide the order — no prerelease tail can lift a
///   build past a higher core, so the missing tail changes nothing;
/// * equal cores are undecidable as soon as either side is truncated, because
///   the truncated string is equally consistent with the stable release and
///   with every prerelease that led to it. `0.11.0` (truncated) against
///   `0.11.0-beta.0.15` is the #694 case: read as complete it is a stable
///   release *newer* than the pending candidate, and the restart check turns a
///   perfectly good upgrade into a reported downgrade.
///
/// Two complete versions are always decided, by [`compare_build_order`].
pub fn compare_reported_build_order(
    left: &ReportedVersion,
    right: &ReportedVersion,
) -> Option<Ordering> {
    if left.is_complete() && right.is_complete() {
        return Some(compare_build_order(&left.text, &right.text));
    }
    let (left_core, _) = split_prerelease(&left.text);
    let (right_core, _) = split_prerelease(&right.text);
    match compare_versions(left_core, right_core) {
        // The cores tie, so the answer lives entirely in a tail at least one
        // side does not have.
        Ordering::Equal => None,
        order => Some(order),
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
    /// which one. Only reached once a source was observed to carry `version`,
    /// so the hint is a path that exists (#725).
    AvailableExternally { version: String, hint: String },
    /// A newer version exists, a packaging tool owns this install, and nothing
    /// the machine subscribes to can deliver it (#725).
    ///
    /// The version is still named — it is published, and the user is entitled
    /// to know — but nothing here points at a package manager or at a desktop
    /// updater, because both would answer that there is nothing to do. What the
    /// surface offers instead is the way to make the delivery path exist:
    /// [`UpdatePresentation::repository_setup`].
    AvailableWithoutSource { version: String, gap: SourceGap },
    /// A newer version is published, this machine has a source configured for it, and the
    /// packaging tool has not read that source yet (#725/#726).
    ///
    /// The state every `.deb` install is in on first launch. Nothing here offers to add a
    /// repository: there is one, written by this project's own `postinst`, and saying
    /// otherwise would be false about the machine and would hand the user commands for a
    /// suite that may not be theirs. What it offers is the one command that settles it.
    AvailableButSourceUnread {
        version: String,
        suite: Option<String>,
    },
    /// A newer version is published and the suite this machine subscribes to
    /// is not currently offering it (#689, #725).
    ///
    /// The `stable` subscriber's state. `stable` deliberately never carries a
    /// candidate build, so announcing one would name a version apt refuses to
    /// install. The machine is up to date *on its channel*, which is what it
    /// says, and it says which channel that is.
    ///
    /// It is deliberately phrased as "not offering" rather than "does not
    /// carry", because `apt-cache` answers from the package lists as they were
    /// last fetched and says so itself. A machine that has not run `apt update`
    /// since a release was published reaches this state too, and the honest
    /// difference between the two is a refresh this app is not privileged to
    /// perform. So the desktop updater — which *is* the thing that refreshes
    /// them — stays on offer here. It cannot contradict this state the way it
    /// contradicted the one #725 reported: either it refreshes and finds the
    /// release, which is strictly better than what the app could say, or it
    /// reports the machine current, which is what this state already says.
    WithheldBySuite { version: String, suite: String },
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
    /// `version` is installed on disk, this process still runs the build it
    /// started with, and nothing this process did put the new one there: a
    /// package manager upgraded the install while the player was running
    /// (#707), which the APT channel makes routine.
    ///
    /// The same truth as [`Self::RestartPending`] — the disk is ahead of the
    /// process — reached without an apply of our own, which is why it is a
    /// state of its own rather than a reuse of that one: no payload was
    /// staged, there is nothing to retry or to skip, and the only thing that
    /// resolves it is the restart the user is being asked for. Reached only
    /// through [`UpdateLifecycle::installed_version_observed`].
    ReplacedOnDisk { version: String },
    /// The process restarted, and the version it can report about itself is too
    /// coarse to tell whether it came up on `target` (#694): a truncated
    /// running version shares its whole string with the prerelease it may still
    /// be. Neither a success nor the #660 failure — the shell says what it
    /// could not observe, and a fresh check settles it against the feed.
    RestartUnverified { target: String },
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
        /// What a recovery from this failure is able to do — which is not
        /// always a repeat of the step that failed (#701).
        recovery: FailureRecovery,
    },
}

/// What a recovery from a [`UpdateState::Failed`] is able to do. It decides
/// both what [`UpdateLifecycle::retry_failed_update`] accepts and what the
/// projection is allowed to call the action, so a surface cannot promise one
/// thing and deliver another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FailureRecovery {
    /// The step that failed can simply be run again: the payload is still
    /// staged, or the offer that was discovered still stands. Whatever the
    /// failed step was, repeating it is what "Try again" means.
    RepeatTheStep,
    /// The step cannot be repeated. A restart that came back on the old build
    /// is the case (#660): the apply consumed the payload that produced it,
    /// and the process asking is a *new* one that has discovered nothing, so
    /// there is nothing left to re-apply. Re-checking first is also what keeps
    /// the recovery from walking straight back into the restart that just
    /// failed — one restart attempt, then the user decides — and it acts on
    /// what is published now rather than on the payload that failed.
    ///
    /// So the recovery is a check, and the projection offers it as a check.
    CheckAgain,
}

impl UpdateState {
    /// The version this state is about, when it is about one. Always the update
    /// *target* — never a claim about the binary currently executing.
    pub fn target_version(&self) -> Option<&str> {
        match self {
            Self::Available { version }
            | Self::AvailableExternally { version, .. }
            // Published, named, and not deliverable here: still the version
            // every control on the surface is about.
            | Self::AvailableWithoutSource { version, .. }
            // Published, and reachable as soon as the lists are read.
            | Self::AvailableButSourceUnread { version, .. }
            | Self::Downloading { version }
            | Self::ReadyToApply { version }
            | Self::Applying { version }
            | Self::RestartPending { version }
            | Self::ReplacedOnDisk { version }
            | Self::Skipped { version, .. } => Some(version),
            // The version the restart was meant to land on, still the only
            // build this state is about — the running one is unknown.
            Self::RestartUnverified { target } => Some(target),
            Self::Failed { target, .. } => target.as_deref(),
            Self::Checking { carried } => carried.as_ref().map(CarriedOffer::version),
            // The build this machine's suite does not carry is not a target:
            // nothing on the surface acts on it, and calling it one would put
            // it on controls ("Update · X") that cannot reach it.
            Self::WithheldBySuite { .. } => None,
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
    /// An announcement with no delivery path (#725). A refresh that fails must
    /// leave it exactly as it was: the version is still published and the
    /// machine still cannot fetch it, and demoting it to a generic failure
    /// would take the repository instructions off the surface.
    WithoutSource {
        version: String,
        gap: SourceGap,
    },
    /// An announcement whose delivery path exists but has not been read (#726). A refresh
    /// that fails must leave it as it was: the source is still configured and the lists are
    /// still unread, and demoting it would put repository instructions on a machine that
    /// already has a repository.
    SourceUnread {
        version: String,
        suite: Option<String>,
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
    /// what the user is looking at — nor forget that the failure was one no
    /// retry can repeat, which would make the refresh hand back a restart
    /// failure dressed as a retryable one.
    Failed {
        version: String,
        reason: String,
        staged: bool,
        recovery: FailureRecovery,
    },
    /// A restart the running version could not confirm (#694). A check is what
    /// settles it, so a check that *fails* settles nothing: the pending target
    /// and the fact that nothing is known about it both survive the refresh.
    UnverifiedRestart {
        target: String,
    },
}

impl CarriedOffer {
    pub fn version(&self) -> &str {
        match self {
            Self::Available { version }
            | Self::AvailableExternally { version, .. }
            | Self::WithoutSource { version, .. }
            | Self::SourceUnread { version, .. }
            | Self::ReadyToApply { version }
            | Self::Skipped { version, .. }
            | Self::Failed { version, .. } => version,
            Self::UnverifiedRestart { target } => target,
        }
    }

    fn into_state(self) -> UpdateState {
        match self {
            Self::Available { version } => UpdateState::Available { version },
            Self::AvailableExternally { version, hint } => {
                UpdateState::AvailableExternally { version, hint }
            }
            Self::WithoutSource { version, gap } => {
                UpdateState::AvailableWithoutSource { version, gap }
            }
            Self::SourceUnread { version, suite } => {
                UpdateState::AvailableButSourceUnread { version, suite }
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
                recovery,
            } => UpdateState::Failed {
                reason,
                target: Some(version),
                staged,
                recovery,
            },
            Self::UnverifiedRestart { target } => UpdateState::RestartUnverified { target },
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
    /// An observed installed version does not establish that this process is
    /// behind it: it is the build already running, an older one, or a version
    /// the strings on hand cannot order at all (#694). Distinct from a refused
    /// transition — the observation was understood, and it says nothing.
    NotBehindInstalled { installed: String },
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
    /// Restart to start running a version that is already installed — put
    /// there by a package manager, not by this app (#707). Nothing is applied:
    /// the new build is on disk either way, and only the process is stale.
    RestartToUseInstalled,
    /// Retry after a failure.
    Retry,
    /// Suppress this exact version so nothing prompts for it again.
    SkipVersion,
    /// Install a version the user previously skipped.
    InstallAnyway,
    /// Put the commands that add the package repository on the clipboard, so
    /// the user can paste them into a terminal (#725).
    ///
    /// The app deliberately does not run them: adding an archive and its
    /// signing key is a privileged, system-wide change, and the privileged
    /// install path was removed in #698. Copying is the whole of what the app
    /// does — which is also why it is safe to offer from a state that can
    /// deliver nothing else.
    CopyRepositorySetup,
    /// Put the command that makes the packaging tool read a source it already has on the
    /// clipboard (#726).
    ///
    /// Separate from [`Self::CopyRepositorySetup`] because the situations are opposites and
    /// the wrong one is harmful: this machine has a repository, and handing it setup commands
    /// would overwrite the suite it is subscribed to.
    CopyRefreshCommand,
    /// Put the command that upgrades **this package**, and nothing unrelated to it, on the
    /// clipboard (#759).
    ///
    /// The one action a system-managed install has for an update its own source can already
    /// deliver. The app does not run it: the packaging tool applies the update, as the
    /// message beside this says, and the privileged install path was removed in #698.
    ///
    /// It exists because the alternative shipped and failed. This state used to offer "Open
    /// software updater", which handed the user their desktop's system-wide updater and a
    /// transaction over every upgradable package on the machine.
    CopyUpgradeCommand,
}

impl UpdateAction {
    /// Label a shell renders on the action control.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CheckNow => "Check for updates",
            Self::DownloadUpdate => "Update now",
            Self::ApplyAndRestart => "Install and restart",
            Self::RestartToFinish => "Restart now",
            Self::RestartToUseInstalled => "Restart OK Player",
            Self::Retry => "Try again",
            Self::SkipVersion => "Skip this version",
            Self::InstallAnyway => "Install anyway",
            Self::CopyRepositorySetup => "Copy setup commands",
            Self::CopyRefreshCommand => "Copy refresh command",
            Self::CopyUpgradeCommand => "Copy upgrade command",
        }
    }

    /// Whether the action makes the app itself change the installed bits. Only
    /// a [`UpdateCapability::SelfApply`] install ever reaches a state that
    /// offers one.
    ///
    /// [`Self::RestartToUseInstalled`] is deliberately not one: a package
    /// manager already wrote the new build, and restarting into it installs
    /// nothing. That is what lets a system-managed install offer the restart
    /// #707 asks for without acquiring an in-app install it must never have.
    pub const fn applies_update_in_app(self) -> bool {
        matches!(
            self,
            Self::DownloadUpdate
                | Self::ApplyAndRestart
                | Self::RestartToFinish
                | Self::InstallAnyway
        )
    }

    /// Whether taking this action puts anything beyond OK Player itself into the user's
    /// hands.
    ///
    /// Every action answers `false`, and that is the invariant rather than an accident
    /// (#759): an application a packaging tool owns offers exactly one action for *itself*.
    /// The action this rule was written for answered `true` — "Open software updater" launched
    /// the desktop's system-wide updater, which builds a transaction over every upgradable
    /// package on the machine, and on the reporting machine that transaction blocked
    /// indefinitely on `tzdata`'s debconf prompt.
    ///
    /// The match is exhaustive on purpose. A new action has to answer here, and
    /// `no_offered_action_reaches_beyond_this_package` refuses any that answers `true`.
    pub const fn reaches_beyond_this_package(self) -> bool {
        match self {
            Self::CheckNow
            | Self::DownloadUpdate
            | Self::ApplyAndRestart
            | Self::RestartToFinish
            | Self::RestartToUseInstalled
            | Self::Retry
            | Self::SkipVersion
            | Self::InstallAnyway
            | Self::CopyRepositorySetup
            | Self::CopyRefreshCommand
            // Named in the command itself, and `--only-upgrade` keeps apt from reaching for
            // anything that is not already installed. What the new version depends on comes
            // with it, as it must for any command that installs the package at all — but
            // nothing unrelated does.
            | Self::CopyUpgradeCommand => false,
        }
    }

    /// Whether taking this action shuts the running process down. Both ways of
    /// getting onto a new build do — applying a staged payload relaunches the
    /// app, and so does finishing an applied one — so a surface can warn before
    /// it happens instead of closing the player out from under the user.
    pub const fn closes_the_app(self) -> bool {
        matches!(
            self,
            Self::ApplyAndRestart | Self::RestartToFinish | Self::RestartToUseInstalled
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
    /// The primary action the surface may offer, if any.
    pub action: Option<UpdateAction>,
    /// Whether the offered actions can be taken right now. A check in flight
    /// keeps the offer it is refreshing on screen — with its own controls — but
    /// they are not actionable until the check settles, which is what the GTK
    /// surface does by disabling rather than hiding them.
    pub actions_enabled: bool,
    /// Whether taking [`Self::action`] shuts the running player down, for this
    /// install kind. Beyond the actions that always do, this covers the
    /// AppImage lane, where accepting the offer downloads, applies and
    /// relaunches in one step — a surface must be able to warn before that,
    /// whatever the action is called.
    pub action_closes_the_app: bool,
    /// Whether a check would be accepted from this state. A state that has
    /// already been told what is on disk — or one with a step in flight — has
    /// nothing to discover, and a surface with its own dedicated check control
    /// disables it rather than offering a button that does nothing.
    pub check_available: bool,
    /// The secondary action offered beside the primary one — today only
    /// "Skip this version" on a live offer. Kept in the projection so a shell
    /// never has to maintain its own parallel offer model.
    pub secondary_action: Option<UpdateAction>,
    /// The command that upgrades **this package**, and nothing unrelated to it, when a
    /// packaging tool that owns this install can deliver the version on screen (#759).
    ///
    /// This is what a system-managed install offers instead of a way into the desktop's
    /// system-wide updater. That offer was removed outright: it was not this app's to make,
    /// because the transaction it starts covers every upgradable package on the machine and
    /// nothing in the app bounds it. What is left names the package, and only the package.
    ///
    /// Nothing in the app runs it — apt applies the update, exactly as the message says.
    pub upgrade_command: Option<&'static str>,
    /// How to give this install a delivery path, when it has none and the
    /// project publishes one (#700, #725). The surface shows the commands and
    /// the key fingerprint; nothing in the app runs them.
    pub repository_setup: Option<RepositorySetup>,
    /// The command that makes the packaging tool read a source this machine already has,
    /// when that is what stands between it and the update (#726). Never shown beside
    /// [`Self::repository_setup`]: a machine either has a repository or does not.
    pub refresh_command: Option<&'static str>,
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
    running_version: ReportedVersion,
    /// What the shell established about the packaging tool's ability to
    /// deliver OK Player on *this* machine (#725). Unestablished until a shell
    /// says otherwise, which is why nothing derived from it may assert that a
    /// delivery path exists.
    package_source: PackageSourceEvidence,
    /// The suite this build was packaged to subscribe to, read by the shell off the stanza
    /// the package carries (#726). The only honest source for the suite in setup
    /// instructions: a constant would hand a candidate tester the stable block.
    packaged_suite: Option<String>,
    state: UpdateState,
    /// What went wrong with the last attempt, when the state itself carried on
    /// regardless — a failed refresh that restored the offer it was
    /// refreshing. Cleared by every transition that succeeds.
    notice: Option<String>,
}

impl UpdateLifecycle {
    /// Starts at [`UpdateState::Idle`] for `install_kind`, running
    /// `running_version`. A plain string is taken as the complete package
    /// version; a shell that can only observe a truncated one passes
    /// [`ReportedVersion::truncated`].
    pub fn new(install_kind: InstallKind, running_version: impl Into<ReportedVersion>) -> Self {
        Self {
            install_kind,
            running_version: running_version.into(),
            state: UpdateState::Idle,
            package_source: PackageSourceEvidence::Unestablished,
            packaged_suite: None,
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
        running_version: impl Into<ReportedVersion>,
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
                // A lane that never discovers a version has no suite to name:
                // the tool it points at is the whole answer.
                hint: install_kind.system_update_hint_text(None),
            },
            package_source: PackageSourceEvidence::Unestablished,
            packaged_suite: None,
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
        running_version: impl Into<ReportedVersion>,
        staged_version: impl Into<ReportedVersion>,
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
        // discarded rather than offered. When the versions on hand cannot
        // decide that (#694), the staged payload is kept: that it exists is an
        // observed fact rather than an ordering conclusion, and throwing away a
        // downloaded update on a guess loses a real one silently, while
        // offering it says which version it is and leaves the choice with the
        // user.
        let state = if compare_reported_build_order(&staged_version, &running_version)
            .is_none_or(|order| order == Ordering::Greater)
        {
            UpdateState::ReadyToApply {
                version: staged_version.text,
            }
        } else {
            UpdateState::Idle
        };
        Ok(Self {
            install_kind,
            running_version,
            state,
            package_source: PackageSourceEvidence::Unestablished,
            packaged_suite: None,
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
        running_version: impl Into<ReportedVersion>,
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
            package_source: PackageSourceEvidence::Unestablished,
            packaged_suite: None,
            notice: None,
        };
        lifecycle.restarted_into(running_version)?;
        Ok(lifecycle)
    }

    pub const fn install_kind(&self) -> InstallKind {
        self.install_kind
    }

    /// What this install can do about updates *here*, which is the packaging's
    /// implied capability narrowed by what the shell established about the
    /// machine (#725).
    ///
    /// A lane whose delivery path is a repository the user subscribes to — the
    /// apt lane — is [`UpdateCapability::SystemManaged`] only once a source was
    /// observed to carry OK Player. An unanswered question is not that
    /// observation: "apt-cache could not be run" is not evidence that apt can
    /// deliver anything, and the whole of #725 is the app asserting a delivery
    /// path it had never established.
    pub fn capability(&self) -> UpdateCapability {
        let implied = self.install_kind.capability();
        if implied == UpdateCapability::SystemManaged
            && self.install_kind.delivery_must_be_established()
            && !self.package_source.source_established()
        {
            return UpdateCapability::SystemUnreachable;
        }
        implied
    }

    /// What the shell established about this machine's delivery path.
    pub const fn package_source(&self) -> &PackageSourceEvidence {
        &self.package_source
    }

    /// Records what the shell observed about the packaging tool's ability to
    /// deliver OK Player. Taken at check time, before the check's own outcome
    /// is reported, so the state a check produces is decided against a current
    /// observation rather than a stale one.
    ///
    /// It settles no state by itself: what a machine can fetch is a standing
    /// fact, and what to *say* about it depends on what the check found.
    pub fn package_source_observed(&mut self, package_source: PackageSourceEvidence) {
        self.package_source = package_source;
    }

    /// Records the channel this build was packaged for, read off the stanza the package
    /// carries at `/usr/share/ok-player/apt/ok-player.sources` (#726).
    ///
    /// It settles nothing by itself. It is what setup instructions take their suite from, so
    /// that a machine being told how to subscribe is told how to subscribe to *its own*
    /// channel. A build that carries no stanza — anything published before #726 — leaves this
    /// unset, and the surface says the channel is unknown rather than inventing one.
    pub fn packaged_suite_observed(&mut self, suite: Option<String>) {
        self.packaged_suite = suite;
    }

    /// The channel this build was packaged for, when the shell could establish it.
    pub fn packaged_suite(&self) -> Option<&str> {
        self.packaged_suite.as_deref()
    }

    pub const fn state(&self) -> &UpdateState {
        &self.state
    }

    /// The version this process is executing — not the version an applied but
    /// unrestarted update would run.
    pub fn running_version(&self) -> &str {
        self.running_version.text()
    }

    /// The running version together with how completely the shell could state
    /// it, for a caller that needs the ordering contract rather than the text.
    pub const fn reported_running_version(&self) -> &ReportedVersion {
        &self.running_version
    }

    /// Begins a check. Allowed from every settled state; refused outright for
    /// an [`UpdateCapability::Unmanaged`] install, which has nothing to check.
    pub fn start_check(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() == UpdateCapability::Unmanaged {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        if !self.check_is_allowed() {
            return Err(self.rejected());
        }
        // A re-check must not lose the offer already on screen: if it fails,
        // that offer comes back exactly as it was.
        let carried = self.carried_offer();
        Ok(self.enter(UpdateState::Checking { carried }))
    }

    /// Whether [`Self::start_check`] would be accepted right now. Projected
    /// into [`UpdatePresentation::check_available`] so a surface with its own
    /// check control reads the rule instead of keeping a second copy of it.
    fn check_is_allowed(&self) -> bool {
        if self.capability() == UpdateCapability::Unmanaged {
            return false;
        }
        matches!(
            self.state,
            UpdateState::Idle
                | UpdateState::UpToDate
                | UpdateState::Available { .. }
                | UpdateState::AvailableExternally { .. }
                // None of these settles anything on its own: a check is how a
                // machine that has just been given a source, or whose suite has
                // moved, or that has just refreshed its package lists, finds
                // that out. The last one especially — the surface hands the user
                // one command to run outside the app, so the app has to be able
                // to see them run it (#726).
                | UpdateState::AvailableWithoutSource { .. }
                | UpdateState::AvailableButSourceUnread { .. }
                | UpdateState::WithheldBySuite { .. }
                | UpdateState::Skipped { .. }
                | UpdateState::Running { .. }
                // A check is exactly how an unverifiable restart gets settled:
                // the feed knows whether the target is still on offer.
                | UpdateState::RestartUnverified { .. }
                | UpdateState::Failed { .. }
        )
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
                recovery,
            } => {
                return Some(CarriedOffer::Failed {
                    version: version.clone(),
                    reason: reason.clone(),
                    staged: *staged,
                    recovery: *recovery,
                });
            }
            // A failed refresh over an unconfirmed restart leaves it exactly as
            // unconfirmed as it was, rather than collapsing it into a generic
            // failure that has forgotten which version it was about.
            UpdateState::RestartUnverified { target } => {
                return Some(CarriedOffer::UnverifiedRestart {
                    target: target.clone(),
                });
            }
            // An announcement with no delivery path comes back with its gap
            // intact: a failed refresh has established nothing new about what
            // this machine can fetch.
            UpdateState::AvailableWithoutSource { version, gap } => {
                return Some(CarriedOffer::WithoutSource {
                    version: version.clone(),
                    gap: *gap,
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
                hint: self
                    .install_kind
                    .system_update_hint_text(self.package_source.suite()),
            }
        })
    }

    /// The check completed and the feed had nothing newer.
    ///
    /// The feed is not the last word for a system-managed install: its own
    /// source can carry a build the feed does not name — the archive is a
    /// rolling window rebuilt from releases, and a machine on the candidate
    /// suite is served by a different lane than the one it polls. What apt can
    /// deliver is an upgrade whoever published it, so it is announced rather
    /// than covered by an "up to date" the package manager would contradict
    /// from the other direction (#725).
    pub fn check_found_none(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if !matches!(self.state, UpdateState::Checking { .. }) {
            return Err(self.rejected());
        }
        if self.capability() == UpdateCapability::SystemManaged
            && let Some(deliverable) = self.package_source.deliverable()
        {
            let next = UpdateState::AvailableExternally {
                version: deliverable.to_owned(),
                hint: self
                    .install_kind
                    .system_update_hint_text(self.package_source.suite()),
            };
            return Ok(self.enter(next));
        }
        Ok(self.enter(UpdateState::UpToDate))
    }

    /// The check found `version` published.
    ///
    /// What that means for this machine is decided here, never by a shell:
    ///
    /// * A [`UpdateCapability::SelfApply`] install gets an actionable
    ///   [`UpdateState::Available`]: it fetches its own payload, so what is
    ///   published is what it can have.
    /// * A [`UpdateCapability::SystemManaged`] install announces **what its own
    ///   source can deliver**, not what the feed published (#725). The feed is
    ///   a list of what exists in the world; apt's answer is what exists for
    ///   this machine, and the two differ by design — a `stable` subscriber is
    ///   deliberately never offered a candidate build (#689). So a source with
    ///   something newer produces [`UpdateState::AvailableExternally`] naming
    ///   *its* version, and a source with nothing produces
    ///   [`UpdateState::WithheldBySuite`], which announces no version to act on
    ///   and says which channel that verdict is about.
    /// * A [`UpdateCapability::SystemUnreachable`] install names the published
    ///   version and offers the way to make it reachable — never a package
    ///   manager that has never heard of it.
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
        let published = version.into();
        let next = match self.capability() {
            UpdateCapability::SelfApply => UpdateState::Available { version: published },
            // A source exists but has not been read: what the feed published is reachable,
            // and one command makes it so (#726).
            UpdateCapability::SystemManaged
                if matches!(
                    self.package_source,
                    PackageSourceEvidence::ConfiguredButUnread { .. }
                ) =>
            {
                UpdateState::AvailableButSourceUnread {
                    version: published,
                    suite: self.package_source.suite().map(str::to_owned),
                }
            }
            // A source exists but has not been read: what the feed published is reachable,
            // and one command makes it so (#726).
            UpdateCapability::SystemManaged
                if matches!(
                    self.package_source,
                    PackageSourceEvidence::ConfiguredButUnread { .. }
                ) =>
            {
                UpdateState::AvailableButSourceUnread {
                    version: published,
                    suite: self.package_source.suite().map(str::to_owned),
                }
            }
            UpdateCapability::SystemManaged => match self.package_source.deliverable() {
                Some(deliverable) => UpdateState::AvailableExternally {
                    version: deliverable.to_owned(),
                    hint: self
                        .install_kind
                        .system_update_hint_text(self.package_source.suite()),
                },
                None => match self.package_source.suite() {
                    Some(suite) => UpdateState::WithheldBySuite {
                        version: published,
                        suite: suite.to_owned(),
                    },
                    // No suite and no deliverable, on a lane whose delivery is
                    // not the kind that has to be established — rpm and flatpak
                    // never discover a version at all, so this is unreachable
                    // today; announcing the published version with the tool's
                    // own hint is what the lane would mean if one ever did.
                    None => UpdateState::AvailableExternally {
                        version: published,
                        hint: self.install_kind.system_update_hint_text(None),
                    },
                },
            },
            UpdateCapability::SystemUnreachable => UpdateState::AvailableWithoutSource {
                version: published,
                gap: self
                    .package_source
                    .gap()
                    // A reachable source cannot produce this capability.
                    .unwrap_or(SourceGap::Unestablished),
            },
            // Refused above.
            UpdateCapability::Unmanaged => return Err(self.rejected()),
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
                // The step that failed was the check, and running it again is
                // exactly what a retry from here does.
                recovery: FailureRecovery::RepeatTheStep,
            })),
        }
    }

    /// Suppresses the discovered version: nothing prompts for it again, but it
    /// stays known so the user can still install it on demand. Mirrors the
    /// per-channel skip the settings already persist.
    pub fn skip_offer(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        let (version, staged) = match &self.state {
            UpdateState::Available { version }
            | UpdateState::AvailableExternally { version, .. }
            // An announcement the machine cannot act on is still one the user
            // may silence; the instructions for making it actionable stay on
            // the surface, because they come from the capability rather than
            // from the offer.
            | UpdateState::AvailableWithoutSource { version, .. } => (version.clone(), false),
            UpdateState::ReadyToApply { version } => (version.clone(), true),
            UpdateState::Failed {
                target: Some(version),
                staged,
                ..
            } => (version.clone(), *staged),
            _ => return Err(self.rejected()),
        };
        // Skipping silences the prompt, not the instructions: a system-managed
        // install must keep being told how to get the release it skipped. An
        // install whose packaging tool cannot reach the release gets no such
        // hint — pointing at apt is exactly the claim #725 is about — and keeps
        // the repository instructions instead.
        let hint = (self.capability() == UpdateCapability::SystemManaged).then(|| {
            self.install_kind
                .system_update_hint_text(self.package_source.suite())
        });
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
    /// The AppImage lane can download and apply in a single call, so this
    /// reports the whole step at once and no separate `ReadyToApply` is left
    /// for the user to act on. It is the one-call shape, not the only shape
    /// that lane may use: a shell that stages first still walks
    /// [`Self::download_finished`] and [`Self::start_apply`]. Lanes that
    /// cannot apply while downloading are refused here.
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
                    recovery: FailureRecovery::RepeatTheStep,
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
                    recovery: FailureRecovery::RepeatTheStep,
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
    /// the old one. The *only* success exit from [`UpdateState::Applying`].
    ///
    /// There is deliberately no transition straight to [`UpdateState::Running`]
    /// here. Becoming the new version means replacing the process, which takes
    /// this lifecycle with it; anything still executing to call such a
    /// transition is by definition the old binary, and letting it claim the new
    /// one is exactly #660. `Running` is reached only by
    /// [`Self::restarted_into`] or [`Self::resumed_after_restart`], which are
    /// given the version that actually came up and check it.
    pub fn apply_needs_restart(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        match &self.state {
            UpdateState::Applying { version } => {
                let version = version.clone();
                Ok(self.enter(UpdateState::RestartPending { version }))
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
                    recovery: FailureRecovery::RepeatTheStep,
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
    ///
    /// A running version the shell could only report truncated leaves the two
    /// indistinguishable (#694): `0.11.0` is what a process running
    /// `0.11.0-beta.0.15` reports *and* what it would report having failed to
    /// leave `0.11.0-beta.0.14`. That is [`UpdateState::RestartUnverified`] —
    /// neither the success nor the downgrade, both of which would be invented.
    pub fn restarted_into(
        &mut self,
        running_version: impl Into<ReportedVersion>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        let UpdateState::RestartPending { version } = &self.state else {
            return Err(self.rejected());
        };
        let pending = version.clone();
        let running_version = running_version.into();
        let order = compare_reported_build_order(
            &running_version,
            &ReportedVersion::complete(pending.clone()),
        );
        let text = running_version.text.clone();
        self.running_version = running_version;
        let next = match order {
            Some(Ordering::Greater | Ordering::Equal) => UpdateState::Running { version: text },
            Some(Ordering::Less) => UpdateState::Failed {
                reason: format!(
                    "restart still runs {text}; the update to {pending} did not take effect"
                ),
                target: Some(pending),
                // The apply already consumed the payload; recovering starts over.
                staged: false,
                // And it starts over with a check: there is no payload to
                // re-apply, this process discovered nothing, and going
                // straight back into another restart is precisely what must
                // not happen behind one press (#701).
                recovery: FailureRecovery::CheckAgain,
            },
            None => UpdateState::RestartUnverified { target: pending },
        };
        Ok(self.enter(next))
    }

    /// The shell observed which version is *installed* — the package the
    /// running process was started from, as its package manager or its on-disk
    /// layout now reports it.
    ///
    /// This is the system-managed counterpart of [`Self::apply_needs_restart`]
    /// (#707). A `.deb`, `.rpm` or Flatpak install can be upgraded by its
    /// package manager *while the player runs*: the process keeps executing
    /// the binary it started with, About keeps reporting that build, and
    /// nothing in the app ever put the new one there, so no transition of the
    /// self-applying lane can describe it. It is not a Linux-only situation
    /// either — an MSI-style install replaced underneath a running process is
    /// the same thing — so the state lives here rather than in a shell.
    ///
    /// The observation becomes a claim only when the versions on hand
    /// *establish* that this process is behind:
    ///
    /// * strictly newer than the running build — the restart is real, and
    ///   [`UpdateState::ReplacedOnDisk`] says so;
    /// * the same build or an older one — nothing to act on, and
    ///   [`UpdateTransitionError::NotBehindInstalled`] reports that without
    ///   touching the state. A package manager rewriting the same version is
    ///   routine (it changes the file, not the build).
    /// * not orderable at all, because a truncated version cannot decide it
    ///   (#694) — refused for the same reason. Announcing a restart onto a
    ///   version that may well be the one already running is the #660 lie from
    ///   the other side, and this state exists to end that class, not to add a
    ///   new instance of it.
    ///
    /// A step already in flight keeps its own truth: a check, a download or an
    /// apply is refused here, and so is a restart this process has already
    /// asked for. What is on disk is a standing fact rather than an event, so
    /// the shell simply offers the observation again once the step settles.
    pub fn installed_version_observed(
        &mut self,
        installed_version: impl Into<ReportedVersion>,
    ) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() == UpdateCapability::Unmanaged {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        let installed_version = installed_version.into();
        if compare_reported_build_order(&installed_version, &self.running_version)
            != Some(Ordering::Greater)
        {
            return Err(UpdateTransitionError::NotBehindInstalled {
                installed: installed_version.text,
            });
        }
        match self.state {
            UpdateState::Checking { .. }
            | UpdateState::Downloading { .. }
            | UpdateState::Applying { .. }
            | UpdateState::RestartPending { .. } => Err(self.rejected()),
            _ => Ok(self.enter(UpdateState::ReplacedOnDisk {
                version: installed_version.text,
            })),
        }
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
    ///
    /// A failure whose recovery is [`FailureRecovery::CheckAgain`] is refused
    /// too (#701): restoring an offer there would put a version back on screen
    /// as actionable when nothing local backs it, and would let a surface
    /// labelled as a retry walk into an apply. [`Self::start_check`] is its
    /// recovery, and the projection offers exactly that.
    pub fn retry_failed_update(&mut self) -> Result<&UpdateState, UpdateTransitionError> {
        if self.capability() != UpdateCapability::SelfApply {
            return Err(UpdateTransitionError::CapabilityForbids(self.capability()));
        }
        let UpdateState::Failed {
            target: Some(version),
            staged,
            recovery: FailureRecovery::RepeatTheStep,
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
            version_in_use: self.running_version.text().to_owned(),
            target_version,
            claim,
            updates_message,
            about_message,
            action: self.action(),
            actions_enabled: !matches!(self.state, UpdateState::Checking { .. }),
            check_available: self.check_is_allowed(),
            action_closes_the_app: self.action_closes_the_app(),
            secondary_action: self.secondary_action(),
            upgrade_command: self.upgrade_command_for(self.presented_state().as_ref()),
            repository_setup: self.repository_setup(),
            refresh_command: self.refresh_command(),
            notice: self.notice.clone(),
        }
    }

    /// The command that upgrades this package and nothing else, when that is what stands
    /// between this machine and the version on screen (#759).
    ///
    /// Three conditions, and each is load-bearing:
    ///
    /// * A packaging tool owns the payload — [`UpdateCapability::SystemManaged`]. Anywhere
    ///   else the app either applies the update itself or has no delivery path at all, and an
    ///   apt command would be advice about a machine that is not this one.
    /// * The tool has something to deliver *right now* — a candidate it has read and that is
    ///   not what is already installed. Not merely "a source exists": a configured source apt
    ///   has not fetched delivers nothing yet (the remedy there is the refresh, #726), and a
    ///   source whose candidate is the installed version delivers nothing either. Either way
    ///   the command would answer "ok-player is already the newest version" under a surface
    ///   that had just named a build, which is #725's disagreement in miniature. The evidence
    ///   is re-read at check time, before the outcome is known, so a refresh that fails can
    ///   restore an offer beside evidence that has moved on — this is what catches that.
    /// * The state is one the command would act on: a build the source carries, or one the
    ///   user silenced and may still want. [`UpdateState::WithheldBySuite`] is deliberately
    ///   not one — that suite is not offering the build, so the command would do nothing.
    ///
    /// What this replaced was "Open software updater", which opened the desktop's system-wide
    /// updater. It is not narrowed here, it is gone: the objection was never which states it
    /// appeared in but what it does, which is start a transaction over every upgradable
    /// package on the machine.
    fn upgrade_command_for(&self, state: &UpdateState) -> Option<&'static str> {
        if self.capability() != UpdateCapability::SystemManaged {
            return None;
        }
        // Not `carries_the_package`: a source that exists is not a source with something to
        // give, and only the second is worth a command.
        self.package_source.deliverable()?;
        matches!(
            state,
            UpdateState::AvailableExternally { .. } | UpdateState::Skipped { .. }
        )
        .then(|| self.install_kind.upgrade_command())
        .flatten()
    }

    /// The instructions for giving this install a delivery path, shown only
    /// where it has none and a version is in play — the moment the user is
    /// being told about something they cannot otherwise get.
    fn repository_setup(&self) -> Option<RepositorySetup> {
        if self.capability() != UpdateCapability::SystemUnreachable {
            return None;
        }
        if !self.install_kind.publishes_a_repository() {
            return None;
        }
        self.presented_state().target_version()?;
        // The suite comes from the channel this build was packaged for, read off the stanza
        // the package carries — never from a constant. A machine with no source still has a
        // channel: the one it was installed from. Where even that cannot be established the
        // commands are withheld, because the only alternative is to guess, and a wrong guess
        // here silently moves the user to another channel (#726).
        apt_repository_setup(self.packaged_suite.as_deref()?)
    }

    /// The command that reads a configured-but-unfetched source, when that is the state.
    fn refresh_command(&self) -> Option<&'static str> {
        matches!(
            self.presented_state().as_ref(),
            UpdateState::AvailableButSourceUnread { .. }
        )
        .then(|| self.install_kind.refresh_command())
        .flatten()
    }

    fn version_claim(&self) -> VersionClaim {
        if self.capability() == UpdateCapability::Unmanaged {
            return VersionClaim::NotApplicable;
        }
        match &self.state {
            UpdateState::UpToDate
            | UpdateState::Running { .. }
            // The newest build the machine's own channel carries *is* what it
            // is running: current, on the only channel the verdict is about.
            | UpdateState::WithheldBySuite { .. } => VersionClaim::Current,
            UpdateState::Available { version }
            | UpdateState::AvailableExternally { version, .. }
            // A published build this machine cannot fetch is still a build this
            // session is behind; what it can do about it is the part that
            // differs.
            | UpdateState::AvailableWithoutSource { version, .. }
            | UpdateState::AvailableButSourceUnread { version, .. }
            | UpdateState::Skipped { version, .. }
            | UpdateState::Downloading { version }
            | UpdateState::ReadyToApply { version }
            | UpdateState::Applying { version }
            // The applied bits are on disk, but this process is still the old
            // binary: superseded, not current (#660).
            | UpdateState::RestartPending { version }
            // Same again for bits a package manager wrote (#707): whoever put
            // the newer build on disk, this process is not running it.
            | UpdateState::ReplacedOnDisk { version } => VersionClaim::Superseded {
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
            // Refreshing an unconfirmed restart does not turn it into a known
            // one: what is being checked is precisely which build is running.
            UpdateState::Checking {
                carried: Some(CarriedOffer::UnverifiedRestart { .. }),
            } => VersionClaim::Unknown,
            // A refresh does not un-know the offer it is refreshing.
            UpdateState::Checking {
                carried: Some(offer),
            } => VersionClaim::Superseded {
                newer: offer.version().to_owned(),
            },
            UpdateState::Idle
            | UpdateState::Checking { carried: None }
            | UpdateState::ManagedExternally { .. }
            // The restart happened; which build came up is precisely what
            // cannot be told, so nothing may be claimed either way (#694).
            | UpdateState::RestartUnverified { .. }
            | UpdateState::Failed { target: None, .. } => VersionClaim::Unknown,
        }
    }

    /// The Updates message for the current state.
    ///
    /// A refresh over a standing offer describes *both*: the offer is what the
    /// surface is showing — with its own controls, kept by [`Self::action`] —
    /// and the check is a status on top of it. Replacing the offer's message
    /// with the check's would hide whether the carried version is available,
    /// skipped, staged or failed, and leave a shell no way to say it except by
    /// keeping a second offer model of its own.
    fn updates_message(&self, claim: &VersionClaim) -> String {
        if matches!(claim, VersionClaim::NotApplicable) {
            return "Updates are disabled for development builds.".to_owned();
        }
        match &self.state {
            UpdateState::Checking {
                carried: Some(offer),
            } => format!(
                "{} {CHECKING_MESSAGE}",
                self.message_for(&offer.clone().into_state())
            ),
            state => self.message_for(state),
        }
    }

    /// The message for one state on its own, with no check in flight over it.
    fn message_for(&self, state: &UpdateState) -> String {
        match state {
            UpdateState::Idle => "OK Player has not checked for updates yet.".to_owned(),
            UpdateState::Checking { .. } => CHECKING_MESSAGE.to_owned(),
            UpdateState::UpToDate => "OK Player is up to date.".to_owned(),
            UpdateState::Available { version } => format!("Version {version} is available."),
            UpdateState::AvailableExternally { version, hint } => {
                format!("Version {version} is available. {hint}")
            }
            UpdateState::AvailableWithoutSource { version, gap } => {
                let tool = self.install_kind.system_update_tool();
                let observation = match gap {
                    SourceGap::NoSource => format!(
                        "No OK Player repository is configured on this system, so {tool} has nothing to install it from."
                    ),
                    SourceGap::Unestablished => format!(
                        "OK Player could not ask {tool} what it can install, so it cannot say whether {tool} would deliver it."
                    ),
                };
                // The instructions are only offered when the channel to offer is known.
                // Guessing one would move the user's subscription, so the alternative is
                // saying so and pointing at the download.
                let remedy = if self.repository_setup().is_some() {
                    format!("Add the repository to get it with {tool}, or download the package.")
                } else {
                    format!(
                        "OK Player cannot tell which channel this build came from, so it will not guess at {tool} commands that could move you to another one — download the package instead."
                    )
                };
                format!("Version {version} is published. {observation} {remedy}")
            }
            UpdateState::AvailableButSourceUnread { version, suite } => {
                let tool = self.install_kind.system_update_tool();
                let subscription = match suite {
                    Some(suite) => format!("the {suite} OK Player repository"),
                    None => "an OK Player repository".to_owned(),
                };
                let refresh = match self.install_kind.refresh_command() {
                    Some(command) => format!(" Run {command} to read it."),
                    None => String::new(),
                };
                format!(
                    "Version {version} is published. This system subscribes to {subscription}, but {tool} has not read it yet, so it cannot install it.{refresh}"
                )
            }
            UpdateState::WithheldBySuite { version, suite } => format!(
                "Version {version} is published, but the {suite} suite this system subscribes to is not offering it. OK Player is up to date on {suite}; refreshing this system's package lists is what would change that answer."
            ),
            UpdateState::Downloading { version } => format!("Downloading version {version}…"),
            UpdateState::ReadyToApply { version } => {
                format!("Version {version} is ready to install.")
            }
            UpdateState::Applying { version } => format!("Installing version {version}…"),
            UpdateState::RestartPending { version } => format!(
                "Version {version} is installed. Restart OK Player to start running it — this session is still on {}.",
                self.running_version
            ),
            UpdateState::ReplacedOnDisk { version } => format!(
                "Version {version} is installed — restart OK Player to use it. This session is still on {}.",
                self.running_version
            ),
            UpdateState::Running { version } => {
                format!("OK Player is now running version {version}.")
            }
            UpdateState::RestartUnverified { target } => format!(
                "OK Player restarted after installing version {target}, but this build reports its version as {} without the part that would tell the two apart, so whether the update took effect cannot be confirmed. Check for updates to settle it.",
                self.running_version
            ),
            UpdateState::Skipped {
                version,
                hint: Some(hint),
                ..
            } => format!("Version {version} was skipped. {hint}"),
            UpdateState::Skipped { version, .. } => {
                format!("Version {version} was skipped.")
            }
            UpdateState::ManagedExternally { hint } => hint.clone(),
            // A failure whose recovery is a check says so, because the check
            // is what the surface is about to offer (#701).
            UpdateState::Failed {
                reason,
                target: Some(version),
                recovery: FailureRecovery::CheckAgain,
                ..
            } => format!(
                "The update to version {version} failed: {reason}. Check for updates to try it again."
            ),
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

    /// The About message for the current state.
    ///
    /// Read off the *presented* state for the same reason the Updates message
    /// is: a refresh keeps the offer it is refreshing on screen, and the two
    /// surfaces are rendered from one projection, so About reading the raw
    /// `Checking` state would say "version X is available" beside an Updates
    /// line that says the same version was skipped or failed.
    fn about_message(&self, claim: &VersionClaim) -> String {
        let running = &self.running_version;
        let presented = self.presented_state();
        match claim {
            VersionClaim::Current => match presented.as_ref() {
                // "Up to date" alone would hide which channel that is true of,
                // next to an Updates line naming a build this one does not
                // carry (#725).
                UpdateState::WithheldBySuite { suite, .. } => {
                    format!("OK Player {running} — up to date on {suite}.")
                }
                _ => format!("OK Player {running} — up to date."),
            },
            VersionClaim::Superseded { newer } => {
                if matches!(presented.as_ref(), UpdateState::ReplacedOnDisk { .. }) {
                    format!(
                        "OK Player {running} — version {newer} is installed; restart to use it."
                    )
                } else if matches!(presented.as_ref(), UpdateState::RestartPending { .. }) {
                    format!("OK Player {running} — restart to finish updating to {newer}.")
                } else if matches!(presented.as_ref(), UpdateState::Failed { .. }) {
                    format!("OK Player {running} — updating to {newer} failed.")
                } else if matches!(presented.as_ref(), UpdateState::Skipped { .. }) {
                    format!("OK Player {running} — version {newer} was skipped.")
                } else if matches!(
                    presented.as_ref(),
                    UpdateState::AvailableWithoutSource { .. }
                ) {
                    format!(
                        "OK Player {running} — version {newer} is published; this install has no repository to get it from."
                    )
                } else {
                    format!("OK Player {running} — version {newer} is available.")
                }
            }
            VersionClaim::Unknown => match presented.as_ref() {
                UpdateState::ManagedExternally { hint } => format!("OK Player {running} — {hint}"),
                UpdateState::RestartUnverified { target } => format!(
                    "OK Player {running} — the update to {target} could not be confirmed from this build's version."
                ),
                _ => format!("OK Player {running}."),
            },
            VersionClaim::NotApplicable => {
                format!("OK Player {running} — development build; updates are disabled.")
            }
        }
    }

    /// The state the surface is presenting controls for. A refresh keeps the
    /// offer it is refreshing on screen, controls and all — `actions_enabled`
    /// says they cannot be pressed yet — so everything derived from the offer
    /// reads it here rather than each deriving it again.
    fn presented_state(&self) -> Cow<'_, UpdateState> {
        match &self.state {
            UpdateState::Checking {
                carried: Some(offer),
            } => Cow::Owned(offer.clone().into_state()),
            state => Cow::Borrowed(state),
        }
    }

    fn action(&self) -> Option<UpdateAction> {
        if self.capability() == UpdateCapability::Unmanaged {
            return None;
        }
        self.action_for(&self.presented_state())
    }

    fn action_for(&self, state: &UpdateState) -> Option<UpdateAction> {
        match state {
            UpdateState::Idle
            | UpdateState::UpToDate
            | UpdateState::Running { .. }
            // Nothing can be applied here — the payload is already on disk;
            // only a check can say which build is running.
            | UpdateState::RestartUnverified { .. } => Some(UpdateAction::CheckNow),
            UpdateState::Available { .. } => Some(UpdateAction::DownloadUpdate),
            // Nothing here can install anything. What the surface can offer is
            // the one step that turns the announcement into something the
            // machine could act on (#725).
            UpdateState::AvailableWithoutSource { .. } => Some(UpdateAction::CopyRepositorySetup),
            // The repository is already there. One command reads it, and that is the whole
            // of what this machine needs (#726).
            UpdateState::AvailableButSourceUnread { .. } => Some(UpdateAction::CopyRefreshCommand),
            // Up to date on this machine's channel: the same offer as any other
            // settled state, which is to look again.
            UpdateState::WithheldBySuite { .. } => Some(UpdateAction::CheckNow),
            UpdateState::ReadyToApply { .. } => Some(UpdateAction::ApplyAndRestart),
            UpdateState::RestartPending { .. } => Some(UpdateAction::RestartToFinish),
            // The build is already installed; the restart is the whole of what
            // is left to do, and it installs nothing.
            UpdateState::ReplacedOnDisk { .. } => Some(UpdateAction::RestartToUseInstalled),
            // A failure the recovery cannot repeat is not a retry, and must
            // not be offered as one: the check it really performs is what the
            // surface says, and the install it discovers is the press after
            // that (#701).
            UpdateState::Failed {
                recovery: FailureRecovery::CheckAgain,
                ..
            } => Some(UpdateAction::CheckNow),
            UpdateState::Failed { .. } => Some(UpdateAction::Retry),
            // A skipped version stays installable on demand, but only where the
            // app installs anything at all.
            UpdateState::Skipped { .. } => match self.capability() {
                UpdateCapability::SelfApply => Some(UpdateAction::InstallAnyway),
                // An install with no delivery path keeps the one action it has:
                // the setup that would give it one (#725).
                UpdateCapability::SystemUnreachable => Some(UpdateAction::CopyRepositorySetup),
                // The system-managed twin of `InstallAnyway`: skipping silenced the
                // prompt, not the version, and the command that installs it is what
                // this lane has instead of an in-app install (#759).
                UpdateCapability::SystemManaged => self
                    .upgrade_command_for(state)
                    .map(|_| UpdateAction::CopyUpgradeCommand),
                UpdateCapability::Unmanaged => None,
            },
            // A system-managed update is applied by the packaging tool and never in the
            // app — but this is the surface the user is looking at, so it hands over the
            // one command that upgrades this package and nothing else (#759). Before that
            // it offered a way into the desktop's system-wide updater and no way to update
            // OK Player, which is a state with an announcement and nothing to act on.
            UpdateState::AvailableExternally { .. } => self
                .upgrade_command_for(state)
                .map(|_| UpdateAction::CopyUpgradeCommand),
            // An install the system owns outright offers not even a check; a
            // download or apply in flight offers nothing, and a check with no
            // offer behind it has nothing to keep on screen.
            UpdateState::ManagedExternally { .. }
            | UpdateState::Checking { .. }
            | UpdateState::Downloading { .. }
            | UpdateState::Applying { .. } => None,
        }
    }

    /// Whether the offered action ends the session. An action that always
    /// restarts does; so does accepting an offer on a lane that applies while
    /// it downloads — but only where accepting it really is the one call that
    /// downloads, applies and relaunches.
    fn action_closes_the_app(&self) -> bool {
        let presented = self.presented_state();
        let Some(action) = self.action() else {
            return false;
        };
        if action.closes_the_app() {
            return true;
        }
        if !self.install_kind.applies_while_downloading() {
            return false;
        }
        match action {
            // Accepting a live offer on the one-call lane is the download,
            // the apply and the relaunch in one step.
            UpdateAction::DownloadUpdate => true,
            // Installing a skipped version is that same one call only when
            // there is nothing staged. A skip that kept a verified payload
            // sends `install_anyway` to `ReadyToApply` and stops there —
            // nothing is applied and nothing closes until the user takes the
            // `ApplyAndRestart` that follows, which says so itself.
            UpdateAction::InstallAnyway => !matches!(
                presented.as_ref(),
                UpdateState::Skipped { staged: true, .. }
            ),
            _ => false,
        }
    }

    /// The secondary action beside [`Self::action`]. A live offer — discovered,
    /// or discovered and then failed — can be skipped; nothing else can.
    fn secondary_action(&self) -> Option<UpdateAction> {
        if self.capability() == UpdateCapability::Unmanaged {
            return None;
        }
        self.secondary_action_for(&self.presented_state())
    }

    fn secondary_action_for(&self, state: &UpdateState) -> Option<UpdateAction> {
        match state {
            UpdateState::Available { .. }
            | UpdateState::AvailableExternally { .. }
            // An announcement with no delivery path is still an announcement
            // the user may silence — the more so, since acting on it takes work.
            | UpdateState::AvailableWithoutSource { .. }
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
    /// A transition taken by name, for tables of steps a state must refuse.
    type Transition =
        for<'a> fn(&'a mut UpdateLifecycle) -> Result<&'a UpdateState, UpdateTransitionError>;

    const SELF_APPLY_KINDS: [InstallKind; 2] =
        [InstallKind::WindowsVelopack, InstallKind::AppImage];
    const SYSTEM_MANAGED_KINDS: [InstallKind; 3] =
        [InstallKind::Deb, InstallKind::Rpm, InstallKind::Flatpak];

    fn evidence() -> InstallEvidence {
        InstallEvidence::default()
    }

    /// A machine subscribed to `suite`, whose packaging tool would install
    /// `deliverable` next.
    fn subscribed(suite: &str, deliverable: Option<&str>) -> PackageSourceEvidence {
        PackageSourceEvidence::Source {
            suite: suite.to_owned(),
            deliverable: deliverable.map(str::to_owned),
        }
    }

    /// A lifecycle whose delivery path is established wherever the kind has one
    /// to establish (#725): the apt lane is only `SystemManaged` once a source
    /// was observed to carry OK Player, so every invariant about the
    /// system-managed lane has to start from a machine that has one.
    fn system_managed_lifecycle(kind: InstallKind, running: &str) -> UpdateLifecycle {
        let mut lifecycle = UpdateLifecycle::new(kind, running);
        if kind.delivery_must_be_established() {
            lifecycle.package_source_observed(subscribed("stable", Some("2.0.0")));
        }
        lifecycle
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
        // A lane whose delivery has to be established reaches a different half
        // of the state space depending on the answer (#725), and an invariant
        // about "every reachable state" has to mean both halves.
        let sources = if kind.delivery_must_be_established() {
            vec![
                PackageSourceEvidence::Unestablished,
                PackageSourceEvidence::NoSource,
                // The state every .deb install passes through on first launch (#726).
                PackageSourceEvidence::ConfiguredButUnread {
                    suite: Some("candidate".to_owned()),
                },
                PackageSourceEvidence::ConfiguredButUnread { suite: None },
                subscribed("stable", Some("2.0.0")),
                subscribed("stable", None),
            ]
        } else {
            vec![PackageSourceEvidence::Unestablished]
        };
        for source in sources {
            sweep_reachable_states_from(kind, source, &mut observe);
        }
    }

    /// One sweep, for one answer about what the packaging tool can deliver.
    fn sweep_reachable_states_from(
        kind: InstallKind,
        package_source: PackageSourceEvidence,
        observe: &mut impl FnMut(&UpdateLifecycle),
    ) {
        let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
        lifecycle.package_source_observed(package_source);
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
            // Last, because a package manager replacing the install underneath
            // the process outranks whatever the session was doing: every state
            // reachable after it is reachable from here.
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.installed_version_observed("4.0.0");
            }),
            Box::new(|life: &mut UpdateLifecycle| {
                let _ = life.start_check();
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
                "extract-and-run AppImage is corroborated by APPDIR holding the executable",
                InstallEvidence {
                    appimage_path: Some("/home/u/OK_Player-x86_64.AppImage".to_owned()),
                    appdir_path: Some("/tmp/appimage_extracted_9f1".to_owned()),
                    executable_path: Some(
                        "/tmp/appimage_extracted_9f1/usr/bin/ok-player".to_owned(),
                    ),
                    package_ownership: PackageOwnership::Unowned,
                    ..evidence()
                },
                InstallKind::AppImage,
            ),
            (
                // Both variables are exported to whatever an AppImage launches,
                // so an unpackaged build started from one inherits them while
                // living nowhere near the image.
                "inherited AppImage variables do not claim an executable outside APPDIR",
                InstallEvidence {
                    appimage_path: Some("/home/u/SomeLauncher-x86_64.AppImage".to_owned()),
                    appdir_path: Some("/tmp/.mount_Launcher".to_owned()),
                    executable_path: Some(
                        "/home/u/dev/ok-player/target/release/okp-linux-gtk".to_owned(),
                    ),
                    package_ownership: PackageOwnership::Unowned,
                    ..evidence()
                },
                InstallKind::DevBuild,
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

    /// Invariant: `Applying` can only succeed into `RestartPending`. Every
    /// other transition attempted from it is refused and leaves the state
    /// untouched — including any that would claim the new version is already
    /// running, which nothing still executing is in a position to know.
    #[test]
    fn applying_can_only_succeed_into_restart_pending() {
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
        assert_eq!(
            restarting.running_version(),
            "1.0.0",
            "a successful apply does not make this process the new build"
        );
        assert_eq!(
            restarting.describe().claim,
            VersionClaim::Superseded {
                newer: "2.0.0".to_owned()
            }
        );

        let mut failing = applying();
        assert_eq!(
            failing.apply_failed("disk full").unwrap(),
            &UpdateState::Failed {
                reason: "disk full".to_owned(),
                target: Some("2.0.0".to_owned()),
                staged: true,
                recovery: FailureRecovery::RepeatTheStep,
            }
        );
    }

    /// Invariant: a `SystemManaged` install never offers an in-app "Update
    /// now"; it announces the update and hands the user the command their package
    /// manager takes (#759). The offer is not nothing — that was the state the
    /// operator was stranded in — but nothing about it installs anything here.
    #[test]
    fn system_managed_never_offers_an_in_app_update_action() {
        for kind in SYSTEM_MANAGED_KINDS {
            let mut lifecycle = system_managed_lifecycle(kind, "1.0.0");
            lifecycle.start_check().unwrap();
            let state = lifecycle.check_found("2.0.0").unwrap().clone();
            assert!(
                matches!(state, UpdateState::AvailableExternally { .. }),
                "{kind} should stop at AvailableExternally, got {state:?}"
            );

            let presentation = lifecycle.describe();
            assert!(
                presentation
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "{kind} must offer no in-app apply, offered {:?}",
                presentation.action
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

            let mut failed_recheck = system_managed_lifecycle(kind, "1.0.0");
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
            // The offer the refresh was over comes back whole, controls included — which
            // for this lane is the command its packaging tool takes, and nothing in-app.
            assert!(
                failed_recheck
                    .describe()
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "{kind} came back with an in-app offer"
            );

            let mut failed_check = system_managed_lifecycle(kind, "1.0.0");
            failed_check.start_check().unwrap();
            failed_check.check_failed("network unreachable").unwrap();
            assert_eq!(
                failed_check.retry_failed_update(),
                Err(UpdateTransitionError::CapabilityForbids(
                    UpdateCapability::SystemManaged
                )),
                "{kind} resumes no in-app offer of its own"
            );

            let mut restored = system_managed_lifecycle(kind, "1.0.0");
            restored.start_check().unwrap();
            restored.check_found("2.0.0").unwrap();
            restored.start_check().unwrap();
            restored.check_failed("feed unavailable").unwrap();
            restored.skip_offer().unwrap();
            assert!(
                matches!(restored.state(), UpdateState::Skipped { .. }),
                "{kind} can still skip a restored announcement"
            );
            assert!(
                restored
                    .describe()
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "{kind} offered an in-app install over a skipped announcement"
            );

            let mut skipped = system_managed_lifecycle(kind, "1.0.0");
            skipped.start_check().unwrap();
            skipped.check_found("2.0.0").unwrap();
            skipped.skip_offer().unwrap();
            assert!(
                skipped
                    .describe()
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "{kind} cannot install a skipped version in app, offered {:?}",
                skipped.describe().action
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

    /// Invariant: the sweep the capability tests run actually walks through
    /// [`UpdateState::ReplacedOnDisk`]. Without this, adding the state would
    /// leave every sweep-based invariant green for the worst possible reason —
    /// never having reached it.
    #[test]
    fn the_state_sweep_reaches_a_replaced_install() {
        for kind in SELF_APPLY_KINDS.iter().chain(&SYSTEM_MANAGED_KINDS) {
            let mut seen = false;
            sweep_reachable_states(*kind, |life| {
                seen |= matches!(life.state(), UpdateState::ReplacedOnDisk { .. });
            });
            assert!(seen, "{kind} never reached ReplacedOnDisk in the sweep");
        }
    }

    /// A package manager can upgrade a system-managed install while the player
    /// runs (#707). The process keeps executing the old build, so the surfaces
    /// must say so — the same class as #660, arriving from the other side.
    #[test]
    fn a_package_upgrade_under_a_running_player_asks_for_the_restart() {
        for kind in SYSTEM_MANAGED_KINDS {
            let mut lifecycle = UpdateLifecycle::new(kind, "0.11.0-beta.0.197");
            lifecycle.start_check().unwrap();
            lifecycle.check_found_none().unwrap();
            assert_eq!(
                lifecycle.describe().claim,
                VersionClaim::Current,
                "{kind} starts out believing it is current"
            );

            lifecycle
                .installed_version_observed("0.11.0-beta.0.208")
                .expect("a newer installed build is a restart this session must mention");

            assert_eq!(
                lifecycle.state(),
                &UpdateState::ReplacedOnDisk {
                    version: "0.11.0-beta.0.208".to_owned()
                }
            );
            let presentation = lifecycle.describe();
            assert_eq!(
                presentation.claim,
                VersionClaim::Superseded {
                    newer: "0.11.0-beta.0.208".to_owned()
                },
                "{kind} may not keep claiming the running build is current"
            );
            assert_eq!(presentation.version_in_use, "0.11.0-beta.0.197");
            assert_eq!(
                presentation.target_version.as_deref(),
                Some("0.11.0-beta.0.208")
            );

            for message in [&presentation.updates_message, &presentation.about_message] {
                assert!(
                    message.contains("0.11.0-beta.0.208")
                        && message.to_lowercase().contains("restart"),
                    "{kind} should name the installed build and the restart, got {message}"
                );
                assert!(
                    !message.contains("up to date"),
                    "{kind} must not still read as current: {message}"
                );
            }
            assert!(
                presentation.updates_message.contains("0.11.0-beta.0.197"),
                "the Updates surface should say which build this session is on: {}",
                presentation.updates_message
            );

            assert_eq!(
                presentation.action,
                Some(UpdateAction::RestartToUseInstalled),
                "{kind} should offer the restart"
            );
            assert!(
                !UpdateAction::RestartToUseInstalled.applies_update_in_app(),
                "{kind} must not acquire an in-app install by offering a restart"
            );
            assert!(presentation.action_closes_the_app);
            assert!(presentation.actions_enabled);
            assert_eq!(
                presentation.secondary_action, None,
                "a build already on disk cannot be skipped"
            );
        }
    }

    /// The state is not a Linux special case: an install replaced underneath a
    /// self-applying process — an MSI-style upgrade of the Windows install —
    /// reaches exactly the same place (#707).
    #[test]
    fn a_self_applying_install_replaced_underneath_reports_the_same_restart() {
        for kind in SELF_APPLY_KINDS {
            let mut lifecycle = UpdateLifecycle::new(kind, "1.0.0");
            lifecycle
                .installed_version_observed("1.1.0")
                .expect("an install replaced on disk is observable on every lane");

            let presentation = lifecycle.describe();
            assert_eq!(
                presentation.claim,
                VersionClaim::Superseded {
                    newer: "1.1.0".to_owned()
                }
            );
            assert_eq!(
                presentation.action,
                Some(UpdateAction::RestartToUseInstalled),
                "{kind} should offer the restart rather than re-download what is installed"
            );
        }
    }

    /// The other half of the contract: an installed version that is not newer
    /// says nothing at all. A package manager rewriting the same version is
    /// routine, and a restart prompt for it would be noise.
    #[test]
    fn an_installed_version_that_is_not_newer_is_never_announced() {
        for installed in ["0.11.0-beta.0.197", "0.11.0-beta.0.196", "0.10.0"] {
            let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.197");
            lifecycle.start_check().unwrap();
            lifecycle.check_found_none().unwrap();

            assert_eq!(
                lifecycle.installed_version_observed(installed),
                Err(UpdateTransitionError::NotBehindInstalled {
                    installed: installed.to_owned()
                }),
                "{installed} is not ahead of the running build"
            );
            assert_eq!(
                lifecycle.state(),
                &UpdateState::UpToDate,
                "a refused observation leaves the state alone"
            );
            assert_eq!(lifecycle.describe().claim, VersionClaim::Current);
            assert_eq!(lifecycle.describe().action, Some(UpdateAction::CheckNow));
        }
    }

    /// And the version the model cannot order is never announced either (#694,
    /// #697): a truncated string is equally consistent with the build already
    /// running, so "restart to use it" would be a guess about the one thing
    /// this state exists to be honest about.
    #[test]
    fn an_unorderable_installed_version_is_never_announced_as_a_restart() {
        // The running build is only known truncated, so an installed
        // prerelease of the same core could be the build already executing.
        let mut truncated_running = UpdateLifecycle::new(
            InstallKind::WindowsVelopack,
            ReportedVersion::truncated("0.11.0"),
        );
        assert_eq!(
            truncated_running
                .installed_version_observed(ReportedVersion::complete("0.11.0-beta.0.208")),
            Err(UpdateTransitionError::NotBehindInstalled {
                installed: "0.11.0-beta.0.208".to_owned()
            })
        );
        assert_eq!(truncated_running.state(), &UpdateState::Idle);

        // And the same when it is the installed version that came back
        // truncated.
        let mut truncated_installed =
            UpdateLifecycle::new(InstallKind::WindowsVelopack, "0.11.0-beta.0.197");
        assert_eq!(
            truncated_installed.installed_version_observed(ReportedVersion::truncated("0.11.0")),
            Err(UpdateTransitionError::NotBehindInstalled {
                installed: "0.11.0".to_owned()
            })
        );

        // A truncated string still decides a different numeric core, because
        // no missing prerelease tail could close that gap.
        let mut higher_core =
            UpdateLifecycle::new(InstallKind::WindowsVelopack, "0.11.0-beta.0.197");
        higher_core
            .installed_version_observed(ReportedVersion::truncated("0.12.0"))
            .expect("a higher core is decided with or without the tail");
        assert_eq!(
            higher_core.state(),
            &UpdateState::ReplacedOnDisk {
                version: "0.12.0".to_owned()
            }
        );
    }

    /// A step in flight is not overwritten by the observation, and the
    /// observation is not lost: it is a standing fact about the disk, so the
    /// shell offers it again once the step settles.
    #[test]
    fn an_upgrade_observed_mid_step_is_refused_and_still_lands_afterwards() {
        let mut checking = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        checking.start_check().unwrap();
        assert_eq!(
            checking.installed_version_observed("2.0.0"),
            Err(UpdateTransitionError::NotAllowedFrom(
                UpdateState::Checking { carried: None }
            ))
        );
        checking.check_found_none().unwrap();
        checking
            .installed_version_observed("2.0.0")
            .expect("the same observation lands once the check settles");
        assert_eq!(
            checking.state(),
            &UpdateState::ReplacedOnDisk {
                version: "2.0.0".to_owned()
            }
        );

        // A restart this process already asked for is left alone: it is
        // already telling the user the only thing there is to do.
        let mut applying = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        applying.start_check().unwrap();
        applying.check_found("1.5.0").unwrap();
        applying.start_download().unwrap();
        assert!(matches!(
            applying.installed_version_observed("2.0.0"),
            Err(UpdateTransitionError::NotAllowedFrom(_))
        ));
        applying.download_and_apply_needs_restart().unwrap();
        assert!(matches!(
            applying.installed_version_observed("2.0.0"),
            Err(UpdateTransitionError::NotAllowedFrom(_))
        ));
        assert_eq!(
            applying.state(),
            &UpdateState::RestartPending {
                version: "1.5.0".to_owned()
            }
        );
    }

    /// Once the disk is ahead, the restart is the only thing on offer — and a
    /// second upgrade landing before the user gets round to it re-points the
    /// state at the build that is actually installed.
    #[test]
    fn a_replaced_install_offers_the_restart_and_nothing_else() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        lifecycle.installed_version_observed("2.0.0").unwrap();

        let refusals: [Transition; 6] = [
            UpdateLifecycle::start_check,
            UpdateLifecycle::skip_offer,
            UpdateLifecycle::start_download,
            UpdateLifecycle::install_anyway,
            UpdateLifecycle::start_apply,
            UpdateLifecycle::retry_failed_update,
        ];
        for refused in refusals {
            assert!(
                refused(&mut lifecycle).is_err(),
                "nothing but the restart is available from {:?}",
                lifecycle.state()
            );
        }
        assert_eq!(
            lifecycle.state(),
            &UpdateState::ReplacedOnDisk {
                version: "2.0.0".to_owned()
            }
        );

        lifecycle
            .installed_version_observed("3.0.0")
            .expect("a further upgrade is still an observation about the disk");
        assert_eq!(
            lifecycle.state(),
            &UpdateState::ReplacedOnDisk {
                version: "3.0.0".to_owned()
            },
            "the state must name the build that is on disk now"
        );
    }

    /// A dev build has no install for anything to replace, and no update lane
    /// to report one on.
    #[test]
    fn a_dev_build_never_reports_a_replaced_install() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::DevBuild, "0.0.0-dev");
        assert_eq!(
            lifecycle.installed_version_observed("9.9.9"),
            Err(UpdateTransitionError::CapabilityForbids(
                UpdateCapability::Unmanaged
            ))
        );
        assert_eq!(lifecycle.state(), &UpdateState::Idle);
    }

    /// A surface that keeps its own check control must be told when pressing
    /// it would be refused — a build already installed has nothing left to
    /// discover, and a control that silently does nothing is its own small lie.
    #[test]
    fn the_projection_says_when_a_check_would_be_refused() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        assert!(lifecycle.describe().check_available);
        lifecycle.start_check().unwrap();
        assert!(
            !lifecycle.describe().check_available,
            "a check in flight is not a state to start another from"
        );
        lifecycle.check_found_none().unwrap();
        assert!(lifecycle.describe().check_available);

        lifecycle.installed_version_observed("2.0.0").unwrap();
        assert!(
            !lifecycle.describe().check_available,
            "the answer is already on disk"
        );
        assert!(lifecycle.start_check().is_err());

        assert!(
            !UpdateLifecycle::new(InstallKind::DevBuild, "0.0.0-dev")
                .describe()
                .check_available
        );

        // The projection and the transition cannot drift: whatever the state,
        // the flag is exactly whether the transition is accepted.
        for kind in SELF_APPLY_KINDS.iter().chain(&SYSTEM_MANAGED_KINDS) {
            sweep_reachable_states(*kind, |life| {
                let claimed = life.describe().check_available;
                let accepted = life.clone().start_check().is_ok();
                assert_eq!(
                    claimed,
                    accepted,
                    "{kind} projected check_available={claimed} in state {:?}",
                    life.state()
                );
            });
        }
    }

    /// #725, the machine that reported it: a `.deb` installed from a
    /// downloaded file, with no OK Player apt source anywhere. The surface may
    /// still name the published build — it exists — but everything it offers
    /// has to be something that can actually happen from here.
    #[test]
    fn a_deb_with_no_apt_source_offers_the_repository_and_never_the_updater() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.packaged_suite_observed(Some("candidate".to_owned()));
        lifecycle.package_source_observed(PackageSourceEvidence::NoSource);
        assert_eq!(
            lifecycle.capability(),
            UpdateCapability::SystemUnreachable,
            "a package manager with nothing to fetch from does not manage this install"
        );

        lifecycle.start_check().unwrap();
        assert_eq!(
            lifecycle.check_found("0.11.0-beta.0.210").unwrap(),
            &UpdateState::AvailableWithoutSource {
                version: "0.11.0-beta.0.210".to_owned(),
                gap: SourceGap::NoSource,
            }
        );

        let presentation = lifecycle.describe();
        assert!(
            presentation.updates_message.contains("0.11.0-beta.0.210"),
            "the published build is still named: {}",
            presentation.updates_message
        );
        assert_eq!(
            presentation.upgrade_command, None,
            "there is no source to upgrade from; the remedy is to add one"
        );
        // The exact sentence #725 is about must not survive anywhere.
        assert!(
            !presentation
                .updates_message
                .contains("Update OK Player with your package manager"),
            "an unreachable apt must not be presented as the update path: {}",
            presentation.updates_message
        );
        let setup = presentation
            .repository_setup
            .expect("the actionable path is the repository the project publishes");
        // The channel is the one this build came from, never a constant (#726).
        assert_eq!(setup, apt_repository_setup("candidate").unwrap());
        assert!(
            presentation.updates_message.contains("repository"),
            "the message says what is missing: {}",
            presentation.updates_message
        );
        assert_eq!(
            presentation.action,
            Some(UpdateAction::CopyRepositorySetup),
            "the only thing the app can do here is hand over the commands"
        );
        assert!(
            !UpdateAction::CopyRepositorySetup.applies_update_in_app(),
            "copying commands installs nothing — the privileged path stays removed (#698)"
        );
        assert!(!presentation.action_closes_the_app);
    }

    /// #725 defect 2, which is #689's design working as intended: `stable`
    /// deliberately never carries a candidate build, so a `stable` subscriber
    /// must never be told about one — apt would refuse to install it, and the
    /// desktop updater would say there is nothing to do.
    /// The feed's version and the offer's version differ by design on a system-managed
    /// install: what the surface names is what apt would install. Anything that records a
    /// decision about the offer — a skip, above all — has to use that version, or it will
    /// look the decision up under a name it was never stored under and the offer will come
    /// straight back.
    #[test]
    fn the_offer_names_what_apt_would_install_rather_than_what_the_feed_published() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.package_source_observed(subscribed("candidate", Some("0.11.0-beta.0.209")));
        lifecycle.start_check().unwrap();
        lifecycle.check_found("0.11.0-beta.0.210").unwrap();

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.target_version.as_deref(),
            Some("0.11.0-beta.0.209"),
            "the surface names the version apt would install"
        );

        // And a state with nothing to act on names no target at all, so a caller that keys a
        // decision off the target has nothing to key it off — rather than the feed's version,
        // which is exactly the version this machine must not be offered.
        let mut withheld = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        withheld.package_source_observed(subscribed("stable", None));
        withheld.start_check().unwrap();
        withheld.check_found("0.11.0-beta.0.210").unwrap();
        assert_eq!(withheld.describe().target_version, None);
    }

    #[test]
    fn a_stable_subscriber_is_never_shown_a_build_its_suite_does_not_carry() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.package_source_observed(subscribed("stable", None));
        lifecycle.start_check().unwrap();

        // The feed publishes a rolling candidate; the suite does not carry it.
        assert_eq!(
            lifecycle.check_found("0.11.0-beta.0.210").unwrap(),
            &UpdateState::WithheldBySuite {
                version: "0.11.0-beta.0.210".to_owned(),
                suite: "stable".to_owned(),
            }
        );

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.target_version, None,
            "a build this machine cannot install is not an update target"
        );
        assert_eq!(presentation.claim, VersionClaim::Current);
        assert_eq!(
            presentation.upgrade_command, None,
            "this suite is not offering the build; a command to install it would do nothing"
        );
        assert_eq!(
            presentation.repository_setup, None,
            "a source is configured"
        );
        assert!(
            presentation.updates_message.contains("stable"),
            "the verdict says which channel it is about: {}",
            presentation.updates_message
        );
        assert!(
            presentation.about_message.contains("stable"),
            "About agrees with it: {}",
            presentation.about_message
        );
        assert_eq!(presentation.action, Some(UpdateAction::CheckNow));
        assert_eq!(presentation.secondary_action, None);
    }

    /// The subscriber the archive was built for: the suite carries the build,
    /// so the app names it, points at apt, and the desktop updater — reading
    /// the same package data — will agree.
    /// A machine whose apt lists are simply out of date reaches the same state as a `stable`
    /// subscriber that will never be offered a candidate — `apt-cache` answers from the last
    /// fetch and says so itself, and this app is not privileged to refresh it. So the state
    /// must not claim the suite *cannot* carry the build, and must keep the one control that
    /// can settle it: looking again.
    #[test]
    fn a_suite_that_is_not_offering_a_build_still_lets_the_user_look_again() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.package_source_observed(subscribed("stable", None));
        lifecycle.start_check().unwrap();
        lifecycle.check_found("0.11.0-beta.0.210").unwrap();

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.action,
            Some(UpdateAction::CheckNow),
            "checking again is what could change this answer"
        );
        assert_eq!(
            presentation.upgrade_command, None,
            "nothing to upgrade to on this channel"
        );
        assert!(
            presentation.repository_setup.is_none(),
            "a subscribed machine is not told to add a repository it already has"
        );
        // The claim is about what the suite is offering now, not about what it can ever hold:
        // the app cannot tell a genuinely withheld candidate from a stale list.
        assert!(
            !presentation.updates_message.contains("does not carry"),
            "the message overstates what apt's cached answer establishes: {}",
            presentation.updates_message
        );
        assert!(
            presentation.updates_message.contains("package lists"),
            "the message says what would change the answer: {}",
            presentation.updates_message
        );
    }

    /// The state every `.deb` install is in on first launch (#726): `postinst` has written
    /// the stanza and nothing has run `apt update`, so `apt-cache policy` still shows only
    /// dpkg's status. Read as `NoSource` this made the app deny a file its own packaging had
    /// just written, and offer commands for a channel that might not be the user's.
    #[test]
    fn a_source_apt_has_not_read_yet_is_offered_a_refresh_and_never_setup() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.packaged_suite_observed(Some("candidate".to_owned()));
        lifecycle.package_source_observed(PackageSourceEvidence::ConfiguredButUnread {
            suite: Some("candidate".to_owned()),
        });
        assert_eq!(
            lifecycle.capability(),
            UpdateCapability::SystemManaged,
            "a source that exists is a delivery path, read or not"
        );

        lifecycle.start_check().unwrap();
        assert_eq!(
            lifecycle.check_found("0.11.0-beta.0.210").unwrap(),
            &UpdateState::AvailableButSourceUnread {
                version: "0.11.0-beta.0.210".to_owned(),
                suite: Some("candidate".to_owned()),
            }
        );

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.action,
            Some(UpdateAction::CopyRefreshCommand),
            "the repository is there; reading it is the whole remedy"
        );
        assert_eq!(presentation.refresh_command, Some("sudo apt update"));
        assert!(
            presentation.repository_setup.is_none(),
            "offering setup here would overwrite the subscription this machine already has"
        );
        assert_eq!(
            presentation.upgrade_command, None,
            "apt has not read the source yet, so it has nothing to upgrade to; the refresh \
             comes first"
        );
        // The sentence must not deny the source, which is the defect this state exists for.
        assert!(
            !presentation
                .updates_message
                .contains("No OK Player repository"),
            "the surface denied a repository the machine has: {}",
            presentation.updates_message
        );
        assert!(
            presentation.updates_message.contains("candidate"),
            "the message names the channel this machine is on: {}",
            presentation.updates_message
        );
        assert!(!UpdateAction::CopyRefreshCommand.applies_update_in_app());

        // The remedy is a command the user runs outside the app, so the app has to be able to
        // observe them running it. A state that offered an instruction and then refused to
        // look again would need a restart to clear a message it had told the user how to fix.
        assert!(
            presentation.check_available,
            "the surface tells the user to refresh, so it must be able to check afterwards"
        );
        lifecycle
            .start_check()
            .expect("a refreshed machine can be looked at again");
        lifecycle.package_source_observed(PackageSourceEvidence::Source {
            suite: "candidate".to_owned(),
            deliverable: Some("0.11.0-beta.0.210".to_owned()),
        });
        lifecycle.check_found("0.11.0-beta.0.210").unwrap();
        let after = lifecycle.describe();
        assert!(
            after.refresh_command.is_none(),
            "the refresh offer survived the refresh: {}",
            after.updates_message
        );
        assert!(after.updates_message.contains("0.11.0-beta.0.210"));
    }

    /// Setup instructions take their suite from the build, never from a constant. A tester who
    /// installed a candidate `.deb` and pasted a hard-coded `stable` block would be moved off
    /// the channel they installed for — by the app's own advice, and silently.
    #[test]
    fn setup_instructions_name_the_channel_the_build_came_from() {
        let mut candidate = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        candidate.packaged_suite_observed(Some("candidate".to_owned()));
        candidate.package_source_observed(PackageSourceEvidence::NoSource);
        candidate.start_check().unwrap();
        candidate.check_found("0.11.0-beta.0.210").unwrap();
        let setup = candidate
            .describe()
            .repository_setup
            .expect("a machine with no source is told how to get one");
        assert_eq!(setup.suite, "candidate");
        assert!(
            setup.commands.contains("ok-player-candidate.sources"),
            "the commands subscribe to the wrong channel: {}",
            setup.commands
        );

        let mut stable = UpdateLifecycle::new(InstallKind::Deb, "0.11.0");
        stable.packaged_suite_observed(Some("stable".to_owned()));
        stable.package_source_observed(PackageSourceEvidence::NoSource);
        stable.start_check().unwrap();
        stable.check_found("0.12.0").unwrap();
        let setup = stable.describe().repository_setup.expect("same for stable");
        assert_eq!(setup.suite, "stable");
        assert!(setup.commands.contains("ok-player.sources"));
        assert!(!setup.commands.contains("ok-player-candidate.sources"));

        // A build that does not record its channel — anything from before #726 — is told so.
        // Guessing would be the same silent channel move, done on less evidence.
        let mut unknown = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        unknown.package_source_observed(PackageSourceEvidence::NoSource);
        unknown.start_check().unwrap();
        unknown.check_found("0.11.0-beta.0.210").unwrap();
        let presentation = unknown.describe();
        assert!(
            presentation.repository_setup.is_none(),
            "a build with no known channel must not be handed one"
        );
        assert!(
            presentation
                .updates_message
                .contains("cannot tell which channel"),
            "the surface should say what it does not know: {}",
            presentation.updates_message
        );
    }

    #[test]
    fn a_candidate_subscriber_is_offered_the_build_its_suite_carries() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "0.11.0-beta.0.208");
        lifecycle.package_source_observed(subscribed("candidate", Some("0.11.0-beta.0.210")));
        assert_eq!(lifecycle.capability(), UpdateCapability::SystemManaged);

        lifecycle.start_check().unwrap();
        let state = lifecycle.check_found("0.11.0-beta.0.210").unwrap().clone();
        let UpdateState::AvailableExternally { version, hint } = state else {
            panic!("a subscribed machine gets the ordinary system-managed offer, got {state:?}");
        };
        assert_eq!(version, "0.11.0-beta.0.210");
        assert!(
            hint.contains("apt") && hint.contains("candidate"),
            "the hint names the tool and the suite that carries it: {hint}"
        );

        let presentation = lifecycle.describe();
        assert_eq!(presentation.repository_setup, None);
        // The state the operator was in. It used to offer nothing but a way into the
        // desktop's system-wide updater (#759): an announcement with no way to act on it,
        // and a button that put every upgradable package on the machine in one transaction.
        assert_eq!(
            presentation.action,
            Some(UpdateAction::CopyUpgradeCommand),
            "the surface that names the version hands over the command that installs it"
        );
        assert_eq!(
            presentation.upgrade_command,
            Some("sudo apt install --only-upgrade ok-player"),
            "and the command names this package and nothing else"
        );
        assert!(
            !presentation.action.unwrap().applies_update_in_app(),
            "apt applies it, not the app: the privileged install path stays removed (#698)"
        );
        assert_eq!(
            presentation.secondary_action,
            Some(UpdateAction::SkipVersion)
        );

        // ...and silencing it does not take the command away: the version stays installable
        // on demand, which is what `InstallAnyway` is for on the self-applying lane.
        lifecycle.skip_offer().unwrap();
        let skipped = lifecycle.describe();
        assert_eq!(skipped.action, Some(UpdateAction::CopyUpgradeCommand));
        assert_eq!(
            skipped.upgrade_command,
            Some("sudo apt install --only-upgrade ok-player")
        );
    }

    /// The three answers apt can give produce three distinct states, and none
    /// of them offers a way to the named version that this machine does not
    /// have.
    #[test]
    fn each_delivery_answer_produces_its_own_state_and_offers_only_what_it_has() {
        let cases = [
            (
                PackageSourceEvidence::NoSource,
                UpdateState::AvailableWithoutSource {
                    version: "2.0.0".to_owned(),
                    gap: SourceGap::NoSource,
                },
            ),
            (
                subscribed("stable", None),
                UpdateState::WithheldBySuite {
                    version: "2.0.0".to_owned(),
                    suite: "stable".to_owned(),
                },
            ),
            (
                subscribed("candidate", Some("2.0.0")),
                UpdateState::AvailableExternally {
                    version: "2.0.0".to_owned(),
                    hint: InstallKind::Deb.system_update_hint_text(Some("candidate")),
                },
            ),
        ];

        let mut seen: Vec<UpdateState> = Vec::new();
        for (source, expected) in cases {
            let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
            lifecycle.package_source_observed(source.clone());
            lifecycle.start_check().unwrap();
            let state = lifecycle.check_found("2.0.0").unwrap().clone();
            assert_eq!(state, expected, "for {source:?}");
            assert!(!seen.contains(&state), "{source:?} must not repeat a state");
            seen.push(state);

            let presentation = lifecycle.describe();
            // The one rule that spans all three: an upgrade command is offered exactly where
            // a source apt has read is actually holding a build to upgrade to. A suite that
            // carries nothing gets no command, because running it would report the machine
            // already current under a surface that had just named a version (#759).
            assert_eq!(
                presentation.upgrade_command.is_some(),
                source.deliverable().is_some(),
                "for {source:?}"
            );
            // And nothing anywhere may offer to install it in the app.
            assert!(
                presentation
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "for {source:?}"
            );
        }
    }

    /// Every install kind, and for the lane that has answers about its delivery path, every
    /// answer.
    fn every_install_kind() -> Vec<InstallKind> {
        SELF_APPLY_KINDS
            .iter()
            .chain(&SYSTEM_MANAGED_KINDS)
            .chain(&[InstallKind::DevBuild])
            .copied()
            .collect()
    }

    /// **The #759 invariant.** Over every install kind and every answer about its delivery
    /// path, nothing the surface offers reaches past OK Player's own package.
    ///
    /// The defect it forbids shipped: a system-managed install offered "Open software
    /// updater", which launched the desktop's system-wide updater and built a transaction over
    /// every upgradable package on the machine. On the reporting machine that transaction
    /// included `tzdata`, whose debconf question blocked `dpkg-preconfigure` indefinitely.
    /// None of it was this app's package, and nothing in the app could bound it — which is why
    /// the rule is about what an action *reaches*, not about which states it appears in.
    #[test]
    fn no_offered_action_reaches_beyond_this_package() {
        for kind in every_install_kind() {
            sweep_reachable_states(kind, |life| {
                let presentation = life.describe();
                for action in [presentation.action, presentation.secondary_action]
                    .into_iter()
                    .flatten()
                {
                    assert!(
                        !action.reaches_beyond_this_package(),
                        "{kind} offered {action:?}, which acts on more than this package, in \
                         {:?}",
                        life.state()
                    );
                }
            });
        }
    }

    /// And the commands themselves, over the same space: every command the surface hands a
    /// user names this package, and none of them is a whole-system upgrade.
    ///
    /// The predicate above is about the shape of the offer; this is about its content. A
    /// command changed to `apt upgrade` would keep every action honest by the first rule and
    /// still hand the user the machine.
    #[test]
    fn every_command_the_surface_offers_names_this_package_and_never_the_whole_machine() {
        // `apt upgrade`, `apt-get dist-upgrade`, `apt full-upgrade`, `dnf upgrade` with no
        // argument — the ways of saying "everything on this machine".
        const WHOLE_MACHINE: [&str; 3] = ["dist-upgrade", "full-upgrade", "apt upgrade"];
        for kind in every_install_kind() {
            sweep_reachable_states(kind, |life| {
                let Some(command) = life.describe().upgrade_command else {
                    return;
                };
                assert!(
                    command.contains("ok-player"),
                    "{kind} offered {command:?}, which does not name the package"
                );
                for whole_machine in WHOLE_MACHINE {
                    assert!(
                        !command.contains(whole_machine),
                        "{kind} offered {command:?}, which upgrades more than this package"
                    );
                }
                assert!(
                    !command.contains("pkexec"),
                    "{kind} offered {command:?}; the privileged path was removed in #698"
                );
            });
        }
    }

    /// The other half of the same rule, and the reason the action could not simply be
    /// deleted: a system-managed install whose own source is holding a build must be given a
    /// way to install it. Removing the updater without this leaves the state the operator
    /// reported from the other side — an announcement with no control under it.
    #[test]
    fn a_system_managed_install_with_a_deliverable_always_offers_the_command() {
        for kind in every_install_kind() {
            sweep_reachable_states(kind, |life| {
                let presentation = life.describe();
                if !matches!(life.state(), UpdateState::AvailableExternally { .. }) {
                    return;
                }
                if kind.upgrade_command().is_none() {
                    return;
                }
                // The command is offered against what apt can deliver now, so a machine
                // whose evidence has since stopped naming one is outside this rule — see
                // `a_skipped_offer_gets_no_command_once_its_source_stops_carrying_one`.
                if life.package_source().deliverable().is_none() {
                    return;
                }
                assert_eq!(
                    presentation.action,
                    Some(UpdateAction::CopyUpgradeCommand),
                    "{kind} announced a build its source carries and offered {:?} for it",
                    presentation.action
                );
                assert_eq!(
                    presentation.upgrade_command,
                    kind.upgrade_command(),
                    "{kind} offered the action without the command it copies"
                );
            });
        }
    }

    /// A source that exists is not the same as a source with something to give.
    ///
    /// The shell re-reads what apt can deliver at check time, *before* the check's outcome is
    /// known, so a refresh that then fails restores the offer it was refreshing beside
    /// evidence that has moved on. `Source { deliverable: None }` is still a source — apt has
    /// read it and it carries the package — but there is nothing to install, and a command
    /// offered here would answer "ok-player is already the newest version" underneath a
    /// surface still naming a build. That disagreement between the app and the tool it points
    /// at is exactly what #725 was.
    #[test]
    fn a_skipped_offer_gets_no_command_once_its_source_stops_carrying_one() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        lifecycle.package_source_observed(subscribed("stable", Some("2.0.0")));
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.skip_offer().unwrap();
        assert_eq!(
            lifecycle.describe().action,
            Some(UpdateAction::CopyUpgradeCommand),
            "while the suite is holding it, the skipped version stays installable on demand"
        );

        // The refresh reads the machine first — the suite is no longer offering anything —
        // and then the feed request fails, which brings the skipped offer back unchanged.
        lifecycle.start_check().unwrap();
        lifecycle.package_source_observed(subscribed("stable", None));
        lifecycle.check_failed("network unreachable").unwrap();
        assert!(matches!(lifecycle.state(), UpdateState::Skipped { .. }));

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.upgrade_command, None,
            "apt has nothing to install; the command would contradict the surface offering it"
        );
        assert_eq!(presentation.action, None);
        assert!(
            presentation.check_available,
            "and looking again is what could change that"
        );
    }

    /// The action and the command are one offer: neither is ever on screen alone. A button
    /// that copies nothing and a command with no way to take it are the two halves of the
    /// same defect.
    #[test]
    fn the_upgrade_action_and_its_command_are_never_separated() {
        for kind in every_install_kind() {
            sweep_reachable_states(kind, |life| {
                let presentation = life.describe();
                assert_eq!(
                    presentation.action == Some(UpdateAction::CopyUpgradeCommand),
                    presentation.upgrade_command.is_some(),
                    "{kind} in {:?} offered one half of the upgrade offer",
                    life.state()
                );
            });
        }
    }

    /// A question that could not be asked is not an answer. `apt-cache` absent
    /// or failing leaves the app knowing nothing about the delivery path, and
    /// knowing nothing may not be rendered as "use your package manager".
    #[test]
    fn an_unanswered_delivery_question_never_becomes_a_delivery_claim() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        lifecycle.packaged_suite_observed(Some("stable".to_owned()));
        assert_eq!(
            *lifecycle.package_source(),
            PackageSourceEvidence::Unestablished,
            "nothing is assumed before a shell has asked"
        );
        assert_eq!(lifecycle.capability(), UpdateCapability::SystemUnreachable);

        lifecycle.start_check().unwrap();
        assert_eq!(
            lifecycle.check_found("2.0.0").unwrap(),
            &UpdateState::AvailableWithoutSource {
                version: "2.0.0".to_owned(),
                gap: SourceGap::Unestablished,
            }
        );
        let presentation = lifecycle.describe();
        assert_eq!(presentation.upgrade_command, None);
        assert!(presentation.repository_setup.is_some());
        assert!(
            presentation.updates_message.contains("could not ask"),
            "an unanswered question says so rather than claiming either way: {}",
            presentation.updates_message
        );
    }

    /// What the surface announces to a subscribed machine is what its own
    /// packaging tool would install, not what the feed published. The two
    /// differ by design — a rolling candidate is published to everyone and
    /// carried only by the suite that subscribes to it (#689) — and only one of
    /// them is a version this machine can actually get.
    #[test]
    fn the_version_announced_is_the_one_the_package_manager_would_install() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        lifecycle.package_source_observed(subscribed("candidate", Some("2.5.0")));
        lifecycle.start_check().unwrap();
        // The feed knows about 3.0.0; this machine's suite carries 2.5.0.
        lifecycle.check_found("3.0.0").unwrap();
        assert_eq!(
            lifecycle.describe().target_version.as_deref(),
            Some("2.5.0")
        );

        // And the other way round: a feed with nothing newer does not hide an
        // upgrade the suite is already serving.
        let mut behind = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        behind.package_source_observed(subscribed("stable", Some("2.0.0")));
        behind.start_check().unwrap();
        behind.check_found_none().unwrap();
        assert_eq!(behind.describe().target_version.as_deref(), Some("2.0.0"));
    }

    /// A refresh that fails over an announcement with no delivery path leaves
    /// it exactly as it was — the instructions the user was reading do not
    /// vanish because the network did.
    #[test]
    fn a_failed_refresh_keeps_an_announcement_that_has_no_delivery_path() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
        lifecycle.packaged_suite_observed(Some("stable".to_owned()));
        lifecycle.package_source_observed(PackageSourceEvidence::NoSource);
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();

        lifecycle.start_check().unwrap();
        lifecycle.check_failed("network unreachable").unwrap();
        assert_eq!(
            lifecycle.state(),
            &UpdateState::AvailableWithoutSource {
                version: "2.0.0".to_owned(),
                gap: SourceGap::NoSource,
            }
        );
        let presentation = lifecycle.describe();
        assert!(presentation.repository_setup.is_some());
        assert_eq!(presentation.upgrade_command, None);
        assert!(presentation.updates_message.contains("network unreachable"));
    }

    #[test]
    fn system_managed_hint_names_the_packaging_tool() {
        for (kind, expected) in [
            (InstallKind::Deb, "apt"),
            (InstallKind::Rpm, "dnf"),
            (InstallKind::Flatpak, "flatpak"),
        ] {
            let mut lifecycle = system_managed_lifecycle(kind, "1.0.0");
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
        // The recovery is a fresh check, so that is what the action says (#701).
        assert_eq!(presentation.action, Some(UpdateAction::CheckNow));
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

    /// Invariant: after a restart that came back on the old build, the action
    /// the projection offers is the one it performs. The recovery is
    /// deliberately a fresh check — the payload died with the process that
    /// applied it, and going straight back into another close/apply/relaunch
    /// behind one press is how a player loops through restarts — so the action
    /// is a check, says it is a check, and the install is the press after it.
    ///
    /// The same rule bounds the restarts: nothing the projection offers from
    /// the failure drives a restart at all, so no path reaches a second one
    /// without the user asking for it.
    #[test]
    fn a_failed_restart_offers_the_check_it_performs_and_never_a_second_restart() {
        let mut lifecycle =
            UpdateLifecycle::resumed_after_restart(InstallKind::AppImage, "1.0.0", "2.0.0")
                .expect("a self-applying install can resume after a restart");
        let failed = lifecycle.state().clone();
        assert!(
            matches!(
                failed,
                UpdateState::Failed {
                    target: Some(_),
                    recovery: FailureRecovery::CheckAgain,
                    ..
                }
            ),
            "the restart ran the old binary, and its recovery is a check: {failed:?}"
        );

        let presentation = lifecycle.describe();
        let action = presentation.action.expect("the failure offers a way out");
        assert_eq!(action, UpdateAction::CheckNow);
        assert_eq!(action.label(), "Check for updates");
        assert!(
            !action.applies_update_in_app(),
            "the offered action installs nothing"
        );
        assert!(
            !presentation.action_closes_the_app,
            "and it cannot end the session on the one-call lane either"
        );
        assert!(
            presentation.updates_message.contains("Check for updates"),
            "the message describes the same recovery, got {}",
            presentation.updates_message
        );

        // The model itself refuses the in-place retry that would put the
        // version back as actionable and let a surface apply it from here.
        assert_eq!(
            lifecycle.retry_failed_update(),
            Err(UpdateTransitionError::NotAllowedFrom(failed.clone()))
        );
        assert_eq!(lifecycle.state(), &failed, "a refusal changes nothing");

        // Taking the offered action does what the label says, and while it
        // runs the failure it is refreshing is still the thing on screen.
        lifecycle.start_check().unwrap();
        assert!(matches!(lifecycle.state(), UpdateState::Checking { .. }));
        let refreshing = lifecycle.describe();
        assert_eq!(refreshing.action, Some(UpdateAction::CheckNow));
        assert!(!refreshing.action_closes_the_app);

        // A refresh that fails hands the failure back as the one it was: still
        // no retry, still the check.
        let mut interrupted = lifecycle.clone();
        interrupted.check_failed("network unreachable").unwrap();
        assert_eq!(interrupted.describe().action, Some(UpdateAction::CheckNow));
        assert!(interrupted.retry_failed_update().is_err());

        // And what the check finds is an offer the user takes deliberately:
        // the install is a second press, and it is the one that says it closes
        // the player.
        lifecycle.check_found("2.0.0").unwrap();
        let rediscovered = lifecycle.describe();
        assert_eq!(rediscovered.action, Some(UpdateAction::DownloadUpdate));
        assert!(
            rediscovered.action_closes_the_app,
            "the install the user chooses is the one that restarts the player"
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
                recovery: FailureRecovery::RepeatTheStep,
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
                recovery: FailureRecovery::RepeatTheStep,
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
            let mut lifecycle = system_managed_lifecycle(kind, "1.0.0");
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
            assert!(
                presentation
                    .action
                    .is_none_or(|action| !action.applies_update_in_app()),
                "{kind} still installs nothing in app, offered {:?}",
                presentation.action
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

    /// Invariant: on the one-call lane, installing a *staged* skipped version
    /// does not close the player — `install_anyway` only makes the payload it
    /// kept ready to apply, and the apply after it is what relaunches. The
    /// warning belongs to the action that really ends the session.
    #[test]
    fn a_staged_skip_does_not_announce_that_installing_it_closes_the_app() {
        let mut staged = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        staged.start_check().unwrap();
        staged.check_found("2.0.0").unwrap();
        staged.start_download().unwrap();
        staged.download_finished().unwrap();
        staged.start_apply().unwrap();
        staged.apply_failed("permission denied").unwrap();
        staged.skip_offer().unwrap();
        assert_eq!(
            staged.state(),
            &UpdateState::Skipped {
                version: "2.0.0".to_owned(),
                hint: None,
                staged: true,
            },
            "the skip kept the verified payload"
        );

        let presentation = staged.describe();
        assert_eq!(presentation.action, Some(UpdateAction::InstallAnyway));
        assert!(
            !presentation.action_closes_the_app,
            "installing a staged skip only stages it for the apply that follows"
        );

        // And that is the truth about the transition, not a guess about it.
        let mut accepted = staged.clone();
        assert_eq!(
            accepted.install_anyway().unwrap(),
            &UpdateState::ReadyToApply {
                version: "2.0.0".to_owned()
            },
            "nothing was applied and nothing closed"
        );
        let next = accepted.describe();
        assert_eq!(next.action, Some(UpdateAction::ApplyAndRestart));
        assert!(
            next.action_closes_the_app,
            "the apply the user takes next is what ends the session"
        );

        // A skip with nothing staged is the single call that downloads,
        // applies and relaunches, and still says so.
        let mut unstaged = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        unstaged.start_check().unwrap();
        unstaged.check_found("2.0.0").unwrap();
        unstaged.skip_offer().unwrap();
        let presentation = unstaged.describe();
        assert_eq!(presentation.action, Some(UpdateAction::InstallAnyway));
        assert!(
            presentation.action_closes_the_app,
            "an unstaged skip is downloaded, applied and relaunched in one step"
        );

        // The distinction survives a refresh, which keeps the skipped offer's
        // control on screen and must describe the same consequence.
        staged.start_check().unwrap();
        let refreshing = staged.describe();
        assert_eq!(refreshing.action, Some(UpdateAction::InstallAnyway));
        assert!(
            !refreshing.action_closes_the_app,
            "a carried staged skip closes nothing either"
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
                recovery: FailureRecovery::RepeatTheStep,
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
                recovery: FailureRecovery::RepeatTheStep,
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

    /// Invariant: a refresh keeps the offer it is refreshing on screen, with
    /// its own controls, and says they are not pressable yet — rather than
    /// making a shell hide them and put them back.
    #[test]
    fn a_check_in_flight_keeps_the_carried_offer_controls() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("2.0.0").unwrap();
        lifecycle.start_check().unwrap();

        let presentation = lifecycle.describe();
        assert_eq!(
            presentation.action,
            Some(UpdateAction::DownloadUpdate),
            "the offer being refreshed keeps its primary control"
        );
        assert_eq!(
            presentation.secondary_action,
            Some(UpdateAction::SkipVersion),
            "and its skip"
        );
        assert!(
            !presentation.actions_enabled,
            "but nothing is pressable while the check is in flight"
        );

        // A skipped offer keeps its own control through a refresh too.
        let mut skipped = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        skipped.start_check().unwrap();
        skipped.check_found("2.0.0").unwrap();
        skipped.skip_offer().unwrap();
        skipped.start_check().unwrap();
        assert_eq!(skipped.describe().action, Some(UpdateAction::InstallAnyway));

        // A first check has no offer to keep, and offers nothing.
        let mut first = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        first.start_check().unwrap();
        let presentation = first.describe();
        assert_eq!(presentation.action, None);
        assert_eq!(presentation.secondary_action, None);
        assert!(!presentation.actions_enabled);

        // Everything settled is pressable.
        let idle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        assert!(idle.describe().actions_enabled);
    }

    /// Invariant: a check running over a standing offer still describes that
    /// offer. The check is a status on top of what the surface is showing, not
    /// a replacement for it — a message that only said "checking" would hide
    /// whether the carried version is available, skipped, staged or failed,
    /// and every shell would need its own offer rendering to get it back.
    #[test]
    fn a_check_over_a_carried_offer_still_describes_that_offer() {
        // A refresh over each kind of standing offer, with the words that
        // distinguish that offer from the others.
        let refreshing_available = {
            let mut life = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            life.start_check().unwrap();
            life.check_found("2.0.0").unwrap();
            life.start_check().unwrap();
            life
        };
        let refreshing_skipped = {
            let mut life = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            life.start_check().unwrap();
            life.check_found("2.0.0").unwrap();
            life.skip_offer().unwrap();
            life.start_check().unwrap();
            life
        };
        let refreshing_staged = {
            let mut life = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
            life.start_check().unwrap();
            life.check_found("2.0.0").unwrap();
            life.start_download().unwrap();
            life.download_finished().unwrap();
            life.start_apply().unwrap();
            life.apply_failed("permission denied").unwrap();
            life.start_check().unwrap();
            life
        };
        let refreshing_external = {
            let mut life = UpdateLifecycle::new(InstallKind::Deb, "1.0.0");
            life.start_check().unwrap();
            life.check_found("2.0.0").unwrap();
            life.start_check().unwrap();
            life
        };

        for (life, expected) in [
            (&refreshing_available, "is available"),
            (&refreshing_skipped, "was skipped"),
            (&refreshing_staged, "failed: permission denied"),
            (&refreshing_external, "apt"),
        ] {
            let message = life.describe().updates_message;
            assert!(
                message.contains(expected),
                "a refresh must keep saying what the carried offer is; wanted {expected:?} in {message:?}"
            );
            assert!(
                message.contains("2.0.0"),
                "and which version it is about, got {message:?}"
            );
            assert!(
                message.contains("Checking for updates…"),
                "while still saying a check is running, got {message:?}"
            );
        }

        // A check with no offer behind it has only itself to report.
        let mut first = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0");
        first.start_check().unwrap();
        assert_eq!(first.describe().updates_message, "Checking for updates…");

        // And About, rendered from the same projection, describes the same
        // offer: the two surfaces cannot disagree about what is being
        // refreshed while it is being refreshed.
        for (life, expected) in [
            (&refreshing_available, "is available"),
            (&refreshing_skipped, "was skipped"),
            (&refreshing_staged, "failed"),
        ] {
            let message = life.describe().about_message;
            assert!(
                message.contains(expected),
                "About must describe the carried offer too; wanted {expected:?} in {message:?}"
            );
        }
        assert!(
            !refreshing_skipped
                .describe()
                .about_message
                .contains("is available"),
            "a skipped offer must not read as an available one on About"
        );
    }

    /// Invariant: a prerelease tail is ordered stage first, counter second, so
    /// promoting from an alpha to a beta of the same core is an upgrade.
    #[test]
    fn a_later_prerelease_stage_outranks_a_higher_counter_in_an_earlier_one() {
        let mut lifecycle = UpdateLifecycle::new(InstallKind::AppImage, "1.0.0-alpha.109");
        lifecycle.start_check().unwrap();
        lifecycle.check_found("1.0.0-beta.1").unwrap();
        lifecycle.start_download().unwrap();
        lifecycle.download_finished().unwrap();
        lifecycle.start_apply().unwrap();
        lifecycle.apply_needs_restart().unwrap();

        assert_eq!(
            lifecycle.restarted_into("1.0.0-beta.1").unwrap(),
            &UpdateState::Running {
                version: "1.0.0-beta.1".to_owned()
            },
            "beta.1 is newer than alpha.109, not older"
        );

        let staged = UpdateLifecycle::resumed_with_staged_update(
            InstallKind::AppImage,
            "1.0.0-alpha.109",
            "1.0.0-beta.1",
        )
        .expect("a self-applying install can resume");
        assert_eq!(
            staged.state(),
            &UpdateState::ReadyToApply {
                version: "1.0.0-beta.1".to_owned()
            },
            "and a staged beta is not discarded as stale"
        );

        // The counter still decides within one stage, and a candidate build cut
        // from a beta is newer than that beta.
        let within_stage = UpdateLifecycle::resumed_with_staged_update(
            InstallKind::AppImage,
            "0.11.0-beta.2",
            "0.11.0-beta.2.41",
        )
        .expect("a self-applying install can resume");
        assert!(matches!(
            within_stage.state(),
            UpdateState::ReadyToApply { .. }
        ));
        let backwards = UpdateLifecycle::resumed_with_staged_update(
            InstallKind::AppImage,
            "0.1.0-linux-alpha.109",
            "0.1.0-linux-alpha.108",
        )
        .expect("a self-applying install can resume");
        assert_eq!(backwards.state(), &UpdateState::Idle);
    }

    // ------------------------------------------------------ the Windows lane

    #[test]
    fn the_windows_lane_walks_check_download_apply_restart_and_says_so_at_every_step() {
        // The Velopack projection end to end (#682): the states are the shared
        // ones, every string comes out of `describe`, and the restart is the
        // only step that can make the new version the running one.
        let mut velopack = UpdateLifecycle::new(InstallKind::WindowsVelopack, "0.11.0-beta.0.14");
        assert_eq!(velopack.capability(), UpdateCapability::SelfApply);
        assert_eq!(
            velopack.describe().updates_message,
            "OK Player has not checked for updates yet."
        );

        velopack.start_check().expect("an installed build checks");
        assert_eq!(velopack.describe().updates_message, "Checking for updates…");
        assert!(
            !velopack.describe().actions_enabled,
            "a check in flight leaves nothing to press"
        );

        velopack
            .check_found("0.11.0-beta.0.15")
            .expect("the feed offered a newer build");
        let offered = velopack.describe();
        assert_eq!(offered.action, Some(UpdateAction::DownloadUpdate));
        assert_eq!(
            offered.updates_message,
            "Version 0.11.0-beta.0.15 is available."
        );
        assert!(
            !offered.action_closes_the_app,
            "Velopack downloads in the background; nothing closes yet"
        );

        velopack.start_download().expect("the payload is fetched");
        velopack.download_finished().expect("and verified");
        let staged = velopack.describe();
        assert_eq!(staged.action, Some(UpdateAction::ApplyAndRestart));
        assert!(staged.action_closes_the_app);

        velopack.start_apply().expect("the user accepted");
        assert_eq!(
            velopack
                .apply_needs_restart()
                .expect("applying can only land on a pending restart"),
            &UpdateState::RestartPending {
                version: "0.11.0-beta.0.15".to_owned()
            }
        );
        let pending = velopack.describe();
        assert_eq!(
            pending.claim,
            VersionClaim::Superseded {
                newer: "0.11.0-beta.0.15".to_owned()
            },
            "the bits are on disk; this process is still the old build (#660)"
        );
        assert_eq!(pending.version_in_use, "0.11.0-beta.0.14");
        assert_eq!(pending.action, Some(UpdateAction::RestartToFinish));

        // The relaunch is a new process: the shell hands the pending target and
        // the complete version it came up as to `resumed_after_restart`.
        let resumed = UpdateLifecycle::resumed_after_restart(
            InstallKind::WindowsVelopack,
            ReportedVersion::complete("0.11.0-beta.0.15"),
            "0.11.0-beta.0.15",
        )
        .expect("Velopack self-applies, so it can resume a pending restart");
        let done = resumed.describe();
        assert_eq!(done.claim, VersionClaim::Current);
        assert_eq!(
            done.updates_message,
            "OK Player is now running version 0.11.0-beta.0.15."
        );
        assert_eq!(
            done.about_message,
            "OK Player 0.11.0-beta.0.15 — up to date."
        );
    }

    #[test]
    fn a_windows_dev_build_reports_updates_disabled_rather_than_a_special_case() {
        // The shell no longer writes "Unavailable (development build)": with no
        // Velopack layout around the executable the install is a dev build, and
        // the projection is the one every unmanaged install gets.
        let dev = detect_install_kind(&InstallEvidence {
            executable_path: Some(r"C:\src\ok-player\bin\OkPlayer.exe".to_owned()),
            velopack_layout_present: false,
            ..evidence()
        });
        assert_eq!(dev, InstallKind::DevBuild);

        let lifecycle = UpdateLifecycle::new(dev, "0.11.0");
        let presentation = lifecycle.describe();
        assert_eq!(presentation.capability, UpdateCapability::Unmanaged);
        assert_eq!(presentation.claim, VersionClaim::NotApplicable);
        assert_eq!(
            presentation.updates_message,
            "Updates are disabled for development builds."
        );
        assert_eq!(
            presentation.about_message,
            "OK Player 0.11.0 — development build; updates are disabled."
        );
        assert_eq!(presentation.action, None);
    }

    // --------------------------------------------- truncated versions (#694)

    #[test]
    fn a_truncated_version_only_decides_what_its_numeric_core_can() {
        let truncated = ReportedVersion::truncated("0.11.0");
        // The tail is what the ordering would have turned on, and it is gone.
        assert_eq!(
            compare_reported_build_order(&truncated, &"0.11.0-beta.0.15".into()),
            None
        );
        assert_eq!(
            compare_reported_build_order(&"0.11.0-beta.0.15".into(), &truncated),
            None
        );
        assert_eq!(
            compare_reported_build_order(&truncated, &"0.11.0".into()),
            None,
            "even against the same string: the truncated one may be any beta of it"
        );
        // A different core is decided by the core alone, so truncation costs
        // nothing there.
        assert_eq!(
            compare_reported_build_order(&truncated, &"0.12.0-beta.1".into()),
            Some(Ordering::Less)
        );
        assert_eq!(
            compare_reported_build_order(&truncated, &"0.10.14".into()),
            Some(Ordering::Greater)
        );
        // Two complete versions keep the full ordering, prerelease rules and
        // all.
        assert_eq!(
            compare_reported_build_order(&"0.11.0".into(), &"0.11.0-beta.0.15".into()),
            Some(Ordering::Greater)
        );
        assert!(ReportedVersion::from("0.11.0").is_complete());
        assert_eq!(
            ReportedVersion::truncated("0.11.0").fidelity(),
            VersionFidelity::Truncated
        );
    }

    #[test]
    fn a_truncated_running_version_neither_confirms_nor_denies_the_restart() {
        // #694 exactly: the process came back on the candidate it was meant to
        // install, but all it can say about itself is "0.11.0" — which read as
        // a complete version is a *stable release*, ranking above the pending
        // prerelease and turning a good upgrade into a reported downgrade.
        let mut velopack = UpdateLifecycle::new(
            InstallKind::WindowsVelopack,
            ReportedVersion::truncated("0.11.0"),
        );
        velopack.start_check().unwrap();
        velopack.check_found("0.11.0-beta.0.15").unwrap();
        velopack.start_download().unwrap();
        velopack.download_finished().unwrap();
        velopack.start_apply().unwrap();
        velopack.apply_needs_restart().unwrap();
        velopack
            .restarted_into(ReportedVersion::truncated("0.11.0"))
            .expect("the restart is reported either way");

        assert_eq!(
            velopack.state(),
            &UpdateState::RestartUnverified {
                target: "0.11.0-beta.0.15".to_owned()
            },
            "neither Running nor the #660 failure — both would be invented"
        );
        let presentation = velopack.describe();
        assert_eq!(
            presentation.claim,
            VersionClaim::Unknown,
            "no claim about the running build is available"
        );
        // Not swallowed either: both surfaces say what could not be confirmed,
        // and name the version it was about.
        assert!(
            presentation.updates_message.contains("cannot be confirmed"),
            "{}",
            presentation.updates_message
        );
        assert!(presentation.updates_message.contains("0.11.0-beta.0.15"));
        assert!(
            presentation
                .about_message
                .contains("could not be confirmed"),
            "{}",
            presentation.about_message
        );
        assert_eq!(
            presentation.target_version.as_deref(),
            Some("0.11.0-beta.0.15")
        );
        // A check is the way out, and it is offered.
        assert_eq!(presentation.action, Some(UpdateAction::CheckNow));
        assert!(presentation.actions_enabled);
        velopack
            .start_check()
            .expect("an unverified restart can still be settled against the feed");
        assert_eq!(
            velopack.describe().claim,
            VersionClaim::Unknown,
            "refreshing an unconfirmed restart does not make it a known one"
        );

        // A check that fails settles nothing, so it must leave the restart as
        // unconfirmed as it found it rather than collapsing it into a generic
        // failure that has forgotten which version it was about.
        velopack
            .check_failed("network unreachable")
            .expect("a refresh can fail");
        assert_eq!(
            velopack.state(),
            &UpdateState::RestartUnverified {
                target: "0.11.0-beta.0.15".to_owned()
            }
        );
        let after_failure = velopack.describe();
        assert!(
            after_failure
                .updates_message
                .contains("cannot be confirmed")
                && after_failure
                    .updates_message
                    .contains("Update check failed: network unreachable"),
            "{}",
            after_failure.updates_message
        );
        assert_eq!(after_failure.claim, VersionClaim::Unknown);
        assert_eq!(after_failure.action, Some(UpdateAction::CheckNow));
    }

    #[test]
    fn a_complete_running_version_still_settles_the_restart_both_ways() {
        // The contract only withholds what a truncated string cannot support:
        // a shell that reports the whole package version — which the Windows
        // one does from the Velopack release identity and the informational
        // assembly version — keeps both verdicts.
        let landed = UpdateLifecycle::resumed_after_restart(
            InstallKind::WindowsVelopack,
            "0.11.0-beta.0.15",
            "0.11.0-beta.0.15",
        )
        .expect("a self-applying install can resume");
        assert_eq!(
            landed.state(),
            &UpdateState::Running {
                version: "0.11.0-beta.0.15".to_owned()
            }
        );

        let stayed = UpdateLifecycle::resumed_after_restart(
            InstallKind::WindowsVelopack,
            "0.11.0-beta.0.14",
            "0.11.0-beta.0.15",
        )
        .expect("a self-applying install can resume");
        assert!(
            matches!(stayed.state(), UpdateState::Failed { target: Some(target), .. } if target == "0.11.0-beta.0.15"),
            "the old binary came back up — #660, still caught: {:?}",
            stayed.state()
        );
    }

    #[test]
    fn a_truncated_running_version_keeps_a_staged_payload_it_cannot_rank() {
        // A staged payload is an observed fact, not an ordering conclusion.
        // With the running version truncated the staleness check cannot run, so
        // the offer survives — losing a downloaded update silently would be the
        // worse half of the guess.
        let resumed = UpdateLifecycle::resumed_with_staged_update(
            InstallKind::WindowsVelopack,
            ReportedVersion::truncated("0.11.0"),
            "0.11.0-beta.0.15",
        )
        .expect("a self-applying install can resume");
        assert_eq!(
            resumed.state(),
            &UpdateState::ReadyToApply {
                version: "0.11.0-beta.0.15".to_owned()
            }
        );
        assert_eq!(
            resumed.describe().action,
            Some(UpdateAction::ApplyAndRestart)
        );
        // A core the truncation cannot hide still discards a stale record.
        let stale = UpdateLifecycle::resumed_with_staged_update(
            InstallKind::WindowsVelopack,
            ReportedVersion::truncated("0.12.0"),
            "0.11.0-beta.0.15",
        )
        .expect("a self-applying install can resume");
        assert_eq!(stale.state(), &UpdateState::Idle);
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
