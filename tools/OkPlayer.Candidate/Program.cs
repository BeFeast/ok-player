using System.Text.Json;
using OkPlayer.Core;

try
{
    return args.FirstOrDefault() switch
    {
        "version" => Version(args[1..]),
        "decide" => Decide(args[1..]),
        "verify-head" => VerifyHead(args[1..]),
        "assemble" => Assemble(args[1..]),
        "prune" => Prune(args[1..]),
        _ => Usage(),
    };
}
catch (Exception ex)
{
    Console.Error.WriteLine(ex.Message);
    return 1;
}

static int Version(string[] args)
{
    Console.WriteLine(WindowsCandidateRelease.Version(long.Parse(Required(args, "--build"))));
    return 0;
}

static int Decide(string[] args)
{
    string sha = Required(args, "--source-sha");
    string? manifestPath = Optional(args, "--manifest");
    string? feedPath = Optional(args, "--feed");
    if ((manifestPath is null) != (feedPath is null))
        throw new ArgumentException("Published manifest and feed must be provided together.");
    WindowsCandidateManifest? manifest = manifestPath is null || !File.Exists(manifestPath)
        ? null
        : WindowsCandidateRelease.ParseManifest(File.ReadAllBytes(manifestPath));
    byte[] feed = feedPath is not null && File.Exists(feedPath) ? File.ReadAllBytes(feedPath) : [];
    WindowsCandidateBuildDecision decision = WindowsCandidateRelease.DecideBuild(sha, manifest, feed);
    Console.WriteLine(JsonSerializer.Serialize(decision, JsonOptions()));
    return 0;
}

static int VerifyHead(string[] args)
{
    WindowsCandidateRelease.VerifyPublishHead(
        Required(args, "--source-sha"),
        Required(args, "--current-main-sha"));
    return 0;
}

static int Assemble(string[] args)
{
    string sourceSha = Required(args, "--source-sha");
    long build = long.Parse(Required(args, "--build"));
    string builder = Required(args, "--builder");
    DateTimeOffset timestamp = DateTimeOffset.Parse(Required(args, "--timestamp-utc"));
    string releases = Path.GetFullPath(Required(args, "--releases"));
    string generatedFeed = Path.Combine(releases, WindowsCandidateRelease.FeedFileName);
    string? previousManifestPath = Optional(args, "--previous-manifest");
    string? previousFeedPath = Optional(args, "--previous-feed");
    string outputFeed = Path.GetFullPath(Required(args, "--output-feed"));
    string outputManifest = Path.GetFullPath(Required(args, "--output-manifest"));

    var artifacts = Directory.EnumerateFiles(releases)
        .Where(path => Path.GetFileName(path) != WindowsCandidateRelease.FeedFileName)
        .ToDictionary(path => Path.GetFileName(path)!, File.ReadAllBytes, StringComparer.Ordinal);
    WindowsCandidateManifest? previousManifest = previousManifestPath is not null && File.Exists(previousManifestPath)
        ? WindowsCandidateRelease.ParseManifest(File.ReadAllBytes(previousManifestPath))
        : null;
    byte[] previousFeed = previousFeedPath is not null && File.Exists(previousFeedPath)
        ? File.ReadAllBytes(previousFeedPath)
        : [];

    WindowsCandidatePlan plan = WindowsCandidateRelease.Assemble(
        sourceSha,
        build,
        builder,
        timestamp,
        File.ReadAllBytes(generatedFeed),
        artifacts,
        previousManifest,
        previousFeed);
    Directory.CreateDirectory(Path.GetDirectoryName(outputFeed)!);
    Directory.CreateDirectory(Path.GetDirectoryName(outputManifest)!);
    File.WriteAllBytes(outputFeed, plan.FeedBytes);
    File.WriteAllBytes(outputManifest, plan.ManifestBytes);
    Console.WriteLine(JsonSerializer.Serialize(new
    {
        version = plan.Manifest.Version,
        build_number = plan.Manifest.BuildNumber,
        source_sha = plan.Manifest.SourceSha,
        upload_order = plan.UploadOrder,
    }, JsonOptions()));
    return 0;
}

static int Prune(string[] args)
{
    WindowsCandidateManifest manifest = WindowsCandidateRelease.ParseManifest(
        File.ReadAllBytes(Required(args, "--manifest")));
    string[] assets = JsonSerializer.Deserialize<string[]>(
        File.ReadAllBytes(Required(args, "--assets"))) ?? [];
    foreach (string asset in WindowsCandidateRelease.PrunePlan(manifest, assets))
        Console.WriteLine(asset);
    return 0;
}

static string Required(string[] args, string name)
    => Optional(args, name) ?? throw new ArgumentException($"Missing required argument {name}.");

static string? Optional(string[] args, string name)
{
    int index = Array.IndexOf(args, name);
    if (index < 0)
        return null;
    if (index + 1 >= args.Length || args[index + 1].StartsWith("--", StringComparison.Ordinal))
        throw new ArgumentException($"Argument {name} requires a value.");
    return args[index + 1];
}

static JsonSerializerOptions JsonOptions() => new()
{
    PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    WriteIndented = true,
};

static int Usage()
{
    Console.Error.WriteLine("Usage: okp-windows-candidate version|decide|verify-head|assemble|prune ...");
    return 2;
}
