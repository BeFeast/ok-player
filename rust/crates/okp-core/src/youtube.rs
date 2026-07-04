//! Recognizes YouTube links and decides the outcome of the Linux "Open URL"
//! surface — the pure core behind the PRD §10.2 Day-2 reservation. That entry
//! point keeps the YouTube slot honest **without** building a generic web
//! browser: it accepts a URL, recognizes YouTube links, and either hands them to
//! the engine or explains that the host is missing the tool that resolves them.
//!
//! YouTube playback rides mpv's `ytdl_hook`, which shells out to an external
//! resolver (`yt-dlp`, or the older `youtube-dl`). When no resolver is on the
//! host, mpv cannot turn a YouTube page URL into a stream, so the shell must say
//! so up front instead of feeding mpv a link it will silently fail to open. This
//! classification and the outcome decision live in the core so they stay
//! shell-agnostic and unit-testable — the freeze boundary keeps this logic out
//! of the GTK shell, which only probes the host and renders the result.
//!
//! This is Linux-forward core with no C# counterpart yet (the Windows shell does
//! not surface a YouTube entry point), so there is no cross-platform spec to
//! mirror — only the rules below.

use crate::media_formats;

/// Registrable domains that mean "this link is a YouTube page a resolver must
/// turn into a stream". A host matches when it equals one of these or is a
/// subdomain of one (so `m.youtube.com` and `music.youtube.com` count, while a
/// look-alike like `notyoutube.com` or `youtube.com.evil.example` does not).
const YOUTUBE_DOMAINS: &[&str] = &["youtube.com", "youtu.be", "youtube-nocookie.com"];

/// External stream resolvers mpv's `ytdl_hook` can drive to turn a YouTube page
/// URL into a playable stream, most-preferred first. The Linux shell probes the
/// host `PATH` for these to decide whether a YouTube link is playable.
pub const YOUTUBE_RESOLVERS: &[&str] = &["yt-dlp", "youtube-dl"];

/// What the "Open URL" field's text points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// Not a usable stream URL (empty, prose, a bare path, `file://`, …).
    Unsupported,
    /// A direct stream URL mpv opens on its own (`http(s)`, `smb`, `rtsp`, …).
    DirectStream,
    /// A YouTube watch / short / playlist / `youtu.be` link that needs a resolver.
    YouTube,
}

/// What the shell should do with the entered URL, given the host's tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenUrlOutcome {
    /// The text is not a usable URL — reject it with a validation hint.
    Reject,
    /// Hand it straight to mpv (a direct stream, or YouTube with a resolver present).
    Play,
    /// A YouTube link, but no resolver is installed — explain the missing tool.
    YouTubeToolingMissing,
}

/// Classifies the "Open URL" field text. A YouTube link is a strict subset of
/// the playable URLs [`media_formats::is_playable_url`] accepts, so existing
/// arbitrary `http(s)://` (and `smb`/`rtsp`/…) playback stays classified as a
/// [`UrlKind::DirectStream`] and keeps working unchanged.
pub fn classify_url(text: Option<&str>) -> UrlKind {
    let Some(trimmed) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return UrlKind::Unsupported;
    };
    if !media_formats::is_playable_url(Some(trimmed)) {
        return UrlKind::Unsupported;
    }
    match url_host(trimmed) {
        Some(host) if is_youtube_host(host) => UrlKind::YouTube,
        _ => UrlKind::DirectStream,
    }
}

/// Decides the deliberate outcome for the entered URL: junk is rejected, direct
/// streams always play, and a YouTube link plays only when a resolver is present
/// — otherwise the shell reports the missing tooling.
pub fn open_url_outcome(text: Option<&str>, resolver_available: bool) -> OpenUrlOutcome {
    match classify_url(text) {
        UrlKind::Unsupported => OpenUrlOutcome::Reject,
        UrlKind::DirectStream => OpenUrlOutcome::Play,
        UrlKind::YouTube if resolver_available => OpenUrlOutcome::Play,
        UrlKind::YouTube => OpenUrlOutcome::YouTubeToolingMissing,
    }
}

/// The host of a `scheme://authority/…` URL, or `None` when there is no usable
/// authority. Strips any `user:pass@` userinfo and `:port` suffix; an IPv6
/// literal (`[::1]`) is kept whole rather than mis-split on its colons.
fn url_host(text: &str) -> Option<&str> {
    let (_scheme, rest) = text.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if host_port.starts_with('[') {
        host_port
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    (!host.is_empty()).then_some(host)
}

/// True when `host` is a YouTube domain or a subdomain of one. Matching a domain
/// only as an exact host or after a `.` prevents look-alikes (`notyoutube.com`,
/// `youtube.com.evil.example`) from being treated as YouTube.
fn is_youtube_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    YOUTUBE_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognizes_youtube_links() {
        for text in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/abc123",
            "https://www.youtube.com/playlist?list=PL0123456789",
            "https://youtu.be/dQw4w9WgXcQ",
            "http://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            "  https://youtu.be/dQw4w9WgXcQ  ", // surrounding whitespace is trimmed
            "https://WWW.YouTube.com/watch?v=x", // host match is case-insensitive
            "https://youtu.be:443/dQw4w9WgXcQ", // explicit port is stripped
        ] {
            assert_eq!(classify_url(Some(text)), UrlKind::YouTube, "{text}");
        }
    }

    #[test]
    fn classify_keeps_arbitrary_streams_direct() {
        for text in [
            "https://example.com/video.mkv",
            "http://host:8080/stream",
            "smb://nas/share/movie.mkv",
            "rtsp://host/live",
            "https://notyoutube.com/watch?v=x", // look-alike host, not a subdomain
            "https://fakeyoutu.be/x",
            "https://youtube.com.evil.example/watch?v=x", // youtube.com is not the host
        ] {
            assert_eq!(classify_url(Some(text)), UrlKind::DirectStream, "{text}");
        }
    }

    #[test]
    fn classify_rejects_non_urls() {
        for text in [
            Some("file:///home/me/movie.mkv"),
            Some("watch this https://youtu.be/x"), // prose with an embedded link
            Some("movie.mkv"),
            Some("youtube.com/watch?v=x"), // no scheme — not a URL
            Some(""),
            Some("   "),
            None,
        ] {
            assert_eq!(classify_url(text), UrlKind::Unsupported, "{text:?}");
        }
    }

    #[test]
    fn outcome_plays_direct_streams_regardless_of_tooling() {
        for resolver_available in [true, false] {
            assert_eq!(
                open_url_outcome(Some("https://example.com/video.mkv"), resolver_available),
                OpenUrlOutcome::Play,
                "resolver_available={resolver_available}"
            );
        }
    }

    #[test]
    fn outcome_gates_youtube_on_the_resolver() {
        let youtube = Some("https://youtu.be/dQw4w9WgXcQ");
        assert_eq!(open_url_outcome(youtube, true), OpenUrlOutcome::Play);
        assert_eq!(
            open_url_outcome(youtube, false),
            OpenUrlOutcome::YouTubeToolingMissing,
        );
    }

    #[test]
    fn outcome_rejects_junk_before_it_matters_whether_tooling_exists() {
        assert_eq!(
            open_url_outcome(Some("not a url at all"), true),
            OpenUrlOutcome::Reject
        );
        assert_eq!(open_url_outcome(None, false), OpenUrlOutcome::Reject);
    }
}
