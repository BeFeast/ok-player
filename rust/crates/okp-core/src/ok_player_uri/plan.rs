//! Launch shape and the execution plan: the last two decisions before a shell acts.

use std::ffi::OsStr;

use super::{Action, Reason, Target, Validated, is_link};

/// How a process launch relates to links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchShape {
    /// No argument is a link; the launch proceeds as before.
    NoLink,
    /// The launch is exactly one link (optionally after a lone `--`). Hand it to
    /// [`super::interpret`]; honour no CLI switch.
    Link(String),
    /// A link arrived together with anything else. The whole launch is rejected: no
    /// argument opens and no CLI switch is honoured.
    Rejected(Reason),
}

/// Check the shape of a launch. `args` are the arguments after the program name.
///
/// An argv that contains an `ok-player:` token must contain exactly that one positional. A
/// lone leading `--` (the end-of-options marker a desktop entry may put before `%u`) is
/// allowed because it is not a switch.
pub fn launch_shape<S: AsRef<OsStr>>(args: &[S]) -> LaunchShape {
    if !args.iter().any(|arg| is_link(arg.as_ref())) {
        return LaunchShape::NoLink;
    }
    // A link is present, and `--` is not one, so in either shape `token` is the link.
    let token = match args {
        [token] => token.as_ref(),
        [separator, token] if separator.as_ref() == "--" => token.as_ref(),
        _ => return LaunchShape::Rejected(Reason::LaunchShape),
    };
    match token.to_str() {
        Some(token) => LaunchShape::Link(token.to_owned()),
        // A byte that is not UTF-8 cannot appear in a valid link.
        None => LaunchShape::Rejected(Reason::InvalidCharacter),
    }
}

/// What is playing when a request is dequeued.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlaybackSummary {
    /// The item the engine is loading or playing, or `None` when the player is idle (or just
    /// started). A shell reports an item whose load failed, or that ended, as `None` even while
    /// its window still names it: nothing plays it, so a link must open rather than seek in it
    /// or queue behind it.
    pub current: Option<Target>,
}

impl PlaybackSummary {
    pub const fn idle() -> Self {
        Self { current: None }
    }

    pub fn playing(current: Target) -> Self {
        Self {
            current: Some(current),
        }
    }
}

/// Where a queued item goes in Up Next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueMode {
    /// After the last item (`add`).
    Append,
    /// Right after the current item (`next`).
    AfterCurrent,
}

/// What a shell does with a validated request.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Save progress, replace playback with `target`, present the window. A local file
    /// builds the folder-sibling Up Next exactly like a file-manager open.
    Open { target: Target, start: Option<f64> },
    /// Put `item` into Up Next, show a toast, do not raise the window. `start_ignored`:
    /// the link carried a valid `t`, which v1 ignores for queue items; show a notice.
    Queue {
        item: Target,
        mode: QueueMode,
        start_ignored: bool,
    },
    /// The link opens the item that is already current: seek instead of reloading.
    Seek { start: f64 },
}

impl Plan {
    /// Whether executing the plan presents (raises) the window.
    pub fn presents(&self) -> bool {
        !matches!(self, Self::Queue { .. })
    }
}

/// Plan a validated request against what plays now. Call it when the request is dequeued
/// and the player is ready, so a cold `open A` followed by `add B` yields `[A, B]`.
///
/// - `add`/`next` with nothing loaded act as `open` (including its `t`).
/// - `open` of the current item with `t` seeks; without `t` it reopens.
pub fn plan(validated: &Validated, playback: &PlaybackSummary) -> Plan {
    let target = validated.target().clone();
    let start = validated.start();
    let Some(current) = playback.current.as_ref() else {
        return Plan::Open { target, start };
    };
    let mode = match validated.action() {
        Action::Open => {
            return match start {
                Some(start) if *current == target => Plan::Seek { start },
                start => Plan::Open { target, start },
            };
        }
        Action::Add => QueueMode::Append,
        Action::Next => QueueMode::AfterCurrent,
    };
    Plan::Queue {
        item: target,
        mode,
        start_ignored: start.is_some(),
    }
}

/// Whether handling a request with `action` presents the window, accepted or not.
///
/// Raising depends only on the action and on whether something is loaded, never on
/// whether the request was accepted; otherwise a page could learn whether a path exists by
/// watching for blur or `visibilitychange`. `action` is `None` for a link that named no v1
/// action, which presents like `open`. For an accepted request this equals
/// [`Plan::presents`] of its plan.
pub fn presents_window(action: Option<Action>, playback: &PlaybackSummary) -> bool {
    !(matches!(action, Some(Action::Add | Action::Next)) && playback.current.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn a_launch_without_links_is_untouched() {
        let empty: [&str; 0] = [];
        assert_eq!(launch_shape(&empty), LaunchShape::NoLink);
        assert_eq!(
            launch_shape(&["--fullscreen", "/media/a.mkv", "https://h.example/b.mp4"]),
            LaunchShape::NoLink
        );
    }

    #[test]
    fn a_link_must_be_the_only_positional() {
        let link = "ok-player://open?url=https%3A%2F%2Fh.example%2Fa.mp4";
        assert_eq!(launch_shape(&[link]), LaunchShape::Link(link.to_owned()));
        assert_eq!(
            launch_shape(&["--", link]),
            LaunchShape::Link(link.to_owned())
        );
        for args in [
            vec![link, "/media/b.mkv"],
            vec!["/media/b.mkv", link],
            vec!["--fullscreen", link],
            vec![link, "--"],
            vec!["--", "--", link],
            vec![link, link],
        ] {
            assert_eq!(
                launch_shape(&args),
                LaunchShape::Rejected(Reason::LaunchShape),
                "{args:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_link_rejects_the_launch() {
        use std::os::unix::ffi::OsStringExt;

        let args = [OsString::from_vec(b"ok-player://open/m/\xff.mkv".to_vec())];
        assert_eq!(
            launch_shape(&args),
            LaunchShape::Rejected(Reason::InvalidCharacter)
        );
        let args = [
            OsString::from("/media/a.mkv"),
            OsString::from_vec(b"ok-player://open/m/\xff.mkv".to_vec()),
        ];
        assert_eq!(
            launch_shape(&args),
            LaunchShape::Rejected(Reason::LaunchShape)
        );
    }

    #[test]
    fn presenting_depends_on_action_and_playback_only() {
        let idle = PlaybackSummary::idle();
        let playing = PlaybackSummary::playing(Target::Path(OsString::from("/m/a.mkv")));
        for playback in [&idle, &playing] {
            assert!(presents_window(Some(Action::Open), playback));
            assert!(presents_window(None, playback));
        }
        assert!(presents_window(Some(Action::Add), &idle));
        assert!(presents_window(Some(Action::Next), &idle));
        assert!(!presents_window(Some(Action::Add), &playing));
        assert!(!presents_window(Some(Action::Next), &playing));
    }
}
