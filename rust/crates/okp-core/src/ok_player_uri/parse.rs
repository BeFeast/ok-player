//! Grammar and strict decoding: link text to [`Request`]. No filesystem access and no
//! judgement of the target beyond its syntax; that is [`super::policy`].

use std::ffi::OsString;

use super::{Action, ActionRequest, MAX_LINK_BYTES, Reason, Request, SCHEME, Target};
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

/// Percent-decode `raw` the way an HTTP server reads a request path: every valid `%XX`
/// decodes, a stray `%` stays literal, and `+` stays `+`. Only for judging the name a URL
/// points at; the link layer itself always decodes strictly with [`percent_decode`].
pub(super) fn percent_decode_lenient(raw: &str) -> Vec<u8> {
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
/// action), path form, then the query — decode every pair, then duplicate keys, then
/// targets.
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
            action: authority.to_ascii_lowercase(),
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

    let pairs = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((
                percent_decode(key, Plus::Space)?,
                percent_decode(value, Plus::Space)?,
            ))
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
        match canonical_key(&key) {
            b"url" => targets.push(RawTarget::Url(value)),
            b"path" => targets.push(RawTarget::Path(value)),
            b"t" => {
                // A malformed optional parameter is dropped with a notice, never fatal.
                start = std::str::from_utf8(&value)
                    .ok()
                    .and_then(|value| time_code::parse(Some(value)));
                start_dropped = start.is_none();
            }
            _ => ignored_keys.push(String::from_utf8_lossy(&key).into_owned()),
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
    fn lenient_decoding_keeps_a_stray_percent_and_decodes_the_rest() {
        assert_eq!(percent_decode_lenient("list.m3%75"), b"list.m3u");
        assert_eq!(percent_decode_lenient("%zz.m3%55"), b"%zz.m3U");
        assert_eq!(percent_decode_lenient("a%2"), b"a%2");
        assert_eq!(percent_decode_lenient("%%41+"), b"%A+");
        assert_eq!(percent_decode_lenient("%252F"), b"%2F");
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
    fn surrounding_whitespace_is_trimmed_before_the_scheme_is_matched() {
        assert!(matches!(
            interpret("  ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4\n"),
            Some(Request::Action(_))
        ));
    }
}
