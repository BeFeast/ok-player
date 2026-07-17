//! Portable classification and user-facing copy for libmpv playback failures.
//!
//! libmpv's `EndFile::Error` code says that loading failed, but not whether the
//! root cause was a missing decoder, the video output, the source, or the
//! application boundary. The event pump therefore supplies the warning/error
//! log payloads that preceded the event and this module turns them into a
//! stable diagnostic. Shells only render the result.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackFailureKind {
    MissingCodec,
    Renderer,
    Source,
    Application,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackFailureEnvironment {
    Generic,
    FedoraNative,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybackFailureDiagnostic {
    pub kind: PlaybackFailureKind,
    pub title: &'static str,
    pub body: String,
    pub details: String,
}

pub fn diagnose_mpv_failure(
    error_code: i32,
    log_lines: &[String],
    environment: PlaybackFailureEnvironment,
) -> PlaybackFailureDiagnostic {
    let normalized = log_lines
        .iter()
        .map(|line| line.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");

    let kind = if contains_any(
        &normalized,
        &[
            "failed to initialize a decoder for codec",
            "could not open codec",
            "decoder init failed",
            "no decoder found for codec",
            "no decoder could be found for codec",
            "decoder not found",
            "video decoder init failed",
            "audio decoder init failed",
            "codec not currently supported in container",
        ],
    ) {
        PlaybackFailureKind::MissingCodec
    } else if contains_any(
        &normalized,
        &[
            "failed initializing any suitable gpu context",
            "failed to create ra context",
            "could not initialize video chain",
            "video output initialization failed",
            "vo/libmpv: failed to initialize",
            "failed to open display",
        ],
    ) {
        PlaybackFailureKind::Renderer
    } else if contains_any(
        &normalized,
        &[
            "file not found",
            "failed to open",
            "http error",
            "server returned",
            "unrecognized file format",
            "invalid data found when processing input",
        ],
    ) {
        PlaybackFailureKind::Source
    } else {
        PlaybackFailureKind::Application
    };

    let (title, body) = match (kind, environment) {
        (PlaybackFailureKind::MissingCodec, PlaybackFailureEnvironment::FedoraNative) => (
            "Codec unavailable",
            "This Fedora installation does not include the codec needed by this media. Optional RPM Fusion multimedia packages can add support."
                .to_owned(),
        ),
        (PlaybackFailureKind::MissingCodec, PlaybackFailureEnvironment::Generic) => (
            "Codec unavailable",
            "This system does not currently provide the codec needed by this media."
                .to_owned(),
        ),
        (PlaybackFailureKind::Renderer, _) => (
            "Video output unavailable",
            "The media decoder started, but the GPU or video output could not be initialized."
                .to_owned(),
        ),
        (PlaybackFailureKind::Source, _) => (
            "Can't open this source",
            "The file or stream could not be read. It may be missing, unavailable, or damaged."
                .to_owned(),
        ),
        (PlaybackFailureKind::Application, _) => (
            "Playback failed",
            "OK Player could not open this source. You can retry or choose another."
                .to_owned(),
        ),
    };

    let mut details = vec![format!("libmpv error {error_code}")];
    details.extend(
        log_lines
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned),
    );
    if kind == PlaybackFailureKind::MissingCodec
        && environment == PlaybackFailureEnvironment::FedoraNative
    {
        details.push(
            "Optional remediation: configure RPM Fusion from https://rpmfusion.org/Configuration and install its multimedia codec packages. OK Player never enables third-party repositories automatically."
                .to_owned(),
        );
    }

    PlaybackFailureDiagnostic {
        kind,
        title,
        body,
        details: details.join("\n"),
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn missing_decoder_is_not_reported_as_a_renderer_failure() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &lines(&[
                "vd: Failed to initialize a decoder for codec 'hevc'.",
                "vo/libmpv: could not initialize video chain",
            ]),
            PlaybackFailureEnvironment::FedoraNative,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert_eq!(diagnostic.title, "Codec unavailable");
        assert!(diagnostic.body.contains("Optional RPM Fusion"));
        assert!(diagnostic.details.contains("rpmfusion.org/Configuration"));
        assert!(diagnostic.details.contains("never enables"));
    }

    #[test]
    fn gpu_initialization_failure_has_distinct_copy() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &lines(&["vo/gpu: Failed initializing any suitable GPU context!"]),
            PlaybackFailureEnvironment::Generic,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Renderer);
        assert_eq!(diagnostic.title, "Video output unavailable");
        assert!(!diagnostic.body.contains("codec"));
    }

    #[test]
    fn source_failure_has_distinct_copy() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &lines(&["stream: HTTP error 404 Not Found"]),
            PlaybackFailureEnvironment::Generic,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Source);
        assert_eq!(diagnostic.title, "Can't open this source");
    }

    #[test]
    fn unknown_failure_keeps_the_generic_application_fallback() {
        let diagnostic = diagnose_mpv_failure(
            -20,
            &lines(&["unknown internal failure"]),
            PlaybackFailureEnvironment::Generic,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::Application);
        assert_eq!(diagnostic.title, "Playback failed");
        assert_eq!(
            diagnostic.details,
            "libmpv error -20\nunknown internal failure"
        );
    }

    #[test]
    fn generic_build_does_not_offer_fedora_remediation() {
        let diagnostic = diagnose_mpv_failure(
            -13,
            &lines(&["Could not open codec h264"]),
            PlaybackFailureEnvironment::Generic,
        );

        assert_eq!(diagnostic.kind, PlaybackFailureKind::MissingCodec);
        assert!(!diagnostic.body.contains("RPM Fusion"));
        assert!(!diagnostic.details.contains("rpmfusion.org"));
    }
}
