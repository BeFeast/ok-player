using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace OkPlayer.Core;

/// <summary>
/// Pure contract for the rolling Windows candidate channel. GitHub Actions and
/// PowerShell only orchestrate tools and asset uploads; SHA decisions, version
/// ordering, Velopack validation, rollback retention, and pruning live here.
/// </summary>
public static class WindowsCandidateRelease
{
    public const int SchemaVersion = 1;
    public const string Channel = "win-candidate";
    public const string PackageId = "com.befeast.okplayer";
    public const string VersionBase = "0.11.0-beta.0";
    public const string FeedFileName = "releases.win-candidate.json";
    public const string ManifestFileName = "candidate.windows.json";

    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        WriteIndented = true,
    };

    public static string Version(long buildNumber)
    {
        if (buildNumber <= 0)
            throw new ArgumentOutOfRangeException(nameof(buildNumber), "Candidate build number must be positive.");
        return $"{VersionBase}.{buildNumber}";
    }

    public static WindowsCandidateBuildDecision DecideBuild(
        string sourceSha,
        WindowsCandidateManifest? published,
        ReadOnlySpan<byte> publishedFeed = default)
    {
        string normalized = NormalizeSha(sourceSha, nameof(sourceSha));
        if (published is not null)
        {
            ValidateManifest(published);
            if (publishedFeed.IsEmpty)
                throw new InvalidDataException("Published candidate manifest has no matching feed.");
            WindowsCandidateArtifact observedFeed = Identity(FeedFileName, publishedFeed.ToArray(), true);
            if (observedFeed.Size != published.Feed.Size
                || !observedFeed.Sha256.Equals(published.Feed.Sha256, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("Published candidate feed does not match its identity manifest.");
            if (string.Equals(normalized, published.SourceSha, StringComparison.OrdinalIgnoreCase))
                return new WindowsCandidateBuildDecision(false, normalized, "unchanged");
        }
        return new WindowsCandidateBuildDecision(true, normalized, "main-advanced");
    }

    public static void VerifyPublishHead(string sourceSha, string currentMainSha)
    {
        string source = NormalizeSha(sourceSha, nameof(sourceSha));
        string current = NormalizeSha(currentMainSha, nameof(currentMainSha));
        if (!string.Equals(source, current, StringComparison.Ordinal))
            throw new InvalidOperationException(
                $"Candidate source {source} is stale; current main is {current}.");
    }

    public static WindowsCandidatePlan Assemble(
        string sourceSha,
        long buildNumber,
        string builder,
        DateTimeOffset timestampUtc,
        ReadOnlySpan<byte> generatedFeedJson,
        IReadOnlyDictionary<string, byte[]> currentArtifacts,
        WindowsCandidateManifest? previousManifest = null,
        ReadOnlySpan<byte> previousFeedJson = default)
    {
        sourceSha = NormalizeSha(sourceSha, nameof(sourceSha));
        if (string.IsNullOrWhiteSpace(builder))
            throw new ArgumentException("Builder identity is required.", nameof(builder));
        if (timestampUtc.Offset != TimeSpan.Zero)
            throw new ArgumentException("Candidate timestamp must be UTC.", nameof(timestampUtc));

        string version = Version(buildNumber);
        VelopackFeed generated = ParseFeed(generatedFeedJson, "generated candidate feed");
        List<VelopackAsset> currentAssets = generated.Assets
            .Select(asset => ValidateCurrentAsset(asset, version, currentArtifacts))
            .ToList();
        if (currentAssets.Count(asset => IsFull(asset)) != 1)
            throw new InvalidDataException("Generated candidate feed must contain exactly one Full package.");

        var artifactIdentities = currentArtifacts
            .OrderBy(pair => pair.Key, StringComparer.Ordinal)
            .Select(pair =>
            {
                WindowsCandidateArtifact identity = Identity(pair.Key, pair.Value, true);
                string? artifactVersion = currentAssets
                    .SingleOrDefault(asset => asset.FileName == pair.Key)?.Version;
                return identity with { Version = artifactVersion };
            })
            .ToList();

        VelopackAsset? previousFull = null;
        if (previousManifest is not null)
        {
            ValidateManifest(previousManifest);
            if (buildNumber <= previousManifest.BuildNumber)
                throw new InvalidOperationException(
                    $"Candidate build {buildNumber} must be newer than published build {previousManifest.BuildNumber}.");
            if (previousFeedJson.IsEmpty)
                throw new InvalidDataException("Previous feed is required when a previous manifest exists.");

            VelopackFeed previousFeed = ParseFeed(previousFeedJson, "previous candidate feed");
            previousFull = previousFeed.Assets.SingleOrDefault(asset =>
                IsFull(asset)
                && asset.PackageId == PackageId
                && asset.Version == previousManifest.Version)
                ?? throw new InvalidDataException(
                    "Previous candidate feed does not contain its manifest-bound Full package.");
            ValidateSafeFileName(previousFull.FileName);

            WindowsCandidateArtifact priorIdentity = previousManifest.Artifacts.SingleOrDefault(asset =>
                asset.Name == previousFull.FileName && asset.Version == previousFull.Version)
                ?? throw new InvalidDataException(
                    "Previous manifest does not identify its rollback Full package.");
            if (!priorIdentity.Sha256.Equals(previousFull.Sha256, StringComparison.OrdinalIgnoreCase)
                || priorIdentity.Size != previousFull.Size)
                throw new InvalidDataException(
                    "Previous feed Full package does not match its published identity manifest.");
            artifactIdentities.Add(priorIdentity with { Current = false });
        }

        var mergedAssets = new List<VelopackAsset>(currentAssets);
        if (previousFull is not null
            && mergedAssets.All(asset => asset.FileName != previousFull.FileName))
            mergedAssets.Add(previousFull);

        byte[] feedBytes = JsonSerializer.SerializeToUtf8Bytes(
            new VelopackFeed(mergedAssets.ToArray()), JsonOptions);
        var feedIdentity = Identity(FeedFileName, feedBytes, true) with { Version = version };
        var manifest = new WindowsCandidateManifest(
            SchemaVersion,
            Channel,
            sourceSha,
            buildNumber,
            version,
            builder.Trim(),
            timestampUtc.ToString("O"),
            feedIdentity,
            artifactIdentities);
        byte[] manifestBytes = JsonSerializer.SerializeToUtf8Bytes(manifest, JsonOptions);

        var uploadNames = currentArtifacts.Keys
            .OrderBy(name => name, StringComparer.Ordinal)
            .Append(ManifestFileName)
            .Append(FeedFileName)
            .ToArray();
        var keepNames = artifactIdentities.Select(asset => asset.Name)
            .Append(ManifestFileName)
            .Append(FeedFileName)
            .ToHashSet(StringComparer.Ordinal);

        return new WindowsCandidatePlan(feedBytes, manifestBytes, manifest, uploadNames, keepNames);
    }

    public static IReadOnlyList<string> PrunePlan(
        WindowsCandidateManifest manifest,
        IEnumerable<string> releaseAssets)
    {
        ValidateManifest(manifest);
        var keep = manifest.Artifacts.Select(asset => asset.Name)
            .Append(ManifestFileName)
            .Append(FeedFileName)
            .ToHashSet(StringComparer.Ordinal);
        return releaseAssets
            .Where(IsRecognizedCandidateAsset)
            .Where(name => !keep.Contains(name))
            .OrderBy(name => name, StringComparer.Ordinal)
            .ToArray();
    }

    public static WindowsCandidateManifest ParseManifest(ReadOnlySpan<byte> json)
        => JsonSerializer.Deserialize<WindowsCandidateManifest>(json, JsonOptions)
           ?? throw new InvalidDataException("Candidate manifest is empty.");

    private static VelopackAsset ValidateCurrentAsset(
        VelopackAsset asset,
        string version,
        IReadOnlyDictionary<string, byte[]> artifacts)
    {
        if (asset.PackageId != PackageId)
            throw new InvalidDataException(
                $"Velopack package id must be {PackageId}, got {asset.PackageId}.");
        if (asset.Version != version)
            throw new InvalidDataException(
                $"Velopack asset {asset.FileName} has version {asset.Version}, expected {version}.");
        ValidateSafeFileName(asset.FileName);
        if (!artifacts.TryGetValue(asset.FileName, out byte[]? bytes))
            throw new InvalidDataException($"Velopack asset {asset.FileName} is missing from the package output.");
        WindowsCandidateArtifact identity = Identity(asset.FileName, bytes, true);
        if (identity.Size != asset.Size
            || !identity.Sha256.Equals(asset.Sha256, StringComparison.OrdinalIgnoreCase))
            throw new InvalidDataException(
                $"Velopack asset {asset.FileName} does not match the feed checksum and size.");
        return asset;
    }

    private static VelopackFeed ParseFeed(ReadOnlySpan<byte> json, string label)
    {
        VelopackFeed? feed;
        try
        {
            feed = JsonSerializer.Deserialize<VelopackFeed>(json, JsonOptions);
        }
        catch (JsonException ex)
        {
            throw new InvalidDataException($"Invalid {label}: {ex.Message}", ex);
        }
        if (feed?.Assets is not { Length: > 0 })
            throw new InvalidDataException($"The {label} contains no assets.");
        return feed;
    }

    private static void ValidateManifest(WindowsCandidateManifest manifest)
    {
        if (manifest.SchemaVersion != SchemaVersion)
            throw new InvalidDataException($"Unsupported Windows candidate schema {manifest.SchemaVersion}.");
        if (manifest.Channel != Channel)
            throw new InvalidDataException($"Candidate manifest channel must be {Channel}.");
        NormalizeSha(manifest.SourceSha, nameof(manifest.SourceSha));
        if (manifest.BuildNumber <= 0 || manifest.Version != Version(manifest.BuildNumber))
            throw new InvalidDataException("Candidate manifest version does not carry its monotonic build number.");
        if (string.IsNullOrWhiteSpace(manifest.Builder))
            throw new InvalidDataException("Candidate manifest builder identity is empty.");
        if (!DateTimeOffset.TryParse(manifest.TimestampUtc, out DateTimeOffset timestamp)
            || timestamp.Offset != TimeSpan.Zero)
            throw new InvalidDataException("Candidate manifest timestamp is not UTC.");
        if (manifest.Feed is null
            || manifest.Feed.Name != FeedFileName
            || manifest.Feed.Version != manifest.Version
            || manifest.Feed.Size <= 0
            || !ValidDigest(manifest.Feed.Sha256))
            throw new InvalidDataException("Candidate manifest feed identity is invalid.");
        if (manifest.Artifacts is null
            || manifest.Artifacts.Count == 0
            || manifest.Artifacts.Any(asset =>
            {
                ValidateSafeFileName(asset.Name);
                return !ValidDigest(asset.Sha256) || asset.Size <= 0;
            })
            || manifest.Artifacts.Select(asset => asset.Name).Distinct(StringComparer.Ordinal).Count()
                != manifest.Artifacts.Count
            || manifest.Artifacts.Count(asset =>
                asset.Current
                && asset.Version == manifest.Version
                && asset.Name.StartsWith(PackageId, StringComparison.Ordinal)
                && asset.Name.Contains(Channel, StringComparison.Ordinal)
                && asset.Name.EndsWith("-full.nupkg", StringComparison.OrdinalIgnoreCase)) != 1)
            throw new InvalidDataException("Candidate manifest artifact identity is invalid.");
    }

    private static WindowsCandidateArtifact Identity(string name, byte[] bytes, bool current)
    {
        ValidateSafeFileName(name);
        if (bytes.Length == 0)
            throw new InvalidDataException($"Candidate artifact {name} is empty.");
        return new WindowsCandidateArtifact(
            name,
            Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant(),
            bytes.LongLength,
            null,
            current);
    }

    private static bool IsFull(VelopackAsset asset)
        => asset.Type.Equals("Full", StringComparison.OrdinalIgnoreCase);

    private static bool IsRecognizedCandidateAsset(string name)
    {
        if (name is FeedFileName or ManifestFileName)
            return true;
        return name.Contains(Channel, StringComparison.Ordinal)
               && name.StartsWith(PackageId, StringComparison.Ordinal)
               && (name.EndsWith(".nupkg", StringComparison.OrdinalIgnoreCase)
                   || name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase)
                   || name.EndsWith(".zip", StringComparison.OrdinalIgnoreCase));
    }

    private static string NormalizeSha(string sha, string parameterName)
    {
        string normalized = sha.Trim().ToLowerInvariant();
        if (normalized.Length != 40 || normalized.Any(c => !Uri.IsHexDigit(c)))
            throw new ArgumentException("Git SHA must be exactly 40 hexadecimal characters.", parameterName);
        return normalized;
    }

    private static void ValidateSafeFileName(string name)
    {
        if (string.IsNullOrWhiteSpace(name)
            || name != Path.GetFileName(name)
            || name.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
            throw new InvalidDataException($"Unsafe candidate artifact name: {name}.");
    }

    private static bool ValidDigest(string digest)
        => digest is { Length: 64 } && digest.All(Uri.IsHexDigit);

    private sealed record VelopackFeed(
        [property: JsonPropertyName("Assets")] VelopackAsset[] Assets);

    private sealed record VelopackAsset(
        [property: JsonPropertyName("PackageId")] string PackageId,
        [property: JsonPropertyName("Version")] string Version,
        [property: JsonPropertyName("Type")] string Type,
        [property: JsonPropertyName("FileName")] string FileName,
        [property: JsonPropertyName("SHA1")] string Sha1,
        [property: JsonPropertyName("SHA256")] string Sha256,
        [property: JsonPropertyName("Size")] long Size);
}

public sealed record WindowsCandidateBuildDecision(bool ShouldBuild, string SourceSha, string Reason);

public sealed record WindowsCandidateArtifact(
    string Name,
    string Sha256,
    long Size,
    string? Version,
    bool Current);

public sealed record WindowsCandidateManifest(
    int SchemaVersion,
    string Channel,
    string SourceSha,
    long BuildNumber,
    string Version,
    string Builder,
    string TimestampUtc,
    WindowsCandidateArtifact Feed,
    IReadOnlyList<WindowsCandidateArtifact> Artifacts);

public sealed record WindowsCandidatePlan(
    byte[] FeedBytes,
    byte[] ManifestBytes,
    WindowsCandidateManifest Manifest,
    IReadOnlyList<string> UploadOrder,
    IReadOnlySet<string> KeepAssets);
