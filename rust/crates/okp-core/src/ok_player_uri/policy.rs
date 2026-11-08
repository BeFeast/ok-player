//! Target policy: decides whether a syntactically valid request may reach the engine.
//!
//! Every syntactic rule runs first; the filesystem is asked only about a path that already
//! passed all of them, and only through the [`FsProbe`] the caller supplies.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::parse::{Plus, os_string_from_bytes, percent_decode, percent_decode_lenient};
use super::probe::{FsKind, FsProbe};
use super::{Action, ActionRequest, Reason, Target};
use crate::media_download::validate_public_http_url;
use crate::media_formats::SUBTITLE_EXTENSIONS;

/// Playlist files never open from a link in v1: a playlist would smuggle further targets
/// (and mpv-internal protocols) past the single-target policy.
pub const PLAYLIST_EXTENSIONS: &[&str] = &[".m3u", ".m3u8", ".pls", ".cue", ".edl"];

/// Which path rules apply. Both are testable on any host, so the Windows rules are pinned
/// now for the later port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathFlavor {
    /// Absolute with exactly one leading `/`; raw bytes allowed.
    Posix,
    /// `X:\…` only; UTF-8; no UNC, device namespace, drive-relative path or stream.
    Windows,
}

impl PathFlavor {
    /// The flavour of the platform this code runs on.
    pub const fn native() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }
}

/// A request that passed [`evaluate`]. Only `evaluate` constructs one, so holding a
/// `Validated` is proof that its target cleared the policy.
#[derive(Clone, Debug, PartialEq)]
pub struct Validated {
    action: Action,
    target: Target,
    start: Option<f64>,
    start_dropped: bool,
    ignored_keys: Vec<String>,
}

impl Validated {
    pub fn action(&self) -> Action {
        self.action
    }

    /// The target to play. A local path is the original path from the link (a `file:` URL
    /// already converted), never a canonicalized one, so history keys match a normal open.
    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn start(&self) -> Option<f64> {
        self.start
    }

    /// `t` was present but malformed and was dropped; show a notice.
    pub fn start_dropped(&self) -> bool {
        self.start_dropped
    }

    /// Unknown keys the link carried, for the log line.
    pub fn ignored_keys(&self) -> &[String] {
        &self.ignored_keys
    }
}

/// Judge `request`'s target. `url=` must be `http`/`https` with a host and no credentials
/// (a local `file:` URL converts to a path); a path must be absolute in `flavor`'s form and
/// name an existing regular file according to `probe`. Neither may name a playlist or
/// subtitle file: a URL by its last path segment, a path by its last component.
///
/// `probe` is consulted at most once, and only after every syntactic rule passed. A real
/// probe can block for its timeout, so call this off the UI thread.
pub fn evaluate(
    request: &ActionRequest,
    probe: &impl FsProbe,
    flavor: PathFlavor,
) -> Result<Validated, Reason> {
    let path = match &request.target {
        Target::Url(url) => match url_target(url, flavor)? {
            UrlTarget::Remote => {
                return Ok(validated(request, Target::Url(url.clone())));
            }
            UrlTarget::LocalFile(path) => path,
        },
        Target::Path(path) => path.clone(),
    };
    check_path_syntax(&path, flavor)?;
    match probe.probe(Path::new(&path)) {
        FsKind::RegularFile => Ok(validated(request, Target::Path(path))),
        FsKind::Directory => Err(Reason::Directory),
        FsKind::Special => Err(Reason::NotRegularFile),
        FsKind::Missing => Err(Reason::NotFound),
        FsKind::Unavailable | FsKind::TimedOut => Err(Reason::PathUnavailable),
    }
}

fn validated(request: &ActionRequest, target: Target) -> Validated {
    Validated {
        action: request.action,
        target,
        start: request.start,
        start_dropped: request.start_dropped,
        ignored_keys: request.ignored_keys.clone(),
    }
}

enum UrlTarget {
    /// An `http`/`https` URL that passes as-is.
    Remote,
    /// A local `file:` URL, converted to a path that still has to pass the path rules.
    LocalFile(OsString),
}

fn url_target(url: &str, flavor: PathFlavor) -> Result<UrlTarget, Reason> {
    // `url=` decodes once; a URL itself never contains raw whitespace or control characters.
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(Reason::InvalidUrl);
    }
    let Some((scheme, rest)) = url.split_once(':') else {
        return Err(Reason::InvalidUrl);
    };
    if !is_uri_scheme(scheme) {
        return Err(Reason::InvalidUrl);
    }
    if scheme.eq_ignore_ascii_case("file") {
        return file_url_path(rest, flavor).map(UrlTarget::LocalFile);
    }
    // An allowlist, not a denylist: smb, rtsp, ftp, every mpv-internal protocol (edl, av,
    // lavfi, memory, fd, ytdl, …) and a nested ok-player link all land here.
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(Reason::UnsupportedScheme);
    }
    validate_public_http_url(url).map_err(|_| Reason::InvalidUrl)?;
    let authority = rest
        .trim_start_matches('/')
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = if authority.starts_with('[') {
        authority.split_inclusive(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() {
        return Err(Reason::InvalidUrl);
    }
    // A playlist or subtitle file rejects by the name the server will look up, exactly as a
    // local one does. The name decodes as the server reads it, so `list.m3%75` is `list.m3u`.
    // The query is not a name: `https://h/play?f=list.m3u` passes.
    if let Some(segment) = super::url_last_segment(url) {
        check_extension(&percent_decode_lenient(segment))?;
    }
    Ok(UrlTarget::Remote)
}

fn is_uri_scheme(scheme: &str) -> bool {
    scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// `file://[localhost]/path` to a path. The `file:` URL's own path is percent-encoded (a
/// space in it is `%20`), so it is decoded once more, strictly, exactly as a file manager
/// reads a file URL; the link layer itself still decoded only once.
fn file_url_path(rest: &str, flavor: PathFlavor) -> Result<OsString, Reason> {
    let Some(after) = rest.strip_prefix("//") else {
        return Err(Reason::InvalidUrl);
    };
    if after.contains(['?', '#']) {
        return Err(Reason::InvalidUrl);
    }
    let Some(slash) = after.find('/') else {
        return Err(Reason::InvalidUrl);
    };
    let (host, path) = after.split_at(slash);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return Err(Reason::RemoteFileUrl);
    }
    let mut bytes = percent_decode(path, Plus::Literal).ok_or(Reason::BadPercentEncoding)?;
    // `file:///C:/media/a.mkv` names `C:\media\a.mkv` on Windows.
    if flavor == PathFlavor::Windows
        && bytes.len() >= 3
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        bytes.remove(0);
        for byte in &mut bytes {
            if *byte == b'/' {
                *byte = b'\\';
            }
        }
    }
    os_string_from_bytes(bytes)
}

fn check_path_syntax(path: &OsStr, flavor: PathFlavor) -> Result<(), Reason> {
    match flavor {
        PathFlavor::Posix => check_posix_path(path.as_encoded_bytes()),
        PathFlavor::Windows => check_windows_path(path.to_str().ok_or(Reason::NonUtf8Path)?),
    }
}

fn check_posix_path(path: &[u8]) -> Result<(), Reason> {
    if path.contains(&0) {
        return Err(Reason::NulInPath);
    }
    if !path.starts_with(b"/") {
        return Err(Reason::RelativePath);
    }
    if path.starts_with(b"//") {
        return Err(Reason::NetworkPath);
    }
    if path == b"/dev" || path.starts_with(b"/dev/") {
        return Err(Reason::DevicePath);
    }
    check_extension(last_component(path, b"/"))
}

fn check_windows_path(path: &str) -> Result<(), Reason> {
    if path.contains('\0') {
        return Err(Reason::NulInPath);
    }
    // Windows treats `/` and `\` alike in these prefixes.
    let normalized = path.replace('/', "\\");
    if [r"\\?\", r"\\.\", r"\??\"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return Err(Reason::DevicePath);
    }
    if normalized.starts_with(r"\\") {
        return Err(Reason::NetworkPath);
    }
    let bytes = path.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return Err(Reason::RelativePath);
    }
    if bytes.get(2) != Some(&b'\\') {
        return Err(Reason::DriveRelativePath);
    }
    if path[2..].contains(':') {
        return Err(Reason::AlternateDataStream);
    }
    // Win32 drops trailing dots and spaces from the last component, so `list.m3u. ` opens
    // `list.m3u`: judge the name Windows will open. The original path is still the target.
    let name = last_component(bytes, b"/\\");
    let kept = name
        .iter()
        .rposition(|byte| !matches!(byte, b'.' | b' '))
        .map_or(0, |last| last + 1);
    check_extension(&name[..kept])
}

fn last_component<'a>(path: &'a [u8], separators: &[u8]) -> &'a [u8] {
    path.rsplit(|byte| separators.contains(byte))
        .next()
        .unwrap_or(path)
}

fn check_extension(file_name: &[u8]) -> Result<(), Reason> {
    let Some(dot) = file_name.iter().rposition(|byte| *byte == b'.') else {
        return Ok(());
    };
    if dot == 0 {
        // `.m3u` is a hidden file without an extension, as `Path::extension` reads it.
        return Ok(());
    }
    let extension = &file_name[dot..];
    let matches = |known: &&str| extension.eq_ignore_ascii_case(known.as_bytes());
    if PLAYLIST_EXTENSIONS.iter().any(matches) {
        return Err(Reason::PlaylistTarget);
    }
    if SUBTITLE_EXTENSIONS.iter().any(matches) {
        return Err(Reason::SubtitleTarget);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_need_exactly_one_leading_slash() {
        assert_eq!(check_posix_path(b"/media/a.mkv"), Ok(()));
        assert_eq!(check_posix_path(b"media/a.mkv"), Err(Reason::RelativePath));
        assert_eq!(check_posix_path(b"//nas/a.mkv"), Err(Reason::NetworkPath));
        assert_eq!(check_posix_path(br"\\nas\a.mkv"), Err(Reason::RelativePath));
    }

    #[test]
    fn device_paths_reject_on_posix_but_lookalikes_do_not() {
        assert_eq!(check_posix_path(b"/dev/sda"), Err(Reason::DevicePath));
        assert_eq!(check_posix_path(b"/dev"), Err(Reason::DevicePath));
        assert_eq!(check_posix_path(b"/devices/a.mkv"), Ok(()));
    }

    #[test]
    fn windows_paths_must_be_drive_absolute_with_a_backslash() {
        assert_eq!(check_windows_path(r"C:\media\a.mkv"), Ok(()));
        assert_eq!(check_windows_path(r"z:\a b\c.MP4"), Ok(()));
        assert_eq!(
            check_windows_path("C:/media/a.mkv"),
            Err(Reason::DriveRelativePath)
        );
        assert_eq!(
            check_windows_path("C:a.mkv"),
            Err(Reason::DriveRelativePath)
        );
        assert_eq!(
            check_windows_path(r"\media\a.mkv"),
            Err(Reason::RelativePath)
        );
        assert_eq!(check_windows_path("a.mkv"), Err(Reason::RelativePath));
        assert_eq!(check_windows_path("C:"), Err(Reason::DriveRelativePath));
    }

    #[test]
    fn windows_network_and_device_prefixes_reject_with_either_slash() {
        for path in [r"\\nas\share\a.mkv", "//nas/share/a.mkv", r"/\nas\a.mkv"] {
            assert_eq!(check_windows_path(path), Err(Reason::NetworkPath), "{path}");
        }
        for path in [
            r"\\?\C:\a.mkv",
            r"\\?\UNC\nas\share\a.mkv",
            r"\\.\PhysicalDrive0",
            "//?/C:/a.mkv",
            r"\??\C:\a.mkv",
        ] {
            assert_eq!(check_windows_path(path), Err(Reason::DevicePath), "{path}");
        }
    }

    #[test]
    fn windows_streams_reject_after_the_drive_colon() {
        assert_eq!(
            check_windows_path(r"C:\media\a.mkv:hidden"),
            Err(Reason::AlternateDataStream)
        );
        assert_eq!(
            check_windows_path(r"C:\media\a.mkv::$DATA"),
            Err(Reason::AlternateDataStream)
        );
    }

    #[test]
    fn playlists_and_subtitles_reject_case_insensitively_on_the_last_component() {
        assert_eq!(check_posix_path(b"/m/a.M3U8"), Err(Reason::PlaylistTarget));
        assert_eq!(check_posix_path(b"/m/a.edl"), Err(Reason::PlaylistTarget));
        assert_eq!(check_posix_path(b"/m/a.Srt"), Err(Reason::SubtitleTarget));
        assert_eq!(
            check_windows_path(r"C:\m\a.cue"),
            Err(Reason::PlaylistTarget)
        );
        // A directory that merely looks like a playlist does not count, nor does a hidden
        // file without an extension.
        assert_eq!(check_posix_path(b"/m/list.m3u/a.mkv"), Ok(()));
        assert_eq!(check_posix_path(b"/m/.m3u"), Ok(()));
        assert_eq!(check_windows_path(r"C:\m.pls\a.mkv"), Ok(()));
    }

    #[test]
    fn windows_judges_the_last_component_without_trailing_dots_and_spaces() {
        for path in [r"C:\m\list.m3u.", r"C:\m\list.m3u ", r"C:\m\list.M3U8. . "] {
            assert_eq!(
                check_windows_path(path),
                Err(Reason::PlaylistTarget),
                "{path}"
            );
        }
        assert_eq!(
            check_windows_path(r"C:\m\subs.srt."),
            Err(Reason::SubtitleTarget)
        );
        assert_eq!(check_windows_path(r"C:\m\clip.mkv."), Ok(()));
        assert_eq!(check_windows_path(r"C:\m\.m3u."), Ok(()));
        assert_eq!(check_windows_path(r"C:\m\..."), Ok(()));
        // POSIX names are exact: a trailing dot is part of the name.
        assert_eq!(check_posix_path(b"/m/list.m3u."), Ok(()));
    }

    #[test]
    fn urls_naming_a_playlist_or_subtitle_reject_by_their_last_segment() {
        for url in [
            "https://cam.example/x.m3u8",
            "https://cam.example/x.M3U?token=1",
            "http://cam.example/x.edl#t=5",
            "https://cam.example/list.m3%75",
            "https://cam.example/%zz.m3%75",
        ] {
            assert!(
                matches!(
                    url_target(url, PathFlavor::Posix),
                    Err(Reason::PlaylistTarget)
                ),
                "{url}"
            );
        }
        assert!(matches!(
            url_target("https://cam.example/subs.srt", PathFlavor::Posix),
            Err(Reason::SubtitleTarget)
        ));
        for url in [
            "https://cam.example/play?f=list.m3u",
            "https://cam.example/list.m3u/clip.mp4",
            "https://cam.example/list.m3u%2",
            "https://cam.example",
        ] {
            assert!(
                matches!(url_target(url, PathFlavor::Posix), Ok(UrlTarget::Remote)),
                "{url}"
            );
        }
    }

    #[test]
    fn file_urls_convert_only_when_local() {
        assert_eq!(
            file_url_path("///media/a%20b.mkv", PathFlavor::Posix),
            Ok(OsString::from("/media/a b.mkv"))
        );
        assert_eq!(
            file_url_path("//LocalHost/media/a.mkv", PathFlavor::Posix),
            Ok(OsString::from("/media/a.mkv"))
        );
        assert_eq!(
            file_url_path("///C:/media/a.mkv", PathFlavor::Windows),
            Ok(OsString::from(r"C:\media\a.mkv"))
        );
        assert_eq!(
            file_url_path("//nas/share/a.mkv", PathFlavor::Posix),
            Err(Reason::RemoteFileUrl)
        );
        for rest in ["/media/a.mkv", "///a.mkv?x", "///a.mkv#x", "//localhost"] {
            assert_eq!(
                file_url_path(rest, PathFlavor::Posix),
                Err(Reason::InvalidUrl),
                "{rest}"
            );
        }
        assert_eq!(
            file_url_path("///a%G1.mkv", PathFlavor::Posix),
            Err(Reason::BadPercentEncoding)
        );
    }

    #[test]
    fn http_urls_need_a_host_and_no_credentials() {
        for url in [
            "https://cam.example/a.mp4",
            "http://192.0.2.10:8080/clips/a.MOV",
            "http://[2001:db8::1]:8080/a.mp4",
            "HTTPS://cam.example/a.mp4#t=5",
        ] {
            assert!(
                matches!(url_target(url, PathFlavor::Posix), Ok(UrlTarget::Remote)),
                "{url}"
            );
        }
        for url in [
            "https:///a.mp4",
            "https://:8080/a.mp4",
            "https://user@cam.example/a.mp4",
            r"https://cam.example\a.mp4",
            "https:cam.example/a.mp4",
            "cam.example/a.mp4",
            "1http://cam.example/a.mp4",
            "https://cam.example/a\u{1}.mp4",
        ] {
            assert!(
                matches!(url_target(url, PathFlavor::Posix), Err(Reason::InvalidUrl)),
                "{url}"
            );
        }
    }
}
