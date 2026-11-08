use std::path::Path;

pub const AUDIO_EXTENSIONS: &[&str] = &[
    ".mp3", ".flac", ".m4a", ".m4b", ".opus", ".wav", ".ogg", ".oga", ".mka", ".aac", ".wv",
    ".ape", ".wma", ".aiff", ".aif", ".dsf", ".dff", ".tak", ".tta", ".mpc", ".ac3", ".dts",
    ".caf", ".spx", ".amr",
];

pub const VIDEO_EXTENSIONS: &[&str] = &[
    ".mkv", ".mp4", ".m4v", ".avi", ".mov", ".webm", ".m2ts", ".ts", ".wmv", ".flv", ".mpg",
    ".mpeg", ".3gp", ".3g2", ".ogv", ".vob", ".divx", ".f4v", ".mts", ".m2t", ".asf", ".rm",
    ".rmvb", ".mxf",
];

pub const SUBTITLE_EXTENSIONS: &[&str] = &[".srt", ".ass", ".ssa", ".sub", ".vtt", ".idx", ".sup"];

pub fn extensions() -> impl Iterator<Item = &'static str> {
    VIDEO_EXTENSIONS
        .iter()
        .copied()
        .chain(AUDIO_EXTENSIONS.iter().copied())
}

pub fn is_media(path: impl AsRef<Path>) -> bool {
    extension_matches(path, extensions())
}

pub fn is_audio(path: impl AsRef<Path>) -> bool {
    extension_matches(path, AUDIO_EXTENSIONS.iter().copied())
}

pub fn is_subtitle(path: impl AsRef<Path>) -> bool {
    extension_matches(path, SUBTITLE_EXTENSIONS.iter().copied())
}

/// True when `file_name` (raw bytes, so a non-UTF-8 name is judged too) ends in a known media
/// extension, read the way [`Path::extension`] reads one: after the last `.`, which must not
/// be the first byte (`.mp4` alone is a hidden file without an extension).
pub fn has_media_extension(file_name: &[u8]) -> bool {
    let Some(dot) = file_name.iter().rposition(|byte| *byte == b'.') else {
        return false;
    };
    dot > 0 && extensions().any(|known| file_name[dot..].eq_ignore_ascii_case(known.as_bytes()))
}

/// The last path segment of an absolute URL, still percent-encoded, without the query or the
/// fragment: `a.MP4` for `https://h/c/a.MP4?x=1`. `None` when the URL has no `://` or no path;
/// `Some("")` when the path ends in `/`.
pub fn url_last_segment(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (_, path) = rest.split_once('/')?;
    path.rsplit('/').next()
}

/// Percent-decode `raw` the way an HTTP server reads a request path: every valid `%XX`
/// decodes, a stray `%` stays literal, and `+` stays `+`.
pub fn percent_decode_lenient(raw: &str) -> Vec<u8> {
    fn hex_value(byte: u8) -> Option<u8> {
        char::from(byte)
            .to_digit(16)
            .and_then(|digit| u8::try_from(digit).ok())
    }
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| {
                let high = hex_value(*bytes.get(index + 1)?)?;
                let low = hex_value(*bytes.get(index + 2)?)?;
                Some(high << 4 | low)
            })
            .flatten();
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    decoded
}

/// The file a direct media URL names: its last path segment, decoded as the server reads it
/// ([`percent_decode_lenient`]), when that name has a known media extension. `None` for a page
/// URL (`https://youtu.be/abc`), a URL whose path ends in `/`, or one without a path.
///
/// This is the one definition of a direct media URL. Up Next labels such a URL by this name,
/// and a direct media URL opened or queued from an `ok-player:` link never arms the replay
/// cache.
pub fn direct_media_url_name(url: &str) -> Option<Vec<u8>> {
    let name = percent_decode_lenient(url_last_segment(url)?);
    has_media_extension(&name).then_some(name)
}

/// True when `url` is a direct media URL; see [`direct_media_url_name`].
pub fn is_direct_media_url(url: &str) -> bool {
    direct_media_url_name(url).is_some()
}

pub fn is_playable_url(text: Option<&str>) -> bool {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return false;
    };

    let Some((scheme, rest)) = text.split_once("://") else {
        return false;
    };

    !scheme.is_empty()
        && !scheme.eq_ignore_ascii_case("file")
        && !rest.is_empty()
        && !text.chars().any(char::is_whitespace)
}

fn extension_matches(
    path: impl AsRef<Path>,
    mut extensions: impl Iterator<Item = &'static str>,
) -> bool {
    let Some(extension) = path.as_ref().extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let extension = format!(".{}", extension.to_ascii_lowercase());
    extensions.any(|known| known == extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_media_true_for_known_media() {
        for path in [r"C:\v\movie.mkv", r"C:\v\clip.MP4", "song.flac"] {
            assert!(is_media(path));
        }
    }

    #[test]
    fn is_media_false_for_subtitles() {
        for path in [r"C:\v\subs.srt", r"C:\v\track.ass", r"C:\v\track.VTT"] {
            assert!(!is_media(path));
        }
    }

    #[test]
    fn is_subtitle_true_for_subtitle_files() {
        for path in [
            r"C:\v\subs.srt",
            r"C:\v\track.ass",
            r"C:\v\track.ssa",
            r"C:\v\track.SUB",
            r"C:\v\track.vtt",
        ] {
            assert!(is_subtitle(path));
        }
    }

    #[test]
    fn is_subtitle_false_for_non_subtitles() {
        for path in [r"C:\v\movie.mkv", r"C:\v\song.flac", r"C:\v\notes.txt"] {
            assert!(!is_subtitle(path));
        }
    }

    #[test]
    fn media_and_subtitle_sets_do_not_overlap() {
        for extension in SUBTITLE_EXTENSIONS {
            assert!(!is_media(format!("x{extension}")));
        }
    }

    #[test]
    fn is_audio_true_for_audio_only_containers() {
        for path in [
            r"C:\music\song.flac",
            r"C:\music\track.MP3",
            "podcast.opus",
            r"C:\music\album.mka",
        ] {
            assert!(is_audio(path));
        }
    }

    #[test]
    fn is_audio_false_for_video_subtitle_and_other() {
        for path in [
            r"C:\v\movie.mkv",
            r"C:\v\clip.mp4",
            r"C:\v\subs.srt",
            r"C:\v\notes.txt",
        ] {
            assert!(!is_audio(path));
        }
    }

    #[test]
    fn audio_extensions_are_all_recognized_media() {
        for extension in AUDIO_EXTENSIONS {
            assert!(is_media(format!("x{extension}")));
        }
    }

    #[test]
    fn media_extensions_are_read_like_path_extensions() {
        for name in [
            &b"a.mkv"[..],
            b"A001.MP4",
            b"\xff.flac",
            b"x.y.mov",
            b"a b.Mp3",
        ] {
            assert!(has_media_extension(name), "{}", name.escape_ascii());
        }
        for name in [
            &b".mp4"[..],
            b"a.mp4.",
            b"a.txt",
            b"a.srt",
            b"a.m3u",
            b"mp4",
            b"",
            b"a.mp4\0",
        ] {
            assert!(!has_media_extension(name), "{}", name.escape_ascii());
        }
        // The same verdict as the Path-based check the local load path uses.
        for name in ["clip.MKV", ".mkv", "clip.mkv.", "notes.txt", "song.flac"] {
            assert_eq!(
                has_media_extension(name.as_bytes()),
                is_media(name),
                "{name}"
            );
        }
    }

    #[test]
    fn lenient_decoding_keeps_a_stray_percent_and_decodes_the_rest() {
        assert_eq!(percent_decode_lenient("list.m3%75"), b"list.m3u");
        assert_eq!(percent_decode_lenient("%zz.m3%55"), b"%zz.m3U");
        assert_eq!(percent_decode_lenient("a%2"), b"a%2");
        assert_eq!(percent_decode_lenient("%%41+"), b"%A+");
        assert_eq!(percent_decode_lenient("%252F"), b"%2F");
    }

    /// The one table for "direct media URL": what the predicate says and what Up Next shows
    /// for the same URL, so the two can never disagree about which URLs name a media file.
    #[test]
    fn direct_media_urls_share_one_predicate_with_their_up_next_label() {
        use crate::playlist::PlaylistItem;

        // (url, is a direct media URL, Up Next label)
        let table: &[(&str, bool, &str)] = &[
            ("https://cam.example/clips/A001.MP4", true, "A001.MP4"),
            (
                "http://cam.example:8080/c/clip.mov?token=abc#t=5",
                true,
                "clip.mov",
            ),
            ("https://cdn.example/audio/song.flac", true, "song.flac"),
            ("https://clips.example/My%20Clip.MOV", true, "My Clip.MOV"),
            ("https://clips.example/caf%C3%A9.mkv?x=1", true, "café.mkv"),
            // An encoded extension is the same file name to the server.
            ("https://clips.example/clip.m%70%34", true, "clip.mp4"),
            ("https://clips.example/clip%2EMKV", true, "clip.MKV"),
            // A stray % stays literal while the rest decodes.
            ("https://clips.example/100%.mp4", true, "100%.mp4"),
            // A name that decodes to bytes a label cannot show keeps its raw segment, but it
            // still names a media file.
            ("https://clips.example/a%0Ab.mp4", true, "a%0Ab.mp4"),
            ("https://clips.example/%FF.mp4", true, "%FF.mp4"),
            ("https://clips.example/%FF.mp4?sig=1", true, "%FF.mp4"),
            // Page URLs and everything that is not a media file name keep the raw last
            // segment, query included.
            ("https://youtu.be/abc123", false, "abc123"),
            ("https://video.example/watch?v=abc", false, "watch?v=abc"),
            (
                "https://www.youtube.com/watch?v=abc&f=a.mp4",
                false,
                "watch?v=abc&f=a.mp4",
            ),
            (
                "https://video.example/play?file=a.mp4",
                false,
                "play?file=a.mp4",
            ),
            (
                "https://stream.example/live/channel-one.m3u8?t=1",
                false,
                "channel-one.m3u8?t=1",
            ),
            (
                "https://cam.example/clip.mp4/",
                false,
                "https://cam.example/clip.mp4/",
            ),
            ("https://cam.example/notes.txt", false, "notes.txt"),
            ("https://cam.example/.mp4", false, ".mp4"),
            ("https://cam.example/clip.mp%34%", false, "clip.mp%34%"),
            ("https://cam.example/clips.d/index", false, "index"),
            ("https://cam.example", false, "cam.example"),
            ("https://video.example/", false, "https://video.example/"),
            ("not a url.mp4", false, "not a url.mp4"),
        ];
        for &(url, direct, label) in table {
            assert_eq!(is_direct_media_url(url), direct, "{url}");
            assert_eq!(
                PlaylistItem::Url(url.to_owned()).display_name(),
                label,
                "label for {url}"
            );
        }
    }

    #[test]
    fn is_playable_url_true_for_absolute_stream_urls() {
        for text in [
            "https://example.com/video.mkv",
            "http://host:8080/stream",
            "smb://nas/share/movie.mkv",
            "rtsp://host/live",
            "  https://example.com/v.mp4  ",
        ] {
            assert!(is_playable_url(Some(text)));
        }
    }

    #[test]
    fn is_playable_url_false_for_paths_paragraphs_and_junk() {
        for text in [
            Some("check out https://example.com/v.mkv it's great"),
            Some("file:///C:/media/movie.mkv"),
            Some("movie.mkv"),
            Some("not a url at all"),
            Some(""),
            Some("   "),
            None,
        ] {
            assert!(!is_playable_url(text));
        }
    }
}
