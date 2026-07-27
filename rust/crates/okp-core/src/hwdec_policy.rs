//! Portable hardware-decoding policy: which mpv `hwdec` value the player asks
//! for, and when a running session must give hardware decoding back.
//!
//! Two rules live here, both learned from issue #675 on a legacy-`radeon`
//! Debian host. There mpv's own `auto-safe` resolved to `vaapi-copy`, and the
//! copy-back read-back cost dropped playback to 0.02x realtime — roughly
//! 1.5 fps — while software decode of the same 1080p30 H.264 file ran at
//! several hundred frames per second. Copy-back is not a graceful degradation
//! step on such a stack; it is strictly worse than not using the GPU at all.
//!
//! 1. **Automatic selection never lands on copy-back.** Instead of handing mpv
//!    an `auto*` family that may resolve to a `*-copy` backend, the automatic
//!    path names zero-copy backends explicitly and terminates the list with
//!    `no`, so the worst automatic outcome is software decode.
//! 2. **A running session self-corrects.** If the media clock falls far behind
//!    wall time while both the decoder and the VO report zero dropped frames —
//!    the exact signature of a read-back stall, as opposed to genuine overload,
//!    which drops frames — hardware decoding is demoted for the session.
//!
//! Both rules yield to the raw `mpv.conf` escape hatch: a user who names a
//! `hwdec` value by hand owns the outcome, including a copy-back one.
//!
//! Pure: no I/O, no mpv handle, no UI. Shells observe, call, and apply.

/// Zero-copy hardware decoders offered to mpv on Linux, in preference order,
/// terminated by `no`.
///
/// mpv walks the list and uses the first backend that initialises, so an
/// absent or broken GPU path ends at software decode rather than at a
/// copy-back backend. Every entry is checked by
/// [`automatic_hwdec_is_copy_free`], which the test suite asserts.
pub const LINUX_ZERO_COPY_HWDEC: &str = "vaapi,nvdec,vdpau,drm,no";

/// The mpv `hwdec` value meaning "decode on the CPU".
pub const HWDEC_OFF: &str = "no";

/// Wall-clock span a below-realtime observation must cover before it can
/// demote. Long enough that start-up, a seek, or a single slow frame cannot
/// trigger it; short enough that a 1.5 fps slideshow is not the steady state.
pub const DEMOTION_WINDOW_SECONDS: f64 = 3.0;

/// Media-clock-to-wall-clock ratio below which a drop-free session counts as
/// stalled. The observed defect ran at 0.02x and a healthy session at 1.01x,
/// so the boundary sits far from both: anything that still advances at half
/// speed is left alone.
pub const DEMOTION_CLOCK_RATIO: f64 = 0.5;

/// Where the effective mpv `hwdec` value came from. Only [`Self::Automatic`]
/// is subject to the runtime demotion rules; everything else is an explicit
/// decision by the user or by renderer policy and is left intact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HwdecSource {
    /// The player chose the value from the "hardware decoding" preference.
    Automatic,
    /// The user turned hardware decoding off in Settings.
    UserDisabled,
    /// Renderer policy forced software decoding (for example a Flatpak install
    /// with no usable `/dev/dri` node).
    RendererForced,
    /// The user set `hwdec` by hand in the raw `mpv.conf` escape hatch.
    UserOverride,
}

impl HwdecSource {
    /// Whether the player may change this value on its own while playing.
    pub const fn allows_runtime_demotion(self) -> bool {
        matches!(self, Self::Automatic)
    }
}

/// The `hwdec` value to start mpv with, and where it came from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HwdecPlan {
    pub value: String,
    pub source: HwdecSource,
}

/// Choose the `hwdec` value for a new mpv instance.
///
/// Precedence, strongest first: the raw `mpv.conf` escape hatch, renderer
/// policy, then the user's hardware-decoding preference. The escape hatch wins
/// outright — including when it names a copy-back backend — because the raw
/// config is the documented way to overrule OK Player's judgement.
///
/// * `hardware_decoding_enabled` — the Settings toggle.
/// * `renderer_forced_hwdec` — a value renderer policy insists on, if any.
/// * `raw_conf_hwdec` — the `hwdec` value found in the user's raw `mpv.conf`.
pub fn plan_hwdec(
    hardware_decoding_enabled: bool,
    renderer_forced_hwdec: Option<&str>,
    raw_conf_hwdec: Option<&str>,
) -> HwdecPlan {
    if let Some(value) = raw_conf_hwdec.map(str::trim).filter(|v| !v.is_empty()) {
        return HwdecPlan {
            value: value.to_owned(),
            source: HwdecSource::UserOverride,
        };
    }
    if let Some(value) = renderer_forced_hwdec
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return HwdecPlan {
            value: value.to_owned(),
            source: HwdecSource::RendererForced,
        };
    }
    if hardware_decoding_enabled {
        HwdecPlan {
            value: LINUX_ZERO_COPY_HWDEC.to_owned(),
            source: HwdecSource::Automatic,
        }
    } else {
        HwdecPlan {
            value: HWDEC_OFF.to_owned(),
            source: HwdecSource::UserDisabled,
        }
    }
}

/// The `hwdec` value the user set by hand in the raw `mpv.conf` escape hatch,
/// if any.
///
/// Takes the options the shell is about to hand the engine rather than the
/// config text, so the answer cannot drift from what mpv actually receives.
/// The engine applies them as flat set-option calls in file order, so the last
/// `hwdec` entry is the one that takes effect and the one returned here.
pub fn hwdec_from_options(options: &[(String, String)]) -> Option<&str> {
    options
        .iter()
        .rfind(|(name, _)| name.eq_ignore_ascii_case("hwdec"))
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
}

/// Whether an mpv hardware-decoder name reads frames back into system memory.
///
/// mpv spells these `<backend>-copy` (`vaapi-copy`, `nvdec-copy`, `drm-copy`,
/// …) plus the `auto-copy` family selector. Comparison is case-insensitive and
/// tolerant of surrounding whitespace so a value straight out of
/// `hwdec-current` can be passed in.
pub fn is_copy_back_hwdec(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("auto-copy")
        || value.eq_ignore_ascii_case("auto-copy-safe")
        || value
            .rsplit_once('-')
            .is_some_and(|(head, tail)| !head.is_empty() && tail.eq_ignore_ascii_case("copy"))
}

/// Whether a `hwdec` request can only resolve to zero-copy backends, checking
/// every entry of a comma-separated candidate list.
///
/// `auto`, `auto-safe`, and friends are *not* copy-free: they are exactly the
/// selectors that produced `vaapi-copy` in issue #675.
pub fn automatic_hwdec_is_copy_free(value: &str) -> bool {
    value
        .split(',')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .all(|candidate| !is_copy_back_hwdec(candidate) && !is_family_selector(candidate))
}

/// Whether a candidate hands the choice back to mpv (`auto`, `auto-safe`, …),
/// which is how issue #675 arrived at `vaapi-copy` in the first place.
fn is_family_selector(candidate: &str) -> bool {
    let candidate = candidate.trim();
    candidate.eq_ignore_ascii_case("auto")
        || candidate
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("auto-"))
}

/// One observation of a playing session, taken from the shell's poll tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackSample {
    /// Monotonic wall-clock timestamp of the observation.
    pub monotonic_ns: u64,
    /// `time-pos`, the media clock, in seconds.
    pub media_time: f64,
    pub paused: bool,
    /// The engine reports it is not actively playing — mpv's `core-idle` or
    /// `paused-for-cache`. A network cache refill freezes the media clock
    /// without dropping a frame, which would otherwise look exactly like a
    /// stalled decoder, so such a span is never scored.
    pub awaiting_data: bool,
    /// Playback speed, so a deliberate slow-motion session is not mistaken for
    /// a stall.
    pub speed: f64,
    /// `decoder-frame-drop-count`.
    pub decoder_drops: i64,
    /// `frame-drop-count`, the VO's own drop counter.
    pub vo_drops: i64,
}

/// Why hardware decoding was given back.
#[derive(Clone, Debug, PartialEq)]
pub enum HwdecDemotion {
    /// mpv resolved the request to a copy-back backend anyway.
    CopyBackSelected { observed: String },
    /// The media clock fell far behind wall time while nothing was dropped.
    BelowRealtimeWithoutDrops {
        /// Observed media-clock advance per second of wall clock.
        clock_ratio: f64,
        /// Wall-clock span the ratio was measured over.
        window_seconds: f64,
        /// The hardware decoder that was active.
        observed: String,
    },
}

impl HwdecDemotion {
    /// A single log line for the application log, explaining the change in the
    /// terms an operator can act on.
    pub fn log_message(&self) -> String {
        match self {
            Self::CopyBackSelected { observed } => format!(
                "Hardware decoding demoted to hwdec={HWDEC_OFF}: mpv selected the copy-back \
                 backend '{observed}', which reads every frame back over the bus and is slower \
                 than software decoding on this system. Set hwdec in Settings → Advanced → \
                 mpv.conf to override."
            ),
            Self::BelowRealtimeWithoutDrops {
                clock_ratio,
                window_seconds,
                observed,
            } => format!(
                "Hardware decoding demoted to hwdec={HWDEC_OFF}: hwdec='{observed}' advanced the \
                 media clock at {clock_ratio:.2}x realtime over {window_seconds:.1}s with zero \
                 decoder and VO frame drops, the signature of a stalled hardware decode path. \
                 Set hwdec in Settings → Advanced → mpv.conf to override."
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    monotonic_ns: u64,
    media_time: f64,
    /// The speed the span was opened at. A span may only be scored against one
    /// speed, so a change re-anchors instead of measuring a 0.25x stretch
    /// against a 1x expectation.
    speed: f64,
    decoder_drops: i64,
    vo_drops: i64,
}

/// Watches a playing session and reports the single moment hardware decoding
/// must be given back.
///
/// The guard is deliberately sticky: it demotes at most once, because the
/// demotion applies to the mpv session, and a value that flapped between
/// hardware and software would be worse than either. A new anchor is taken
/// whenever the measurement is invalidated (pause, seek, a dropped frame), so
/// only sustained, drop-free slowness can trip it.
#[derive(Clone, Debug)]
pub struct HwdecGuard {
    source: HwdecSource,
    demoted: bool,
    anchor: Option<Anchor>,
}

impl HwdecGuard {
    pub const fn new(source: HwdecSource) -> Self {
        Self {
            source,
            demoted: false,
            anchor: None,
        }
    }

    /// Whether this guard has already demoted hardware decoding.
    pub const fn has_demoted(&self) -> bool {
        self.demoted
    }

    /// Forget the in-flight measurement, without forgetting a demotion that
    /// already happened. Shells call this when the media changes or the
    /// playhead jumps, so a fresh file is not judged on the previous one.
    pub fn reset_measurement(&mut self) {
        self.anchor = None;
    }

    /// Take back a demotion the shell could not apply.
    ///
    /// [`Self::observe`] reports at most one demotion and then goes quiet, so a
    /// failed `hwdec` write would otherwise leave the bad backend running for
    /// the rest of the session with nothing left to catch it. Calling this
    /// re-arms the guard, and the next sustained stall reports again.
    pub fn demotion_failed(&mut self) {
        self.demoted = false;
        self.anchor = None;
    }

    /// Feed one poll-tick observation. Returns `Some` exactly once, on the tick
    /// that decides hardware decoding must be given back; the caller then sets
    /// mpv's `hwdec` to [`HWDEC_OFF`] and logs
    /// [`HwdecDemotion::log_message`].
    ///
    /// `hwdec_current` is mpv's `hwdec-current` property: `None` or `"no"`
    /// means software decoding is already in effect and there is nothing to
    /// demote.
    pub fn observe(
        &mut self,
        sample: PlaybackSample,
        hwdec_current: Option<&str>,
    ) -> Option<HwdecDemotion> {
        if self.demoted || !self.source.allows_runtime_demotion() {
            return None;
        }
        let Some(active) = hwdec_current
            .map(str::trim)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case(HWDEC_OFF))
        else {
            // Software decoding is already in effect; there is nothing to
            // demote and no meaningful span to keep measuring.
            self.anchor = None;
            return None;
        };

        if is_copy_back_hwdec(active) {
            self.demoted = true;
            self.anchor = None;
            return Some(HwdecDemotion::CopyBackSelected {
                observed: active.to_owned(),
            });
        }

        if sample.paused || sample.awaiting_data || !sample.speed.is_finite() || sample.speed <= 0.0
        {
            self.anchor = None;
            return None;
        }

        let Some(anchor) = self.anchor else {
            self.anchor = Some(Anchor::from(sample));
            return None;
        };

        // Any dropped frame means the machine is genuinely overloaded rather
        // than stalled on a read-back, and dropping frames is already mpv
        // degrading gracefully. Re-anchor instead of demoting.
        //
        // A speed change invalidates the span for a different reason: the media
        // clock advanced under one expectation and would be scored under
        // another.
        if sample.decoder_drops > anchor.decoder_drops
            || sample.vo_drops > anchor.vo_drops
            || sample.speed != anchor.speed
        {
            self.anchor = Some(Anchor::from(sample));
            return None;
        }

        let wall_seconds = sample.monotonic_ns.saturating_sub(anchor.monotonic_ns) as f64 / 1e9;
        let media_advance = sample.media_time - anchor.media_time;
        let expected = wall_seconds * sample.speed;

        // A seek in either direction, or a counter reset, invalidates the span.
        if media_advance < 0.0
            || sample.decoder_drops < anchor.decoder_drops
            || sample.vo_drops < anchor.vo_drops
            || (expected > 0.0 && media_advance > expected * 2.0)
        {
            self.anchor = Some(Anchor::from(sample));
            return None;
        }

        if wall_seconds < DEMOTION_WINDOW_SECONDS || expected <= 0.0 {
            return None;
        }

        let clock_ratio = media_advance / expected;
        if clock_ratio < DEMOTION_CLOCK_RATIO {
            self.demoted = true;
            self.anchor = None;
            return Some(HwdecDemotion::BelowRealtimeWithoutDrops {
                clock_ratio,
                window_seconds: wall_seconds,
                observed: active.to_owned(),
            });
        }

        // Healthy span: start a fresh one so the window stays recent.
        self.anchor = Some(Anchor::from(sample));
        None
    }
}

impl From<PlaybackSample> for Anchor {
    fn from(sample: PlaybackSample) -> Self {
        Self {
            monotonic_ns: sample.monotonic_ns,
            media_time: sample.media_time,
            speed: sample.speed,
            decoder_drops: sample.decoder_drops,
            vo_drops: sample.vo_drops,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND_NS: u64 = 1_000_000_000;

    fn sample(second: u64, media_time: f64) -> PlaybackSample {
        PlaybackSample {
            monotonic_ns: second * SECOND_NS,
            media_time,
            paused: false,
            awaiting_data: false,
            speed: 1.0,
            decoder_drops: 0,
            vo_drops: 0,
        }
    }

    #[test]
    fn automatic_selection_never_offers_a_copy_back_backend() {
        let plan = plan_hwdec(true, None, None);

        assert_eq!(plan.source, HwdecSource::Automatic);
        assert!(
            automatic_hwdec_is_copy_free(&plan.value),
            "automatic hwdec {} may resolve to copy-back",
            plan.value
        );
        for candidate in plan.value.split(',') {
            assert!(
                !is_copy_back_hwdec(candidate),
                "{candidate} is a copy-back backend"
            );
        }
    }

    #[test]
    fn automatic_selection_offers_a_zero_copy_backend_and_a_software_floor() {
        let plan = plan_hwdec(true, None, None);
        let candidates = plan.value.split(',').collect::<Vec<_>>();

        assert!(candidates.contains(&"vaapi"), "{candidates:?}");
        assert_eq!(
            candidates.last().copied(),
            Some(HWDEC_OFF),
            "the candidate list must end at software decoding"
        );
    }

    #[test]
    fn family_selectors_are_not_treated_as_copy_free() {
        for selector in ["auto", "auto-safe", "auto-copy", "auto-copy-safe"] {
            assert!(
                !automatic_hwdec_is_copy_free(selector),
                "{selector} delegates the choice back to mpv"
            );
        }
    }

    #[test]
    fn copy_back_names_are_recognised_across_backends() {
        for value in [
            "vaapi-copy",
            "nvdec-copy",
            "drm-copy",
            "cuda-copy",
            "VAAPI-COPY",
            " vaapi-copy ",
            "auto-copy",
        ] {
            assert!(is_copy_back_hwdec(value), "{value} should be copy-back");
        }
        for value in ["vaapi", "nvdec", "no", "vdpau", "drm", "-copy", ""] {
            assert!(
                !is_copy_back_hwdec(value),
                "{value} should not be copy-back"
            );
        }
    }

    #[test]
    fn user_off_is_preserved() {
        let plan = plan_hwdec(false, None, None);

        assert_eq!(plan.value, HWDEC_OFF);
        assert_eq!(plan.source, HwdecSource::UserDisabled);
        assert!(!plan.source.allows_runtime_demotion());
    }

    #[test]
    fn raw_mpv_conf_override_wins_over_every_other_input() {
        let plan = plan_hwdec(true, Some("no"), Some("vaapi-copy"));

        assert_eq!(plan.value, "vaapi-copy");
        assert_eq!(plan.source, HwdecSource::UserOverride);
    }

    #[test]
    fn renderer_policy_overrides_the_settings_toggle() {
        let plan = plan_hwdec(true, Some("no"), None);

        assert_eq!(plan.value, HWDEC_OFF);
        assert_eq!(plan.source, HwdecSource::RendererForced);
    }

    fn options(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn an_mpv_conf_hwdec_option_is_read_back_as_the_override() {
        assert_eq!(
            hwdec_from_options(&options(&[("profile", "gpu-hq"), ("hwdec", "vaapi-copy")])),
            Some("vaapi-copy")
        );
        // The engine applies the options in order, so the last one wins.
        assert_eq!(
            hwdec_from_options(&options(&[("hwdec", "vaapi"), ("hwdec", "nvdec-copy")])),
            Some("nvdec-copy")
        );
        assert_eq!(
            hwdec_from_options(&options(&[("HWDEC", "vaapi-copy")])),
            Some("vaapi-copy"),
            "mpv option names are case-insensitive"
        );
        assert_eq!(hwdec_from_options(&options(&[("hwdec", "  ")])), None);
        assert_eq!(hwdec_from_options(&options(&[("profile", "gpu-hq")])), None);
        assert_eq!(hwdec_from_options(&[]), None);
    }

    #[test]
    fn an_mpv_conf_copy_back_choice_survives_the_whole_policy() {
        let conf = options(&[("hwdec", "vaapi-copy")]);
        let plan = plan_hwdec(true, None, hwdec_from_options(&conf));
        assert_eq!(plan.value, "vaapi-copy");

        let mut guard = HwdecGuard::new(plan.source);
        assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi-copy")), None);
        assert_eq!(guard.observe(sample(30, 10.2), Some("vaapi-copy")), None);
        assert!(!guard.has_demoted());
    }

    #[test]
    fn blank_overrides_are_ignored() {
        let plan = plan_hwdec(true, Some("  "), Some(""));

        assert_eq!(plan.source, HwdecSource::Automatic);
    }

    #[test]
    fn observed_copy_back_backend_demotes_immediately() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        let demotion = guard.observe(sample(0, 0.0), Some("vaapi-copy"));

        assert_eq!(
            demotion,
            Some(HwdecDemotion::CopyBackSelected {
                observed: "vaapi-copy".to_owned()
            })
        );
        assert!(guard.has_demoted());
    }

    #[test]
    fn observed_zero_copy_backend_does_not_demote() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 0.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(4, 4.0), Some("vaapi")), None);
        assert!(!guard.has_demoted());
    }

    #[test]
    fn software_decoding_is_never_demoted() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 0.0), Some("no")), None);
        assert_eq!(guard.observe(sample(10, 0.2), Some("no")), None);
        assert_eq!(guard.observe(sample(20, 0.4), None), None);
        assert!(!guard.has_demoted());
    }

    #[test]
    fn below_realtime_without_drops_demotes_exactly_once() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        // The measured defect: 0.02x realtime, nothing dropped.
        assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(2, 10.04), Some("vaapi")), None);
        let demotion = guard.observe(sample(5, 10.1), Some("vaapi"));

        match &demotion {
            Some(HwdecDemotion::BelowRealtimeWithoutDrops {
                clock_ratio,
                window_seconds,
                observed,
            }) => {
                assert!(*clock_ratio < DEMOTION_CLOCK_RATIO, "{clock_ratio}");
                assert!(
                    *window_seconds >= DEMOTION_WINDOW_SECONDS,
                    "{window_seconds}"
                );
                assert_eq!(observed, "vaapi");
            }
            other => panic!("expected a below-realtime demotion, got {other:?}"),
        }
        assert!(!demotion.expect("demotion").log_message().is_empty());

        // Sticky: the same stall must not be reported again.
        for second in 6..30 {
            assert_eq!(
                guard.observe(sample(second, 10.1), Some("vaapi")),
                None,
                "demotion repeated at second {second}"
            );
        }
    }

    #[test]
    fn below_realtime_with_drops_is_genuine_overload_and_does_not_demote() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);
        let mut drops = 0;

        for second in 0..30 {
            drops += 3;
            let mut sample = sample(second, 10.0 + second as f64 * 0.02);
            sample.decoder_drops = drops;
            assert_eq!(
                guard.observe(sample, Some("vaapi")),
                None,
                "dropped frames mean overload, not a stall (second {second})"
            );
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn vo_drops_alone_also_count_as_overload() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);
        let mut drops = 0;

        for second in 0..30 {
            drops += 1;
            let mut sample = sample(second, 10.0 + second as f64 * 0.02);
            sample.vo_drops = drops;
            assert_eq!(guard.observe(sample, Some("vaapi")), None);
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn a_healthy_session_never_demotes() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        for second in 0..120 {
            assert_eq!(
                guard.observe(sample(second, second as f64 * 1.01), Some("vaapi")),
                None
            );
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn a_paused_session_never_demotes() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        for second in 0..60 {
            let mut sample = sample(second, 42.0);
            sample.paused = true;
            assert_eq!(guard.observe(sample, Some("vaapi")), None);
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn slow_motion_playback_is_measured_against_its_own_speed() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        for second in 0..60 {
            let mut sample = sample(second, second as f64 * 0.25);
            sample.speed = 0.25;
            assert_eq!(guard.observe(sample, Some("vaapi")), None);
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn returning_from_slow_motion_to_normal_speed_does_not_demote() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        // Open a span during healthy 0.1x playback: the media clock advances at
        // a tenth of wall time, correctly, because that is what was asked for.
        for second in 0..3 {
            let mut sample = sample(second, second as f64 * 0.1);
            sample.speed = 0.1;
            assert_eq!(guard.observe(sample, Some("vaapi")), None);
        }

        // Back to 1x, and one healthy second of it closes the three-second
        // window. Scoring that whole window against 1x would read 0.4x and
        // demote a decoder that never missed a frame.
        assert_eq!(
            guard.observe(sample(3, 1.2), Some("vaapi")),
            None,
            "a speed change must invalidate the span, not demote"
        );
        assert!(!guard.has_demoted());

        // Steady 1x playback afterwards stays healthy too.
        for second in 4..12 {
            let media_time = 1.2 + (second - 3) as f64;
            assert_eq!(
                guard.observe(sample(second, media_time), Some("vaapi")),
                None
            );
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn a_network_cache_stall_is_not_a_decoder_stall() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        // Buffering freezes the media clock with `pause` false and no drops:
        // the demotion signature exactly, but the decoder is fine.
        for second in 0..30 {
            let mut sample = sample(second, 10.0);
            sample.awaiting_data = true;
            assert_eq!(guard.observe(sample, Some("vaapi")), None);
        }
        assert!(!guard.has_demoted());

        // Once the cache refills, healthy playback still must not demote.
        for second in 30..40 {
            let media_time = 10.0 + (second - 30) as f64;
            assert_eq!(
                guard.observe(sample(second, media_time), Some("vaapi")),
                None
            );
        }
        assert!(!guard.has_demoted());
    }

    #[test]
    fn a_demotion_the_shell_could_not_apply_is_reported_again() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert!(guard.observe(sample(0, 10.0), Some("vaapi-copy")).is_some());
        assert!(guard.has_demoted());

        // The shell's `hwdec` write failed, so the bad backend is still live.
        guard.demotion_failed();
        assert!(!guard.has_demoted());

        assert!(guard.observe(sample(1, 10.0), Some("vaapi-copy")).is_some());
        assert!(guard.has_demoted());
    }

    #[test]
    fn a_backward_seek_re_anchors_instead_of_demoting() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 600.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(5, 10.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(10, 15.0), Some("vaapi")), None);
        assert!(!guard.has_demoted());
    }

    #[test]
    fn a_forward_seek_re_anchors_instead_of_hiding_a_later_stall() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi")), None);
        // Jump far ahead: the span is meaningless, so it must not be scored.
        assert_eq!(guard.observe(sample(1, 900.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(3, 900.02), Some("vaapi")), None);
        assert!(guard.observe(sample(6, 900.05), Some("vaapi")).is_some());
    }

    #[test]
    fn a_short_stall_inside_the_window_does_not_demote() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(1, 10.0), Some("vaapi")), None);
        assert_eq!(guard.observe(sample(2, 10.0), Some("vaapi")), None);
        assert!(!guard.has_demoted());
    }

    #[test]
    fn reset_measurement_keeps_a_demotion_but_drops_the_span() {
        let mut guard = HwdecGuard::new(HwdecSource::Automatic);

        assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi")), None);
        guard.reset_measurement();
        // Without the reset this span would have scored 0.01x and demoted.
        assert_eq!(guard.observe(sample(5, 10.05), Some("vaapi")), None);

        assert!(guard.observe(sample(9, 10.09), Some("vaapi")).is_some());
        assert!(guard.has_demoted());
        guard.reset_measurement();
        assert!(guard.has_demoted());
    }

    #[test]
    fn explicit_user_sources_are_never_demoted_at_runtime() {
        for source in [
            HwdecSource::UserOverride,
            HwdecSource::UserDisabled,
            HwdecSource::RendererForced,
        ] {
            let mut guard = HwdecGuard::new(source);

            // Both signatures at once: copy-back backend and a stalled clock.
            assert_eq!(guard.observe(sample(0, 10.0), Some("vaapi-copy")), None);
            assert_eq!(guard.observe(sample(5, 10.05), Some("vaapi-copy")), None);
            assert_eq!(guard.observe(sample(60, 10.2), Some("vaapi-copy")), None);
            assert!(!guard.has_demoted(), "{source:?} must be left alone");
        }
    }

    #[test]
    fn demotion_messages_name_the_backend_and_the_escape_hatch() {
        let copy_back = HwdecDemotion::CopyBackSelected {
            observed: "vaapi-copy".to_owned(),
        }
        .log_message();
        assert!(copy_back.contains("vaapi-copy"), "{copy_back}");
        assert!(copy_back.contains("mpv.conf"), "{copy_back}");

        let stalled = HwdecDemotion::BelowRealtimeWithoutDrops {
            clock_ratio: 0.02,
            window_seconds: 5.0,
            observed: "vaapi".to_owned(),
        }
        .log_message();
        assert!(stalled.contains("0.02x"), "{stalled}");
        assert!(stalled.contains("mpv.conf"), "{stalled}");
    }
}
