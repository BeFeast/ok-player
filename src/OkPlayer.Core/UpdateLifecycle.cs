namespace OkPlayer.Core;

// The Windows projection of the shared update lifecycle, mirroring okp_core::update_lifecycle
// (issues #680/#682): the same install kinds, the same states, the same transitions, and —
// load-bearing — the same user-facing strings, produced only by UpdateLifecycle.Describe. The shell
// renders what the projection returns and never writes update text of its own, so the Updates card
// and the About block cannot disagree, and a pending restart cannot be drawn as "you are on the new
// version" (#660). Deliberate divergences from the Rust module are recorded in
// docs/core-compatibility.md.

/// <summary>How the running copy of OK Player was installed. Only the two kinds a Windows process
/// can be: the Linux lanes of the shared model have no producer here.</summary>
public enum InstallKind
{
    /// <summary>Windows install laid out and updated by Velopack.</summary>
    WindowsVelopack,

    /// <summary>A build no packaging owns: a dev run, a portable copy. Updates are disabled rather
    /// than pretended.</summary>
    DevBuild,
}

/// <summary>What an install may do about an available update.</summary>
public enum UpdateCapability
{
    /// <summary>The app downloads and applies the update itself.</summary>
    SelfApply,

    /// <summary>Nothing updates this install; the app says so instead of claiming to be current.</summary>
    Unmanaged,
}

/// <summary>How completely the shell was able to state a version (#694).</summary>
public enum VersionFidelity
{
    /// <summary>The exact package version, prerelease tail included.</summary>
    Complete,

    /// <summary>A normalised form: the numeric core is the build's, but the prerelease tail was
    /// dropped on the way. <c>App.AppVersion</c> read off the assembly's numeric
    /// <see cref="System.Version"/> is this: <c>0.11.0-beta.0.14</c> and <c>0.11.0-beta.0.15</c>
    /// both come back as <c>0.11.0</c>, which looks exactly like a stable release.</summary>
    Truncated,
}

/// <summary>A version string together with what the shell knows about it. Mirrors
/// <c>okp_core::update_lifecycle::ReportedVersion</c>.</summary>
public readonly record struct ReportedVersion(string Text, VersionFidelity Fidelity)
{
    /// <summary>The exact package version of the build.</summary>
    public static ReportedVersion Complete(string text) => new(text, VersionFidelity.Complete);

    /// <summary>A version whose prerelease tail the shell could not observe.</summary>
    public static ReportedVersion Truncated(string text) => new(text, VersionFidelity.Truncated);

    public bool IsComplete => Fidelity == VersionFidelity.Complete;

    public override string ToString() => Text;
}

/// <summary>Which position in the lifecycle a state is.</summary>
public enum UpdateStateKind
{
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    ReadyToApply,
    Applying,
    RestartPending,
    RestartUnverified,
    Running,
    Failed,
}

/// <summary>What a recovery from a failed step is able to do. It decides both what
/// <see cref="UpdateLifecycle.RetryFailedUpdate"/> accepts and what the projection is allowed to
/// call the action, so a surface cannot promise one thing and deliver another (#701). Mirrors
/// <c>okp_core::update_lifecycle::FailureRecovery</c>.</summary>
public enum FailureRecovery
{
    /// <summary>The step that failed can simply be run again: the payload is still staged, or the
    /// offer that was discovered still stands. Repeating it is what "Try again" means.</summary>
    RepeatTheStep,

    /// <summary>The step cannot be repeated. A restart that came back on the old build is the case
    /// (#660): the apply consumed the payload that produced it, and the process asking is a new one
    /// that discovered nothing. Re-checking first is also what keeps the recovery from walking
    /// straight back into the restart that just failed — one restart attempt, then the user decides.
    /// So the recovery is a check, and the projection offers it as a check.</summary>
    CheckAgain,
}

/// <summary>One position in the update lifecycle.
/// <c>Idle → Checking → UpToDate | Available | Failed</c>, and from <c>Available</c>:
/// <c>Downloading → ReadyToApply → Applying → RestartPending → Running</c>.</summary>
public sealed record UpdateState
{
    private UpdateState(
        UpdateStateKind kind,
        string? version,
        string? reason,
        bool staged,
        UpdateState? carried,
        FailureRecovery recovery = FailureRecovery.RepeatTheStep)
    {
        Kind = kind;
        Version = version;
        Reason = reason;
        Staged = staged;
        Carried = carried;
        Recovery = recovery;
    }

    public UpdateStateKind Kind { get; }

    /// <summary>The version the state is about: the update target, the pending restart's target, or
    /// — for <see cref="UpdateStateKind.Running"/> — the version that actually came up.</summary>
    public string? Version { get; }

    /// <summary>Why the last step gave up. <see cref="UpdateStateKind.Failed"/> only.</summary>
    public string? Reason { get; }

    /// <summary>Whether a verified payload for <see cref="Version"/> is still on disk, so a retry
    /// re-applies it instead of downloading it again. <see cref="UpdateStateKind.Failed"/> only.</summary>
    public bool Staged { get; }

    /// <summary>The offer that was on screen when a re-check started, restored intact if the check
    /// fails. <see cref="UpdateStateKind.Checking"/> only.</summary>
    public UpdateState? Carried { get; }

    /// <summary>What a recovery from this failure is able to do — not always a repeat of the step
    /// that failed (#701). <see cref="UpdateStateKind.Failed"/> only.</summary>
    public FailureRecovery Recovery { get; }

    public static UpdateState Idle { get; } = new(UpdateStateKind.Idle, null, null, false, null);
    public static UpdateState UpToDate { get; } = new(UpdateStateKind.UpToDate, null, null, false, null);

    public static UpdateState Checking(UpdateState? carried) =>
        new(UpdateStateKind.Checking, carried?.Version, null, false, carried);

    public static UpdateState Available(string version) =>
        new(UpdateStateKind.Available, version, null, false, null);

    public static UpdateState Downloading(string version) =>
        new(UpdateStateKind.Downloading, version, null, false, null);

    public static UpdateState ReadyToApply(string version) =>
        new(UpdateStateKind.ReadyToApply, version, null, false, null);

    public static UpdateState Applying(string version) =>
        new(UpdateStateKind.Applying, version, null, false, null);

    public static UpdateState RestartPending(string version) =>
        new(UpdateStateKind.RestartPending, version, null, false, null);

    /// <summary>The process restarted and the version it can report about itself is too coarse to
    /// tell whether it came up on <paramref name="target"/> (#694).</summary>
    public static UpdateState RestartUnverified(string target) =>
        new(UpdateStateKind.RestartUnverified, target, null, false, null);

    public static UpdateState Running(string version) =>
        new(UpdateStateKind.Running, version, null, false, null);

    public static UpdateState Failed(
        string reason,
        string? target,
        bool staged,
        FailureRecovery recovery = FailureRecovery.RepeatTheStep) =>
        new(UpdateStateKind.Failed, target, reason, staged, null, recovery);

    /// <summary>The version this state is about, when it is about one. Always the update
    /// <em>target</em> — never a claim about the binary currently executing, which is why
    /// <see cref="UpdateStateKind.Running"/> has none.</summary>
    public string? TargetVersion => Kind switch
    {
        UpdateStateKind.Idle or UpdateStateKind.UpToDate or UpdateStateKind.Running => null,
        _ => Version,
    };
}

/// <summary>The single action a surface may offer for the current state.</summary>
public enum UpdateAction
{
    CheckNow,
    DownloadUpdate,
    ApplyAndRestart,
    RestartToFinish,
    Retry,
}

/// <summary>What a surface may tell the user about the binary that is executing right now — the
/// field #660 got wrong.</summary>
public enum VersionClaim
{
    /// <summary>The running binary is the newest version this install knows about.</summary>
    Current,

    /// <summary>A newer version is known and the running binary is not it — every state between
    /// discovery and the restart that actually swaps the binary, <c>RestartPending</c> included.</summary>
    Superseded,

    /// <summary>Nothing is known: no check has run, one is in flight or failed, or the restart's
    /// outcome could not be read off a truncated version (#694).</summary>
    Unknown,

    /// <summary>This install is not updated at all.</summary>
    NotApplicable,
}

/// <summary>Everything the Updates surface and the About surface are allowed to show, derived from
/// one state by <see cref="UpdateLifecycle.Describe"/>.</summary>
public sealed record UpdatePresentation(
    InstallKind InstallKind,
    UpdateCapability Capability,
    string VersionInUse,
    string? TargetVersion,
    VersionClaim Claim,
    string UpdatesMessage,
    string AboutMessage,
    UpdateAction? Action,
    bool ActionsEnabled,
    bool ActionClosesTheApp);

/// <summary>Facts about the running process that the shell collects for
/// <see cref="UpdateLifecycle.Detect"/>. Pure input: the core decides, the shell observes.</summary>
public sealed record InstallEvidence(bool VelopackLayoutPresent);

/// <summary>Ordering rules for build versions, including what a truncated version cannot decide
/// (#694). Mirrors the private ordering of <c>okp_core::update_lifecycle</c>.</summary>
public static class VersionOrder
{
    /// <summary>Orders two complete build versions, where a release outranks the prereleases that
    /// led to it (<c>1.0.0</c> after <c>1.0.0-beta.1</c>) and numeric runs order within a lane
    /// (<c>alpha.10</c> after <c>alpha.9</c>).</summary>
    public static int CompareBuildOrder(string left, string right)
    {
        (string leftCore, string? leftPre) = SplitPrerelease(left);
        (string rightCore, string? rightPre) = SplitPrerelease(right);
        int core = CompareNumericRuns(leftCore, rightCore);
        if (core != 0)
            return core;
        if (leftPre is null && rightPre is null)
            return 0;
        if (leftPre is null)
            return 1;  // a release outranks any prerelease of the same core
        if (rightPre is null)
            return -1;
        return ComparePrerelease(leftPre, rightPre);
    }

    /// <summary>Orders two reported builds, or returns null when the strings on hand cannot decide
    /// it. A truncated string carries the numeric core and nothing else, so different cores still
    /// decide the order, while equal cores are undecidable as soon as either side is truncated: the
    /// truncated string is equally consistent with the stable release and with every prerelease
    /// that led to it (#694).</summary>
    public static int? CompareReportedBuildOrder(ReportedVersion left, ReportedVersion right)
    {
        if (left.IsComplete && right.IsComplete)
            return CompareBuildOrder(left.Text, right.Text);
        (string leftCore, _) = SplitPrerelease(left.Text);
        (string rightCore, _) = SplitPrerelease(right.Text);
        int core = CompareNumericRuns(leftCore, rightCore);
        // The cores tie, so the answer lives entirely in a tail at least one side does not have.
        return core == 0 ? null : core;
    }

    /// <summary>Compares by numeric runs, falling back to an ordinal tiebreak when every number
    /// matches. Mirrors <c>okp_core::update_selection::compare_versions</c>.</summary>
    private static int CompareNumericRuns(string left, string right)
    {
        List<ulong> leftKey = SortKey(left);
        List<ulong> rightKey = SortKey(right);
        int max = Math.Max(leftKey.Count, rightKey.Count);
        for (int i = 0; i < max; i++)
        {
            ulong leftPart = i < leftKey.Count ? leftKey[i] : 0;
            ulong rightPart = i < rightKey.Count ? rightKey[i] : 0;
            if (leftPart != rightPart)
                return leftPart < rightPart ? -1 : 1;
        }
        return string.CompareOrdinal(left, right);
    }

    private static List<ulong> SortKey(string version)
    {
        var key = new List<ulong>();
        int start = -1;
        for (int i = 0; i <= version.Length; i++)
        {
            bool digit = i < version.Length && char.IsAsciiDigit(version[i]);
            if (digit && start < 0)
                start = i;
            else if (!digit && start >= 0)
            {
                key.Add(ulong.TryParse(version.AsSpan(start, i - start), out ulong value) ? value : 0);
                start = -1;
            }
        }
        return key;
    }

    /// <summary>Orders two prerelease tails identifier by identifier, so the stage is compared
    /// before its counter (<c>alpha.109</c> precedes <c>beta.1</c>). Within an identifier two
    /// numbers compare numerically and anything else compares ordinally; a number sorts before a
    /// word, and a tail that is a prefix of another sorts before it.</summary>
    private static int ComparePrerelease(string left, string right)
    {
        string[] leftParts = left.Split('.');
        string[] rightParts = right.Split('.');
        int max = Math.Max(leftParts.Length, rightParts.Length);
        for (int i = 0; i < max; i++)
        {
            if (i >= leftParts.Length)
                return -1;
            if (i >= rightParts.Length)
                return 1;
            bool leftIsNumber = ulong.TryParse(leftParts[i], out ulong leftNumber);
            bool rightIsNumber = ulong.TryParse(rightParts[i], out ulong rightNumber);
            int order = (leftIsNumber, rightIsNumber) switch
            {
                (true, true) => leftNumber.CompareTo(rightNumber),
                // A numeric identifier ranks below an alphanumeric one.
                (true, false) => -1,
                (false, true) => 1,
                (false, false) => string.CompareOrdinal(leftParts[i], rightParts[i]),
            };
            if (order != 0)
                return order;
        }
        return 0;
    }

    private static (string Core, string? Prerelease) SplitPrerelease(string version)
    {
        int dash = version.IndexOf('-', StringComparison.Ordinal);
        return dash < 0 ? (version, null) : (version[..dash], version[(dash + 1)..]);
    }
}

/// <summary>The update state machine for one install. Constructed with the install kind and the
/// version this process is running; every transition either advances the state or returns false and
/// leaves it untouched.
///
/// Deliberate divergences from <c>okp_core::update_lifecycle</c>: a refused transition returns
/// <c>false</c> rather than a typed error (the reason is for the Rust surfaces, and a refusal never
/// changes the state either way); a resume asked for on a dev build degrades to a plain
/// <see cref="UpdateStateKind.Idle"/> lifecycle instead of an error, since the caller has nothing
/// else to do with it; the offer a re-check carries is the previous <see cref="UpdateState"/>
/// itself rather than a separate carried-offer type; and the skip/system-managed lanes are absent
/// with their kinds.</summary>
public sealed class UpdateLifecycle
{
    /// <summary>What the Updates surface says while a check is in flight. A refresh over a standing
    /// offer appends it to that offer's own message instead of replacing it, so the offer stays
    /// legible while it is being refreshed.</summary>
    private const string CheckingMessage = "Checking for updates…";

    private readonly InstallKind _installKind;
    private ReportedVersion _runningVersion;
    private UpdateState _state;

    /// <summary>What went wrong with the last attempt when the state itself carried on regardless —
    /// a failed refresh that restored the offer it was refreshing. Cleared by every transition that
    /// succeeds.</summary>
    private string? _notice;

    /// <summary>Starts at <see cref="UpdateState.Idle"/> for <paramref name="installKind"/>.</summary>
    public UpdateLifecycle(InstallKind installKind, ReportedVersion runningVersion)
        : this(installKind, runningVersion, UpdateState.Idle)
    {
    }

    private UpdateLifecycle(InstallKind installKind, ReportedVersion runningVersion, UpdateState state)
    {
        _installKind = installKind;
        _runningVersion = runningVersion;
        _state = state;
    }

    /// <summary>Decides the install kind from shell-gathered evidence. Pure: no filesystem, no
    /// environment, no process inspection. On Windows the Velopack layout — an <c>Update.exe</c>
    /// beside a <c>current</c> directory — is the one signal; without it the process is a dev or
    /// portable build, which is not the same thing as being up to date.</summary>
    public static InstallKind Detect(InstallEvidence evidence)
    {
        ArgumentNullException.ThrowIfNull(evidence);
        return evidence.VelopackLayoutPresent ? InstallKind.WindowsVelopack : InstallKind.DevBuild;
    }

    /// <summary>Rebuilds the lifecycle in a process that started with an update already downloaded
    /// and staged but not yet applied — the user downloaded it, closed the app, and came back.
    /// A record that is not newer than what is running is stale and is discarded rather than
    /// offered; when the versions on hand cannot decide that (#694) the payload is kept, because
    /// that it exists is an observed fact rather than an ordering conclusion, and throwing away a
    /// downloaded update on a guess loses a real one silently.</summary>
    public static UpdateLifecycle ResumedWithStagedUpdate(
        InstallKind installKind, ReportedVersion runningVersion, ReportedVersion stagedVersion)
    {
        if (Capability(installKind) != UpdateCapability.SelfApply)
            return new UpdateLifecycle(installKind, runningVersion);
        int? order = VersionOrder.CompareReportedBuildOrder(stagedVersion, runningVersion);
        UpdateState state = order is null or > 0
            ? UpdateState.ReadyToApply(stagedVersion.Text)
            : UpdateState.Idle;
        return new UpdateLifecycle(installKind, runningVersion, state);
    }

    /// <summary>Rebuilds the lifecycle in the process that came up after a self-applied update, and
    /// settles the restart in one step. A self-apply replaces the process, so the
    /// <c>RestartPending</c> lifecycle that staged the update dies with the old one; the shell
    /// persists the pending target across the restart and passes it here with the version the new
    /// process actually came up as, so a restart that silently came back on the old binary is
    /// caught across the process boundary too (#660).</summary>
    public static UpdateLifecycle ResumedAfterRestart(
        InstallKind installKind, ReportedVersion runningVersion, string pendingVersion)
    {
        if (Capability(installKind) != UpdateCapability.SelfApply)
            return new UpdateLifecycle(installKind, runningVersion);
        var lifecycle = new UpdateLifecycle(
            installKind, runningVersion, UpdateState.RestartPending(pendingVersion));
        lifecycle.RestartedInto(runningVersion);
        return lifecycle;
    }

    public InstallKind InstallKind => _installKind;

    public UpdateCapability Capability() => Capability(_installKind);

    private static UpdateCapability Capability(InstallKind kind) =>
        kind == InstallKind.WindowsVelopack ? UpdateCapability.SelfApply : UpdateCapability.Unmanaged;

    public UpdateState State => _state;

    /// <summary>The version this process is executing — not the version an applied but unrestarted
    /// update would run.</summary>
    public ReportedVersion RunningVersion => _runningVersion;

    /// <summary>Begins a check. Allowed from every settled state; refused outright for an
    /// <see cref="UpdateCapability.Unmanaged"/> install, which has nothing to check.</summary>
    public bool StartCheck()
    {
        if (Capability() == UpdateCapability.Unmanaged)
            return false;
        switch (_state.Kind)
        {
            case UpdateStateKind.Idle:
            case UpdateStateKind.UpToDate:
            case UpdateStateKind.Available:
            case UpdateStateKind.Running:
            // A check is exactly how an unverifiable restart gets settled: the feed knows whether
            // the target is still on offer.
            case UpdateStateKind.RestartUnverified:
            case UpdateStateKind.Failed:
                // A re-check must not lose the offer already on screen: if it fails, that offer
                // comes back exactly as it was.
                return Enter(UpdateState.Checking(CarriedOffer()));
            default:
                return false;
        }
    }

    private UpdateState? CarriedOffer() => _state.Kind switch
    {
        UpdateStateKind.Available => _state,
        UpdateStateKind.Failed when _state.Version is not null => _state,
        // A check is what settles an unconfirmed restart, so a check that fails settles nothing:
        // the pending target and the fact that nothing is known about it both survive the refresh
        // instead of collapsing into a generic failure that has forgotten the version.
        UpdateStateKind.RestartUnverified => _state,
        _ => null,
    };

    /// <summary>The check completed and found nothing newer.</summary>
    public bool CheckFoundNone() =>
        _state.Kind == UpdateStateKind.Checking && Enter(UpdateState.UpToDate);

    /// <summary>The check found <paramref name="version"/>.</summary>
    public bool CheckFound(string version)
    {
        if (Capability() == UpdateCapability.Unmanaged || _state.Kind != UpdateStateKind.Checking)
            return false;
        return Enter(UpdateState.Available(version));
    }

    /// <summary>The check itself failed (no network, bad manifest, …). The offer that was on screen
    /// before the refresh comes back intact, with the error carried beside it as a notice; only a
    /// check that had no offer to protect ends in <c>Failed</c>.</summary>
    public bool CheckFailed(string reason)
    {
        if (_state.Kind != UpdateStateKind.Checking)
            return false;
        UpdateState? carried = _state.Carried;
        return carried is null
            ? Enter(UpdateState.Failed(reason, null, staged: false))
            : EnterWithNotice(carried, $"Update check failed: {reason}");
    }

    /// <summary>Starts fetching the discovered payload.</summary>
    public bool StartDownload() =>
        _state.Kind == UpdateStateKind.Available
        && Enter(UpdateState.Downloading(_state.Version!));

    /// <summary>The payload is downloaded and verified.</summary>
    public bool DownloadFinished() =>
        _state.Kind == UpdateStateKind.Downloading
        && Enter(UpdateState.ReadyToApply(_state.Version!));

    /// <summary>The download or its verification failed. Nothing is staged.</summary>
    public bool DownloadFailed(string reason) =>
        _state.Kind == UpdateStateKind.Downloading
        && Enter(UpdateState.Failed(reason, _state.Version, staged: false));

    /// <summary>Starts applying the staged payload.</summary>
    public bool StartApply() =>
        _state.Kind == UpdateStateKind.ReadyToApply
        && Enter(UpdateState.Applying(_state.Version!));

    /// <summary>Applying succeeded: the new version is on disk, this process still runs the old
    /// one. The only success exit from <c>Applying</c> — becoming the new version means replacing
    /// the process, so anything still executing to claim it is by definition the old binary, which
    /// is exactly #660.</summary>
    public bool ApplyNeedsRestart() =>
        _state.Kind == UpdateStateKind.Applying
        && Enter(UpdateState.RestartPending(_state.Version!));

    /// <summary>Applying failed. <c>Applying</c> is only reachable from <c>ReadyToApply</c>, so the
    /// verified payload is still on disk and a retry applies it.</summary>
    public bool ApplyFailed(string reason) =>
        _state.Kind == UpdateStateKind.Applying
        && Enter(UpdateState.Failed(reason, _state.Version, staged: true));

    /// <summary>The process restarted and reports the version it came back as. Coming back on the
    /// pending target — or on anything newer — completes the update; coming back older is #660
    /// itself. A running version the shell could only report truncated leaves the two
    /// indistinguishable (#694): that is <c>RestartUnverified</c>, neither the success nor the
    /// downgrade, both of which would be invented.</summary>
    public bool RestartedInto(ReportedVersion runningVersion)
    {
        if (_state.Kind != UpdateStateKind.RestartPending)
            return false;
        string pending = _state.Version!;
        int? order = VersionOrder.CompareReportedBuildOrder(
            runningVersion, ReportedVersion.Complete(pending));
        _runningVersion = runningVersion;
        UpdateState next = order switch
        {
            null => UpdateState.RestartUnverified(pending),
            >= 0 => UpdateState.Running(runningVersion.Text),
            _ => UpdateState.Failed(
                $"restart still runs {runningVersion.Text}; the update to {pending} did not take effect",
                pending,
                // The apply already consumed the payload; recovering starts over — and it starts
                // with a check, never with another restart behind one press (#701).
                staged: false,
                recovery: FailureRecovery.CheckAgain),
        };
        return Enter(next);
    }

    /// <summary>Restores the offer a failure interrupted, without a fresh discovery round: a
    /// download or an apply that failed kept its target, so the same version becomes actionable
    /// again. A verified payload is not thrown away because applying it failed once — the retry
    /// re-applies rather than re-downloads. A check that failed before finding anything has nothing
    /// to restore; <see cref="StartCheck"/> is its retry. So is a failure whose recovery is
    /// <see cref="FailureRecovery.CheckAgain"/> (#701): restoring an offer there would put a version
    /// back on screen as actionable when nothing local backs it.</summary>
    public bool RetryFailedUpdate()
    {
        if (Capability() != UpdateCapability.SelfApply
            || _state.Kind != UpdateStateKind.Failed
            || _state.Recovery != FailureRecovery.RepeatTheStep
            || _state.Version is not { } version)
            return false;
        return Enter(_state.Staged
            ? UpdateState.ReadyToApply(version)
            : UpdateState.Available(version));
    }

    /// <summary>Everything the surfaces may show for the current state. The only place update text
    /// is produced.</summary>
    public UpdatePresentation Describe()
    {
        VersionClaim claim = Claim();
        string updates = UpdatesMessage(claim);
        if (_notice is { } notice)
            updates = $"{updates} {notice}";
        UpdateAction? action = Action();
        return new UpdatePresentation(
            InstallKind: _installKind,
            Capability: Capability(),
            VersionInUse: _runningVersion.Text,
            TargetVersion: _state.TargetVersion,
            Claim: claim,
            UpdatesMessage: updates,
            AboutMessage: AboutMessage(claim),
            Action: action,
            ActionsEnabled: _state.Kind != UpdateStateKind.Checking,
            ActionClosesTheApp: action is UpdateAction.ApplyAndRestart or UpdateAction.RestartToFinish);
    }

    /// <summary>The label a shell renders on the action control.</summary>
    public static string Label(UpdateAction action) => action switch
    {
        UpdateAction.CheckNow => "Check for updates",
        UpdateAction.DownloadUpdate => "Update now",
        UpdateAction.ApplyAndRestart => "Install and restart",
        UpdateAction.RestartToFinish => "Restart now",
        UpdateAction.Retry => "Try again",
        _ => "Check for updates",
    };

    private VersionClaim Claim()
    {
        if (Capability() == UpdateCapability.Unmanaged)
            return VersionClaim.NotApplicable;
        return _state.Kind switch
        {
            UpdateStateKind.UpToDate or UpdateStateKind.Running => VersionClaim.Current,
            // The applied bits are on disk, but this process is still the old binary: superseded,
            // not current (#660). A failure that kept its target, and a refresh over a standing
            // offer, both still know a newer build exists.
            UpdateStateKind.Available
                or UpdateStateKind.Downloading
                or UpdateStateKind.ReadyToApply
                or UpdateStateKind.Applying
                or UpdateStateKind.RestartPending => VersionClaim.Superseded,
            UpdateStateKind.Failed when _state.Version is not null => VersionClaim.Superseded,
            // Refreshing an unconfirmed restart does not turn it into a known one: what is being
            // checked is precisely which build is running.
            UpdateStateKind.Checking when _state.Carried is { Kind: UpdateStateKind.RestartUnverified }
                => VersionClaim.Unknown,
            UpdateStateKind.Checking when _state.Carried is not null => VersionClaim.Superseded,
            // The restart happened; which build came up is precisely what cannot be told, so
            // nothing may be claimed either way (#694).
            _ => VersionClaim.Unknown,
        };
    }

    /// <summary>The Updates message for the current state. A refresh over a standing offer
    /// describes both: the offer is what the surface is showing — with its own controls, kept by
    /// <see cref="Action"/> — and the check is a status on top of it. Replacing the offer's message
    /// with the check's would hide what the carried version was. Mirrors
    /// <c>okp_core::update_lifecycle</c>.</summary>
    private string UpdatesMessage(VersionClaim claim)
    {
        if (claim == VersionClaim.NotApplicable)
            return "Updates are disabled for development builds.";
        if (_state.Kind == UpdateStateKind.Checking && _state.Carried is { } carried)
            return $"{MessageFor(carried)} {CheckingMessage}";
        return MessageFor(_state);
    }

    /// <summary>The message for one state on its own, with no check in flight over it.</summary>
    private string MessageFor(UpdateState state)
    {
        string version = state.Version ?? string.Empty;
        return state.Kind switch
        {
            UpdateStateKind.Idle => "OK Player has not checked for updates yet.",
            UpdateStateKind.Checking => CheckingMessage,
            UpdateStateKind.UpToDate => "OK Player is up to date.",
            UpdateStateKind.Available => $"Version {version} is available.",
            UpdateStateKind.Downloading => $"Downloading version {version}…",
            UpdateStateKind.ReadyToApply => $"Version {version} is ready to install.",
            UpdateStateKind.Applying => $"Installing version {version}…",
            UpdateStateKind.RestartPending =>
                $"Version {version} is installed. Restart OK Player to start running it — this session is still on {_runningVersion.Text}.",
            UpdateStateKind.RestartUnverified =>
                $"OK Player restarted after installing version {version}, but this build reports its version as {_runningVersion.Text} without the part that would tell the two apart, so whether the update took effect cannot be confirmed. Check for updates to settle it.",
            UpdateStateKind.Running => $"OK Player is now running version {version}.",
            // A failure whose recovery is a check says so, because the check is what the surface is
            // about to offer (#701).
            UpdateStateKind.Failed
                when state.Version is not null && state.Recovery == FailureRecovery.CheckAgain =>
                $"The update to version {version} failed: {state.Reason}. Check for updates to try it again.",
            UpdateStateKind.Failed when state.Version is not null =>
                $"The update to version {version} failed: {state.Reason}",
            _ => $"Update failed: {state.Reason}",
        };
    }

    /// <summary>The About message for the current state. Read off the presented state for the same
    /// reason the Updates message is: the two surfaces come from one projection, so About reading
    /// the raw Checking state would say "version X is available" beside an Updates line saying the
    /// same version failed.</summary>
    private string AboutMessage(VersionClaim claim)
    {
        string running = _runningVersion.Text;
        UpdateState presented = PresentedState;
        string target = presented.Version ?? string.Empty;
        return claim switch
        {
            VersionClaim.Current => $"OK Player {running} — up to date.",
            VersionClaim.Superseded => presented.Kind switch
            {
                UpdateStateKind.RestartPending =>
                    $"OK Player {running} — restart to finish updating to {target}.",
                UpdateStateKind.Failed => $"OK Player {running} — updating to {target} failed.",
                _ => $"OK Player {running} — version {target} is available.",
            },
            VersionClaim.NotApplicable =>
                $"OK Player {running} — development build; updates are disabled.",
            _ => presented.Kind == UpdateStateKind.RestartUnverified
                ? $"OK Player {running} — the update to {target} could not be confirmed from this build's version."
                : $"OK Player {running}.",
        };
    }

    /// <summary>The state the surface is presenting. A refresh keeps the offer it is refreshing on
    /// screen, controls and all — ActionsEnabled says they cannot be pressed yet — so everything
    /// derived from the offer reads it here rather than each deriving it again.</summary>
    private UpdateState PresentedState =>
        _state.Kind == UpdateStateKind.Checking && _state.Carried is { } carried ? carried : _state;

    private UpdateAction? Action()
    {
        if (Capability() == UpdateCapability.Unmanaged)
            return null;
        UpdateState state = PresentedState;
        return state.Kind switch
        {
            UpdateStateKind.Idle
                or UpdateStateKind.UpToDate
                or UpdateStateKind.Running
                // Nothing can be applied here — the payload is already on disk; only a check can
                // say which build is running.
                or UpdateStateKind.RestartUnverified => UpdateAction.CheckNow,
            UpdateStateKind.Available => UpdateAction.DownloadUpdate,
            UpdateStateKind.ReadyToApply => UpdateAction.ApplyAndRestart,
            UpdateStateKind.RestartPending => UpdateAction.RestartToFinish,
            // A failure the recovery cannot repeat is not a retry, and must not be offered as one:
            // the check it really performs is what the surface says, and the install it discovers is
            // the press after that (#701).
            UpdateStateKind.Failed when state.Recovery == FailureRecovery.CheckAgain
                => UpdateAction.CheckNow,
            UpdateStateKind.Failed => UpdateAction.Retry,
            // A download or an apply in flight offers nothing, and a check with no offer behind it
            // has nothing to keep on screen.
            _ => null,
        };
    }

    private bool Enter(UpdateState next)
    {
        _state = next;
        _notice = null;
        return true;
    }

    private bool EnterWithNotice(UpdateState next, string notice)
    {
        _state = next;
        _notice = notice;
        return true;
    }
}
