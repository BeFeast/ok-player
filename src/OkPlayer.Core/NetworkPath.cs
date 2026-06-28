using System;
using System.IO;

namespace OkPlayer.Core;

/// <summary>Classifies a path as a network location — a UNC path (<c>\\server\share\…</c>) or a path on a
/// <b>mapped network drive</b> (an SMB/NFS mount surfaced as e.g. <c>Z:\</c>). Used to <b>bypass synchronous
/// filesystem probes</b> (<see cref="File.Exists"/> and friends) for network paths on the UI thread: an SMB
/// share that is slow, offline, or auth-gated makes a synchronous stat block the calling thread for the full
/// SMB session timeout (~60s, since <see cref="File.Exists"/> swallows the timeout and only then returns
/// <c>false</c>) — fatal on the dispatcher, where it freezes the whole window. For such paths we skip the
/// stat and hand the path straight to libmpv, which opens it off its own threads and reports failure instead
/// of freezing the app. A local-and-missing file (e.g. an unplugged USB drive reporting
/// <see cref="DriveType.NoRootDirectory"/>) is deliberately NOT treated as network — it falls through to the
/// normal existence check.</summary>
public static class NetworkPath
{
    /// <summary>True for a UNC path or a path on a mapped network drive (whose <see cref="DriveType.Network"/>
    /// stays reported even while the share is disconnected).</summary>
    public static bool IsNetwork(string path) => IsNetwork(path, ProbeRootDriveType);

    /// <summary>Testable core: the root drive-type probe is injected so classification can be unit-tested
    /// without depending on the volumes actually mounted on the test machine (the probe returns <c>null</c>
    /// when the root can't be classified).</summary>
    internal static bool IsNetwork(string path, Func<string, DriveType?> rootDriveType)
    {
        if (string.IsNullOrEmpty(path))
            return false;
        // Peel the extended-length / device prefix (\\?\ or \\.\) first, so an extended-length UNC path
        // (\\?\UNC\server\share — network) is told apart from an extended-length LOCAL path (\\?\C:\dir\file —
        // a drive, NOT network). A bare "\\" check alone misclassifies \\?\C:\… as a share.
        string p = path;
        if (p.StartsWith(@"\\?\", StringComparison.Ordinal) || p.StartsWith(@"\\.\", StringComparison.Ordinal))
        {
            p = p[4..];
            if (p.StartsWith(@"UNC\", StringComparison.OrdinalIgnoreCase))
                return true; // \\?\UNC\server\share — a network share
            // otherwise \\?\C:\… or \\?\Volume{…}\… — a local volume; classify the remainder as a normal path
        }
        else if (p.StartsWith(@"\\", StringComparison.Ordinal))
        {
            return true; // plain UNC: \\server\share
        }
        if (!Path.IsPathRooted(p))
            return false;
        string? root = Path.GetPathRoot(p);
        if (string.IsNullOrEmpty(root))
            return false;
        return rootDriveType(root) == DriveType.Network; // only a mapped network drive bypasses File.Exists
    }

    private static DriveType? ProbeRootDriveType(string root)
    {
        try { return new DriveInfo(root).DriveType; }
        catch { return null; } // unclassifiable root -> treat as local; File.Exists is the decider
    }
}
