//! Sidecar lyrics discovery — the portable step that finds a `.lrc` sheet next to a local audio
//! file so the shell can render it with the [`crate::lrc`] parser. Ported from the path rule in the
//! Windows `LyricsService.TryReadSidecar` (`src/OkPlayer.App/Services/LyricsService.cs`); the
//! divergences are recorded in `docs/core-compatibility.md`.
//!
//! Only the sidecar seam lives here. The metadata-keyed cache and the LRCLIB network fetch that the
//! Windows service layers on top are deliberately out of this slice (issue #189): they need the
//! private-session / history policy wired through first. The seam is documented, not stubbed — a
//! later port adds `read_cached`/`fetch_lrclib` beside these functions.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use crate::lrc::{self, LrcDocument, LrcLine};

/// Upper bound on sidecar `.lrc` file size (1 MiB) so a corrupt or hostile file
/// next to an audio track never stalls or exhausts memory.
pub const MAX_SIDECAR_BYTES: u64 = 1024 * 1024;

/// Canonical header for synced or fallback local lyrics.
pub const LYRICS_HEADER: &str = "LYRICS";

/// Canonical header for untimed (plain text) lyrics.
pub const LYRICS_PLAIN_HEADER: &str = "LYRICS · NOT SYNCED";

/// Honest fallback text when no sidecar `.lrc` exists for the track.
pub const LYRICS_EMPTY_MESSAGE: &str = "No lyrics found for this track";

/// Honest fallback text when a sidecar `.lrc` exists but cannot be read or contains no usable lines.
pub const LYRICS_INVALID_MESSAGE: &str = "Local lyrics file could not be read";

/// Quiet glyph rendered for an intentional empty/instrumental gap line in a synced sheet.
pub const LYRICS_GAP_GLYPH: &str = "♪";

const SIDECAR_EXTENSIONS: [&str; 8] = ["lrc", "LRC", "Lrc", "lrC", "lRc", "lRC", "LrC", "LRc"];

/// Resolved presentation kind for local sidecar lyrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalLyricsStatus {
    Synced,
    Plain,
    Missing,
    Invalid,
}

/// Portable result of discovering and parsing local sidecar lyrics for one audio track.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalLyrics {
    pub status: LocalLyricsStatus,
    pub document: LrcDocument,
}

impl Default for LocalLyrics {
    fn default() -> Self {
        Self::missing()
    }
}

impl LocalLyrics {
    #[must_use]
    pub fn missing() -> Self {
        Self {
            status: LocalLyricsStatus::Missing,
            document: LrcDocument::default(),
        }
    }

    #[must_use]
    pub fn invalid() -> Self {
        Self {
            status: LocalLyricsStatus::Invalid,
            document: LrcDocument::default(),
        }
    }

    /// Build a resolved view from an already-parsed document (for previews and tests).
    #[must_use]
    pub fn from_document(document: LrcDocument) -> Self {
        let status = if document.is_empty() {
            LocalLyricsStatus::Missing
        } else if document.has_timings {
            LocalLyricsStatus::Synced
        } else {
            LocalLyricsStatus::Plain
        };
        Self { status, document }
    }

    #[must_use]
    pub fn header(&self) -> &'static str {
        match self.status {
            LocalLyricsStatus::Plain => LYRICS_PLAIN_HEADER,
            LocalLyricsStatus::Synced | LocalLyricsStatus::Missing | LocalLyricsStatus::Invalid => {
                LYRICS_HEADER
            }
        }
    }

    #[must_use]
    pub fn fallback_message(&self) -> Option<&'static str> {
        match self.status {
            LocalLyricsStatus::Missing => Some(LYRICS_EMPTY_MESSAGE),
            LocalLyricsStatus::Invalid => Some(LYRICS_INVALID_MESSAGE),
            LocalLyricsStatus::Synced | LocalLyricsStatus::Plain => None,
        }
    }

    /// Active line index for synced lyrics at `position_seconds`, or `None` for plain/fallback
    /// sheets, non-finite positions, or before the first timestamp.
    #[must_use]
    pub fn active_index(&self, position_seconds: f64) -> Option<usize> {
        if self.status != LocalLyricsStatus::Synced || !position_seconds.is_finite() {
            return None;
        }
        lrc::active_index(&self.document.lines, position_seconds)
    }

    /// Renderable text for one lyric line, returning `(text, is_gap)`.
    #[must_use]
    pub fn display_line_text(line: &LrcLine) -> (&str, bool) {
        if line.text.trim().is_empty() {
            (LYRICS_GAP_GLYPH, true)
        } else {
            (line.text.as_str(), false)
        }
    }
}

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

enum SidecarRead {
    Text(String),
    Missing,
    Invalid,
}

fn read_sidecar_bounded(media_path: &Path) -> SidecarRead {
    let Some(base) = sidecar_path(media_path) else {
        return SidecarRead::Missing;
    };
    let mut saw_invalid = false;
    for extension in SIDECAR_EXTENSIONS {
        let candidate = base.with_extension(extension);
        let file = match File::open(&candidate) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => {
                saw_invalid = true;
                continue;
            }
        };
        if let Ok(meta) = file.metadata()
            && (!meta.is_file() || meta.len() > MAX_SIDECAR_BYTES)
        {
            saw_invalid = true;
            continue;
        }
        let mut bytes = Vec::new();
        if file
            .take(MAX_SIDECAR_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_SIDECAR_BYTES
        {
            saw_invalid = true;
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            saw_invalid = true;
            continue;
        };
        let stripped = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
        return SidecarRead::Text(stripped);
    }
    if saw_invalid {
        SidecarRead::Invalid
    } else {
        SidecarRead::Missing
    }
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
    match read_sidecar_bounded(media_path) {
        SidecarRead::Text(text) => Some(text),
        SidecarRead::Missing | SidecarRead::Invalid => None,
    }
}

/// Discover, read, and parse the local sidecar `.lrc` for `media_path`, distinguishing synced
/// lyrics, plain untimed lyrics, missing sidecars, and unreadable/empty sidecars.
#[must_use]
pub fn load_sidecar(media_path: &Path) -> LocalLyrics {
    match read_sidecar_bounded(media_path) {
        SidecarRead::Missing => LocalLyrics::missing(),
        SidecarRead::Invalid => LocalLyrics::invalid(),
        SidecarRead::Text(text) => {
            let document = lrc::parse(Some(&text));
            if document.is_empty() {
                LocalLyrics::invalid()
            } else if document.has_timings {
                LocalLyrics {
                    status: LocalLyricsStatus::Synced,
                    document,
                }
            } else {
                LocalLyrics {
                    status: LocalLyricsStatus::Plain,
                    document,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_test_fixtures::unique_temp_dir;
    use std::fs;

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

    #[test]
    fn load_sidecar_classifies_synced_plain_missing_and_invalid_variations() {
        let dir = unique_temp_dir("okp-lyrics-load");

        // Synced with UTF-8 BOM and a malformed line skipped cleanly.
        let synced = dir.path().join("Synced.mp3");
        fs::write(&synced, b"x").expect("synced media");
        fs::write(
            dir.path().join("Synced.lrc"),
            "\u{feff}[bad:tag]ignored\n[00:01.00]First line\n[00:03.00]\n[00:05.00]Third line\n",
        )
        .expect("synced lrc");
        let loaded = load_sidecar(&synced);
        assert_eq!(loaded.status, LocalLyricsStatus::Synced);
        assert_eq!(loaded.header(), LYRICS_HEADER);
        assert_eq!(loaded.fallback_message(), None);
        assert_eq!(loaded.document.lines.len(), 3);
        assert_eq!(loaded.active_index(0.5), None);
        assert_eq!(loaded.active_index(1.0), Some(0));
        assert_eq!(loaded.active_index(3.2), Some(1));
        assert_eq!(loaded.active_index(f64::NAN), None);
        assert_eq!(
            LocalLyrics::display_line_text(&loaded.document.lines[1]),
            (LYRICS_GAP_GLYPH, true)
        );
        assert_eq!(
            LocalLyrics::display_line_text(&loaded.document.lines[2]),
            ("Third line", false)
        );

        // Plain untimed lyrics.
        let plain = dir.path().join("Plain.flac");
        fs::write(&plain, b"x").expect("plain media");
        fs::write(dir.path().join("Plain.lrc"), "First plain\nSecond plain\n").expect("plain lrc");
        let loaded = load_sidecar(&plain);
        assert_eq!(loaded.status, LocalLyricsStatus::Plain);
        assert_eq!(loaded.header(), LYRICS_PLAIN_HEADER);
        assert_eq!(loaded.fallback_message(), None);
        assert_eq!(loaded.document.lines.len(), 2);
        assert_eq!(loaded.active_index(1.0), None);

        // Missing sidecar.
        let missing = dir.path().join("Missing.mp3");
        fs::write(&missing, b"x").expect("missing media");
        let loaded = load_sidecar(&missing);
        assert_eq!(loaded.status, LocalLyricsStatus::Missing);
        assert_eq!(loaded.fallback_message(), Some(LYRICS_EMPTY_MESSAGE));
        assert!(loaded.document.is_empty());

        // Corrupt non-UTF-8 sidecar.
        let corrupt = dir.path().join("Corrupt.mp3");
        fs::write(&corrupt, b"x").expect("corrupt media");
        fs::write(dir.path().join("Corrupt.lrc"), [0xff, 0xfe, 0x00, 0x01]).expect("corrupt lrc");
        let loaded = load_sidecar(&corrupt);
        assert_eq!(loaded.status, LocalLyricsStatus::Invalid);
        assert_eq!(loaded.fallback_message(), Some(LYRICS_INVALID_MESSAGE));
        assert!(loaded.document.is_empty());

        // Blank / tags-only sidecar.
        let tags_only = dir.path().join("TagsOnly.mp3");
        fs::write(&tags_only, b"x").expect("tags media");
        fs::write(
            dir.path().join("TagsOnly.lrc"),
            "[ti:Only Title]\n[ar:Artist]\n   \n",
        )
        .expect("tags lrc");
        let loaded = load_sidecar(&tags_only);
        assert_eq!(loaded.status, LocalLyricsStatus::Invalid);
        assert_eq!(loaded.fallback_message(), Some(LYRICS_INVALID_MESSAGE));

        // Oversized sidecar (> MAX_SIDECAR_BYTES).
        let huge = dir.path().join("Huge.mp3");
        fs::write(&huge, b"x").expect("huge media");
        fs::write(
            dir.path().join("Huge.lrc"),
            vec![b'a'; (MAX_SIDECAR_BYTES as usize) + 64],
        )
        .expect("huge lrc");
        let loaded = load_sidecar(&huge);
        assert_eq!(loaded.status, LocalLyricsStatus::Invalid);

        // Stream URLs never probe disk.
        let loaded = load_sidecar(Path::new("https://example.com/song.mp3"));
        assert_eq!(loaded.status, LocalLyricsStatus::Missing);
    }
}
