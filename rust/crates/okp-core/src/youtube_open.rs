//! The Linux "Open URL" surface's YouTube handling (PRD §10.2 reserves a native
//! YouTube/yt-dlp slot for Day-2, without shipping a generic web browser). A YouTube page
//! URL is not a media stream libmpv can open on its own — mpv's `ytdl` hook shells out to
//! **yt-dlp** to resolve the real stream. So a YouTube URL has one of two deliberate
//! outcomes: it plays when yt-dlp is on the host, or it lands in a clear missing-tooling
//! state that names the tool — never a silent hand-off to the engine that fails with a
//! generic error.
//!
//! No parsing/classification or business logic belongs in a shell (freeze-boundary): the
//! host recognition, the outcome decision, and the user-facing copy live here so the Linux
//! shell (and a future Windows port) only probe for the tool and render the result. The
//! tool probe itself is impure (it scans `PATH`), so the shell injects its result as a bool
//! — mirroring how [`crate::network_path::is_network`] takes an injected drive-type probe.
//!
//! An arbitrary `http(s)://` (or `rtsp://`, `smb://`, …) stream URL is **not** YouTube and
//! is untouched by this module — it stays on the existing direct-to-engine path.

/// The tool OK Player relies on to turn a YouTube page URL into a playable stream: mpv's
/// `ytdl` hook invokes it. Named once here so the shell's `PATH` probe and the
/// missing-tooling copy can never drift apart.
pub const YOUTUBE_RESOLVER: &str = "yt-dlp";

/// The registrable domains that mark a URL as YouTube. A host matches a domain when it *is*
/// that domain or is a subdomain of it (`www.`, `m.`, `music.`, …). `youtube-nocookie.com`
/// is the privacy-embed host; `youtu.be` is the short-link host.
const YOUTUBE_DOMAINS: &[&str] = &["youtube.com", "youtu.be", "youtube-nocookie.com"];

/// What the shell should do with a URL typed into "Open URL". Pure and deterministic given
/// the URL and whether the resolver is installed, so the outcome is unit-testable without a
/// host probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenUrlOutcome {
    /// Not a URL OK Player can act on (empty, a bare path, a sentence with a link in it,
    /// `file://`, …). The shell asks for a valid stream URL.
    Invalid,
    /// A direct stream URL that is not YouTube (`http(s)://`, `rtsp://`, `smb://`, …). Hand
    /// it straight to the engine, exactly as before this surface existed.
    PlayDirect,
    /// A YouTube URL and the resolver ([`YOUTUBE_RESOLVER`]) is available. Hand it to the
    /// engine, whose `ytdl` hook shells out to the resolver to open the real stream.
    PlayYouTube,
    /// A YouTube URL but the resolver is missing. Nothing is handed to the engine; the shell
    /// shows the missing-tooling state (see [`tooling_missing_notice`]) instead of letting a
    /// generic engine failure stand in for it.
    YouTubeToolingMissing,
}

/// True when `url` points at YouTube (any of [`YOUTUBE_DOMAINS`] or a subdomain of one).
/// Recognition is by host, so `https://www.youtube.com/watch?v=…`, `https://youtu.be/…`,
/// and `https://music.youtube.com/…` all match, while a look-alike host
/// (`https://notyoutube.com/…`, `https://youtube.com.evil.test/…`) does not.
pub fn is_youtube_url(url: &str) -> bool {
    let Some(host) = url_host(url.trim()) else {
        return false;
    };
    // Case-insensitive (RFC 3986 hosts are), and tolerate a fully-qualified trailing dot.
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    YOUTUBE_DOMAINS
        .iter()
        .any(|domain| host_matches_domain(&host, domain))
}

/// Decide the outcome for a URL typed into "Open URL". `resolver_available` is whether the
/// shell found [`YOUTUBE_RESOLVER`] on the host — the one impure input, injected so this
/// stays pure. A non-URL is [`OpenUrlOutcome::Invalid`]; a non-YouTube stream URL is always
/// [`OpenUrlOutcome::PlayDirect`] regardless of the resolver (existing playback is
/// untouched); a YouTube URL resolves to play-or-missing-tooling on the probe.
pub fn resolve_open_url(url: &str, resolver_available: bool) -> OpenUrlOutcome {
    if !crate::media_formats::is_playable_url(Some(url)) {
        return OpenUrlOutcome::Invalid;
    }
    if !is_youtube_url(url) {
        return OpenUrlOutcome::PlayDirect;
    }
    if resolver_available {
        OpenUrlOutcome::PlayYouTube
    } else {
        OpenUrlOutcome::YouTubeToolingMissing
    }
}

/// The in-app explanation for a YouTube URL that cannot be opened because the resolver is
/// not installed. Names the exact tool so the state is actionable, and stays quiet about a
/// browser OK Player does not ship.
pub fn tooling_missing_notice() -> String {
    format!("YouTube links need {YOUTUBE_RESOLVER} — install it to open them.")
}

/// The always-visible "Open URL" hint describing YouTube support, keyed to whether the
/// resolver was detected. Lets the shell explain the state up front (before the user
/// submits) without duplicating the tool name or the wording.
pub fn youtube_support_hint(resolver_available: bool) -> String {
    if resolver_available {
        format!("YouTube links open via {YOUTUBE_RESOLVER}.")
    } else {
        format!("YouTube links need {YOUTUBE_RESOLVER}, which isn't installed.")
    }
}

/// The host component of a URL — everything after `scheme://` up to the first `/`, `?`, or
/// `#`, with any `user:pass@` userinfo and `:port` stripped. `None` when the text has no
/// `://` authority or the host is empty. Deliberately tiny (no URL-crate dependency): it
/// only needs to recover the host for domain matching, not to fully validate the URL —
/// [`crate::media_formats::is_playable_url`] guards validity in [`resolve_open_url`].
fn url_host(url: &str) -> Option<&str> {
    let (_scheme, rest) = url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    // Userinfo (`user@`, `user:pass@`) precedes the host; a port (`:443`) follows it.
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    (!host.is_empty()).then_some(host)
}

/// True when `host` is `domain` exactly or a subdomain of it. The leading-dot guard keeps a
/// look-alike registrable name (`notyoutube.com` vs `youtube.com`) from matching: only a
/// real label boundary (`www.youtube.com`) counts.
fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_youtube_url_matches_youtube_hosts_and_subdomains() {
        for url in [
            "https://youtube.com/watch?v=abc123",
            "https://www.youtube.com/watch?v=abc123",
            "https://m.youtube.com/watch?v=abc123",
            "https://music.youtube.com/watch?v=abc123",
            "http://youtu.be/abc123",
            "https://youtu.be/abc123?t=42",
            "https://www.youtube-nocookie.com/embed/abc123",
            "https://www.youtube.com:443/watch?v=abc123", // explicit port
            "https://YouTube.com/watch?v=abc123",         // case-insensitive host
            "https://youtube.com./watch?v=abc123",        // fully-qualified trailing dot
        ] {
            assert!(is_youtube_url(url), "{url}");
        }
    }

    #[test]
    fn is_youtube_url_rejects_look_alikes_and_non_youtube() {
        for url in [
            "https://notyoutube.com/watch?v=abc", // suffix without a label boundary
            "https://youtube.com.evil.test/watch", // youtube.com is not the registrable host
            "https://example.com/youtube.com/clip", // youtube.com only in the path
            "https://vimeo.com/12345",
            "https://example.com/video.mkv",
            "rtsp://host/live",
            "not a url",
            "",
        ] {
            assert!(!is_youtube_url(url), "{url}");
        }
    }

    #[test]
    fn resolve_open_url_rejects_non_urls() {
        // Junk, bare paths, embedded links, and `file://` are never playable — the probe
        // does not change that.
        for text in [
            "",
            "   ",
            "movie.mkv",
            "watch this: https://youtu.be/abc",
            "file:///home/user/clip.mkv",
        ] {
            assert_eq!(
                resolve_open_url(text, true),
                OpenUrlOutcome::Invalid,
                "{text}"
            );
            assert_eq!(
                resolve_open_url(text, false),
                OpenUrlOutcome::Invalid,
                "{text}"
            );
        }
    }

    #[test]
    fn resolve_open_url_plays_non_youtube_streams_regardless_of_tooling() {
        // Requirement: existing arbitrary stream-URL playback stays intact and never
        // depends on the YouTube resolver being present.
        for url in [
            "https://example.com/video.mkv",
            "http://host:8080/stream",
            "smb://nas/share/movie.mkv",
            "rtsp://host/live",
        ] {
            assert_eq!(
                resolve_open_url(url, true),
                OpenUrlOutcome::PlayDirect,
                "{url}"
            );
            assert_eq!(
                resolve_open_url(url, false),
                OpenUrlOutcome::PlayDirect,
                "{url}"
            );
        }
    }

    #[test]
    fn resolve_open_url_youtube_outcome_follows_the_tooling_probe() {
        let url = "https://www.youtube.com/watch?v=abc123";
        // Resolver present -> hand to the engine (its ytdl hook resolves the stream).
        assert_eq!(resolve_open_url(url, true), OpenUrlOutcome::PlayYouTube);
        // Resolver missing -> the deliberate missing-tooling state, not a silent hand-off.
        assert_eq!(
            resolve_open_url(url, false),
            OpenUrlOutcome::YouTubeToolingMissing
        );
    }

    #[test]
    fn tooling_missing_notice_names_the_resolver() {
        let notice = tooling_missing_notice();
        assert!(notice.contains(YOUTUBE_RESOLVER), "{notice}");
        // The copy stays about the tool, never implying a bundled browser.
        assert!(!notice.to_ascii_lowercase().contains("browser"), "{notice}");
    }

    #[test]
    fn youtube_support_hint_reflects_detection_and_names_the_resolver() {
        let present = youtube_support_hint(true);
        let missing = youtube_support_hint(false);
        assert!(present.contains(YOUTUBE_RESOLVER), "{present}");
        assert!(missing.contains(YOUTUBE_RESOLVER), "{missing}");
        // The two states read differently so the surface is honest about tooling.
        assert_ne!(present, missing);
    }
}
