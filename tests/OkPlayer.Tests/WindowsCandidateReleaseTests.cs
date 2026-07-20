using System.Text;
using System.Text.Json;
using OkPlayer.Core;

namespace OkPlayer.Tests;

public sealed class WindowsCandidateReleaseTests
{
    private const string SourceSha = "0123456789abcdef0123456789abcdef01234567";
    private const string OtherSha = "89abcdef0123456789abcdef0123456789abcdef";

    [Fact]
    public void Version_UsesMonotonicOrderingKey()
    {
        Assert.Equal("0.11.0-beta.0.42", WindowsCandidateRelease.Version(42));
        Assert.Throws<ArgumentOutOfRangeException>(() => WindowsCandidateRelease.Version(0));
    }

    [Fact]
    public void DecideBuild_SkipsOnlyTheAlreadyPromotedSha()
    {
        byte[] feed = Encoding.UTF8.GetBytes("published-feed");
        WindowsCandidateManifest published = Manifest(SourceSha, 41) with
        {
            Feed = new WindowsCandidateArtifact(
                WindowsCandidateRelease.FeedFileName, Sha(feed), feed.LongLength,
                WindowsCandidateRelease.Version(41), true),
        };

        Assert.False(WindowsCandidateRelease.DecideBuild(SourceSha.ToUpperInvariant(), published, feed).ShouldBuild);
        Assert.True(WindowsCandidateRelease.DecideBuild(OtherSha, published, feed).ShouldBuild);
        Assert.True(WindowsCandidateRelease.DecideBuild(SourceSha, null).ShouldBuild);
        Assert.Throws<InvalidDataException>(() =>
            WindowsCandidateRelease.DecideBuild(SourceSha, published, Encoding.UTF8.GetBytes("tampered")));
    }

    [Fact]
    public void VerifyPublishHead_RejectsAStaleBuild()
    {
        WindowsCandidateRelease.VerifyPublishHead(SourceSha, SourceSha.ToUpperInvariant());
        Assert.Throws<InvalidOperationException>(
            () => WindowsCandidateRelease.VerifyPublishHead(SourceSha, OtherSha));
    }

    [Fact]
    public void Assemble_ValidatesBytesAndRetainsOnePreviousFullPackage()
    {
        byte[] currentPackage = Encoding.UTF8.GetBytes("current-full-package");
        byte[] setup = Encoding.UTF8.GetBytes("candidate-setup");
        string currentName = FullName(42);
        string previousName = FullName(41);
        WindowsCandidateManifest previous = Manifest(SourceSha, 41, previousName);
        byte[] previousFeed = Feed(41, previousName, previous.Artifacts.Single().Sha256,
            previous.Artifacts.Single().Size);
        var artifacts = new Dictionary<string, byte[]>
        {
            [currentName] = currentPackage,
            [$"{WindowsCandidateRelease.PackageId}-{WindowsCandidateRelease.Channel}-Setup.exe"] = setup,
        };

        WindowsCandidatePlan plan = WindowsCandidateRelease.Assemble(
            OtherSha,
            42,
            "github-actions/windows-latest",
            DateTimeOffset.Parse("2026-07-20T12:00:00Z"),
            Feed(42, currentName, Sha(currentPackage), currentPackage.LongLength),
            artifacts,
            previous,
            previousFeed);

        using JsonDocument feed = JsonDocument.Parse(plan.FeedBytes);
        JsonElement.ArrayEnumerator assets = feed.RootElement.GetProperty("Assets").EnumerateArray();
        string[] fullNames = assets
            .Where(asset => asset.GetProperty("Type").GetString() == "Full")
            .Select(asset => asset.GetProperty("FileName").GetString()!)
            .ToArray();
        Assert.Equal(new[] { currentName, previousName }, fullNames);
        Assert.Equal(42, plan.Manifest.BuildNumber);
        Assert.Equal(3, plan.Manifest.Artifacts.Count);
        Assert.Contains(plan.Manifest.Artifacts, artifact => artifact.Name == previousName && !artifact.Current);
        Assert.Equal(WindowsCandidateRelease.FeedFileName, plan.UploadOrder[^1]);
        Assert.Equal(WindowsCandidateRelease.ManifestFileName, plan.UploadOrder[^2]);
    }

    [Fact]
    public void Assemble_RejectsWrongPackageIdOrTamperedBytes()
    {
        byte[] package = Encoding.UTF8.GetBytes("package");
        string name = FullName(2);
        var artifacts = new Dictionary<string, byte[]> { [name] = package };
        byte[] wrongId = Feed(2, name, Sha(package), package.LongLength, "OkPlayer");
        byte[] wrongHash = Feed(2, name, new string('0', 64), package.LongLength);

        Assert.Throws<InvalidDataException>(() => WindowsCandidateRelease.Assemble(
            SourceSha, 2, "builder", DateTimeOffset.Parse("2026-07-20T12:00:00Z"), wrongId, artifacts));
        Assert.Throws<InvalidDataException>(() => WindowsCandidateRelease.Assemble(
            SourceSha, 2, "builder", DateTimeOffset.Parse("2026-07-20T12:00:00Z"), wrongHash, artifacts));
    }

    [Fact]
    public void PrunePlan_RemovesOnlyRecognizedAssetsOutsideCurrentAndPrevious()
    {
        WindowsCandidateManifest manifest = Manifest(SourceSha, 42, FullName(42), FullName(41));
        string stale = FullName(40);
        string unknown = "operator-note.txt";

        IReadOnlyList<string> prune = WindowsCandidateRelease.PrunePlan(manifest,
            new[] { FullName(42), FullName(41), stale, unknown, WindowsCandidateRelease.FeedFileName });

        Assert.Equal(new[] { stale }, prune);
    }

    private static WindowsCandidateManifest Manifest(string sha, long build, params string[] names)
    {
        if (names.Length == 0)
            names = [FullName(build)];
        var artifacts = names.Select((name, index) => new WindowsCandidateArtifact(
            name,
            new string((char)('a' + index), 64),
            100 + index,
            WindowsCandidateRelease.Version(build - index),
            index == 0)).ToArray();
        return new WindowsCandidateManifest(
            WindowsCandidateRelease.SchemaVersion,
            WindowsCandidateRelease.Channel,
            sha.ToLowerInvariant(),
            build,
            WindowsCandidateRelease.Version(build),
            "github-actions/windows-latest",
            "2026-07-20T12:00:00.0000000+00:00",
            new WindowsCandidateArtifact(WindowsCandidateRelease.FeedFileName, new string('f', 64), 123,
                WindowsCandidateRelease.Version(build), true),
            artifacts);
    }

    private static byte[] Feed(long build, string name, string sha256, long size,
        string packageId = WindowsCandidateRelease.PackageId)
        => JsonSerializer.SerializeToUtf8Bytes(new
        {
            Assets = new[]
            {
                new
                {
                    PackageId = packageId,
                    Version = WindowsCandidateRelease.Version(build),
                    Type = "Full",
                    FileName = name,
                    SHA1 = new string('1', 40),
                    SHA256 = sha256,
                    Size = size,
                },
            },
        });

    private static string FullName(long build)
        => $"{WindowsCandidateRelease.PackageId}-{WindowsCandidateRelease.Version(build)}-{WindowsCandidateRelease.Channel}-full.nupkg";

    private static string Sha(byte[] bytes)
        => Convert.ToHexString(System.Security.Cryptography.SHA256.HashData(bytes)).ToLowerInvariant();
}
