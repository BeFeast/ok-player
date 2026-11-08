//! The `ok-player://` link scheme, v1 (PRD §13.4, epic #845).
//!
//! A page the user writes — a local `file://` HTML page or a LAN web app — launches OK
//! Player with a link such as `ok-player://open?url=https%3A%2F%2Fcam.lan%2Fclip.MP4&t=90`.
//! Everything that decides what such a link means lives here, so a shell only executes a
//! typed result and never parses or judges a link itself (freeze boundary):
//!
//! 1. [`launch_shape`] — an argv that carries a link must carry nothing else.
//! 2. [`interpret`] — grammar and a strict, single percent-decode into a typed [`Request`].
//!    `None` means the text is not ours and the caller opens it as a normal path or URL.
//! 3. [`evaluate`] — the target policy: `http`/`https` URLs only, absolute local paths that
//!    name an existing regular file, no playlists or subtitles. Syntactic checks finish
//!    before the [`FsProbe`] is asked anything.
//! 4. [`plan`] — what to do with a [`Validated`] request given what is playing now.
//!
//! The grammar (normative copy in #845):
//!
//! ```text
//! ok-player://<action>?<target>[&t=<time>]
//!
//! action = open | add | next          ; ASCII, case-insensitive
//! target = url=<pct(absolute http(s) URL)>
//!        | path=<pct(absolute local path)>    ; exactly one target
//! t      = seconds (90, 90.5) | m:ss | h:mm:ss  ; alias: resume
//! ```
//!
//! plus the path form `ok-player://<action>/<absolute-path>` (segments percent-encoded, `+`
//! literal) and the opaque form `ok-player:<action>?…`. The shared cases in
//! `tests/fixtures/ok_player_uri/cases.json` pin every rule for this crate and for the later
//! Windows port.
//!
//! Nothing from a link ever becomes an engine option: a request carries one target, an
//! optional start offset, and the names of the keys that were ignored (for the log).

mod parse;
mod plan;
pub mod policy;
mod probe;

use std::ffi::{OsStr, OsString};

pub use parse::interpret;
pub use plan::{
    LaunchShape, Plan, PlaybackSummary, QueueMode, launch_shape, plan, presents_window,
};
pub use policy::{PathFlavor, Validated, evaluate};
pub use probe::{FsKind, FsProbe, StdFsProbe};

/// The scheme name, without the `:` or `//` (e.g. for building an
/// `x-scheme-handler/ok-player` MIME type or an `ok-player://…` URI).
pub const SCHEME: &str = "ok-player";

/// The largest link accepted, in bytes of the raw (still percent-encoded) text.
pub const MAX_LINK_BYTES: usize = 4096;

/// At most this many ignored key names are kept for the log line; further unknown keys are
/// still ignored, just not named.
pub const MAX_IGNORED_KEYS: usize = 16;

/// A name the log line repeats from a link (an ignored key, an unsupported action) is cut to
/// this many characters, after escaping.
pub const MAX_LOGGED_NAME_CHARS: usize = 64;

/// What a link asks the player to do with its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Play the target now, replacing whatever plays.
    Open,
    /// Append the target to Up Next (plays it now when nothing is loaded).
    Add,
    /// Insert the target right after the current item (plays it now when nothing is loaded).
    Next,
}

impl Action {
    /// The lowercase action name as it appears in a link.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Add => "add",
            Self::Next => "next",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        [Self::Open, Self::Add, Self::Next]
            .into_iter()
            .find(|action| name.eq_ignore_ascii_case(action.as_str()))
    }
}

/// The one thing a link points at.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A local path. POSIX paths keep their raw bytes, so non-UTF-8 filenames work.
    Path(OsString),
    /// An absolute URL, exactly as it will reach the engine.
    Url(String),
}

/// A syntactically valid request for one of the v1 actions. Not yet judged: hand it to
/// [`evaluate`] before acting on it.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionRequest {
    pub action: Action,
    pub target: Target,
    /// The start offset in seconds from `t` (or its alias `resume`).
    pub start: Option<f64>,
    /// `t` was present but malformed, so it was dropped; the shell shows a notice.
    pub start_dropped: bool,
    /// Keys the link carried that v1 does not know, in link order. Logged, never forwarded.
    /// Each name is made safe for a log line: everything but printable ASCII is escaped
    /// (`\n`, `\u{e9}`), it is cut to [`MAX_LOGGED_NAME_CHARS`], and at most
    /// [`MAX_IGNORED_KEYS`] names are kept.
    pub ignored_keys: Vec<String>,
}

/// How a token that uses the `ok-player` scheme was read.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// A well-formed request for a v1 action.
    Action(ActionRequest),
    /// A well-formed authority naming an action v1 does not have (`pause`, `play`, …). The
    /// rest of the link is not read. `action` is the lowercased name, made safe for a log line
    /// like an ignored key; a shell reports [`Reason::UnsupportedAction`] and never shows it.
    Unsupported { action: String },
    /// The link is ours but breaks the grammar. `action` is the v1 action when the link got
    /// far enough to name one, so the shell can raise (or not) exactly as it would for an
    /// accepted request with that action.
    Malformed {
        action: Option<Action>,
        reason: Reason,
    },
}

impl Request {
    /// The v1 action the link names, when it got far enough to name one.
    pub fn action(&self) -> Option<Action> {
        match self {
            Self::Action(request) => Some(request.action),
            Self::Unsupported { .. } => None,
            Self::Malformed { action, .. } => *action,
        }
    }

    /// Why the link is rejected before its target is judged; `None` for a well-formed v1
    /// request, which still has to pass [`evaluate`]. The toast is the reason's fixed
    /// [`Reason::message`], so nothing from the link reaches the screen.
    pub fn rejection(&self) -> Option<Reason> {
        match self {
            Self::Action(_) => None,
            Self::Unsupported { .. } => Some(Reason::UnsupportedAction),
            Self::Malformed { reason, .. } => Some(*reason),
        }
    }
}

/// Why a link (or a launch carrying one) was rejected. [`Reason::as_str`] is the stable name
/// used by the shared fixture and the log line; [`Reason::message`] is the fixed toast text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    /// The raw link is longer than [`MAX_LINK_BYTES`].
    TooLong,
    /// Raw whitespace or a control character, which no valid link contains.
    InvalidCharacter,
    /// A non-empty fragment (an unencoded `#`).
    Fragment,
    /// The authority carries userinfo (`user@`).
    Userinfo,
    /// The authority carries a port (`:`).
    Port,
    /// No action at all.
    MissingAction,
    /// The action contains characters an action name cannot contain.
    InvalidAction,
    /// A well-formed action name that v1 does not have (`pause`, `play`, …).
    UnsupportedAction,
    /// A `%` not followed by two hex digits.
    BadPercentEncoding,
    /// A URL nested in the path form (`ok-player://open/https://…`).
    NestedUrl,
    /// A query key appears more than once (`t` and `resume` count as one key).
    DuplicateKey,
    /// More than one target (`url`, `path`, or the path form).
    MultipleTargets,
    /// No target, or an empty one.
    MissingTarget,
    /// `url=` does not decode to UTF-8.
    NonUtf8Url,
    /// A path that is not UTF-8 where the platform needs it (Windows).
    NonUtf8Path,
    /// A NUL byte in a path.
    NulInPath,
    /// A path that is not absolute in the form v1 accepts.
    RelativePath,
    /// A network path (`//host/…`, `\\host\…`).
    NetworkPath,
    /// A device path (`/dev/…`, `\\?\…`, `\\.\…`).
    DevicePath,
    /// A Windows drive path without a backslash after the colon (`C:foo`, `C:/foo`).
    DriveRelativePath,
    /// A Windows alternate data stream (`a.mkv:stream`).
    AlternateDataStream,
    /// A playlist file (`.m3u .m3u8 .pls .cue .edl`).
    PlaylistTarget,
    /// A subtitle file.
    SubtitleTarget,
    /// A local file whose name has no known media extension (the list the file chooser and
    /// Up Next use), so the player would refuse to open or queue it.
    NotMediaFile,
    /// Nothing exists at the path.
    NotFound,
    /// The path names a directory.
    Directory,
    /// The path names something other than a regular file (FIFO, socket, device).
    NotRegularFile,
    /// The path could not be checked (timeout, permissions, I/O error).
    PathUnavailable,
    /// A URL scheme other than `http`, `https`, or a local `file` URL.
    UnsupportedScheme,
    /// A URL that is not an absolute, credential-free URL with a host.
    InvalidUrl,
    /// A `file:` URL that names another host.
    RemoteFileUrl,
    /// A launch carried a link together with other arguments.
    LaunchShape,
}

impl Reason {
    /// Every reason, in declaration order.
    pub const ALL: &[Reason] = &[
        Self::TooLong,
        Self::InvalidCharacter,
        Self::Fragment,
        Self::Userinfo,
        Self::Port,
        Self::MissingAction,
        Self::InvalidAction,
        Self::UnsupportedAction,
        Self::BadPercentEncoding,
        Self::NestedUrl,
        Self::DuplicateKey,
        Self::MultipleTargets,
        Self::MissingTarget,
        Self::NonUtf8Url,
        Self::NonUtf8Path,
        Self::NulInPath,
        Self::RelativePath,
        Self::NetworkPath,
        Self::DevicePath,
        Self::DriveRelativePath,
        Self::AlternateDataStream,
        Self::PlaylistTarget,
        Self::SubtitleTarget,
        Self::NotMediaFile,
        Self::NotFound,
        Self::Directory,
        Self::NotRegularFile,
        Self::PathUnavailable,
        Self::UnsupportedScheme,
        Self::InvalidUrl,
        Self::RemoteFileUrl,
        Self::LaunchShape,
    ];

    /// The stable snake_case name of the reason.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooLong => "too_long",
            Self::InvalidCharacter => "invalid_character",
            Self::Fragment => "fragment",
            Self::Userinfo => "userinfo",
            Self::Port => "port",
            Self::MissingAction => "missing_action",
            Self::InvalidAction => "invalid_action",
            Self::UnsupportedAction => "unsupported_action",
            Self::BadPercentEncoding => "bad_percent_encoding",
            Self::NestedUrl => "nested_url",
            Self::DuplicateKey => "duplicate_key",
            Self::MultipleTargets => "multiple_targets",
            Self::MissingTarget => "missing_target",
            Self::NonUtf8Url => "non_utf8_url",
            Self::NonUtf8Path => "non_utf8_path",
            Self::NulInPath => "nul_in_path",
            Self::RelativePath => "relative_path",
            Self::NetworkPath => "network_path",
            Self::DevicePath => "device_path",
            Self::DriveRelativePath => "drive_relative_path",
            Self::AlternateDataStream => "alternate_data_stream",
            Self::PlaylistTarget => "playlist_target",
            Self::SubtitleTarget => "subtitle_target",
            Self::NotMediaFile => "not_media_file",
            Self::NotFound => "not_found",
            Self::Directory => "directory",
            Self::NotRegularFile => "not_regular_file",
            Self::PathUnavailable => "path_unavailable",
            Self::UnsupportedScheme => "unsupported_scheme",
            Self::InvalidUrl => "invalid_url",
            Self::RemoteFileUrl => "remote_file_url",
            Self::LaunchShape => "launch_shape",
        }
    }

    /// Fixed toast text for the rejection. It never contains anything from the link, so a
    /// path cannot leak into the toast or into Copy Diagnostics.
    pub const fn message(self) -> &'static str {
        match self {
            Self::TooLong => "Link ignored: it is too long",
            Self::InvalidCharacter => "Link ignored: it contains characters a link cannot have",
            Self::Fragment => "Link ignored: it has a # fragment",
            Self::Userinfo | Self::Port => "Link ignored: the action is malformed",
            Self::MissingAction | Self::InvalidAction => "Link ignored: it names no valid action",
            Self::UnsupportedAction => "Link ignored: OK Player does not support this action",
            Self::BadPercentEncoding => "Link ignored: malformed % encoding",
            Self::NestedUrl => "Link ignored: a URL must be passed as url=",
            Self::DuplicateKey => "Link ignored: a parameter appears twice",
            Self::MultipleTargets => "Link ignored: it names more than one target",
            Self::MissingTarget => "Link ignored: it names no target",
            Self::NonUtf8Url | Self::InvalidUrl => "Link ignored: the URL is not valid",
            Self::NonUtf8Path
            | Self::NulInPath
            | Self::RelativePath
            | Self::DriveRelativePath
            | Self::AlternateDataStream => "Link ignored: the path is not a valid absolute path",
            Self::NetworkPath => "Link ignored: network paths are not supported",
            Self::DevicePath => "Link ignored: device paths are not supported",
            Self::PlaylistTarget => "Link ignored: playlists cannot be opened from a link",
            Self::SubtitleTarget => "Link ignored: subtitle files cannot be opened from a link",
            Self::NotMediaFile => "Link ignored: the file is not a media file",
            Self::NotFound => "Link ignored: the file does not exist",
            Self::Directory => "Link ignored: folders cannot be opened from a link",
            Self::NotRegularFile => "Link ignored: the path is not a regular file",
            Self::PathUnavailable => "Link ignored: the file could not be checked",
            Self::UnsupportedScheme => "Link ignored: only http and https URLs are supported",
            Self::RemoteFileUrl => "Link ignored: file URLs must be local",
            Self::LaunchShape => "Link ignored: it was launched together with other arguments",
        }
    }
}

/// True when `token` uses the `ok-player` scheme (case-insensitively, surrounding whitespace
/// ignored). Works on raw OS arguments, so a non-UTF-8 token is still recognized as a link
/// and never mistaken for a path.
pub fn is_link(token: &OsStr) -> bool {
    if let Some(text) = token.to_str() {
        return parse::strip_scheme(text.trim()).is_some();
    }
    let bytes = token.as_encoded_bytes().trim_ascii();
    let prefix = SCHEME.len() + 1;
    bytes.len() >= prefix
        && bytes[..SCHEME.len()].eq_ignore_ascii_case(SCHEME.as_bytes())
        && bytes[SCHEME.len()] == b':'
}

/// True when `url` is a direct media URL: its last path segment, decoded as the server reads
/// it, has a known media extension (`https://cam.lan/clips/a.MP4?token=1`). The one shared
/// definition lives in [`crate::media_formats::is_direct_media_url`].
///
/// Such a URL opened or queued from a link never arms the replay cache; a page URL
/// (`https://youtu.be/…`) keeps the normal behaviour.
pub use crate::media_formats::is_direct_media_url;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_link_recognizes_the_scheme_in_any_case_and_only_the_scheme() {
        for token in [
            "ok-player://open?url=x",
            "OK-Player:open",
            "  ok-player://next  ",
            "ok-player:",
        ] {
            assert!(is_link(OsStr::new(token)), "{token}");
        }
        for token in [
            "okplayer://open",
            "ok-player",
            "/media/ok-player:clip.mkv",
            "https://example.test/ok-player://x",
            "",
        ] {
            assert!(!is_link(OsStr::new(token)), "{token}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn is_link_recognizes_a_non_utf8_link_token() {
        use std::os::unix::ffi::OsStrExt;

        assert!(is_link(OsStr::from_bytes(
            b"ok-player://open/media/\xff.mkv"
        )));
        assert!(!is_link(OsStr::from_bytes(b"/media/\xff.mkv")));
    }

    #[test]
    fn every_reason_has_a_distinct_name_and_link_free_message() {
        // Adding a variant breaks this match until it is also listed in `Reason::ALL`.
        let listed = |reason: Reason| match reason {
            Reason::TooLong
            | Reason::InvalidCharacter
            | Reason::Fragment
            | Reason::Userinfo
            | Reason::Port
            | Reason::MissingAction
            | Reason::InvalidAction
            | Reason::UnsupportedAction
            | Reason::BadPercentEncoding
            | Reason::NestedUrl
            | Reason::DuplicateKey
            | Reason::MultipleTargets
            | Reason::MissingTarget
            | Reason::NonUtf8Url
            | Reason::NonUtf8Path
            | Reason::NulInPath
            | Reason::RelativePath
            | Reason::NetworkPath
            | Reason::DevicePath
            | Reason::DriveRelativePath
            | Reason::AlternateDataStream
            | Reason::PlaylistTarget
            | Reason::SubtitleTarget
            | Reason::NotMediaFile
            | Reason::NotFound
            | Reason::Directory
            | Reason::NotRegularFile
            | Reason::PathUnavailable
            | Reason::UnsupportedScheme
            | Reason::InvalidUrl
            | Reason::RemoteFileUrl
            | Reason::LaunchShape => Reason::ALL.contains(&reason),
        };
        let mut names = std::collections::HashSet::new();
        for &reason in Reason::ALL {
            assert!(listed(reason), "{reason:?}");
            assert!(names.insert(reason.as_str()), "{reason:?} name repeats");
            assert!(reason.message().starts_with("Link ignored: "), "{reason:?}");
        }
        assert_eq!(names.len(), 32);
    }
}
