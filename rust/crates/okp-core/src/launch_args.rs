//! Parses the player's command line for the companion-library launch contract (PRD §13.1) —
//! port of `src/OkPlayer.Core/LaunchArgs.cs`; the C# suite in
//! `tests/OkPlayer.Tests/LaunchArgsTests.cs` is the executable spec. A media file/URL plus
//! optional `--resume <time>` and `--sub`/`--audio` track preselection the library uses to
//! open the player at an exact position with a chosen subtitle/audio track. Pure and
//! engine-agnostic; the caller validates which positional is a real file (URL / exists on
//! disk) and applies the result.

use crate::time_code;

/// An explicit `--sub`/`--audio` preselection: a 1-based mpv track id, or off
/// (`no`/`off` on the command line — "select none").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackSelection {
    Off,
    Id(i32),
}

/// The parsed command line. `files`: positional tokens in order (the caller picks the first
/// that is a URL or an existing file). `resume_seconds`: the parsed `--resume` value in
/// seconds, or `None` when absent or malformed (a resume of 0 is meaningful — "start from the
/// beginning", overriding remembered position). `sub`/`audio`: the track to preselect, or
/// `None` when absent/malformed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaunchArgs {
    pub files: Vec<String>,
    pub resume_seconds: Option<f64>,
    pub sub: Option<TrackSelection>,
    pub audio: Option<TrackSelection>,
}

/// Parse the command-line arguments **excluding** the executable (i.e. what
/// `std::env::args().skip(1)` yields).
pub fn parse<S: AsRef<str>>(args: &[S]) -> LaunchArgs {
    let mut parsed = LaunchArgs::default();

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_ref();
        if arg.is_empty() {
            i += 1;
            continue;
        }

        // `.or(<prev>)`: a malformed/missing value yields None but must not wipe an earlier
        // valid one (e.g. `--sub 2 --sub bad` keeps 2). A later *valid* repeat still wins.
        if let Some(inline) = try_match_option(arg, "resume") {
            parsed.resume_seconds = consume(inline, args, &mut i, |s| time_code::parse(Some(s)))
                .or(parsed.resume_seconds);
        } else if let Some(inline) = try_match_option(arg, "sub") {
            parsed.sub = consume(inline, args, &mut i, parse_track_id).or(parsed.sub);
        } else if let Some(inline) = try_match_option(arg, "audio") {
            parsed.audio = consume(inline, args, &mut i, parse_track_id).or(parsed.audio);
        } else if !arg.starts_with('-') {
            // Positional — including unmatched `/…` tokens. C# drops those as unknown
            // Windows-style switches, but on POSIX `/home/alice/movie.mkv` is an absolute
            // path; only the documented names (matched above) keep the slash-switch
            // spelling. Divergence recorded in docs/core-compatibility.md.
            parsed.files.push(arg.to_string());
        }
        // else: an unknown `-` switch — ignore (file associations may append flags)

        i += 1;
    }
    parsed
}

/// Resolve an option's value: the inline part (`--opt=value`) if present, else the following
/// token — but only consume that token when it actually parses, so a path after a bare
/// `--opt` stays a positional instead of being silently swallowed.
fn consume<T, S: AsRef<str>>(
    inline_value: Option<&str>,
    args: &[S],
    i: &mut usize,
    parse: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    if let Some(inline) = inline_value {
        return parse(inline);
    }
    if *i + 1 < args.len()
        && let Some(next) = parse(args[*i + 1].as_ref())
    {
        *i += 1;
        return Some(next);
    }
    None
}

/// Classify a `--sub`/`--audio` value as a track selection (a 1-based mpv id, or `no`/`off`).
/// Public so a shell that *also* accepts `--sub <file>` (as the Linux GTK shell does) can tell
/// a track hint (`--sub 2`) from a subtitle path (`--sub movie.srt`): a path returns `None`.
#[must_use]
pub fn parse_track_selection(value: &str) -> Option<TrackSelection> {
    parse_track_id(value)
}

/// A track id is a positive integer — mpv track ids are 1-based, and `0` means "auto", not a
/// real track, so it (and anything non-numeric) is `None`/ignored. `no`/`off` → off, "select
/// none".
fn parse_track_id(s: &str) -> Option<TrackSelection> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("no") || s.eq_ignore_ascii_case("off") {
        return Some(TrackSelection::Off);
    }
    // Digits only (no sign, no inner whitespace), like C# NumberStyles.None.
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id: i32 = s.parse().ok()?;
    (id >= 1).then_some(TrackSelection::Id(id))
}

/// Matches `--name`, `-name` or `/name` (case-insensitive). Returns `None` when the token is
/// not this option; `Some(None)` when it matches with no inline value (the value, if any, is
/// the following token); `Some(Some(value))` when the token carries an inline value
/// (`--name=value` or `--name:value`).
fn try_match_option<'a>(token: &'a str, name: &str) -> Option<Option<&'a str>> {
    let body = token.trim_start_matches(['-', '/']);
    if body.len() == token.len() {
        return None; // no switch prefix at all
    }
    let sep = body.find(['=', ':']);
    let key = sep.map_or(body, |sep| &body[..sep]);
    if !key.eq_ignore_ascii_case(name) {
        return None;
    }
    Some(sep.map(|sep| &body[sep + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVIE: &str = r"C:\media\movie.mkv";

    #[test]
    fn parse_empty_returns_empty() {
        let parsed = parse::<&str>(&[]);
        assert!(parsed.files.is_empty());
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_file_only_no_resume() {
        let parsed = parse(&[MOVIE]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_resume_before_file() {
        let parsed = parse(&["--resume", "90", MOVIE]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, Some(90.0));
    }

    #[test]
    fn parse_resume_after_file() {
        let parsed = parse(&[MOVIE, "--resume", "90"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, Some(90.0));
    }

    #[test]
    fn parse_inline_resume_value() {
        let cases = [
            ("--resume=90", 90.0),
            ("--resume:90", 90.0),
            ("-resume=90", 90.0),
            ("/resume=90", 90.0),
            ("--resume=1:23:45", 5025.0),
            ("--resume=83.5", 83.5),
        ];
        for (token, expected) in cases {
            let parsed = parse(&[MOVIE, token]);
            assert_eq!(parsed.files, [MOVIE], "{token}");
            assert_eq!(parsed.resume_seconds, Some(expected), "{token}");
        }
    }

    #[test]
    fn parse_timecode_as_separate_value() {
        let parsed = parse(&[MOVIE, "--resume", "1:23:45"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, Some(5025.0));
    }

    #[test]
    fn parse_resume_zero_is_kept_not_treated_as_absent() {
        let parsed = parse(&[MOVIE, "--resume", "0"]);
        assert_eq!(parsed.resume_seconds, Some(0.0));
    }

    #[test]
    fn parse_malformed_resume_value_is_ignored_and_next_token_stays_positional() {
        // "--resume" with a non-timecode following it: the value parses to None and must NOT
        // swallow the path.
        let parsed = parse(&["--resume", MOVIE]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_inline_malformed_resume_is_none() {
        let parsed = parse(&[MOVIE, "--resume=abc"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_bare_resume_at_end_is_none() {
        let parsed = parse(&[MOVIE, "--resume"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_unknown_dash_switches_are_ignored() {
        let parsed = parse(&["--fullscreen", MOVIE, "-x"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, None);
    }

    #[test]
    fn parse_posix_absolute_path_is_positional() {
        let parsed = parse(&["/home/alice/movie.mkv", "--resume", "90"]);
        assert_eq!(parsed.files, ["/home/alice/movie.mkv"]);
        assert_eq!(parsed.resume_seconds, Some(90.0));
    }

    #[test]
    fn parse_unmatched_slash_token_is_positional() {
        // Divergence from C# (which ignores `/foo` as an unknown Windows switch): on POSIX
        // it is an absolute path, so it stays positional and the caller's URL/exists-on-disk
        // validation filters it. See docs/core-compatibility.md.
        let parsed = parse(&["/foo", MOVIE]);
        assert_eq!(parsed.files, ["/foo", MOVIE]);
    }

    #[test]
    fn parse_slash_spelled_documented_options_are_still_switches() {
        let parsed = parse(&["/home/alice/movie.mkv", "/resume", "90", "/sub", "2"]);
        assert_eq!(parsed.files, ["/home/alice/movie.mkv"]);
        assert_eq!(parsed.resume_seconds, Some(90.0));
        assert_eq!(parsed.sub, Some(TrackSelection::Id(2)));
    }

    #[test]
    fn parse_multiple_positionals_kept_in_order() {
        // The caller picks the first that is a URL / existing file.
        let parsed = parse(&["garbage", MOVIE]);
        assert_eq!(parsed.files, ["garbage", MOVIE]);
    }

    #[test]
    fn parse_url_is_positional() {
        let parsed = parse(&["https://example.com/a.mp4", "--resume", "12"]);
        assert_eq!(parsed.files, ["https://example.com/a.mp4"]);
        assert_eq!(parsed.resume_seconds, Some(12.0));
    }

    #[test]
    fn parse_sub_and_audio_track_ids() {
        let parsed = parse(&[MOVIE, "--sub", "2", "--audio=1"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.sub, Some(TrackSelection::Id(2)));
        assert_eq!(parsed.audio, Some(TrackSelection::Id(1)));
    }

    #[test]
    fn parse_sub_off_is_off() {
        for token in ["no", "off", "OFF"] {
            let parsed = parse(&[MOVIE, "--sub", token]);
            assert_eq!(parsed.sub, Some(TrackSelection::Off), "{token}");
        }
    }

    #[test]
    fn parse_audio_off_is_off() {
        let parsed = parse(&[MOVIE, "--audio=no"]);
        assert_eq!(parsed.audio, Some(TrackSelection::Off));
    }

    #[test]
    fn parse_no_track_flags_are_none() {
        let parsed = parse(&[MOVIE]);
        assert_eq!(parsed.sub, None);
        assert_eq!(parsed.audio, None);
    }

    #[test]
    fn parse_malformed_track_id_is_none_and_does_not_swallow_next_token() {
        // "abc" is not a number; "2.5" is not an integer.
        for bad in ["abc", "2.5"] {
            let parsed = parse(&["--sub", bad, MOVIE]);
            // Bad value isn't a track id -> stays positional, path preserved.
            assert_eq!(parsed.files, [bad, MOVIE], "{bad}");
            assert_eq!(parsed.sub, None, "{bad}");
        }
    }

    #[test]
    fn parse_negative_literal_track_id_is_rejected() {
        // "-1" as a literal is rejected (only no/off yield off) and, leading-dash, is treated
        // as a switch.
        let parsed = parse(&["--sub", "-1", MOVIE]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.sub, None);
    }

    #[test]
    fn parse_all_flags_together() {
        let parsed = parse(&["--resume", "1:30", MOVIE, "--sub", "3", "--audio", "2"]);
        assert_eq!(parsed.files, [MOVIE]);
        assert_eq!(parsed.resume_seconds, Some(90.0));
        assert_eq!(parsed.sub, Some(TrackSelection::Id(3)));
        assert_eq!(parsed.audio, Some(TrackSelection::Id(2)));
    }

    #[test]
    fn parse_track_id_zero_is_rejected_because_mpv_ids_are_1_based() {
        // mpv reads aid/sid 0 as "auto", not track 0 — so 0 is ignored rather than silently
        // selecting auto.
        let parsed = parse(&[MOVIE, "--sub=0", "--audio=0"]);
        assert_eq!(parsed.sub, None);
        assert_eq!(parsed.audio, None);
    }

    #[test]
    fn parse_later_malformed_repeat_keeps_earlier_valid_value() {
        let parsed = parse(&[MOVIE, "--resume=90", "--resume=bad", "--sub=2", "--sub=bad"]);
        assert_eq!(parsed.resume_seconds, Some(90.0)); // the malformed repeat must not wipe the valid 90
        assert_eq!(parsed.sub, Some(TrackSelection::Id(2))); // nor the valid 2
    }

    #[test]
    fn parse_later_valid_repeat_wins() {
        let parsed = parse(&[MOVIE, "--resume=90", "--resume=120"]);
        assert_eq!(parsed.resume_seconds, Some(120.0));
    }
}
