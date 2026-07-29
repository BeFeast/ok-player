//! Portable policy and lifetime-only geometry for app-owned companion windows.

use crate::window_fit::{WindowRect, WindowSize};

/// Long-lived surfaces that belong to one player window without blocking it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanionWindowKind {
    Settings,
    MediaInfo,
}

/// Platform-independent window semantics enforced by every native shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompanionWindowPolicy {
    pub modal: bool,
    pub resizable: bool,
    pub always_on_top: bool,
    pub single_instance: bool,
    pub retain_on_close: bool,
    pub parent_input_enabled: bool,
    pub minimum_size: WindowSize,
    pub natural_size: WindowSize,
}

/// The shared contract for each long-lived companion surface.
pub const fn companion_window_policy(kind: CompanionWindowKind) -> CompanionWindowPolicy {
    match kind {
        CompanionWindowKind::Settings => CompanionWindowPolicy {
            modal: false,
            resizable: true,
            always_on_top: false,
            single_instance: true,
            retain_on_close: true,
            parent_input_enabled: true,
            minimum_size: WindowSize {
                width: 760,
                height: 480,
            },
            natural_size: WindowSize {
                width: 760,
                height: 560,
            },
        },
        CompanionWindowKind::MediaInfo => CompanionWindowPolicy {
            modal: false,
            resizable: true,
            always_on_top: false,
            single_instance: true,
            retain_on_close: false,
            parent_input_enabled: true,
            minimum_size: WindowSize {
                width: 520,
                height: 420,
            },
            natural_size: WindowSize {
                width: 720,
                height: 571,
            },
        },
    }
}

/// Clamp the first or restored size to the active monitor work area.
pub fn companion_window_size(
    kind: CompanionWindowKind,
    restored: Option<WindowSize>,
    work_area: WindowRect,
) -> WindowSize {
    let policy = companion_window_policy(kind);
    let requested = restored.unwrap_or(policy.natural_size);
    let max_width = work_area.width.max(1);
    let max_height = work_area.height.max(1);
    let min_width = policy.minimum_size.width.min(max_width);
    let min_height = policy.minimum_size.height.min(max_height);

    WindowSize {
        width: requested.width.clamp(min_width, max_width),
        height: requested.height.clamp(min_height, max_height),
    }
}

/// The height a companion window should take once its own content can be measured.
///
/// `natural_size` above is only a first guess made before a page exists, so a window that
/// keeps it arrives pre-truncated whenever its content is taller - with the desktop still
/// half empty. The size that matters is the one the built page asks for, bounded by the
/// work area the shell reports (never the raw monitor rectangle: a panel or a dock must
/// stay uncovered) and by the policy minimum.
///
/// `shown` is the height this session has already presented for the same window. Paging
/// between surfaces of different lengths therefore grows the window to fit and never
/// shrinks it back, so switching pages cannot make the window jump smaller under the
/// pointer. A page taller than the work area is not grown into: it scrolls.
pub fn companion_content_height(
    kind: CompanionWindowKind,
    content_natural: i32,
    shown: i32,
    work_area_height: i32,
) -> i32 {
    let policy = companion_window_policy(kind);
    let cap = work_area_height.max(1);
    let floor = policy.minimum_size.height.max(shown).min(cap);
    content_natural.clamp(floor, cap)
}

/// Whether a companion window's size has stopped being the shell's to choose.
///
/// `applied` is the last size the shell asked for and `observed` is the size the window
/// actually has. A difference in either dimension means someone else decided - the reader
/// dragging any edge, or a compositor placing the window - and a size chosen out there
/// outranks a size a page wants, so automatic sizing ends for the session. Width counts as
/// much as height: a page is measured at the width it will be laid out at, so a window the
/// reader widened has to be left alone as much as one they made shorter.
pub fn companion_size_taken_over(applied: WindowSize, observed: WindowSize) -> bool {
    if observed.width <= 0 || observed.height <= 0 {
        return false;
    }
    observed != applied
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK_AREA: WindowRect = WindowRect {
        x: 0,
        y: 0,
        width: 1280,
        height: 852,
    };

    #[test]
    fn every_companion_is_non_modal_resizable_and_single_instance() {
        for kind in [
            CompanionWindowKind::Settings,
            CompanionWindowKind::MediaInfo,
        ] {
            let policy = companion_window_policy(kind);
            assert!(!policy.modal);
            assert!(policy.resizable);
            assert!(!policy.always_on_top);
            assert!(policy.single_instance);
            assert!(policy.parent_input_enabled);
        }
        assert!(companion_window_policy(CompanionWindowKind::Settings).retain_on_close);
        assert!(!companion_window_policy(CompanionWindowKind::MediaInfo).retain_on_close);
    }

    #[test]
    fn natural_and_restored_sizes_stay_inside_the_work_area() {
        assert_eq!(
            companion_window_size(CompanionWindowKind::MediaInfo, None, WORK_AREA),
            WindowSize {
                width: 720,
                height: 571,
            }
        );
        assert_eq!(
            companion_window_size(
                CompanionWindowKind::MediaInfo,
                Some(WindowSize {
                    width: 1600,
                    height: 1000,
                }),
                WORK_AREA,
            ),
            WindowSize {
                width: 1280,
                height: 852,
            }
        );
    }

    #[test]
    fn a_small_work_area_wins_over_the_normal_minimum() {
        assert_eq!(
            companion_window_size(
                CompanionWindowKind::Settings,
                None,
                WindowRect {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 400,
                },
            ),
            WindowSize {
                width: 640,
                height: 400,
            }
        );
    }

    #[test]
    fn a_page_that_fits_opens_at_its_own_height_instead_of_the_first_guess() {
        // The About page measured on a 1080p desktop: taller than the 560px first guess
        // and far shorter than the work area, so nothing may be cut off.
        assert_eq!(
            companion_content_height(CompanionWindowKind::Settings, 753, 0, 1032),
            753
        );
    }

    #[test]
    fn a_page_taller_than_the_work_area_stops_at_the_work_area_and_scrolls() {
        assert_eq!(
            companion_content_height(CompanionWindowKind::Settings, 1400, 0, 1032),
            1032
        );
    }

    #[test]
    fn a_short_page_never_goes_under_the_policy_minimum() {
        assert_eq!(
            companion_content_height(CompanionWindowKind::Settings, 320, 0, 1032),
            480
        );
    }

    #[test]
    fn paging_grows_the_window_and_never_shrinks_it_back() {
        let about = companion_content_height(CompanionWindowKind::Settings, 753, 0, 1032);
        let shortcuts = companion_content_height(CompanionWindowKind::Settings, 900, about, 1032);
        assert_eq!(shortcuts, 900);
        // Back to the shorter page: the window keeps the height it already showed.
        assert_eq!(
            companion_content_height(CompanionWindowKind::Settings, 753, shortcuts, 1032),
            900
        );
    }

    #[test]
    fn a_size_the_shell_did_not_ask_for_ends_automatic_sizing() {
        let height = companion_content_height(CompanionWindowKind::Settings, 753, 0, 1032);
        let applied = WindowSize { width: 760, height };
        // The window is the size it was asked to be: still the shell's to grow.
        assert!(!companion_size_taken_over(applied, applied));
        // Dragged shorter by hand, or placed by a compositor: no longer.
        assert!(companion_size_taken_over(
            applied,
            WindowSize {
                width: 760,
                height: 620,
            }
        ));
        // Dragged only sideways counts too: the page is measured at that width.
        assert!(companion_size_taken_over(
            applied,
            WindowSize { width: 900, height }
        ));
        // An unmapped window has no allocation yet and decides nothing.
        assert!(!companion_size_taken_over(
            applied,
            WindowSize {
                width: 0,
                height: 0,
            }
        ));
        assert!(!companion_size_taken_over(
            applied,
            WindowSize {
                width: 760,
                height: 0,
            }
        ));
    }

    #[test]
    fn a_work_area_smaller_than_what_was_shown_still_wins() {
        // A window dragged onto a shorter monitor may not keep a height that would hang
        // over the panel there.
        assert_eq!(
            companion_content_height(CompanionWindowKind::Settings, 900, 900, 600),
            600
        );
    }
}
