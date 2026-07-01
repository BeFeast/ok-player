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
