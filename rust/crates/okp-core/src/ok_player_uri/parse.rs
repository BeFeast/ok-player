//! Grammar and strict decoding: link text to [`Request`]. No filesystem access and no
//! judgement of the target beyond its syntax; that is [`super::policy`].

use std::ffi::OsString;

use super::{
    Action, ActionRequest, MAX_IGNORED_KEYS, MAX_LINK_BYTES, MAX_LOGGED_NAME_CHARS, Reason,
    Request, SCHEME, Target,
};
use crate::time_code;

/// How a literal `+` decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plus {
    /// `+` stays `+` (path segments, `file:` URL paths).
    Literal,
    /// `+` is a space (query keys and values, like `URLSearchParams`).
    Space,
}

/// Percent-decode `raw` exactly once, strictly: a `%` that is not followed by two hex digits
/// fails the whole value. `%252F` decodes to the literal text `%2F`, never to `/`.
pub(super) fn percent_decode(raw: &str, plus: Plus) -> Option<Vec<u8>> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let high = hex_value(*bytes.get(index + 1)?)?;
                let low = hex_value(*bytes.get(index + 2)?)?;
                decoded.push(high << 4 | low);
                index += 3;
            }
            b'+' if plus == Plus::Space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decoded bytes as an OS path. POSIX keeps raw bytes; elsewhere a path must be UTF-8.
pub(super) fn os_string_from_bytes(bytes: Vec<u8>) -> Result<OsString, Reason> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(bytes))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes)
            .map(OsString::from)
            .map_err(|_| Reason::NonUtf8Path)
    }
}

/// The remainder after an `ok-player:` scheme prefix, or `None` when `text` uses another
/// scheme or none. The scheme name matches case-insensitively (RFC 3986).
pub(super) fn strip_scheme(text: &str) -> Option<&str> {
    let colon = text.find(':')?;
    text[..colon]
        .eq_ignore_ascii_case(SCHEME)
        .then(|| &text[colon + 1..])
}

/// Classify a command-line or open-URI token against the `ok-player` scheme.
///
/// Returns `None` when the token does **not** use the scheme: the caller handles it as a
/// normal path or media URL, so existing open behaviour is unchanged. Returns `Some` for
/// every token that does use it, and the caller must then never hand the token to the
/// engine: only a [`Request::Action`] that passes [`super::evaluate`] leads to playback.
///
/// The checks run in a fixed order, so one link always yields the same reason (the shared
/// fixture depends on it): length, raw characters, fragment, authority (userinfo, port,
/// action), path form, then the query — decode every key and every value but `t`'s, then
/// duplicate keys, then targets. A `t` (or `resume`) value that does not decode is dropped
/// with a notice like any other malformed `t` (rule 7), not rejected.
pub fn interpret(text: &str) -> Option<Request> {
    let token = text.trim();
    let rest = strip_scheme(token)?;
    Some(
        parse(token, rest).unwrap_or_else(|(action, reason)| Request::Malformed { action, reason }),
    )
}

type Failure = (Option<Action>, Reason);

fn parse(token: &str, rest: &str) -> Result<Request, Failure> {
    if token.len() > MAX_LINK_BYTES {
        return Err((None, Reason::TooLong));
    }
    // A link never contains raw whitespace or control characters; their presence means the
    // token is malformed or is trying to smuggle something past a naive handler.
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err((None, Reason::InvalidCharacter));
    }

    // The authority form and the opaque form read the same after the optional `//`.
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    let rest = match rest.split_once('#') {
        Some((_, fragment)) if !fragment.is_empty() => return Err((None, Reason::Fragment)),
        Some((before, _)) => before,
        None => rest,
    };
    let (hier, query) = match rest.split_once('?') {
        Some((hier, query)) => (hier, query),
        None => (rest, ""),
    };
    let (authority, path) = hier.split_at(hier.find('/').unwrap_or(hier.len()));

    if authority.contains('@') {
        return Err((None, Reason::Userinfo));
    }
    if authority.contains(':') {
        return Err((None, Reason::Port));
    }
    if authority.is_empty() {
        return Err((None, Reason::MissingAction));
    }
    if !authority
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err((None, Reason::InvalidAction));
    }
    let Some(action) = Action::from_name(authority) else {
        return Ok(Request::Unsupported {
            action: log_safe_name(authority.to_ascii_lowercase().as_bytes()),
        });
    };
    let fail = |reason| (Some(action), reason);

    // Path form: `ok-player://open/<absolute path>`. An empty path or exactly `/` is not a
    // target, so `ok-player://open/?url=…` reads like `ok-player://open?url=…`.
    let path_target = if path.is_empty() || path == "/" {
        None
    } else {
        let bytes =
            percent_decode(path, Plus::Literal).ok_or_else(|| fail(Reason::BadPercentEncoding))?;
        if starts_with_nested_url(&bytes) {
            return Err(fail(Reason::NestedUrl));
        }
        Some(bytes)
    };

    // `None` as a value: a `t` value that did not decode, dropped below with a notice. Every
    // other key and value decodes strictly or rejects the link.
    let pairs = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let key = percent_decode(key, Plus::Space)?;
            let value = percent_decode(value, Plus::Space);
            if value.is_none() && canonical_key(&key) != b"t" {
                return None;
            }
            Some((key, value))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| fail(Reason::BadPercentEncoding))?;

    let mut seen: Vec<&[u8]> = Vec::with_capacity(pairs.len());
    for (key, _) in &pairs {
        let key = canonical_key(key);
        if seen.contains(&key) {
            return Err(fail(Reason::DuplicateKey));
        }
        seen.push(key);
    }

    let mut targets = path_target
        .map(RawTarget::Path)
        .into_iter()
        .collect::<Vec<_>>();
    let mut start = None;
    let mut start_dropped = false;
    let mut ignored_keys = Vec::new();
    for (key, value) in pairs {
        match (canonical_key(&key), value) {
            (b"url", Some(value)) => targets.push(RawTarget::Url(value)),
            (b"path", Some(value)) => targets.push(RawTarget::Path(value)),
            (b"t", value) => {
                // A malformed optional parameter is dropped with a notice, never fatal.
                start = value
                    .as_deref()
                    .and_then(|value| std::str::from_utf8(value).ok())
                    .and_then(|value| time_code::parse(Some(value)));
                start_dropped = start.is_none();
            }
            _ if ignored_keys.len() < MAX_IGNORED_KEYS => ignored_keys.push(log_safe_name(&key)),
            _ => {}
        }
    }

    if targets.len() > 1 {
        return Err(fail(Reason::MultipleTargets));
    }
    let Some(target) = targets.pop() else {
        return Err(fail(Reason::MissingTarget));
    };
    let target = match target {
        RawTarget::Url(value) | RawTarget::Path(value) if value.is_empty() => {
            return Err(fail(Reason::MissingTarget));
        }
        RawTarget::Url(value) => {
            Target::Url(String::from_utf8(value).map_err(|_| fail(Reason::NonUtf8Url))?)
        }
        RawTarget::Path(value) => Target::Path(os_string_from_bytes(value).map_err(fail)?),
    };

    Ok(Request::Action(ActionRequest {
        action,
        target,
        start,
        start_dropped,
        ignored_keys,
    }))
}

/// A name from a link, made safe to repeat in a log line: printable ASCII stays, everything
/// else (control characters, bidi overrides, any non-ASCII) is escaped like
/// [`char::escape_default`], bytes that are not UTF-8 become U+FFFD first, and the result is
/// cut to [`MAX_LOGGED_NAME_CHARS`] characters with a trailing `...`.
fn log_safe_name(name: &[u8]) -> String {
    let mut escaped = String::with_capacity(name.len());
    for c in String::from_utf8_lossy(name).chars() {
        if c.is_ascii_graphic() || c == ' ' {
            escaped.push(c);
        } else {
            escaped.extend(c.escape_default());
        }
    }
    if escaped.chars().count() <= MAX_LOGGED_NAME_CHARS {
        return escaped;
    }
    let mut cut = escaped
        .chars()
        .take(MAX_LOGGED_NAME_CHARS - 3)
        .collect::<String>();
    cut.push_str("...");
    cut
}

enum RawTarget {
    Url(Vec<u8>),
    Path(Vec<u8>),
}

/// `resume` is an alias of `t`: both name the same key, so both together are a duplicate.
fn canonical_key(key: &[u8]) -> &[u8] {
    if key == b"resume" { b"t" } else { key }
}

/// True when the decoded path form starts with a URL: its first segment is exactly a URI
/// scheme followed by `:` (`/https://host/…`, `/edl://…`, `/ok-player:…`).
fn starts_with_nested_url(path: &[u8]) -> bool {
    let path = path.strip_prefix(b"/").unwrap_or(path);
    let segment = path.split(|byte| *byte == b'/').next().unwrap_or_default();
    let Some((colon, scheme)) = segment.split_last() else {
        return false;
    };
    *colon == b':'
        && scheme.first().is_some_and(u8::is_ascii_alphabetic)
        && scheme
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_happens_exactly_once() {
        assert_eq!(
            percent_decode("%252F", Plus::Literal).as_deref(),
            Some(&b"%2F"[..])
        );
        assert_eq!(
            percent_decode("%252e%252e", Plus::Space).as_deref(),
            Some(&b"%2e%2e"[..])
        );
        assert_eq!(
            percent_decode("a%2Fb%2fc", Plus::Literal).as_deref(),
            Some(&b"a/b/c"[..])
        );
    }

    #[test]
    fn a_percent_without_two_hex_digits_fails_the_whole_value() {
        for raw in ["%G1", "%1G", "%", "%4", "abc%", "%%41", "a%2"] {
            assert_eq!(percent_decode(raw, Plus::Space), None, "{raw}");
        }
    }

    #[test]
    fn plus_is_a_space_only_where_asked() {
        assert_eq!(
            percent_decode("a+b%2Bc", Plus::Space).as_deref(),
            Some(&b"a b+c"[..])
        );
        assert_eq!(
            percent_decode("a+b%2Bc", Plus::Literal).as_deref(),
            Some(&b"a+b+c"[..])
        );
    }

    #[test]
    fn decoding_yields_raw_bytes_including_nul_and_non_utf8() {
        assert_eq!(
            percent_decode("%00%FF%e2%82%ac", Plus::Literal).as_deref(),
            Some(&[0x00, 0xff, 0xe2, 0x82, 0xac][..])
        );
    }

    #[test]
    fn nested_url_detection_reads_only_the_first_segment() {
        for path in [
            &b"/https://host/a.mp4"[..],
            b"/https:/host",
            b"/edl://x",
            b"/ok-player:",
            b"/C:/x.mkv",
        ] {
            assert!(
                starts_with_nested_url(path),
                "{}",
                String::from_utf8_lossy(path)
            );
        }
        for path in [
            &b"/media/https://x"[..],
            b"/notes:2024/a.mkv",
            b"/:/a.mkv",
            b"/1http:/a.mkv",
            b"/media/a.mkv",
        ] {
            assert!(
                !starts_with_nested_url(path),
                "{}",
                String::from_utf8_lossy(path)
            );
        }
    }

    #[test]
    fn unsupported_actions_are_reported_lowercased_without_reading_the_rest() {
        assert_eq!(
            interpret("ok-player://PAUSE?x=%ZZ#frag"),
            Some(Request::Malformed {
                action: None,
                reason: Reason::Fragment,
            })
        );
        assert_eq!(
            interpret("ok-player://Pause?x=%ZZ"),
            Some(Request::Unsupported {
                action: "pause".to_owned(),
            })
        );
    }

    #[test]
    fn a_malformed_request_remembers_its_v1_action() {
        assert_eq!(
            interpret("ok-player://add?url=%G1"),
            Some(Request::Malformed {
                action: Some(Action::Add),
                reason: Reason::BadPercentEncoding,
            })
        );
    }

    #[test]
    fn logged_names_escape_everything_but_printable_ascii_and_are_capped() {
        assert_eq!(log_safe_name(b"autoplay"), "autoplay");
        assert_eq!(log_safe_name(b"a b\"c"), "a b\"c");
        assert_eq!(log_safe_name(b"x\ny\r\t"), "x\\ny\\r\\t");
        assert_eq!(log_safe_name(b"\x1b[31m"), "\\u{1b}[31m");
        assert_eq!(
            log_safe_name("caf\u{e9}\u{202e}".as_bytes()),
            "caf\\u{e9}\\u{202e}"
        );
        assert_eq!(log_safe_name(b"\xff"), "\\u{fffd}");

        let long = log_safe_name("k".repeat(500).as_bytes());
        assert_eq!(long.chars().count(), MAX_LOGGED_NAME_CHARS);
        assert!(long.ends_with("..."));
        // The cap counts the escaped form, so escapes cannot stretch the line either.
        let escaped = log_safe_name("\n".repeat(200).as_bytes());
        assert_eq!(escaped.chars().count(), MAX_LOGGED_NAME_CHARS);
        assert!(escaped.starts_with("\\n\\n"));
    }

    #[test]
    fn ignored_keys_reach_the_request_sanitized_and_capped_in_number() {
        let link = format!(
            "ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4&evil%0Akey=1&{}",
            (0..40)
                .map(|index| format!("k{index}=1"))
                .collect::<Vec<_>>()
                .join("&")
        );
        let Some(Request::Action(request)) = interpret(&link) else {
            panic!("the link must parse");
        };
        assert_eq!(request.ignored_keys.len(), MAX_IGNORED_KEYS);
        assert_eq!(request.ignored_keys[0], "evil\\nkey");
        assert_eq!(request.ignored_keys[1], "k0");
        assert!(
            request
                .ignored_keys
                .iter()
                .all(|key| !key.chars().any(char::is_control))
        );
    }

    #[test]
    fn a_malformed_escape_in_t_drops_t_but_nowhere_else() {
        for t in ["t=%ZZ", "t=1%3", "resume=%G1", "t=%"] {
            let link = format!("ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4&{t}");
            let Some(Request::Action(request)) = interpret(&link) else {
                panic!("{link} must parse");
            };
            assert_eq!(request.start, None, "{t}");
            assert!(request.start_dropped, "{t}");
        }
        // The key itself still decodes strictly, as does every other value.
        for tail in ["%ZZ=1", "x=%ZZ", "path=%ZZ"] {
            let link = format!("ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4&{tail}");
            assert_eq!(
                interpret(&link).and_then(|request| request.rejection()),
                Some(Reason::BadPercentEncoding),
                "{tail}"
            );
        }
        // A dropped t still counts as the t key.
        assert_eq!(
            interpret("ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4&t=%ZZ&resume=5")
                .and_then(|request| request.rejection()),
            Some(Reason::DuplicateKey)
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_before_the_scheme_is_matched() {
        assert!(matches!(
            interpret("  ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4\n"),
            Some(Request::Action(_))
        ));
    }
}
