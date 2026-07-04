//! The reserved `ok-player://` external-control scheme (PRD §13.4). OK Player registers
//! the scheme now so packaging and desktop integration can advertise it cleanly, but
//! programmatic control *through* it is a **[Later]** seam: MVP launch-with-resume uses
//! process invocation / CLI args (see [`crate::launch_args`]). So a shell must never hand
//! an `ok-player://` token to the media engine as if it were a stream URL — it parses the
//! request with [`interpret`] and reports it back instead. Every well-formed request is
//! recognized and reserved (never silently executed), and anything malformed is rejected
//! outright, so the scheme cannot smuggle an unsupported command into playback.
//!
//! Pure and engine-agnostic; a shell calls [`interpret`] on a command-line / open-URI
//! token *before* treating it as a media path or URL, and surfaces the outcome as a local
//! diagnostic.

/// The reserved scheme name, without the `:` or `//` (e.g. for building an
/// `x-scheme-handler/ok-player` MIME type or an `ok-player://…` URI).
pub const SCHEME: &str = "ok-player";

/// How OK Player interprets an `ok-player://` request today. External programmatic control
/// is reserved for a future release, so no variant executes a command — the distinction is
/// only how the request is reported back to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A syntactically valid request. `command` is the parsed verb (the URI authority,
    /// ASCII-lowercased) — e.g. `open` for `ok-player://open?path=…`. Reserved for a future
    /// release: parsed and reported, never executed today.
    Reserved { command: String },
    /// The `ok-player` scheme is present but the request is malformed — it names no command,
    /// or carries whitespace / control characters that a safe URI never contains. Rejected
    /// outright.
    Malformed,
}

/// Classify a command-line / open-URI token against the reserved scheme.
///
/// Returns `None` when the token does **not** use the `ok-player` scheme — the caller then
/// handles it as a normal path or media URL, so existing file/URL open behavior is
/// unchanged. Returns `Some(Request)` when the token *is* an `ok-player` request; the
/// caller must then report it (never play it), because no command is executable yet.
///
/// Both the authority form (`ok-player://open`) and the opaque form (`ok-player:open`) are
/// recognized, because the desktop `x-scheme-handler/ok-player` association fires on the
/// scheme regardless of the `//`. The scheme name is matched case-insensitively (RFC 3986)
/// and the command is ASCII-lowercased so classification is deterministic.
pub fn interpret(text: &str) -> Option<Request> {
    let rest = strip_scheme(text.trim())?;

    // A URI we would ever act on has no raw whitespace or control characters; their
    // presence means the token is malformed (or an attempt to smuggle something past a
    // naive handler), so reject it rather than parse a command out of it.
    if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Some(Request::Malformed);
    }

    let command = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if command.is_empty() {
        return Some(Request::Malformed);
    }

    Some(Request::Reserved { command })
}

/// The remainder after an `ok-player` scheme prefix (with the optional `//` authority
/// marker peeled), or `None` when `text` uses a different scheme or none at all.
fn strip_scheme(text: &str) -> Option<&str> {
    let colon = text.find(':')?;
    if !text[..colon].eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let after_colon = &text[colon + 1..];
    Some(after_colon.strip_prefix("//").unwrap_or(after_colon))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reserved(command: &str) -> Option<Request> {
        Some(Request::Reserved {
            command: command.to_owned(),
        })
    }

    #[test]
    fn non_scheme_tokens_are_not_ours() {
        for text in [
            "https://example.com/video.mkv",
            "file:///tmp/movie.mkv",
            "/media/movie.mkv",
            "movie.mkv",
            "smb://nas/share/clip.mkv",
            "",
            "://open",
            "okplayer://open", // near-miss scheme name must not match
        ] {
            assert_eq!(interpret(text), None, "{text}");
        }
    }

    #[test]
    fn well_formed_requests_are_reserved_and_never_executed() {
        assert_eq!(interpret("ok-player://open"), reserved("open"));
        // Query and fragment are stripped down to the command verb.
        assert_eq!(
            interpret("ok-player://open?path=/media/a.mkv&resume=90"),
            reserved("open")
        );
        assert_eq!(interpret("ok-player://play/now"), reserved("play"));
        // The opaque form (no `//`) is the same scheme and is recognized too.
        assert_eq!(interpret("ok-player:enqueue"), reserved("enqueue"));
    }

    #[test]
    fn scheme_is_case_insensitive_and_command_is_normalized() {
        assert_eq!(interpret("OK-Player://Open"), reserved("open"));
        assert_eq!(interpret("  ok-player://PAUSE  "), reserved("pause"));
    }

    #[test]
    fn malformed_requests_are_rejected_outright() {
        for text in [
            "ok-player://",           // no command
            "ok-player:",             // opaque form, no command
            "ok-player:///play",      // empty authority
            "ok-player://open now",   // raw whitespace
            "ok-player://open\u{7}x", // control character
        ] {
            assert_eq!(interpret(text), Some(Request::Malformed), "{text}");
        }
    }
}
