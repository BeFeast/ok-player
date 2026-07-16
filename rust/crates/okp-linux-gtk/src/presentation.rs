use super::*;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use okp_core::presentation_evidence::{
    PRESENTATION_EVIDENCE_SCHEMA_VERSION, PresentationAction, PresentationBackend,
    PresentationRecord,
};

static MONOTONIC_ORIGIN: OnceLock<Instant> = OnceLock::new();

pub(crate) struct PresentationRecorder {
    writer: Mutex<BufWriter<File>>,
    sequence: AtomicU64,
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
        let recorder = Arc::new(Self {
            writer: Mutex::new(BufWriter::new(file)),
            sequence: AtomicU64::new(0),
        });
        recorder.write(&PresentationRecord::Session {
            schema_version: PRESENTATION_EVIDENCE_SCHEMA_VERSION,
            backend,
        });
        Some(recorder)
    }

    pub(crate) fn record_present(&self, size: okp_mpv::RenderTargetSize, boundary: &str) {
        self.write(&PresentationRecord::Present {
            monotonic_ns: monotonic_ns(),
            sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
            width: size.width,
            height: size.height,
            boundary: boundary.to_owned(),
        });
    }

    pub(crate) fn record_playback(
        &self,
        playback: PlaybackState,
        diagnostics: okp_mpv::PlaybackDiagnostics,
    ) {
        self.write(&PresentationRecord::Playback {
            monotonic_ns: monotonic_ns(),
            time_pos: playback.time_pos,
            speed: playback.speed.unwrap_or(1.0),
            hwdec_current: diagnostics.hwdec_current,
            decoder_drops: diagnostics.decoder_drops,
            vo_drops: diagnostics.vo_drops,
        });
    }

    pub(crate) fn record_action(&self, action: PresentationAction) {
        self.write(&PresentationRecord::Action {
            monotonic_ns: monotonic_ns(),
            action,
        });
    }

    fn write(&self, record: &PresentationRecord) {
        let json = match serde_json::to_string(record) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("Failed to serialize presentation evidence: {error}");
                return;
            }
        };
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if writeln!(writer, "{json}")
            .and_then(|_| writer.flush())
            .is_err()
        {
            eprintln!("Failed to write presentation evidence");
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
