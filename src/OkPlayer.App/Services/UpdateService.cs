using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Core;
using Velopack;
using Velopack.Sources;

namespace OkPlayer.App.Services;

/// <summary>Drives Velopack through the shared update lifecycle (issue #682). Velopack owns the
/// network, the disk and the relaunch; <see cref="OkPlayer.Core.UpdateLifecycle"/> owns which state
/// the app is in and every word said about it, so the Updates card, the About block and this
/// service cannot disagree, and an applied-but-not-restarted update is never drawn as "you are on
/// the new version" (#660).
///
/// A dev or portable build has no Velopack layout around it, so it is an
/// <see cref="InstallKind.DevBuild"/>: updates are disabled and said to be, rather than the old
/// hand-written "Unavailable (development build)". All network/disk work runs off the UI thread;
/// <see cref="ApplyAndRestart"/> tears the process down and must be called from the UI thread.
/// <see cref="Changed"/> may fire OFF the UI thread — handlers must marshal before touching UI.</summary>
public sealed class UpdateService
{
    private readonly UpdateManager _mgr;
    private readonly object _gate = new();          // guards _lifecycle; transitions arrive from the check worker and the UI thread
    private readonly UpdateLifecycle _lifecycle;
    private volatile UpdateInfo? _pending;           // a downloaded, ready-to-apply update (null until found + downloaded); written by the check worker, read on the UI thread
    private int _checking;                          // 0/1 guard so overlapping background checks don't stack
    private bool _markerHeld;                       // guarded by _gate: the pending-restart marker is still on disk because the restart it records is unconfirmed

    public UpdateService()
    {
        // Stable installs use the static releases.win.json on GitHub Pages (UpdateFeed.WinBaseUrl), NOT the
        // GitHub release listing: GithubSource only ever inspected the first 10 entries of that listing, so
        // any 10 releases without the win feed asset silently blinded the installed fleet (issues #130/#131).
        // Candidate packages stamp an assembly-metadata override for the isolated rolling release and
        // releases.win-candidate.json; normal builds carry no override, so the stable URL/channel remains the
        // default. SimpleWebSource supports both layouts and downloads URL-valued entries as-is.
        // A failed feed fetch (HTTP error, offline) THROWS out of CheckForUpdatesAsync — it is never an empty
        // feed — which is what keeps a failed check distinct from a confirmed "up to date". Constructing this
        // is safe on any build; only the operations gate on the install kind.
        UpdateFeedConfiguration feed = UpdateFeed.Resolve(typeof(App).Assembly);
        var source = new SimpleWebSource(feed.BaseUrl);
        _mgr = new UpdateManager(source, new UpdateOptions { ExplicitChannel = feed.Channel });
        _lifecycle = BuildLifecycle();
        UpdatePresentation start = Presentation;
        // The installed-app CI lane reads this line: the update surface reports a state, not a
        // hand-written string.
        Log.Info($"update: install={start.InstallKind} capability={start.Capability} "
            + $"state={_lifecycle.State.Kind} claim={start.Claim} version={start.VersionInUse} — {start.UpdatesMessage}");
    }

    /// <summary>Where this process starts in the lifecycle. Three openings, in order of what the
    /// machine can be observed to have done: it came back from a restart this app asked for (the
    /// marker below), it starts with a payload a previous run downloaded but never applied
    /// (Velopack keeps the pending release on disk), or it starts clean.</summary>
    private UpdateLifecycle BuildLifecycle()
    {
        InstallKind kind = UpdateLifecycle.Detect(new InstallEvidence(VelopackLayoutPresent: IsInstalled()));
        ReportedVersion running = App.RunningVersion;
        if (PendingRestartMarker.Read() is { } target)
        {
            // Settles the restart against the version that actually came up: on the target (or
            // newer) it completes, on the old binary it is the #660 failure, and on a version too
            // coarse to tell them apart it is neither (#694).
            UpdateLifecycle resumed = UpdateLifecycle.ResumedAfterRestart(kind, running, target);
            // A settled restart consumes the marker. An unconfirmed one does not: it is the only
            // record of which version this install was told it was getting, and a relaunch before
            // the settling check succeeds — offline, or with automatic checks off — would otherwise
            // come up at Idle having quietly forgotten the whole thing.
            _markerHeld = resumed.State.Kind == UpdateStateKind.RestartUnverified;
            if (!_markerHeld)
                PendingRestartMarker.Clear();
            return resumed;
        }
        if (StagedVersion() is { } staged)
            return UpdateLifecycle.ResumedWithStagedUpdate(kind, running, ReportedVersion.Complete(staged));
        return new UpdateLifecycle(kind, running);
    }

    /// <summary>Velopack's own answer to "is this process running out of an installed layout?" — it
    /// looks for the Update.exe beside the <c>current</c> directory that only an installed build
    /// has. False in dev / portable builds.</summary>
    private bool IsInstalled()
    {
        try { return _mgr.IsInstalled; }
        catch (Exception ex) { Log.Warn("UpdateService.IsInstalled: " + ex.Message); return false; }
    }

    /// <summary>The version of a payload staged and waiting to be applied — this session's download
    /// or one a previous run left on disk (<c>SetAutoApplyOnStartup(false)</c> means it isn't
    /// applied behind the user's back, so without this it would look like "no update" until a fresh
    /// online check re-found it). Null when nothing is staged.</summary>
    private string? StagedVersion()
    {
        try { return (_pending?.TargetFullRelease ?? _mgr.UpdatePendingRestart)?.Version?.ToString(); }
        catch (Exception ex) { Log.Warn("UpdateService.StagedVersion: " + ex.Message); return null; }
    }

    /// <summary>True only for a real Velopack-installed build; false in dev / portable, where updates
    /// no-op and the surfaces say so.</summary>
    public bool IsSupported => _lifecycle.Capability() == UpdateCapability.SelfApply;

    /// <summary>Everything the surfaces may show, derived from the current state. Immutable, so it is
    /// safe to hand to the UI thread.</summary>
    public UpdatePresentation Presentation
    {
        get { lock (_gate) { return _lifecycle.Describe(); } }
    }

    /// <summary>Raised when the update state changes (check started/finished, update downloaded). May
    /// fire OFF the UI thread — marshal before touching XAML.</summary>
    public event Action? Changed;

    /// <summary>Ask the update feed for a newer release and, if found, download it in the background.
    /// Safe to call fire-and-forget on launch. No-ops on dev/portable builds, when a check is already
    /// running, or when an update is already staged. Never throws — a failed check (offline,
    /// rate-limited, torn-down feed) becomes a state the surfaces can report.</summary>
    public async Task CheckAndDownloadAsync()
    {
        if (!IsSupported)
            return; // not a Velopack build
        if (Interlocked.Exchange(ref _checking, 1) == 1)
            return; // another check already running
        try
        {
            if (!Transition(life => life.StartCheck()))
                return; // an apply is in flight, or a payload is already staged and waiting
            UpdateInfo? info = await _mgr.CheckForUpdatesAsync().ConfigureAwait(false);
            if (info is null)
            {
                Transition(life => life.CheckFoundNone());
                return; // already current
            }
            string target = info.TargetFullRelease?.Version?.ToString() ?? string.Empty;
            if (target.Length == 0)
            {
                // A release with no version cannot be ordered, named, or verified after a restart —
                // report the check as failed rather than offering a nameless update.
                Transition(life => life.CheckFailed("the update feed offered a release with no version"));
                return;
            }
            Transition(life => life.CheckFound(target));
            Transition(life => life.StartDownload());
            await _mgr.DownloadUpdatesAsync(info).ConfigureAwait(false);
            _pending = info;
            Transition(life => life.DownloadFinished());
        }
        catch (Exception ex)
        {
            Log.Error("UpdateService.CheckAndDownload: " + ex.Message);
            // Which half failed decides what survives: a failed check restores the offer it was
            // refreshing, a failed download keeps its target retryable.
            Transition(life => life.State.Kind == UpdateStateKind.Downloading
                ? life.DownloadFailed(ex.Message)
                : life.CheckFailed(ex.Message));
        }
        finally
        {
            Interlocked.Exchange(ref _checking, 0);
            Changed?.Invoke();
        }
    }

    /// <summary>Take the offered action. <see cref="UpdateAction.CheckNow"/> and
    /// <see cref="UpdateAction.Retry"/> are fire-and-forget; the two that apply an update shut the
    /// process down, so they must be called from the UI thread once the user agrees.</summary>
    public void Invoke(UpdateAction action)
    {
        switch (action)
        {
            case UpdateAction.CheckNow:
            case UpdateAction.DownloadUpdate:
                _ = CheckAndDownloadAsync();
                break;
            case UpdateAction.Retry:
                // Restoring the interrupted offer is all a retry does. A payload that survived a
                // failed apply comes back as "Install and restart" — the action that says it closes
                // the app — and the user takes it from there; applying it from here would tear the
                // session down behind a button labelled "Try again". A check that failed before
                // finding anything has no offer to restore, so a fresh check is its retry.
                if (!Transition(life => life.RetryFailedUpdate()))
                    _ = CheckAndDownloadAsync();
                break;
            case UpdateAction.ApplyAndRestart:
            case UpdateAction.RestartToFinish:
                ApplyAndRestart();
                break;
        }
    }

    /// <summary>Apply the staged update and relaunch the app. This shuts the process down, so call it
    /// on the UI thread once the user agrees. No-op if nothing is staged.</summary>
    public void ApplyAndRestart()
    {
        // Prefer this session's downloaded update; otherwise apply a package left staged on disk by a prior run
        // (so a download isn't lost when the user relaunched before pressing Restart).
        VelopackAsset? staged;
        try { staged = _pending?.TargetFullRelease ?? _mgr.UpdatePendingRestart; }
        catch (Exception ex) { Log.Error("UpdateService.ApplyAndRestart: " + ex.Message); return; }
        if (staged is null)
            return;
        if (!Transition(life => life.StartApply()))
            return; // nothing to apply from this state
        string target = staged.Version?.ToString() ?? string.Empty;
        // Written before the apply, because a successful ApplyUpdatesAndRestart never returns: the
        // next process reads it to settle whether the restart actually landed on the new build.
        PendingRestartMarker.Write(target);
        try
        {
            _mgr.ApplyUpdatesAndRestart(staged);
        }
        catch (Exception ex)
        {
            // The process is still here, so the restart never happened and the marker would make the
            // next launch report an update that was never applied.
            PendingRestartMarker.Clear();
            Log.Error("UpdateService.ApplyAndRestart: " + ex.Message);
            Transition(life => life.ApplyFailed(ex.Message));
        }
    }

    /// <summary>Run a transition under the lock and announce it. Returns whether the state moved —
    /// the lifecycle never changes on a refusal, so a caller may simply ignore one.</summary>
    private bool Transition(Func<UpdateLifecycle, bool> transition)
    {
        bool moved;
        UpdateStateKind kind;
        UpdatePresentation presentation;
        lock (_gate)
        {
            moved = transition(_lifecycle);
            kind = _lifecycle.State.Kind;
            presentation = _lifecycle.Describe();
            // The marker is released only once the restart it records is actually settled. A check
            // in flight has settled nothing — it can fail, or the app can close mid-request, and
            // either way the next process still needs the pending target — so the check itself,
            // which carries the unconfirmed restart, keeps it. What releases it is the outcome:
            // a check that found something, or found nothing. ApplyAndRestart writes its own marker
            // afterwards.
            bool stillUnsettled = kind == UpdateStateKind.RestartUnverified
                || (kind == UpdateStateKind.Checking
                    && _lifecycle.State.Carried is { Kind: UpdateStateKind.RestartUnverified });
            if (moved && _markerHeld && !stillUnsettled)
            {
                PendingRestartMarker.Clear();
                _markerHeld = false;
            }
        }
        if (!moved)
            return false;
        Log.Info($"update: state={kind} claim={presentation.Claim} — {presentation.UpdatesMessage}");
        Changed?.Invoke();
        return true;
    }

    /// <summary>The version this process asked the machine to restart into, carried across the
    /// relaunch that replaces it. Velopack keeps the packages; nothing in it remembers what the old
    /// process was expecting, which is what the restart check needs to catch a relaunch that came
    /// back on the old binary (#660). One line of text beside the settings file, removed as soon as
    /// it is read so a settled restart can never be reported twice.</summary>
    private static class PendingRestartMarker
    {
        private static string MarkerPath => Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
            "OkPlayer",
            "pending-update");

        public static void Write(string version)
        {
            try
            {
                string? dir = Path.GetDirectoryName(MarkerPath);
                if (dir is not null)
                    Directory.CreateDirectory(dir);
                File.WriteAllText(MarkerPath, version);
            }
            catch (Exception ex) { Log.Warn("update: could not record the pending restart: " + ex.Message); }
        }

        /// <summary>The recorded pending version, or null when there is none. The marker is removed
        /// by <see cref="Clear"/> once the restart it records has actually been settled — reading it
        /// is not settling it.</summary>
        public static string? Read()
        {
            try
            {
                if (!File.Exists(MarkerPath))
                    return null;
                string version = File.ReadAllText(MarkerPath).Trim();
                return version.Length == 0 ? null : version;
            }
            catch (Exception ex) { Log.Warn("update: could not read the pending restart: " + ex.Message); return null; }
        }

        public static void Clear()
        {
            try { File.Delete(MarkerPath); }
            catch (Exception ex) { Log.Warn("update: could not clear the pending restart: " + ex.Message); }
        }
    }
}
