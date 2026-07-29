//! Name the widget that holds keyboard focus, for input harnesses.
//!
//! Tab traversal is the one live acceptance row with no outcome a harness can read: the
//! keys reach GTK, GTK moves focus, and nothing says so. Without a name per stop, an
//! automated check could only assert that keys were sent - which is not evidence, because
//! it passes just as happily when focus never moves at all.
//!
//! This module owns the portable half: turning what GTK knows about a widget into a
//! stable, low-cardinality token. The shell only gathers the values and prints the line.

/// Prefix shared by every emitted focus line.
pub const FOCUS_PREFIX: &str = "interaction: focus";

/// Token used when focus leaves the window entirely.
pub const NO_FOCUS: &str = "none";

/// A stable name for one focus stop.
///
/// The shell's own CSS classes are the most durable identity a widget has here - they are
/// what the stylesheet and the geometry planes already key on - so an `okp-` class wins.
/// An accessible label is the next best answer because it is what a screen reader would
/// announce. The widget type is the last resort, and it is still enough to tell two
/// consecutive stops apart, which is what the row actually asserts.
pub fn focus_token<'a>(
    type_name: &'a str,
    css_classes: impl IntoIterator<Item = &'a str>,
    accessible_label: Option<&'a str>,
) -> String {
    if let Some(class) = css_classes
        .into_iter()
        .filter(|class| class.starts_with("okp-"))
        .min_by_key(|class| (class.len(), *class))
    {
        return sanitize(class);
    }
    if let Some(label) = accessible_label.map(str::trim).filter(|l| !l.is_empty()) {
        return format!("label:{}", sanitize(label));
    }
    sanitize(type_name)
}

/// Render one focus line for the diagnostic stream.
pub fn focus_line(token: &str, sequence: u64) -> String {
    format!("{FOCUS_PREFIX} target={token} seq={sequence}")
}

/// Collapse a name to one whitespace-free lowercase token so a shell harness can compare
/// two stops with a plain string test.
fn sanitize(value: &str) -> String {
    let mut token = String::with_capacity(value.len());
    let mut pending_separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
            if pending_separator && !token.is_empty() {
                token.push('-');
            }
            pending_separator = false;
            token.push(character.to_ascii_lowercase());
        } else {
            pending_separator = true;
        }
    }
    if token.is_empty() {
        NO_FOCUS.to_owned()
    } else {
        token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_css_class_names_the_stop() {
        assert_eq!(
            focus_token("GtkButton", ["circular", "okp-player-window-pin"], None),
            "okp-player-window-pin"
        );
    }

    #[test]
    fn the_shortest_shell_class_wins_so_the_name_is_stable_across_state_classes() {
        assert_eq!(
            focus_token(
                "GtkButton",
                ["okp-player-window-control", "okp-player-window-pin"],
                None
            ),
            "okp-player-window-pin"
        );
        // Adding a state class must not rename the stop.
        assert_eq!(
            focus_token(
                "GtkButton",
                [
                    "okp-player-window-pin",
                    "okp-player-window-control",
                    "is-selected"
                ],
                None
            ),
            "okp-player-window-pin"
        );
    }

    #[test]
    fn an_accessible_label_names_a_widget_the_stylesheet_does_not_claim() {
        assert_eq!(
            focus_token("GtkScale", ["horizontal"], Some("Volume")),
            "label:volume"
        );
    }

    #[test]
    fn the_widget_type_is_the_last_resort() {
        assert_eq!(focus_token("GtkEntry", [], None), "gtkentry");
        assert_eq!(focus_token("GtkEntry", [], Some("   ")), "gtkentry");
    }

    #[test]
    fn tokens_never_carry_whitespace_a_shell_test_would_split_on() {
        let token = focus_token("GtkButton", [], Some("Audio tracks / output"));
        assert_eq!(token, "label:audio-tracks-output");
        assert!(!token.contains(char::is_whitespace));
    }

    #[test]
    fn an_unnameable_widget_still_produces_a_token() {
        assert_eq!(focus_token("///", [], None), NO_FOCUS);
    }

    #[test]
    fn the_rendered_line_carries_the_prefix_a_harness_greps_for() {
        assert_eq!(
            focus_line("okp-player-window-pin", 7),
            "interaction: focus target=okp-player-window-pin seq=7"
        );
    }
}
