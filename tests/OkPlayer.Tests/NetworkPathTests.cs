using OkPlayer.Core;

namespace OkPlayer.Tests;

/// <summary>Direct coverage for <see cref="NetworkPath"/> — the shared classifier that lets the launch /
/// reveal / paste / sidecar paths BYPASS <see cref="File.Exists"/> for network locations, so a dead SMB mount
/// can't freeze the UI thread on a synchronous stat (~60s SMB timeout). (HistoryService also forwards here.)</summary>
public class NetworkPathTests
{
    [Theory]
    [InlineData(@"\\nas\media\movie.mkv", true)]        // plain UNC share
    [InlineData(@"\\?\UNC\nas\media\movie.mkv", true)]  // extended-length UNC
    [InlineData(@"\\?\C:\media\movie.mkv", false)]      // extended-length LOCAL path — a drive, not a share
    [InlineData(@"C:\media\movie.mkv", false)]          // local fixed drive
    [InlineData(@"movie.mkv", false)]                   // relative — not rooted
    [InlineData("", false)]                             // empty
    public void IsNetwork_ClassifiesUncAndLocalPaths(string path, bool expected)
        => Assert.Equal(expected, NetworkPath.IsNetwork(path));

    // Path rooted on whatever OS the test runs on, so the drive-type branch is actually reached (the
    // engine-agnostic job runs on Linux, where "Z:\" isn't rooted). The probe is injected — no real volume.
    private static string RootedMediaPath()
        => Path.Combine(Path.GetPathRoot(Path.GetFullPath("."))!, "media", "movie.mkv");

    [Theory]
    [InlineData(DriveType.Network, true)]           // mapped network drive — bypasses File.Exists
    [InlineData(DriveType.Fixed, false)]            // local fixed disk
    [InlineData(DriveType.NoRootDirectory, false)]  // unplugged local drive — must NOT be treated as network
    public void IsNetwork_OnlyMappedNetworkDriveBypasses(DriveType type, bool expected)
        => Assert.Equal(expected, NetworkPath.IsNetwork(RootedMediaPath(), _ => type));

    [Fact] // probe couldn't classify the root -> treat as local
    public void IsNetwork_UnclassifiableRoot_IsLocal()
        => Assert.False(NetworkPath.IsNetwork(RootedMediaPath(), _ => null));
}
