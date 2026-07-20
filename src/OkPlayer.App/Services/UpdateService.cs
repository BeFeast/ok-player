using System;
using System.Threading;
using System.Threading.Tasks;
using OkPlayer.Core;
using Velopack;
using Velopack.Sources;

namespace OkPlayer.App.Services;

/// <summary>Wraps Velopack's <see cref="UpdateManager"/>: a background "is there a newer release?" check,
/// a download, and an apply-on-restart. Ships dark until the app is actually distributed via the Velopack
/// installer — until then <see cref="IsSupported"/> is false (a dev or portable build reports
/// <c>IsInstalled == false</c>) and every operation is a safe no-op. All network/disk work runs off the UI
/// thread; <see cref="ApplyAndRestart"/> tears the process down and must be called from the UI thread.
/// State changes raise <see cref="Changed"/>, which may fire OFF the UI thread — handlers must marshal to their
/// own dispatcher before touching UI.</summary>
public sealed class UpdateService
{
    private readonly UpdateManager _mgr;
    private UpdateInfo? _pending;        // a downloaded, ready-to-apply update (null until found + downloaded)
    private int _checking;               // 0/1 guard so overlapping background checks don't stack
    private volatile bool _checkedOk;    // a check has completed successfully at least once this session
    private volatile bool _lastCheckFailed; // the most recent check/download threw (offline, feed unreachable, …)

    public UpdateService()
    {
        // Stable installs use the static releases.win.json on GitHub Pages (UpdateFeed.WinBaseUrl), NOT the
        // GitHub release listing: GithubSource only ever inspected the first 10 entries of that listing, so
        // any 10 releases without the win feed asset silently blinded the installed fleet (issues #130/#131).
        // Candidate packages stamp an assembly-metadata override for the isolated rolling release and
        // releases.win-candidate.json; normal builds carry no override, so the stable URL/channel remains the
        // default. SimpleWebSource supports both layouts and downloads URL-valued entries as-is.
        // A failed feed fetch (HTTP error, offline) THROWS out of CheckForUpdatesAsync — it is never an empty
        // feed — which is what keeps LastCheckFailed honest below. Constructing this is safe on any build;
        // only the operations gate on IsInstalled.
        UpdateFeedConfiguration feed = UpdateFeed.Resolve(typeof(App).Assembly);
        var source = new SimpleWebSource(feed.BaseUrl);
        _mgr = new UpdateManager(source, new UpdateOptions { ExplicitChannel = feed.Channel });
    }

    /// <summary>True only for a real Velopack-installed build; false in dev / portable, where updates no-op.</summary>
    public bool IsSupported => _mgr.IsInstalled;

    /// <summary>A check is in flight right now (so the UI can show a spinner / disable the button).</summary>
    public bool IsChecking => Volatile.Read(ref _checking) == 1;

    /// <summary>A check has completed successfully at least once this session — so the UI can tell a confirmed
    /// "up to date" apart from "haven't checked yet" (auto-check off, or before the launch check returns).</summary>
    public bool CheckedOk => _checkedOk;

    /// <summary>The most recent check/download failed (offline, rate-limited, feed unreachable) — so the UI can
    /// say "couldn't check" instead of implying a confirmed up-to-date result.</summary>
    public bool LastCheckFailed => _lastCheckFailed;

    /// <summary>A newer release is staged and a restart will apply it — either downloaded this session
    /// (<c>_pending</c>) or left staged on disk by a previous run (Velopack's
    /// <see cref="UpdateManager.UpdatePendingRestart"/>). The latter is what lets a download survive the user
    /// relaunching before they press Restart — <c>SetAutoApplyOnStartup(false)</c> means it isn't auto-applied,
    /// so without this it would look like "no update" until a fresh online check re-found it.</summary>
    public bool UpdateReady => _pending is not null || _mgr.UpdatePendingRestart is not null;

    /// <summary>Version string of the staged, ready-to-apply update (this session's, or a prior run's), or null
    /// when none is staged.</summary>
    public string? PendingVersion => (_pending?.TargetFullRelease ?? _mgr.UpdatePendingRestart)?.Version?.ToString();

    /// <summary>Raised when the update state changes (check started/finished, update downloaded). May fire OFF
    /// the UI thread — marshal before touching XAML.</summary>
    public event Action? Changed;

    /// <summary>Ask the update feed for a newer release and, if found, download it in the background. Safe to call
    /// fire-and-forget on launch. No-ops on dev/portable builds, when a check is already running, or when an
    /// update is already staged. Never throws — a failed check (offline, rate-limited, torn-down feed) is logged
    /// and swallowed so it can never break the app.</summary>
    public async Task CheckAndDownloadAsync()
    {
        if (!IsSupported || UpdateReady)
            return; // not a Velopack build, or an update is already staged (this session or left on disk by a prior run)
        if (Interlocked.Exchange(ref _checking, 1) == 1)
            return; // another check already running
        Changed?.Invoke();
        try
        {
            UpdateInfo? info = await _mgr.CheckForUpdatesAsync().ConfigureAwait(false);
            _checkedOk = true;          // the check itself succeeded (whether or not it found an update)
            _lastCheckFailed = false;
            if (info is null)
                return; // already current
            await _mgr.DownloadUpdatesAsync(info).ConfigureAwait(false);
            _pending = info;
        }
        catch (Exception ex)
        {
            _lastCheckFailed = true;
            Log.Error("UpdateService.CheckAndDownload: " + ex.Message);
        }
        finally
        {
            Interlocked.Exchange(ref _checking, 0);
            Changed?.Invoke();
        }
    }

    /// <summary>Apply the staged update and relaunch the app. This shuts the process down, so call it on the UI
    /// thread once the user agrees. No-op if nothing is staged.</summary>
    public void ApplyAndRestart()
    {
        // Prefer this session's downloaded update; otherwise apply a package left staged on disk by a prior run
        // (so a download isn't lost when the user relaunched before pressing Restart).
        VelopackAsset? staged = _pending?.TargetFullRelease ?? _mgr.UpdatePendingRestart;
        if (staged is not null)
            _mgr.ApplyUpdatesAndRestart(staged);
    }
}
