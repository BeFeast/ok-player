using OkPlayer.Core;

namespace OkPlayer.Tests;

/// <summary>The Windows projection of the shared update lifecycle (issues #682/#694), pinned against the
/// same behaviour <c>okp_core::update_lifecycle</c>'s suite pins: the states a check/download/apply walks,
/// the strings the surfaces are allowed to show, the invariant that an applied-but-unrestarted update is
/// never "you are on the new version" (#660), and what a truncated version may and may not decide.</summary>
public class UpdateLifecycleTests
{
    private static ReportedVersion Complete(string version) => ReportedVersion.Complete(version);

    [Fact]
    public void InstalledLayoutIsAVelopackInstall_AnythingElseIsADevBuild()
    {
        Assert.Equal(InstallKind.WindowsVelopack, UpdateLifecycle.Detect(new InstallEvidence(true)));
        Assert.Equal(InstallKind.DevBuild, UpdateLifecycle.Detect(new InstallEvidence(false)));
        Assert.Equal(
            UpdateCapability.SelfApply,
            new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0")).Capability());
        Assert.Equal(
            UpdateCapability.Unmanaged,
            new UpdateLifecycle(InstallKind.DevBuild, Complete("0.11.0")).Capability());
    }

    [Fact]
    public void TheWindowsLaneWalksCheckDownloadApplyAndRestartAndSaysSoAtEveryStep()
    {
        var velopack = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.Equal("OK Player has not checked for updates yet.", velopack.Describe().UpdatesMessage);
        Assert.Equal(UpdateAction.CheckNow, velopack.Describe().Action);

        Assert.True(velopack.StartCheck());
        Assert.Equal("Checking for updates…", velopack.Describe().UpdatesMessage);
        Assert.False(velopack.Describe().ActionsEnabled);

        Assert.True(velopack.CheckFound("0.11.0-beta.0.15"));
        UpdatePresentation offered = velopack.Describe();
        Assert.Equal("Version 0.11.0-beta.0.15 is available.", offered.UpdatesMessage);
        Assert.Equal(UpdateAction.DownloadUpdate, offered.Action);
        Assert.False(offered.ActionClosesTheApp); // Velopack downloads in the background

        Assert.True(velopack.StartDownload());
        Assert.Equal("Downloading version 0.11.0-beta.0.15…", velopack.Describe().UpdatesMessage);
        Assert.True(velopack.DownloadFinished());
        UpdatePresentation staged = velopack.Describe();
        Assert.Equal("Version 0.11.0-beta.0.15 is ready to install.", staged.UpdatesMessage);
        Assert.Equal(UpdateAction.ApplyAndRestart, staged.Action);
        Assert.True(staged.ActionClosesTheApp);

        Assert.True(velopack.StartApply());
        Assert.True(velopack.ApplyNeedsRestart());
        UpdatePresentation pending = velopack.Describe();
        Assert.Equal(UpdateStateKind.RestartPending, velopack.State.Kind);
        // #660: the bits are on disk, this process is still the old build, and no surface may say otherwise.
        Assert.Equal(VersionClaim.Superseded, pending.Claim);
        Assert.Equal("0.11.0-beta.0.14", pending.VersionInUse);
        Assert.Equal("0.11.0-beta.0.15", pending.TargetVersion);
        Assert.Equal(
            "Version 0.11.0-beta.0.15 is installed. Restart OK Player to start running it — this session is still on 0.11.0-beta.0.14.",
            pending.UpdatesMessage);
        Assert.Equal(
            "OK Player 0.11.0-beta.0.14 — restart to finish updating to 0.11.0-beta.0.15.",
            pending.AboutMessage);
        Assert.Equal(UpdateAction.RestartToFinish, pending.Action);

        // The relaunch is a new process: the shell hands it the pending target and the version it came up as.
        UpdateLifecycle resumed = UpdateLifecycle.ResumedAfterRestart(
            InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.15"), "0.11.0-beta.0.15");
        UpdatePresentation done = resumed.Describe();
        Assert.Equal(VersionClaim.Current, done.Claim);
        Assert.Equal("OK Player is now running version 0.11.0-beta.0.15.", done.UpdatesMessage);
        Assert.Equal("OK Player 0.11.0-beta.0.15 — up to date.", done.AboutMessage);
    }

    [Fact]
    public void ADevBuildReportsUpdatesDisabledRatherThanASpecialCase()
    {
        var dev = new UpdateLifecycle(InstallKind.DevBuild, Complete("0.11.0"));
        UpdatePresentation presentation = dev.Describe();
        Assert.Equal(VersionClaim.NotApplicable, presentation.Claim);
        Assert.Equal("Updates are disabled for development builds.", presentation.UpdatesMessage);
        Assert.Equal("OK Player 0.11.0 — development build; updates are disabled.", presentation.AboutMessage);
        Assert.Null(presentation.Action);
        Assert.False(dev.StartCheck());
        Assert.Equal(UpdateStateKind.Idle, dev.State.Kind);
    }

    [Fact]
    public void ARestartThatStillRunsTheOldBinaryIsAFailure()
    {
        UpdateLifecycle stayed = UpdateLifecycle.ResumedAfterRestart(
            InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"), "0.11.0-beta.0.15");
        Assert.Equal(UpdateStateKind.Failed, stayed.State.Kind);
        Assert.Equal("0.11.0-beta.0.15", stayed.State.Version);
        Assert.Equal(
            "The update to version 0.11.0-beta.0.15 failed: restart still runs 0.11.0-beta.0.14; the update to 0.11.0-beta.0.15 did not take effect",
            stayed.Describe().UpdatesMessage);
        Assert.Equal(UpdateAction.Retry, stayed.Describe().Action);

        // Coming back on the target, or on something newer still, completes it.
        Assert.Equal(
            UpdateStateKind.Running,
            UpdateLifecycle.ResumedAfterRestart(
                InstallKind.WindowsVelopack, Complete("0.12.0"), "0.11.0-beta.0.15").State.Kind);
    }

    [Fact]
    public void ATruncatedRunningVersionNeitherConfirmsNorDeniesTheRestart()
    {
        // #694: the process came back on the candidate it was told to install, but all it can say about
        // itself is "0.11.0" — which read as a complete version is a stable release, ranking above the
        // pending prerelease and turning a good upgrade into a reported downgrade.
        UpdateLifecycle resumed = UpdateLifecycle.ResumedAfterRestart(
            InstallKind.WindowsVelopack, ReportedVersion.Truncated("0.11.0"), "0.11.0-beta.0.15");

        Assert.Equal(UpdateStateKind.RestartUnverified, resumed.State.Kind);
        UpdatePresentation presentation = resumed.Describe();
        Assert.Equal(VersionClaim.Unknown, presentation.Claim);
        Assert.Equal("0.11.0-beta.0.15", presentation.TargetVersion);
        // Not swallowed either: both surfaces say what could not be confirmed and name the version.
        Assert.Contains("cannot be confirmed", presentation.UpdatesMessage);
        Assert.Contains("0.11.0-beta.0.15", presentation.UpdatesMessage);
        Assert.Contains("could not be confirmed", presentation.AboutMessage);
        // A check is the way out, and it is offered.
        Assert.Equal(UpdateAction.CheckNow, presentation.Action);
        Assert.True(resumed.StartCheck());
        // Refreshing an unconfirmed restart does not turn it into a known one…
        Assert.Equal(VersionClaim.Unknown, resumed.Describe().Claim);
        // …and a check that fails settles nothing, so the restart stays as unconfirmed as it was
        // instead of collapsing into a generic failure that has forgotten the version.
        Assert.True(resumed.CheckFailed("network unreachable"));
        Assert.Equal(UpdateStateKind.RestartUnverified, resumed.State.Kind);
        Assert.Equal("0.11.0-beta.0.15", resumed.State.Version);
        UpdatePresentation afterFailure = resumed.Describe();
        Assert.Contains("cannot be confirmed", afterFailure.UpdatesMessage);
        Assert.Contains("Update check failed: network unreachable", afterFailure.UpdatesMessage);
        Assert.Equal(VersionClaim.Unknown, afterFailure.Claim);
        Assert.Equal(UpdateAction.CheckNow, afterFailure.Action);
    }

    [Fact]
    public void ATruncatedRunningVersionKeepsAStagedPayloadItCannotRank()
    {
        // A staged payload is an observed fact, not an ordering conclusion: with the running version
        // truncated the staleness check cannot run, and silently dropping a downloaded update is the worse
        // half of the guess.
        UpdateLifecycle kept = UpdateLifecycle.ResumedWithStagedUpdate(
            InstallKind.WindowsVelopack, ReportedVersion.Truncated("0.11.0"), Complete("0.11.0-beta.0.15"));
        Assert.Equal(UpdateStateKind.ReadyToApply, kept.State.Kind);
        Assert.Equal(UpdateAction.ApplyAndRestart, kept.Describe().Action);

        // A core the truncation cannot hide still discards a stale record.
        UpdateLifecycle stale = UpdateLifecycle.ResumedWithStagedUpdate(
            InstallKind.WindowsVelopack, ReportedVersion.Truncated("0.12.0"), Complete("0.11.0-beta.0.15"));
        Assert.Equal(UpdateStateKind.Idle, stale.State.Kind);

        // And a complete running version keeps the full check.
        UpdateLifecycle backwards = UpdateLifecycle.ResumedWithStagedUpdate(
            InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.15"), Complete("0.11.0-beta.0.14"));
        Assert.Equal(UpdateStateKind.Idle, backwards.State.Kind);
    }

    [Fact]
    public void ATruncatedVersionOnlyDecidesWhatItsNumericCoreCan()
    {
        var truncated = ReportedVersion.Truncated("0.11.0");
        Assert.Null(VersionOrder.CompareReportedBuildOrder(truncated, Complete("0.11.0-beta.0.15")));
        Assert.Null(VersionOrder.CompareReportedBuildOrder(Complete("0.11.0-beta.0.15"), truncated));
        // Even against the same string: the truncated one may be any beta of it.
        Assert.Null(VersionOrder.CompareReportedBuildOrder(truncated, Complete("0.11.0")));
        // A different core is decided by the core alone, so truncation costs nothing there.
        Assert.True(VersionOrder.CompareReportedBuildOrder(truncated, Complete("0.12.0-beta.1")) < 0);
        Assert.True(VersionOrder.CompareReportedBuildOrder(truncated, Complete("0.10.14")) > 0);
        // Two complete versions keep the full ordering.
        Assert.True(VersionOrder.CompareReportedBuildOrder(Complete("0.11.0"), Complete("0.11.0-beta.0.15")) > 0);
        Assert.True(VersionOrder.CompareReportedBuildOrder(Complete("0.11.0"), Complete("0.11.0")) == 0);
    }

    [Theory]
    // A release outranks the prereleases that led to it.
    [InlineData("1.0.0", "1.0.0-beta.1", 1)]
    // Numeric runs order within one stage…
    [InlineData("0.1.0-linux-alpha.109", "0.1.0-linux-alpha.110", -1)]
    // …and the stage is compared before its counter.
    [InlineData("0.1.0-alpha.109", "0.1.0-beta.1", -1)]
    // A tail that is a prefix of another sorts before it (the candidates cut from a beta).
    [InlineData("0.11.0-beta.2", "0.11.0-beta.2.41", -1)]
    [InlineData("0.10.14", "0.11.0", -1)]
    [InlineData("0.11.0-beta.0.15", "0.11.0-beta.0.15", 0)]
    public void BuildOrderMatchesTheSharedOrderingRules(string left, string right, int expected)
    {
        Assert.Equal(expected, Math.Sign(VersionOrder.CompareBuildOrder(left, right)));
        Assert.Equal(-expected, Math.Sign(VersionOrder.CompareBuildOrder(right, left)));
    }

    [Fact]
    public void AFailedRecheckRestoresTheOfferItWasRefreshingWithTheErrorBeside()
    {
        var velopack = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(velopack.StartCheck());
        Assert.True(velopack.CheckFound("0.11.0-beta.0.15"));
        Assert.True(velopack.StartCheck());
        Assert.True(velopack.CheckFailed("network unreachable"));

        Assert.Equal(UpdateStateKind.Available, velopack.State.Kind);
        Assert.Equal(
            "Version 0.11.0-beta.0.15 is available. Update check failed: network unreachable",
            velopack.Describe().UpdatesMessage);

        // A check that had no offer to protect is the only one that ends in Failed.
        var fresh = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(fresh.StartCheck());
        Assert.True(fresh.CheckFailed("network unreachable"));
        Assert.Equal(UpdateStateKind.Failed, fresh.State.Kind);
        Assert.Equal("Update failed: network unreachable", fresh.Describe().UpdatesMessage);
        Assert.Equal(VersionClaim.Unknown, fresh.Describe().Claim);
        // Nothing was discovered, so there is no offer to retry — a fresh check is the way back.
        Assert.False(fresh.RetryFailedUpdate());
        Assert.True(fresh.StartCheck());
    }

    /// <summary>A check running over a standing offer still describes that offer: the check is a
    /// status on top of what the surface is showing, not a replacement for it. Mirrors
    /// <c>okp_core::update_lifecycle</c>'s rule so the two ports cannot drift (#696).</summary>
    [Fact]
    public void ACheckOverACarriedOfferStillDescribesThatOffer()
    {
        var refreshing = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(refreshing.StartCheck());
        Assert.True(refreshing.CheckFound("0.11.0-beta.0.15"));
        Assert.True(refreshing.StartCheck());
        Assert.Equal(
            "Version 0.11.0-beta.0.15 is available. Checking for updates…",
            refreshing.Describe().UpdatesMessage);

        // A refresh over a failure keeps the error it is refreshing, not just the fact of a check.
        var failed = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(failed.StartCheck());
        Assert.True(failed.CheckFound("0.11.0-beta.0.15"));
        Assert.True(failed.StartDownload());
        Assert.True(failed.DownloadFailed("checksum mismatch"));
        Assert.True(failed.StartCheck());
        Assert.Equal(
            "The update to version 0.11.0-beta.0.15 failed: checksum mismatch Checking for updates…",
            failed.Describe().UpdatesMessage);

        // A check with no offer behind it has only itself to report.
        var first = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(first.StartCheck());
        Assert.Equal("Checking for updates…", first.Describe().UpdatesMessage);

        // About comes from the same projection and must describe the same offer, rather than
        // reading a carried failure as an available update.
        Assert.Equal(
            "OK Player 0.11.0-beta.0.14 — updating to 0.11.0-beta.0.15 failed.",
            failed.Describe().AboutMessage);
    }

    [Fact]
    public void AFailedApplyRetriesTheStagedPayloadInsteadOfDownloadingAgain()
    {
        var velopack = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(velopack.StartCheck());
        Assert.True(velopack.CheckFound("0.11.0-beta.0.15"));
        Assert.True(velopack.StartDownload());
        Assert.True(velopack.DownloadFinished());
        Assert.True(velopack.StartApply());
        Assert.True(velopack.ApplyFailed("the update process could not be started"));

        Assert.Equal(UpdateAction.Retry, velopack.Describe().Action);
        Assert.True(velopack.RetryFailedUpdate());
        Assert.Equal(UpdateStateKind.ReadyToApply, velopack.State.Kind);

        // A download that failed has nothing staged, so its retry goes back through the feed.
        var again = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0-beta.0.14"));
        Assert.True(again.StartCheck());
        Assert.True(again.CheckFound("0.11.0-beta.0.15"));
        Assert.True(again.StartDownload());
        Assert.True(again.DownloadFailed("checksum mismatch"));
        Assert.True(again.RetryFailedUpdate());
        Assert.Equal(UpdateStateKind.Available, again.State.Kind);
    }

    [Fact]
    public void ARefusedTransitionNeverChangesTheState()
    {
        var velopack = new UpdateLifecycle(InstallKind.WindowsVelopack, Complete("0.11.0"));
        Assert.False(velopack.DownloadFinished());
        Assert.False(velopack.ApplyNeedsRestart());
        Assert.False(velopack.RestartedInto(Complete("0.12.0")));
        Assert.Equal(UpdateStateKind.Idle, velopack.State.Kind);

        // A check in flight is not a settled state: nothing may start over it.
        Assert.True(velopack.StartCheck());
        Assert.False(velopack.StartCheck());
        Assert.Equal(UpdateStateKind.Checking, velopack.State.Kind);
    }
}
