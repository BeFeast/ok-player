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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecEnvironment {
    System,
    FedoraRpm,
    Flatpak,
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

    /// The no-DRI Flatpak fallback could not initialize or present. This is
    /// deliberately explicit about both the missing resource and the sandbox
    /// permission that restores the normal renderer.
    pub fn flatpak_dri_unavailable(detail: impl Into<String>) -> Self {
        Self {
            kind: PlaybackFailureKind::Renderer,
            title: "Graphics access unavailable".to_owned(),
            message: "OK Player could not start its software fallback because GPU/DRI access is unavailable. Grant the Flatpak --device=dri permission, then retry."
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
    environment: CodecEnvironment,
) -> PlaybackFailureDiagnostic {
    let normalized = engine_messages
        .iter()
        .map(|message| message.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if normalized.iter().any(|message| is_codec_failure(message)) {
        return missing_codec_diagnostic(environment, Some(error_code));
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
    environment: CodecEnvironment,
) -> Option<PlaybackFailureDiagnostic> {
    engine_messages
        .iter()
        .map(|message| message.to_ascii_lowercase())
        .any(|message| is_codec_failure(&message))
        .then(|| {
            let mut diagnostic = missing_codec_diagnostic(environment, None);
            diagnostic.detail = match environment {
                CodecEnvironment::Flatpak => {
                    "libmpv reached EOF after reporting an unavailable Flatpak codec extension."
                        .to_owned()
                }
                _ => "libmpv reached EOF after reporting an unavailable system codec.".to_owned(),
            };
            diagnostic
        })
}

/// Diagnose a decoder warning as soon as libmpv emits it. This path is used
/// when another stream (usually audio) would otherwise keep the clock moving
/// until EOF behind a video track that never produced a frame.
pub fn diagnose_mpv_runtime(
    engine_messages: &[String],
    environment: CodecEnvironment,
) -> Option<PlaybackFailureDiagnostic> {
    engine_messages
        .iter()
        .map(|message| message.to_ascii_lowercase())
        .any(|message| is_codec_failure(&message))
        .then(|| missing_codec_diagnostic(environment, None))
}

/// Lightweight semantic filter used by engine wrappers before they enqueue a
/// runtime decoder-failure event. The user-facing diagnosis remains in this
/// module so shells never parse libmpv log text.
pub fn is_mpv_codec_failure(message: &str) -> bool {
    is_codec_failure(&message.to_ascii_lowercase())
}

fn missing_codec_diagnostic(
    environment: CodecEnvironment,
    error_code: Option<i32>,
) -> PlaybackFailureDiagnostic {
    let message = match environment {
        CodecEnvironment::FedoraRpm => {
            "The system codec needed by this file is unavailable. OK Player uses Fedora's system mpv/FFmpeg libraries. Optionally follow RPM Fusion's official setup instructions to add broader codec support; OK Player will not enable third-party repositories for you."
        }
        CodecEnvironment::Flatpak => {
            "This Flatpak runtime does not include the codec needed by this file. Install the matching org.freedesktop.Platform.codecs-extra extension, then retry."
        }
        CodecEnvironment::System => {
            "The system mpv/FFmpeg installation does not provide the codec needed by this file. Install codec support through your operating system, then retry."
        }
    };
    let detail = match (environment, error_code) {
        (CodecEnvironment::Flatpak, Some(error_code)) => {
            format!("Required Flatpak codec extension is unavailable (libmpv error {error_code}).")
        }
        (CodecEnvironment::Flatpak, None) => {
            "Required Flatpak codec extension is unavailable.".to_owned()
        }
        (_, Some(error_code)) => {
            format!("Required system codec is unavailable (libmpv error {error_code}).")
        }
        (_, None) => "Required system codec is unavailable.".to_owned(),
    };
    PlaybackFailureDiagnostic {
        kind: PlaybackFailureKind::MissingCodec,
        title: "Codec unavailable".to_owned(),
        message: message.to_owned(),
        detail,
    }
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
            CodecEnvironment::FedoraRpm,
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
            CodecEnvironment::System,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert!(!diagnostic.message.contains("RPM Fusion"));
    }

    #[test]
    fn gpu_initialization_is_not_reported_as_a_codec_problem() {
        let diagnostic = diagnose_mpv_failure(
            -12,
            &messages(&["vo/gpu: Failed initializing any suitable GPU context"]),
            CodecEnvironment::FedoraRpm,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Renderer);
        assert_eq!(diagnostic.title, "Video renderer unavailable");
        assert!(!diagnostic.message.contains("RPM Fusion"));
    }

    #[test]
    fn unknown_failure_stays_a_generic_application_error() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &messages(&["stream: HTTP error 404"]),
            CodecEnvironment::FedoraRpm,
        );

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
            CodecEnvironment::FedoraRpm,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
    }

    #[test]
    fn decoder_failure_at_eof_remains_a_codec_diagnostic() {
        let diagnostic = diagnose_mpv_eof(
            &messages(&["[ffmpeg/video] Decoder not found for codec hevc"]),
            CodecEnvironment::FedoraRpm,
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
        assert!(
            diagnose_mpv_eof(
                &messages(&["cplayer: finished playback"]),
                CodecEnvironment::FedoraRpm,
            )
            .is_none()
        );
    }

    #[test]
    fn flatpak_runtime_decoder_failure_names_the_codec_extension() {
        let diagnostic = diagnose_mpv_runtime(
            &messages(&["vd: Failed to initialize a decoder for codec h264"]),
            CodecEnvironment::Flatpak,
        )
        .expect("decoder warning should fail immediately");

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert!(diagnostic.message.contains("codecs-extra"));
        assert_eq!(
            diagnostic.detail,
            "Required Flatpak codec extension is unavailable."
        );
    }

    #[test]
    fn benign_runtime_warning_does_not_stop_playback() {
        assert!(
            diagnose_mpv_runtime(
                &messages(&["ao/pipewire: underrun recovered"]),
                CodecEnvironment::Flatpak,
            )
            .is_none()
        );
    }

    #[test]
    fn flatpak_dri_failure_names_the_missing_access_and_permission() {
        let diagnostic = PlaybackFailureDiagnostic::flatpak_dri_unavailable(
            "libmpv software render context initialization failed",
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Renderer);
        assert_eq!(diagnostic.title, "Graphics access unavailable");
        assert!(diagnostic.message.contains("GPU/DRI"));
        assert!(diagnostic.message.contains("--device=dri"));
        assert_eq!(
            diagnostic.detail,
            "libmpv software render context initialization failed"
        );
    }
}
