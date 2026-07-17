//! Sidecar lyrics discovery — the portable step that finds a `.lrc` sheet next to a local audio
//! file so the shell can render it with the [`crate::lrc`] parser. Ported from the path rule in the
//! Windows `LyricsService.TryReadSidecar` (`src/OkPlayer.App/Services/LyricsService.cs`); the
//! divergences are recorded in `docs/core-compatibility.md`.
//!
//! Only the sidecar seam lives here. The metadata-keyed cache and the LRCLIB network fetch that the
//! Windows service layers on top are deliberately out of this slice (issue #189): they need the
//! private-session / history policy wired through first. The seam is documented, not stubbed — a
//! later port adds `read_cached`/`fetch_lrclib` beside these functions.

use std::fs;
use std::path::{Path, PathBuf};

/// The canonical sidecar `.lrc` path for a local media file: same directory, same file stem, a
/// lowercase `.lrc` extension. So `track.flac` → `track.lrc` (never `track.flac.lrc`) and
/// `track.tar.gz` → `track.tar.lrc`, mirroring the Windows `GetFileNameWithoutExtension + ".lrc"`
/// rule (only the last extension is replaced).
///
/// Returns `None` for a stream URL (any path containing `://`, mirroring the C# `Contains("://")`
/// guard), an empty path, or a path with no file name to hang a stem on (e.g. `/` or `music/`).
pub fn sidecar_path(media_path: &Path) -> Option<PathBuf> {
    if media_path.as_os_str().is_empty() {
        return None;
    }
    // A stream URL never has a local sidecar. `smb://`, `https://`, … all carry `://`.
    if media_path.to_string_lossy().contains("://") {
        return None;
    }
    // No file name (a bare directory or root) → nothing to append `.lrc` to.
    media_path.file_name()?;

    let mut candidate = media_path.to_path_buf();
    candidate.set_extension("lrc");
    Some(candidate)
}

/// Read the sidecar `.lrc` for a local media file, or `None` when there is none — no local path, no
/// sidecar on disk, or an unreadable one (an unmounted share, a permission error). Never fails: any
/// I/O error resolves to "no lyrics", exactly like the Windows service's `catch { return null; }`.
///
/// The Windows original leans on a case-insensitive `File.Exists`; Linux filesystems are
/// case-sensitive, so [`sidecar_path`]'s canonical stem is probed with the `.lrc` extension in every
/// ASCII-case spelling. A sheet exported as `Track.lrc`, `Track.LRC`, `Track.Lrc`, or any mixed case
/// therefore resolves. These are bounded direct reads, never a directory scan (which could stall on
/// a slow network mount, and would run on every track that has no sidecar at all — the common case).
pub fn read_sidecar(media_path: &Path) -> Option<String> {
    let base = sidecar_path(media_path)?;
    for extension in ["lrc", "LRC", "Lrc", "lrC", "lRc", "lRC", "LrC", "LRc"] {
        let candidate = base.with_extension(extension);
        if let Ok(text) = fs::read_to_string(&candidate) {
            return Some(text);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;

    #[test]
    fn sidecar_path_swaps_the_extension_for_lrc() {
        let cases = [
            ("/music/Song.flac", "/music/Song.lrc"),
            ("/music/Song.mp3", "/music/Song.lrc"),
            ("relative/track.opus", "relative/track.lrc"),
            ("bare.m4a", "bare.lrc"),
        ];
        for (media, expected) in cases {
            assert_eq!(
                sidecar_path(Path::new(media)),
                Some(PathBuf::from(expected)),
                "{media}"
            );
        }
    }

    #[test]
    fn sidecar_path_only_replaces_the_last_extension() {
        // Mirrors GetFileNameWithoutExtension: "a.tar.gz" → stem "a.tar", so the sidecar is
        // "a.tar.lrc", not "a.lrc".
        assert_eq!(
            sidecar_path(Path::new("/m/a.tar.gz")),
            Some(PathBuf::from("/m/a.tar.lrc"))
        );
    }

    #[test]
    fn sidecar_path_appends_lrc_when_there_is_no_extension() {
        assert_eq!(
            sidecar_path(Path::new("/m/track")),
            Some(PathBuf::from("/m/track.lrc"))
        );
    }

    #[test]
    fn sidecar_path_is_none_for_stream_urls() {
        for url in [
            "https://example.com/song.mp3",
            "http://host:8080/stream.flac",
            "smb://nas/share/album.opus",
        ] {
            assert_eq!(sidecar_path(Path::new(url)), None, "{url}");
        }
    }

    #[test]
    fn sidecar_path_is_none_without_a_file_name() {
        // An empty path or a bare root has no file name to hang a stem on. (A trailing slash like
        // `music/` is normalised by `Path` to the file name `music`, so it is a valid stem.)
        for path in ["", "/"] {
            assert_eq!(sidecar_path(Path::new(path)), None, "{path:?}");
        }
    }

    #[test]
    fn read_sidecar_reads_a_same_named_lrc() {
        let dir = unique_temp_dir("okp-lyrics-read");
        let media = dir.path().join("Song.flac");
        fs::write(&media, b"not really audio").expect("media file");
        fs::write(dir.path().join("Song.lrc"), "[00:01.00]hello").expect("sidecar");

        assert_eq!(read_sidecar(&media).as_deref(), Some("[00:01.00]hello"));
    }

    #[test]
    fn read_sidecar_accepts_an_uppercase_extension() {
        let dir = unique_temp_dir("okp-lyrics-upper");
        let media = dir.path().join("Track.mp3");
        fs::write(&media, b"x").expect("media file");
        // Only an uppercase-extension sheet exists; the lowercase probe misses, the fallback hits.
        fs::write(dir.path().join("Track.LRC"), "plain words").expect("sidecar");

        assert_eq!(read_sidecar(&media).as_deref(), Some("plain words"));
    }

    #[test]
    fn read_sidecar_accepts_a_mixed_case_extension() {
        // A title-cased `.Lrc` is neither the lowercase nor the all-uppercase spelling, yet the
        // Windows case-insensitive `File.Exists` would find it. Every ASCII casing must resolve on a
        // case-sensitive Linux filesystem too.
        for extension in ["Lrc", "lRc", "lrC", "LRc"] {
            let dir = unique_temp_dir("okp-lyrics-mixed");
            let media = dir.path().join("Track.flac");
            fs::write(&media, b"x").expect("media file");
            fs::write(dir.path().join(format!("Track.{extension}")), "mixed case")
                .expect("sidecar");

            assert_eq!(
                read_sidecar(&media).as_deref(),
                Some("mixed case"),
                "Track.{extension}"
            );
        }
    }

    #[test]
    fn read_sidecar_is_none_when_absent() {
        let dir = unique_temp_dir("okp-lyrics-absent");
        let media = dir.path().join("Lonely.opus");
        fs::write(&media, b"x").expect("media file");

        assert_eq!(read_sidecar(&media), None);
    }

    #[test]
    fn read_sidecar_is_none_for_urls() {
        assert_eq!(read_sidecar(Path::new("https://example.com/a.mp3")), None);
    }
}
