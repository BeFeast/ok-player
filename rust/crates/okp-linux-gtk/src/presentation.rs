use super::*;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::Condvar;
use std::sync::atomic::{AtomicU64, Ordering};

use okp_core::presentation_evidence::{
    PRESENTATION_EVIDENCE_SCHEMA_VERSION, PresentationAction, PresentationBackend,
    PresentationRecord,
};

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

pub(crate) struct PresentationRecorder {
    queue: Arc<PresentationQueue>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    sequence: AtomicU64,
}

struct PresentationQueue {
    state: Mutex<PresentationQueueState>,
    wake: Condvar,
}

struct PresentationQueueState {
    pending: Vec<QueuedPresentationRecord>,
    shutdown: bool,
}

enum QueuedPresentationRecord {
    Present {
        monotonic_ns: u64,
        sequence: u64,
        width: i32,
        height: i32,
        boundary: &'static str,
    },
    Evidence {
        record: PresentationRecord,
        flush: bool,
    },
}

impl QueuedPresentationRecord {
    fn into_record(self) -> (PresentationRecord, bool) {
        match self {
            Self::Present {
                monotonic_ns,
                sequence,
                width,
                height,
                boundary,
            } => (
                PresentationRecord::Present {
                    monotonic_ns,
                    sequence,
                    width,
                    height,
                    boundary: boundary.to_owned(),
                },
                false,
            ),
            Self::Evidence { record, flush } => (record, flush),
        }
    }
}

impl PresentationRecorder {
    pub(crate) fn from_env(backend: PresentationBackend) -> Option<Arc<Self>> {
        let path = env::var_os("OKP_PRESENT_LOG")?;
        let file = match File::create(&path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("Failed to create presentation evidence log: {error}");
                return None;
            }
        };
        let queue = Arc::new(PresentationQueue {
            state: Mutex::new(PresentationQueueState {
                pending: Vec::with_capacity(256),
                shutdown: false,
            }),
            wake: Condvar::new(),
        });
        let worker_queue = Arc::clone(&queue);
        let worker = match std::thread::Builder::new()
            .name("okp-presentation-evidence".to_owned())
            .spawn(move || write_presentation_records(BufWriter::new(file), worker_queue))
        {
            Ok(worker) => worker,
            Err(error) => {
                eprintln!("Failed to start presentation evidence writer: {error}");
                return None;
            }
        };
        let recorder = Arc::new(Self {
            queue,
            worker: Mutex::new(Some(worker)),
            sequence: AtomicU64::new(0),
        });
        recorder.write(QueuedPresentationRecord::Evidence {
            record: PresentationRecord::Session {
                schema_version: PRESENTATION_EVIDENCE_SCHEMA_VERSION,
                backend,
            },
            flush: true,
        });
        Some(recorder)
    }

    pub(crate) fn record_present(&self, size: okp_mpv::RenderTargetSize, boundary: &'static str) {
        self.write(QueuedPresentationRecord::Present {
            monotonic_ns: monotonic_ns(),
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            width: size.width,
            height: size.height,
            boundary,
        });
    }

    pub(crate) fn record_playback(
        &self,
        playback: PlaybackState,
        diagnostics: okp_mpv::PlaybackDiagnostics,
    ) {
        self.write(QueuedPresentationRecord::Evidence {
            record: PresentationRecord::Playback {
                monotonic_ns: monotonic_ns(),
                time_pos: playback.time_pos,
                speed: playback.speed.unwrap_or(1.0),
                hwdec_current: diagnostics.hwdec_current,
                decoder_drops: diagnostics.decoder_drops,
                vo_drops: diagnostics.vo_drops,
            },
            flush: false,
        });
    }

    pub(crate) fn record_action(&self, action: PresentationAction) {
        self.write(QueuedPresentationRecord::Evidence {
            record: PresentationRecord::Action {
                monotonic_ns: monotonic_ns(),
                action,
            },
            flush: true,
        });
    }

    fn write(&self, record: QueuedPresentationRecord) {
        let flush = matches!(
            record,
            QueuedPresentationRecord::Evidence { flush: true, .. }
        );
        self.queue
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .push(record);
        if flush {
            self.queue.wake.notify_one();
        }
    }
}

impl Drop for PresentationRecorder {
    fn drop(&mut self) {
        self.queue
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shutdown = true;
        self.queue.wake.notify_one();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            && worker.join().is_err()
        {
            eprintln!("Presentation evidence writer panicked");
        }
    }
}

fn write_presentation_records(mut writer: BufWriter<File>, queue: Arc<PresentationQueue>) {
    loop {
        let (records, shutdown) = {
            let state = queue
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let (mut state, _) = queue
                .wake
                .wait_timeout(state, Duration::from_secs(1))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let records = std::mem::replace(&mut state.pending, Vec::with_capacity(256));
            (records, state.shutdown)
        };
        let mut flush = shutdown;
        for queued in records {
            let (record, record_flush) = queued.into_record();
            flush |= record_flush;
            let json = match serde_json::to_string(&record) {
                Ok(json) => json,
                Err(error) => {
                    eprintln!("Failed to serialize presentation evidence: {error}");
                    continue;
                }
            };
            if writeln!(writer, "{json}").is_err() {
                eprintln!("Failed to write presentation evidence");
                return;
            }
        }
        if flush && writer.flush().is_err() {
            eprintln!("Failed to flush presentation evidence");
            return;
        }
        if shutdown {
            return;
        }
    }
}

pub(crate) fn monotonic_ns() -> u64 {
    MONOTONIC_ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_nanos()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_evidence_clock_does_not_move_backwards() {
        let first = monotonic_ns();
        let second = monotonic_ns();
        assert!(second >= first);
    }
}
