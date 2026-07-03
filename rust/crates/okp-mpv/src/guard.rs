//! Debug-only tripwire for blocking mpv property reads on the UI thread.
//!
//! The Windows shell learned in the #33 open-time freeze that a synchronous
//! mpv call issued from the thread that drives the UI can deadlock against a
//! briefly-busy core. Its DEBUG render-thread guard
//! (`MpvContext.MarkRenderThread`) turned that class of freeze into a
//! deterministic dev-time failure. This module is the Rust equivalent for the
//! GTK shell: the shell marks the GLib main-context thread at attach time,
//! and every blocking property read from that thread is recorded and reported
//! with a backtrace.
//!
//! Decision: violations hard-log (once per property shape) instead of
//! aborting. The 200 ms state poll and the popover builders that used to read
//! mpv synchronously now project the background event pump's snapshot instead
//! (see `pump`), so a green debug run issues no blocking reads at all. The
//! guard stays armed as the regression backstop: any new synchronous read that
//! creeps back onto the main context still logs a loud backtrace, and the
//! violation counter lets tests assert the tripwire fires. Release builds
//! compile the guard out entirely.

use std::backtrace::Backtrace;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::thread::{self, ThreadId};

#[derive(Default)]
pub(crate) struct BlockingReadGuard {
    ui_thread: Cell<Option<ThreadId>>,
    violations: Cell<usize>,
    reported: RefCell<HashSet<String>>,
}

impl BlockingReadGuard {
    pub(crate) fn mark_ui_thread(&self) {
        self.ui_thread.set(Some(thread::current().id()));
    }

    pub(crate) fn violations(&self) -> usize {
        self.violations.get()
    }

    pub(crate) fn check_blocking_read(&self, property: &str) {
        if !is_violation(self.ui_thread.get(), thread::current().id()) {
            return;
        }

        self.violations.set(self.violations.get() + 1);
        if self.reported.borrow_mut().insert(dedup_key(property)) {
            eprintln!(
                "[okp-mpv] blocking read of '{property}' on the marked UI (GLib main-context) \
                 thread: a busy core can block this call and freeze the UI. Read it via the \
                 event/observe path or off the main context instead.\n{}",
                Backtrace::force_capture()
            );
        }
    }
}

fn is_violation(ui_thread: Option<ThreadId>, current: ThreadId) -> bool {
    ui_thread == Some(current)
}

/// Collapses numeric path segments (`track-list/3/title` -> `track-list/*/title`)
/// so per-index loops report one backtrace per read shape, not one per item.
fn dedup_key(property: &str) -> String {
    property
        .split('/')
        .map(|segment| {
            if !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_digit()) {
                "*"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmarked_guard_never_flags() {
        assert!(!is_violation(None, thread::current().id()));
    }

    #[test]
    fn flags_only_the_marked_thread() {
        let marked = thread::current().id();
        let other = thread::spawn(|| thread::current().id()).join().unwrap();

        assert!(is_violation(Some(marked), marked));
        assert!(!is_violation(Some(marked), other));
    }

    #[test]
    fn counts_every_violation_but_reports_each_read_shape_once() {
        let guard = BlockingReadGuard::default();
        guard.mark_ui_thread();

        guard.check_blocking_read("time-pos");
        guard.check_blocking_read("time-pos");
        guard.check_blocking_read("track-list/0/title");
        guard.check_blocking_read("track-list/7/title");

        assert_eq!(guard.violations(), 4);
        assert_eq!(guard.reported.borrow().len(), 2);
    }

    #[test]
    fn reads_off_the_marked_thread_are_clean() {
        let other = thread::spawn(|| thread::current().id()).join().unwrap();
        let guard = BlockingReadGuard::default();
        guard.ui_thread.set(Some(other));

        guard.check_blocking_read("time-pos");

        assert_eq!(guard.violations(), 0);
    }

    #[test]
    fn dedup_key_collapses_numeric_segments() {
        assert_eq!(dedup_key("time-pos"), "time-pos");
        assert_eq!(dedup_key("track-list/12/lang"), "track-list/*/lang");
        assert_eq!(dedup_key("chapter-list/0/time"), "chapter-list/*/time");
    }
}
