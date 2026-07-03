using OkPlayer.Core;
using Velopack;
using Velopack.Locators;
using Velopack.Logging;
using Velopack.Sources;

namespace OkPlayer.Tests;

/// <summary>Pins the exact Velopack (1.2.0) contract the static-feed updater relies on (issue #131).
/// <see cref="UpdateFeed.WinBaseUrl"/> replaced GithubSource discovery, whose 10-entry release-listing
/// window any burst of feed-less releases could bury (#130). The load-bearing behaviors, each of which
/// would silently strand the installed fleet if a Velopack bump changed it:
/// (1) SimpleWebSource asks for exactly {WinBaseUrl}releases.win.json;
/// (2) an asset whose FileName is an absolute URL downloads from that URL as-is — this is what lets the
///     Pages-hosted manifest point packages back at GitHub release assets (publish-win-feed.yml rewrites
///     FileName to absolute URLs, so packages never move to Pages with its 100 MB file cap);
/// (3) a failed feed fetch THROWS — it is never conflated with an empty feed, which is what keeps the
///     About dialog's "couldn't check" distinct from a confirmed "up to date";
/// (4) the manifest JSON the workflow emits round-trips through Velopack's parser;
/// (5) UpdateManager constructed exactly as UpdateService constructs it resolves the win channel and
///     discovers a newer version through the static URL — no CI job compiles the WinUI app, so this
///     doubles as the compile pin for that API shape.</summary>
public class UpdateFeedTests
{
    [Fact] // Velopack resolves the index by appending to the base, so a missing slash would 404 the fleet.
    public void WinBaseUrl_IsHttpsAndSlashTerminated()
    {
        Assert.StartsWith("https://", UpdateFeed.WinBaseUrl);
        Assert.EndsWith("/", UpdateFeed.WinBaseUrl);
        Assert.True(Uri.TryCreate(UpdateFeed.WinBaseUrl, UriKind.Absolute, out _));
    }

    [Fact]
    public async Task GetReleaseFeed_RequestsTheStaticWinManifest()
    {
        var downloader = new RecordingDownloader { StringResult = """{"Assets":[]}""" };
        var source = new SimpleWebSource(UpdateFeed.WinBaseUrl, downloader);

        await source.GetReleaseFeed(NullVelopackLogger.Instance, "OkPlayer", UpdateFeed.WinChannel);

        var requested = new Uri(Assert.Single(downloader.StringUrls));
        // Query params (os/arch/local version telemetry) are allowed to vary; the location must not.
        Assert.Equal(UpdateFeed.WinBaseUrl + "releases.win.json",
            requested.GetLeftPart(UriPartial.Path));
    }

    [Fact] // The rewritten manifest carries absolute URLs; Velopack must fetch them verbatim, not {base}+{name}.
    public async Task DownloadReleaseEntry_UrlValuedFileName_DownloadsFromThatUrl()
    {
        const string assetUrl =
            "https://github.com/BeFeast/ok-player/releases/download/v0.10.14/OkPlayer-0.10.14-full.nupkg";
        var downloader = new RecordingDownloader();
        var source = new SimpleWebSource(UpdateFeed.WinBaseUrl, downloader);
        var asset = new VelopackAsset { FileName = assetUrl };

        await source.DownloadReleaseEntry(NullVelopackLogger.Instance, asset, "local.nupkg", _ => { }, default);

        Assert.Equal(assetUrl, Assert.Single(downloader.FileUrls));
    }

    [Fact] // A bare FileName resolves against the feed base — the layout a co-located static host would use.
    public async Task DownloadReleaseEntry_BareFileName_ResolvesAgainstTheFeedBase()
    {
        var downloader = new RecordingDownloader();
        var source = new SimpleWebSource(UpdateFeed.WinBaseUrl, downloader);
        var asset = new VelopackAsset { FileName = "OkPlayer-0.10.14-full.nupkg" };

        await source.DownloadReleaseEntry(NullVelopackLogger.Instance, asset, "local.nupkg", _ => { }, default);

        Assert.Equal(UpdateFeed.WinBaseUrl + "OkPlayer-0.10.14-full.nupkg",
            Assert.Single(downloader.FileUrls));
    }

    [Fact] // Fetch failure must surface as a throw (-> LastCheckFailed), never parse as "no updates".
    public async Task GetReleaseFeed_FetchFailure_Throws()
    {
        var downloader = new RecordingDownloader { StringError = new HttpRequestException("404") };
        var source = new SimpleWebSource(UpdateFeed.WinBaseUrl, downloader);

        await Assert.ThrowsAsync<HttpRequestException>(
            () => source.GetReleaseFeed(NullVelopackLogger.Instance, "OkPlayer", UpdateFeed.WinChannel));
    }

    [Fact] // The empty manifest is the legitimate "nothing published yet" state — parses, zero assets, no throw.
    public void EmptyManifest_ParsesToZeroAssets()
        => Assert.Empty(VelopackAssetFeed.FromJson("""{"Assets":[]}""").Assets);

    [Fact] // Round-trip the manifest shape publish-win-feed.yml emits: vpk's fields + URL-rewritten FileName.
    public void RewrittenManifest_RoundTripsThroughVelopackParser()
    {
        const string json = """
            {
              "Assets": [
                {
                  "PackageId": "OkPlayer",
                  "Version": "0.10.14",
                  "Type": "Full",
                  "FileName": "https://github.com/BeFeast/ok-player/releases/download/v0.10.14/OkPlayer-0.10.14-full.nupkg",
                  "SHA1": "0000000000000000000000000000000000000000",
                  "SHA256": "0000000000000000000000000000000000000000000000000000000000000000",
                  "Size": 123456789
                },
                {
                  "PackageId": "OkPlayer",
                  "Version": "0.10.14",
                  "Type": "Delta",
                  "FileName": "https://github.com/BeFeast/ok-player/releases/download/v0.10.14/OkPlayer-0.10.14-delta.nupkg",
                  "SHA1": "0000000000000000000000000000000000000000",
                  "SHA256": "0000000000000000000000000000000000000000000000000000000000000000",
                  "Size": 1234
                }
              ]
            }
            """;

        var feed = VelopackAssetFeed.FromJson(json);

        Assert.Equal(2, feed.Assets.Length);
        var full = Assert.Single(feed.Assets, a => a.Type == VelopackAssetType.Full);
        Assert.Equal("0.10.14", full.Version.ToString());
        Assert.StartsWith("https://github.com/BeFeast/ok-player/releases/download/", full.FileName);
        Assert.Contains(feed.Assets, a => a.Type == VelopackAssetType.Delta);
    }

    [Fact] // The UpdateService construction, verbatim (source + options), driven through a real check.
    public async Task UpdateManager_BuiltLikeUpdateService_FindsNewerVersionViaTheWinManifest()
    {
        var downloader = new RecordingDownloader
        {
            StringResult = """
                {
                  "Assets": [
                    {
                      "PackageId": "OkPlayer",
                      "Version": "99.0.0",
                      "Type": "Full",
                      "FileName": "https://github.com/BeFeast/ok-player/releases/download/v99.0.0/OkPlayer-99.0.0-full.nupkg",
                      "SHA1": "0000000000000000000000000000000000000000",
                      "SHA256": "0000000000000000000000000000000000000000000000000000000000000000",
                      "Size": 1
                    }
                  ]
                }
                """
        };
        var packagesDir = Directory.CreateTempSubdirectory("okp-updatefeed-test");
        try
        {
            var source = new SimpleWebSource(UpdateFeed.WinBaseUrl, downloader);
            var mgr = new UpdateManager(source,
                new UpdateOptions { ExplicitChannel = UpdateFeed.WinChannel },
                new TestVelopackLocator("OkPlayer", "0.10.14", packagesDir.FullName));

            var info = await mgr.CheckForUpdatesAsync();

            // ExplicitChannel flowed through: the manager asked the static URL for the WIN manifest
            // (on this Linux test host the platform default would have been "linux").
            var requested = new Uri(Assert.Single(downloader.StringUrls));
            Assert.Equal(UpdateFeed.WinBaseUrl + "releases.win.json", requested.GetLeftPart(UriPartial.Path));
            Assert.NotNull(info);
            Assert.Equal("99.0.0", info!.TargetFullRelease.Version.ToString());
        }
        finally
        {
            packagesDir.Delete(recursive: true);
        }
    }

    /// <summary>Captures every URL Velopack asks for; canned string result or error for the feed fetch.</summary>
    private sealed class RecordingDownloader : IFileDownloader
    {
        public List<string> StringUrls { get; } = [];
        public List<string> FileUrls { get; } = [];
        public string StringResult { get; set; } = """{"Assets":[]}""";
        public Exception? StringError { get; set; }

        public Task<string> DownloadString(string url, IDictionary<string, string>? headers = null, double timeout = 30)
        {
            StringUrls.Add(url);
            return StringError is null ? Task.FromResult(StringResult) : Task.FromException<string>(StringError);
        }

        public Task DownloadFile(string url, string targetFile, Action<int> progress,
            IDictionary<string, string>? headers = null, double timeout = 30, CancellationToken cancelToken = default)
        {
            FileUrls.Add(url);
            return Task.CompletedTask;
        }

        public Task<byte[]> DownloadBytes(string url, IDictionary<string, string>? headers = null, double timeout = 30)
        {
            StringUrls.Add(url);
            return Task.FromResult(Array.Empty<byte>());
        }
    }

}
