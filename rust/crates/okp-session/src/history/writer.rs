//! Writes the history file on a thread of its own, so the session never holds
//! its lock while the file is written and flushed to disk. On Apple platforms
//! the flush is `F_FULLFSYNC`, which waits for the drive's cache.
//!
//! Every document holds the whole history, so a newer one queued before the
//! thread gets to an older one replaces it: however many changes arrive, at most
//! one write is waiting.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use super::store::HistoryFile;

/// Called on the writer thread after each write, so the session collects the
/// outcome. It must not wait for the session.
pub(crate) type Waker = Arc<dyn Fn() + Send + Sync>;

pub(crate) struct Writer {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    /// A document was queued, or the writer should stop.
    work: Condvar,
    /// A write finished.
    finished: Condvar,
}

#[derive(Default)]
struct State {
    /// The newest document not yet written.
    pending: Option<Vec<u8>>,
    /// How many documents have been queued.
    queued: u64,
    /// The number of the last document written or failed; a document replaced
    /// before it was written counts as done with the one that replaced it.
    done: u64,
    /// What the latest write did, until the session collects it.
    outcome: Option<Outcome>,
    /// Where the file that could not be read was moved.
    kept: Option<PathBuf>,
    waker: Option<Waker>,
    stopping: bool,
    /// Writes carried out.
    writes: usize,
}

/// What the writes since the session last looked did.
pub(crate) struct Outcome {
    /// The result of the latest write.
    pub(crate) result: io::Result<()>,
    /// One of them moved the unreadable file aside.
    pub(crate) moved_aside: bool,
}

impl Writer {
    /// Start the thread that writes `file`.
    pub(crate) fn start(file: HistoryFile) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                kept: file.kept().map(Path::to_path_buf),
                ..State::default()
            }),
            work: Condvar::new(),
            finished: Condvar::new(),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("okp-session-history".to_owned())
            .spawn(move || run(&thread_shared, file))
            .expect("spawning the history writer thread must succeed");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    pub(crate) fn set_waker(&self, waker: Waker) {
        lock(&self.shared.state).waker = Some(waker);
    }

    /// Queue `document` for writing, replacing one still waiting, and return its
    /// number.
    pub(crate) fn submit(&self, document: Vec<u8>) -> u64 {
        let mut state = lock(&self.shared.state);
        state.pending = Some(document);
        state.queued += 1;
        let number = state.queued;
        drop(state);
        self.shared.work.notify_one();
        number
    }

    /// Write `document` and wait for the result. The caller must not queue
    /// anything else meanwhile, so the result is this document's.
    pub(crate) fn write_now(&self, document: Vec<u8>) -> io::Result<()> {
        let number = self.submit(document);
        let mut state = self.wait_until(lock(&self.shared.state), number);
        state
            .outcome
            .take()
            .map_or(Ok(()), |outcome| outcome.result)
    }

    /// Wait until every queued document has been written.
    pub(crate) fn wait_idle(&self) {
        let state = lock(&self.shared.state);
        let queued = state.queued;
        drop(self.wait_until(state, queued));
    }

    fn wait_until<'a>(
        &'a self,
        mut state: MutexGuard<'a, State>,
        number: u64,
    ) -> MutexGuard<'a, State> {
        while state.done < number {
            state = self
                .shared
                .finished
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state
    }

    /// What the writes since the last call did, if any finished.
    pub(crate) fn take_outcome(&self) -> Option<Outcome> {
        lock(&self.shared.state).outcome.take()
    }

    /// Where the file that could not be read was moved, once it has been.
    pub(crate) fn kept(&self) -> Option<PathBuf> {
        lock(&self.shared.state).kept.clone()
    }

    #[cfg(test)]
    pub(crate) fn probe(&self) -> Probe {
        Probe(Arc::clone(&self.shared))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        lock(&self.shared.state).stopping = true;
        self.shared.work.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The writer thread: write the newest queued document until told to stop,
/// writing whatever is still queued first.
fn run(shared: &Shared, mut file: HistoryFile) {
    loop {
        let (number, document) = {
            let mut state = lock(&shared.state);
            loop {
                if let Some(document) = state.pending.take() {
                    break (state.queued, document);
                }
                if state.stopping {
                    return;
                }
                state = shared
                    .work
                    .wait(state)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let had_kept = file.kept().is_some();
        let result = file.write(&document);
        let waker = {
            let mut state = lock(&shared.state);
            let moved_aside = (!had_kept && file.kept().is_some())
                || state
                    .outcome
                    .as_ref()
                    .is_some_and(|outcome| outcome.moved_aside);
            state.outcome = Some(Outcome {
                result,
                moved_aside,
            });
            state.kept = file.kept().map(Path::to_path_buf);
            state.done = number;
            state.writes += 1;
            state.waker.clone()
        };
        shared.finished.notify_all();
        if let Some(waker) = waker {
            waker();
        }
    }
}

/// A test's view of the writer behind a session.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct Probe(Arc<Shared>);

#[cfg(test)]
impl Probe {
    /// Wait until every queued document has been written.
    pub(crate) fn settle(&self) {
        let mut state = lock(&self.0.state);
        let queued = state.queued;
        while state.done < queued {
            state = self
                .0
                .finished
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// How many times the file has been written.
    pub(crate) fn writes(&self) -> usize {
        lock(&self.0.state).writes
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
