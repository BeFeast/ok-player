//! Classifies a path as a network location — port of `src/OkPlayer.Core/NetworkPath.cs`; the
//! C# suite in `tests/OkPlayer.Tests/NetworkPathTests.cs` is the executable spec. A network
//! location is a UNC path (`\\server\share\…`) or a path on a **mapped network drive** (an
//! SMB/NFS mount surfaced as e.g. `Z:\`). Used to **bypass synchronous filesystem probes**
//! (stat calls and friends) for network paths on the UI thread: an SMB share that is slow,
//! offline, or auth-gated makes a synchronous stat block the calling thread for the full SMB
//! session timeout (~60s) — fatal on the dispatcher, where it freezes the whole window. For
//! such paths the shell skips the stat and hands the path straight to libmpv, which opens it
//! off its own threads and reports failure instead of freezing the app. A local-and-missing
//! file (e.g. an unplugged USB drive reporting [`DriveType::NoRootDirectory`]) is
//! deliberately NOT treated as network — it falls through to the normal existence check.

/// How a volume root is classified — the port of `System.IO.DriveType`, which the injected
/// probe reports. Only [`DriveType::Network`] makes a rooted path a network location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveType {
    Unknown,
    NoRootDirectory,
    Removable,
    Fixed,
    Network,
    CdRom,
    Ram,
}

/// True for a UNC path or a path on a mapped network drive (whose [`DriveType::Network`]
/// stays reported even while the share is disconnected). The root drive-type probe is
/// injected — the platform shell supplies it (Windows: `DriveInfo`; a probe can't be pure) —
/// so classification is unit-testable without depending on the volumes actually mounted on
/// the test machine. The probe returns `None` when the root can't be classified (treat as
/// local; the normal existence check is the decider).
pub fn is_network(path: &str, root_drive_type: impl Fn(&str) -> Option<DriveType>) -> bool {
    if path.is_empty() {
        return false;
    }
    // Peel the extended-length / device prefix (\\?\ or \\.\) first, so an extended-length
    // UNC path (\\?\UNC\server\share — network) is told apart from an extended-length LOCAL
    // path (\\?\C:\dir\file — a drive, NOT network). A bare "\\" check alone misclassifies
    // \\?\C:\… as a share.
    let mut p = path;
    if let Some(rest) = p.strip_prefix(r"\\?\").or_else(|| p.strip_prefix(r"\\.\")) {
        if rest
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(r"UNC\"))
        {
            return true; // \\?\UNC\server\share — a network share
        }
        // Otherwise \\?\C:\… or \\?\Volume{…}\… — a local volume; classify the remainder as
        // a normal path.
        p = rest;
    } else if p.starts_with(r"\\") {
        return true; // plain UNC: \\server\share
    }
    let Some(root) = path_root(p) else {
        return false; // not rooted (or no usable root)
    };
    // Only a mapped network drive bypasses the existence check.
    root_drive_type(root) == Some(DriveType::Network)
}

/// The volume root of a rooted path, or `None` for a relative path. C# defers this to
/// `System.IO.Path` (whose rules change per OS); the port recognizes the union of both
/// platforms' rooted shapes — an ASCII drive-letter root (`C:`, `C:\`, `C:/`) and a
/// separator root (`\` or `/`) — so classification is deterministic everywhere and the
/// injected probe alone decides network-ness.
fn path_root(p: &str) -> Option<&str> {
    let bytes = p.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let with_separator = matches!(bytes.get(2), Some(b'\\' | b'/'));
        return Some(if with_separator { &p[..3] } else { &p[..2] });
    }
    if matches!(bytes.first(), Some(b'\\' | b'/')) {
        return Some(&p[..1]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The C# theory runs against the real `DriveInfo` probe on a machine with no network
    /// drives mounted; an unclassifiable-root probe reproduces that environment.
    #[test]
    fn is_network_classifies_unc_and_local_paths() {
        let cases = [
            (r"\\nas\media\movie.mkv", true),       // plain UNC share
            (r"\\?\UNC\nas\media\movie.mkv", true), // extended-length UNC
            (r"\\?\C:\media\movie.mkv", false), // extended-length LOCAL path — a drive, not a share
            (r"C:\media\movie.mkv", false),     // local fixed drive
            ("movie.mkv", false),               // relative — not rooted
            ("", false),                        // empty
        ];
        for (path, expected) in cases {
            assert_eq!(is_network(path, |_| None), expected, "{path}");
        }
    }

    /// The rooted shape the C# suite exercises on its engine-agnostic (Linux) runs.
    const ROOTED_MEDIA_PATH: &str = "/media/movie.mkv";

    #[test]
    fn is_network_only_mapped_network_drive_bypasses() {
        let cases = [
            (DriveType::Network, true), // mapped network drive — bypasses the existence check
            (DriveType::Fixed, false),  // local fixed disk
            (DriveType::NoRootDirectory, false), // unplugged local drive — must NOT be treated as network
        ];
        for (drive_type, expected) in cases {
            assert_eq!(
                is_network(ROOTED_MEDIA_PATH, |_| Some(drive_type)),
                expected,
                "{drive_type:?}"
            );
        }
    }

    // Probe couldn't classify the root -> treat as local.
    #[test]
    fn is_network_unclassifiable_root_is_local() {
        assert!(!is_network(ROOTED_MEDIA_PATH, |_| None));
    }

    #[test]
    fn is_network_probes_the_extracted_root() {
        let expect_root = |expected: &'static str| {
            move |root: &str| {
                assert_eq!(root, expected);
                Some(DriveType::Network)
            }
        };
        // A mapped drive letter — the Windows shape the C# suite can't reach on Linux.
        assert!(is_network(r"Z:\media\movie.mkv", expect_root(r"Z:\")));
        // An extended-length local path is peeled, then classified by its drive root.
        assert!(is_network(r"\\?\Z:\media\movie.mkv", expect_root(r"Z:\")));
        // A separator-rooted path probes its one-character root.
        assert!(is_network(ROOTED_MEDIA_PATH, expect_root("/")));
        // A drive-relative path roots at the bare drive.
        assert!(is_network("Z:media", expect_root("Z:")));
    }
}
