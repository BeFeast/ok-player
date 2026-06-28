using System;
using System.Threading;
using System.Threading.Tasks;
using Velopack;
using Velopack.Sources;

namespace OkPlayer.App.Services;

/// <summary>Wraps Velopack's <see cref="UpdateManager"/>: a background "is there a newer GitHub release?" check,
/// a download, and an apply-on-restart. Ships dark until the app is actually distributed via the Velopack
/// installer — until then <see cref="IsSupported"/> is false (a dev or portable build reports
/// <c>IsInstalled == false</c>) and every operation is a safe no-op. All network/disk work runs off the UI
/// thread; <see cref="ApplyAndRestart"/> tears the process down and must be called from the UI thread.
/// State changes raise <see cref="Changed"/>, which may fire OFF the UI thread — handlers must marshal to their
/// own dispatcher before touching UI.</summary>
public sealed class UpdateService
{
    private readonly UpdateManager _mgr;
    private UpdateInfo? _pending;   // a downloaded, ready-to-apply update (null until found + downloaded)
    private int _checking;          // 0/1 guard so overlapping background checks don't stack

    public UpdateService()
    {
        // The GitHub Release is the update feed. prerelease:true so the pre-1.0 beta (GitHub pre-release) builds
        // are offered; accessToken null because the repo is public (unauthenticated GitHub API is plenty for a
        // once-per-launch check). Constructing this is safe on any build; only the operations gate on IsInstalled.
        var source = new GithubSource("https://github.com/BeFeast/ok-player", accessToken: null, prerelease: true);
        _mgr = new UpdateManager(source);
    }

    /// <summary>True only for a real Velopack-installed build; false in dev / portable, where updates no-op.</summary>
    public bool IsSupported => _mgr.IsInstalled;

    /// <summary>A check is in flight right now (so the UI can show a spinner / disable the button).</summary>
    public bool IsChecking => Volatile.Read(ref _checking) == 1;

    /// <summary>A newer release is downloaded and a restart will apply it.</summary>
    public bool UpdateReady => _pending is not null;

    /// <summary>Version string of the downloaded, ready-to-apply update, or null when none is staged.</summary>
    public string? PendingVersion => _pending?.TargetFullRelease?.Version?.ToString();

    /// <summary>Raised when the update state changes (check started/finished, update downloaded). May fire OFF
    /// the UI thread — marshal before touching XAML.</summary>
    public event Action? Changed;

    /// <summary>Ask GitHub for a newer release and, if found, download it in the background. Safe to call
    /// fire-and-forget on launch. No-ops on dev/portable builds, when a check is already running, or when an
    /// update is already staged. Never throws — a failed check (offline, rate-limited, torn-down feed) is logged
    /// and swallowed so it can never break the app.</summary>
    public async Task CheckAndDownloadAsync()
    {
        if (!IsSupported || _pending is not null)
            return;
        if (Interlocked.Exchange(ref _checking, 1) == 1)
            return; // another check already running
        Changed?.Invoke();
        try
        {
            UpdateInfo? info = await _mgr.CheckForUpdatesAsync().ConfigureAwait(false);
            if (info is null)
                return; // already current
            await _mgr.DownloadUpdatesAsync(info).ConfigureAwait(false);
            _pending = info;
        }
        catch (Exception ex) { Log.Error("UpdateService.CheckAndDownload: " + ex.Message); }
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
        if (_pending is null)
            return;
        _mgr.ApplyUpdatesAndRestart(_pending);
    }
}
