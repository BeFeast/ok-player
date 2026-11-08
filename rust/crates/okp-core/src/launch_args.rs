//! Parses the player's command line for the companion-library launch contract (PRD §13.1) —
//! port of `src/OkPlayer.Core/LaunchArgs.cs`; the C# suite in
//! `tests/OkPlayer.Tests/LaunchArgsTests.cs` is the executable spec. A media file/URL plus
//! optional `--resume <time>` and `--sub`/`--audio` track preselection the library uses to
//! open the player at an exact position with a chosen subtitle/audio track. Pure and
//! engine-agnostic; the caller validates which positional is a real file (URL / exists on
//! disk) and applies the result.

use std::collections::VecDeque;

use crate::time_code;

/// Why a resume target was selected. Explicit companion launch positions are applied exactly for
/// that launch; remembered positions remain subject to the normal barely-started / near-end rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeOrigin {
    ExplicitLaunch,
    Remembered,
}

/// A resolved resume target after explicit-launch precedence has been applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResumeTarget {
    pub seconds: f64,
    pub origin: ResumeOrigin,
}

impl ResumeTarget {
    /// Return the seek position for a known media duration. Explicit launch targets deliberately
    /// bypass remembered-position thresholds (including an explicit zero); remembered targets use
    /// the PRD §12.1 5% / completion-window policy.
    #[must_use]
    pub fn seek_position(self, duration: f64) -> Option<f64> {
        if !duration.is_finite() || duration <= 0.0 {
            return None;
        }

        match self.origin {
            ResumeOrigin::ExplicitLaunch => Some(self.seconds.min(duration)),
            ResumeOrigin::Remembered
                if self.seconds > duration * 0.05
                    && self.seconds < crate::recents_shelf::completion_start(duration) =>
            {
                Some(self.seconds)
            }
            ResumeOrigin::Remembered => None,
        }
    }
}

/// Choose the one-shot resume target for an open. A valid explicit launch target always wins over
/// remembered state, but does not mutate it; callers consume the returned value for this open only.
#[must_use]
pub fn resolve_resume(explicit: Option<f64>, remembered: Option<f64>) -> Option<ResumeTarget> {
    valid_resume(explicit)
        .map(|seconds| ResumeTarget {
            seconds,
            origin: ResumeOrigin::ExplicitLaunch,
        })
        .or_else(|| {
            valid_resume(remembered).map(|seconds| ResumeTarget {
                seconds,
                origin: ResumeOrigin::Remembered,
            })
        })
}

fn valid_resume(value: Option<f64>) -> Option<f64> {
    value.filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

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

/// Launch requests waiting for the player, released one at a time in arrival order.
///
/// Native shells can construct their window and media engine in either order, and a running
/// single-instance shell receives launches forwarded by later processes at any moment: before its
/// engine exists, while an earlier launch is still waiting, or in quick succession. A launch must
/// not reach the engine while the primary window is unmapped or the engine is not ready, must not
/// be lost when those edges arrive in an unexpected order, and must not overtake an earlier one.
///
/// The queue releases its oldest request once the window is mapped, the player is ready and no
/// earlier request is still being applied. The shell applies the released request, then reports it
/// with [`LaunchDelivery::mark_applied`], which releases the next one. Every request comes out
/// exactly once. `T` is the shell's own request type, typically an enum, so further request kinds
/// share the same ordering.
#[derive(Debug)]
pub struct LaunchDelivery<T> {
    pending: VecDeque<T>,
    window_mapped: bool,
    player_ready: bool,
    applying: bool,
}

impl<T> Default for LaunchDelivery<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> LaunchDelivery<T> {
    /// An empty queue whose window is not mapped and whose player is not ready yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            window_mapped: false,
            player_ready: false,
            applying: false,
        }
    }

    /// A queue holding the cold-start launch as its first request.
    #[must_use]
    pub fn with_startup(request: T) -> Self {
        let mut delivery = Self::new();
        delivery.pending.push_back(request);
        delivery
    }

    /// Queue a request behind every earlier one. Returns it straight back when the player can take
    /// it now: window mapped, player ready, nothing earlier waiting or being applied.
    #[must_use = "a released request must be applied and then reported with mark_applied"]
    pub fn push(&mut self, request: T) -> Option<T> {
        self.pending.push_back(request);
        self.release()
    }

    /// Record that the primary window is mapped and eligible to receive a launch.
    #[must_use = "a released request must be applied and then reported with mark_applied"]
    pub fn mark_window_mapped(&mut self) -> Option<T> {
        self.window_mapped = true;
        self.release()
    }

    /// Record that the media engine can accept a launch.
    #[must_use = "a released request must be applied and then reported with mark_applied"]
    pub fn mark_player_ready(&mut self) -> Option<T> {
        self.player_ready = true;
        self.release()
    }

    /// Record that the media engine is gone again (detached for shutdown, or torn down to be
    /// rebuilt). Later requests wait until [`LaunchDelivery::mark_player_ready`] is reported again.
    pub fn mark_player_unavailable(&mut self) {
        self.player_ready = false;
    }

    /// Record that the request released last has been applied. Returns the next request when the
    /// player can take it now.
    #[must_use = "a released request must be applied and then reported with mark_applied"]
    pub fn mark_applied(&mut self) -> Option<T> {
        self.applying = false;
        self.release()
    }

    /// Requests still waiting, not counting one that has been released and is being applied.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Whether no request is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn release(&mut self) -> Option<T> {
        if self.applying || !self.window_mapped || !self.player_ready {
            return None;
        }
        let request = self.pending.pop_front()?;
        self.applying = true;
        Some(request)
    }
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
    fn startup_launch_waits_for_window_and_player_in_either_order() {
        let mut window_first = LaunchDelivery::with_startup("window-first");
        assert_eq!(window_first.mark_window_mapped(), None);
        assert_eq!(window_first.mark_player_ready(), Some("window-first"));
        assert_eq!(window_first.mark_player_ready(), None);
        assert_eq!(window_first.mark_applied(), None);

        let mut player_first = LaunchDelivery::with_startup("player-first");
        assert_eq!(player_first.mark_player_ready(), None);
        assert_eq!(player_first.mark_window_mapped(), Some("player-first"));
        assert_eq!(player_first.mark_window_mapped(), None);
        assert_eq!(player_first.mark_applied(), None);
    }

    #[test]
    fn forwarded_launch_before_readiness_is_released_after_readiness() {
        let mut delivery = LaunchDelivery::new();
        assert_eq!(delivery.push("forwarded"), None);
        assert_eq!(delivery.mark_window_mapped(), None);
        assert_eq!(delivery.len(), 1);

        assert_eq!(delivery.mark_player_ready(), Some("forwarded"));
        assert!(delivery.is_empty());
        assert_eq!(delivery.mark_applied(), None);
    }

    #[test]
    fn launches_keep_arrival_order_and_wait_for_the_previous_one() {
        let mut delivery = LaunchDelivery::with_startup("startup");
        assert_eq!(delivery.mark_window_mapped(), None);
        assert_eq!(delivery.push("first"), None);
        assert_eq!(delivery.push("second"), None);

        assert_eq!(delivery.mark_player_ready(), Some("startup"));
        // Arrives while the startup launch is still being applied: it queues behind the others.
        assert_eq!(delivery.push("third"), None);
        assert_eq!(delivery.len(), 3);

        assert_eq!(delivery.mark_applied(), Some("first"));
        assert_eq!(delivery.mark_applied(), Some("second"));
        assert_eq!(delivery.mark_applied(), Some("third"));
        assert_eq!(delivery.mark_applied(), None);
        assert!(delivery.is_empty());
    }

    #[test]
    fn launch_while_playing_is_released_at_once() {
        let mut delivery = LaunchDelivery::with_startup("startup");
        assert_eq!(delivery.mark_player_ready(), None);
        assert_eq!(delivery.mark_window_mapped(), Some("startup"));
        assert_eq!(delivery.mark_applied(), None);

        assert_eq!(delivery.push("replacement"), Some("replacement"));
        assert_eq!(delivery.mark_applied(), None);
        assert_eq!(delivery.push("another"), Some("another"));
        assert_eq!(delivery.mark_applied(), None);
    }

    #[test]
    fn launches_wait_while_the_player_is_unavailable() {
        let mut delivery = LaunchDelivery::new();
        assert_eq!(delivery.mark_window_mapped(), None);
        assert_eq!(delivery.mark_player_ready(), None);

        delivery.mark_player_unavailable();
        assert_eq!(delivery.push("held"), None);
        assert_eq!(delivery.push("next"), None);

        assert_eq!(delivery.mark_player_ready(), Some("held"));
        assert_eq!(delivery.mark_applied(), Some("next"));
        assert_eq!(delivery.mark_applied(), None);
    }

    #[test]
    fn launch_request_kinds_share_one_ordered_queue() {
        #[derive(Debug, PartialEq)]
        enum Request {
            Args(&'static str),
            Other(u8),
        }

        let mut delivery = LaunchDelivery::new();
        assert_eq!(delivery.push(Request::Args("file")), None);
        assert_eq!(delivery.push(Request::Other(7)), None);
        assert_eq!(delivery.push(Request::Args("url")), None);
        assert_eq!(delivery.mark_window_mapped(), None);

        assert_eq!(delivery.mark_player_ready(), Some(Request::Args("file")));
        assert_eq!(delivery.mark_applied(), Some(Request::Other(7)));
        assert_eq!(delivery.mark_applied(), Some(Request::Args("url")));
        assert_eq!(delivery.mark_applied(), None);
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

    #[test]
    fn explicit_resume_wins_without_mutating_remembered_value() {
        let remembered = Some(240.0);
        let target = resolve_resume(Some(90.0), remembered).expect("explicit resume");

        assert_eq!(
            target,
            ResumeTarget {
                seconds: 90.0,
                origin: ResumeOrigin::ExplicitLaunch,
            }
        );
        assert_eq!(remembered, Some(240.0));
    }

    #[test]
    fn explicit_zero_and_near_end_targets_bypass_remembered_resume_thresholds() {
        let zero = resolve_resume(Some(0.0), Some(240.0)).expect("explicit zero");
        let near_end = resolve_resume(Some(590.0), Some(240.0)).expect("explicit near end");

        assert_eq!(zero.seek_position(600.0), Some(0.0));
        assert_eq!(near_end.seek_position(600.0), Some(590.0));
    }

    #[test]
    fn remembered_resume_still_uses_barely_started_and_completion_thresholds() {
        let barely_started = resolve_resume(None, Some(30.0)).expect("remembered start");
        let middle = resolve_resume(None, Some(120.0)).expect("remembered middle");
        let near_end = resolve_resume(None, Some(570.0)).expect("remembered end");

        assert_eq!(barely_started.seek_position(600.0), None);
        assert_eq!(middle.seek_position(600.0), Some(120.0));
        assert_eq!(near_end.seek_position(600.0), None);
    }

    #[test]
    fn invalid_explicit_resume_falls_back_to_valid_remembered_position() {
        let target = resolve_resume(Some(f64::NAN), Some(120.0)).expect("remembered fallback");
        assert_eq!(target.origin, ResumeOrigin::Remembered);
        assert_eq!(target.seek_position(600.0), Some(120.0));
    }
}
