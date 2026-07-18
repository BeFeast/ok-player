//! Playback-failure diagnostics shared by desktop shells.
//!
//! libmpv reports a compact numeric end-file error and emits the useful cause
//! through log-message event payloads. This module classifies those payloads so
//! shells can distinguish a missing system codec from a renderer/GPU failure
//! without parsing engine text themselves or issuing a blocking property read.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackFailureKind {
    MissingCodec,
    Renderer,
    Application,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackFailureDiagnostic {
    pub kind: PlaybackFailureKind,
    pub title: String,
    pub message: String,
    /// Short copyable reason. Raw engine logs are deliberately not exposed.
    pub detail: String,
}

impl PlaybackFailureDiagnostic {
    pub fn application(detail: impl Into<String>) -> Self {
        Self {
            kind: PlaybackFailureKind::Application,
            title: "Playback failed".to_owned(),
            message: "OK Player could not open this source. You can retry or choose another."
                .to_owned(),
            detail: detail.into(),
        }
    }
}

/// Classify a failed libmpv load from the error event and the warning/error log
/// payloads that preceded it for the same source.
pub fn diagnose_mpv_failure(
    error_code: i32,
    engine_messages: &[String],
    native_fedora_rpm: bool,
) -> PlaybackFailureDiagnostic {
    let normalized = engine_messages
        .iter()
        .map(|message| message.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if normalized.iter().any(|message| is_codec_failure(message)) {
        let message = if native_fedora_rpm {
            "The system codec needed by this file is unavailable. OK Player uses Fedora's system mpv/FFmpeg libraries. Optionally follow RPM Fusion's official setup instructions to add broader codec support; OK Player will not enable third-party repositories for you."
        } else {
            "The system mpv/FFmpeg installation does not provide the codec needed by this file. Install codec support through your operating system, then retry."
        };
        return PlaybackFailureDiagnostic {
            kind: PlaybackFailureKind::MissingCodec,
            title: "Codec unavailable".to_owned(),
            message: message.to_owned(),
            detail: format!("Required system codec is unavailable (libmpv error {error_code})."),
        };
    }

    if normalized
        .iter()
        .any(|message| is_renderer_failure(message))
    {
        return PlaybackFailureDiagnostic {
            kind: PlaybackFailureKind::Renderer,
            title: "Video renderer unavailable".to_owned(),
            message: "OK Player could not initialize video output. Check the graphics driver and the current Wayland/X11 session, then retry."
                .to_owned(),
            detail: format!("Video output or GPU initialization failed (libmpv error {error_code})."),
        };
    }

    PlaybackFailureDiagnostic::application(format!("libmpv error {error_code}"))
}

/// Diagnose an EOF that still carried an active-stream decoder failure. mpv can
/// report this shape after `FileLoaded` when another stream kept the demuxer
/// alive, so the absence of `EndFileReason::Error` does not make the decoder
/// warning harmless. Benign EOF log traffic is ignored.
pub fn diagnose_mpv_eof(
    engine_messages: &[String],
    native_fedora_rpm: bool,
) -> Option<PlaybackFailureDiagnostic> {
    engine_messages
        .iter()
        .map(|message| message.to_ascii_lowercase())
        .any(|message| is_codec_failure(&message))
        .then(|| {
            let mut diagnostic = diagnose_mpv_failure(0, engine_messages, native_fedora_rpm);
            diagnostic.detail =
                "libmpv reached EOF after reporting an unavailable system codec.".to_owned();
            diagnostic
        })
}

fn is_codec_failure(message: &str) -> bool {
    [
        "codec not found",
        "decoder not found",
        "failed to find a decoder",
        "no decoder for",
        "no decoder found",
        "unsupported codec",
        "could not open codec",
        "failed to open codec",
        "could not initialize decoder",
        "failed to initialize a decoder",
        "failed to initialize decoder",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn is_renderer_failure(message: &str) -> bool {
    [
        "failed initializing any suitable gpu context",
        "failed to initialize a gpu context",
        "failed to create rendering context",
        "could not create rendering context",
        "could not initialize video output",
        "failed to initialize video output",
        "video output init failed",
        "no video output",
        "vo/gpu: failed",
        "libplacebo: failed",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn missing_decoder_gets_optional_fedora_remediation() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &messages(&["[ffmpeg/video] Decoder not found for codec hevc"]),
            true,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert_eq!(diagnostic.title, "Codec unavailable");
        assert!(diagnostic.message.contains("RPM Fusion"));
        assert!(diagnostic.message.contains("Optionally"));
        assert!(diagnostic.message.contains("will not enable"));
        assert!(!diagnostic.detail.contains("ffmpeg/video"));
    }

    #[test]
    fn non_fedora_codec_message_does_not_recommend_a_specific_repository() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &messages(&["Failed to initialize a decoder for stream 0"]),
            false,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert!(!diagnostic.message.contains("RPM Fusion"));
    }

    #[test]
    fn gpu_initialization_is_not_reported_as_a_codec_problem() {
        let diagnostic = diagnose_mpv_failure(
            -12,
            &messages(&["vo/gpu: Failed initializing any suitable GPU context"]),
            true,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Renderer);
        assert_eq!(diagnostic.title, "Video renderer unavailable");
        assert!(!diagnostic.message.contains("RPM Fusion"));
    }

    #[test]
    fn unknown_failure_stays_a_generic_application_error() {
        let diagnostic = diagnose_mpv_failure(-13, &messages(&["stream: HTTP error 404"]), true);

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Application);
        assert_eq!(diagnostic.title, "Playback failed");
        assert_eq!(diagnostic.detail, "libmpv error -13");
    }

    #[test]
    fn codec_classification_wins_when_renderer_noise_is_also_present() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &messages(&[
                "vo/gpu: Failed to create rendering context",
                "ad: Could not open codec",
            ]),
            true,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
    }

    #[test]
    fn decoder_failure_at_eof_remains_a_codec_diagnostic() {
        let diagnostic = diagnose_mpv_eof(
            &messages(&["[ffmpeg/video] Decoder not found for codec hevc"]),
            true,
        )
        .expect("decoder failure must not be discarded just because mpv reported EOF");

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert!(diagnostic.message.contains("RPM Fusion"));
        assert_eq!(
            diagnostic.detail,
            "libmpv reached EOF after reporting an unavailable system codec."
        );
    }

    #[test]
    fn benign_eof_logs_do_not_become_failures() {
        assert!(diagnose_mpv_eof(&messages(&["cplayer: finished playback"]), true).is_none());
    }
}
