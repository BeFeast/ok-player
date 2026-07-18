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
}
