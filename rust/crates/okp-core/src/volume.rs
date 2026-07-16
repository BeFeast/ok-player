//! Portable volume-control state and geometry.

pub const MIN_VOLUME: f64 = 0.0;
pub const UNITY_VOLUME: f64 = 100.0;
pub const MAX_VOLUME: f64 = 130.0;
pub const DEFAULT_RESTORE_VOLUME: f64 = UNITY_VOLUME;
const OBSERVED_LEVEL_EPSILON: f64 = 0.005;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VolumeState {
    level: f64,
    remembered_nonzero: f64,
}

impl Default for VolumeState {
    fn default() -> Self {
        Self::new(DEFAULT_RESTORE_VOLUME)
    }
}

impl VolumeState {
    pub fn new(level: f64) -> Self {
        let level = clamp_level(level);
        Self {
            level,
            remembered_nonzero: if level > MIN_VOLUME {
                level
            } else {
                DEFAULT_RESTORE_VOLUME
            },
        }
    }

    pub fn level(self) -> f64 {
        self.level
    }

    pub fn remembered_nonzero(self) -> f64 {
        self.remembered_nonzero
    }

    pub fn is_muted(self) -> bool {
        self.level <= MIN_VOLUME
    }

    pub fn is_boosted(self) -> bool {
        self.level > UNITY_VOLUME
    }

    pub fn set_level(&mut self, level: f64) -> f64 {
        self.level = clamp_level(level);
        if self.level > MIN_VOLUME {
            self.remembered_nonzero = self.level;
        }
        self.level
    }

    pub fn nudge(&mut self, delta: f64) -> f64 {
        self.set_level(self.level + finite_or_zero(delta))
    }

    /// Ctrl+primary-click quick reset: land on exactly unity and clear mute,
    /// regardless of whether the prior level was below unity, boosted, or muted.
    /// The remembered level re-bases to unity so a later mute round-trips to 100%.
    pub fn reset_to_unity(&mut self) -> f64 {
        self.set_level(UNITY_VOLUME)
    }

    pub fn toggle_mute(&mut self) -> f64 {
        if self.is_muted() {
            self.level = self.remembered_nonzero;
        } else {
            self.remembered_nonzero = self.level;
            self.level = MIN_VOLUME;
        }
        self.level
    }

    pub fn unity_fraction() -> f64 {
        UNITY_VOLUME / MAX_VOLUME
    }

    pub fn level_fraction(self) -> f64 {
        self.level / MAX_VOLUME
    }

    pub fn teal_fraction(self) -> f64 {
        self.level.min(UNITY_VOLUME) / MAX_VOLUME
    }

    pub fn boost_fraction(self) -> f64 {
        (self.level - UNITY_VOLUME).max(0.0) / MAX_VOLUME
    }

    pub fn readout(self) -> String {
        if self.is_muted() {
            "Muted".to_owned()
        } else if (self.level - self.level.round()).abs() < 0.05 {
            format!("{:.0}%", self.level)
        } else {
            format!("{:.1}%", self.level)
        }
    }
}

pub fn clamp_level(level: f64) -> f64 {
    if level.is_finite() {
        level.clamp(MIN_VOLUME, MAX_VOLUME)
    } else {
        DEFAULT_RESTORE_VOLUME
    }
}

pub fn parse_readout(text: &str) -> Option<f64> {
    let value = text.trim().strip_suffix('%').unwrap_or(text.trim()).trim();
    value
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(clamp_level)
}

/// Reconcile an asynchronously observed engine level with the latest UI projection.
/// Older observations are ignored until mpv publishes the projected value, preventing
/// rapid nudges from re-basing on a stale poll snapshot.
pub fn reconcile_observed_level(pending: &mut Option<f64>, observed: f64) -> Option<f64> {
    let observed = clamp_level(observed);
    match *pending {
        Some(projected) if (projected - observed).abs() < OBSERVED_LEVEL_EPSILON => {
            *pending = None;
            Some(observed)
        }
        Some(_) => None,
        None => Some(observed),
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_the_native_range_and_places_unity_at_76_9_percent() {
        assert_eq!(clamp_level(-1.0), 0.0);
        assert_eq!(clamp_level(131.0), 130.0);
        assert_eq!(clamp_level(f64::NAN), 100.0);
        assert!((VolumeState::unity_fraction() - 0.769_230_769).abs() < 1e-9);
    }

    #[test]
    fn separates_teal_unity_fill_from_amber_boost_fill() {
        let normal = VolumeState::new(65.0);
        assert_eq!(normal.teal_fraction(), 0.5);
        assert_eq!(normal.boost_fraction(), 0.0);
        assert!(!normal.is_boosted());

        let boosted = VolumeState::new(130.0);
        assert_eq!(boosted.teal_fraction(), VolumeState::unity_fraction());
        assert!((boosted.boost_fraction() - (30.0 / 130.0)).abs() < 1e-9);
        assert!(boosted.is_boosted());
    }

    #[test]
    fn mute_restores_the_previous_nonzero_level() {
        let mut volume = VolumeState::new(72.5);
        assert_eq!(volume.toggle_mute(), 0.0);
        assert!(volume.is_muted());
        assert_eq!(volume.remembered_nonzero(), 72.5);
        assert_eq!(volume.toggle_mute(), 72.5);
    }

    #[test]
    fn ctrl_click_reset_lands_on_exact_unity_from_every_starting_state() {
        let mut below = VolumeState::new(54.7);
        assert_eq!(below.reset_to_unity(), UNITY_VOLUME);
        assert!(!below.is_muted());
        assert!(!below.is_boosted());
        assert_eq!(below.level(), 100.0);

        let mut boosted = VolumeState::new(124.0);
        assert!(boosted.is_boosted());
        assert_eq!(boosted.reset_to_unity(), UNITY_VOLUME);
        assert!(!boosted.is_boosted());

        let mut muted = VolumeState::new(72.0);
        muted.toggle_mute();
        assert!(muted.is_muted());
        assert_eq!(muted.reset_to_unity(), UNITY_VOLUME);
        assert!(!muted.is_muted());
        assert_eq!(muted.readout(), "100%");

        // The reset re-bases the remembered level, so mute after a reset
        // restores unity instead of the pre-reset value.
        assert_eq!(muted.remembered_nonzero(), UNITY_VOLUME);
        assert_eq!(muted.toggle_mute(), 0.0);
        assert_eq!(muted.toggle_mute(), UNITY_VOLUME);
    }

    #[test]
    fn pointer_wheel_and_keyboard_nudges_share_clamping() {
        let mut volume = VolumeState::new(129.5);
        assert_eq!(volume.nudge(1.0), 130.0);
        assert_eq!(volume.nudge(1.0), 130.0);
        assert_eq!(volume.nudge(-0.1), 129.9);
        volume.set_level(0.0);
        assert_eq!(volume.nudge(-1.0), 0.0);
        assert_eq!(volume.nudge(0.1), 0.1);
    }

    #[test]
    fn readout_exposes_muted_integer_and_fine_values() {
        assert_eq!(VolumeState::new(0.0).readout(), "Muted");
        assert_eq!(VolumeState::new(100.0).readout(), "100%");
        assert_eq!(VolumeState::new(54.7).readout(), "54.7%");
    }

    #[test]
    fn exact_readout_input_accepts_percent_text_and_clamps() {
        assert_eq!(parse_readout("54.71%"), Some(54.71));
        assert_eq!(parse_readout(" 132 "), Some(130.0));
        assert_eq!(parse_readout("loud"), None);
        assert_eq!(parse_readout("NaN"), None);
    }

    #[test]
    fn stale_engine_observations_cannot_rebase_rapid_ui_nudges() {
        let mut pending = Some(79.1);
        assert_eq!(reconcile_observed_level(&mut pending, 78.0), None);
        assert_eq!(pending, Some(79.1));

        let mut projected = VolumeState::new(pending.unwrap());
        assert_eq!(projected.nudge(1.0), 80.1);
        pending = Some(projected.level());
        assert_eq!(reconcile_observed_level(&mut pending, 79.1), None);
        assert_eq!(reconcile_observed_level(&mut pending, 80.1), Some(80.1));
        assert_eq!(pending, None);
        assert_eq!(reconcile_observed_level(&mut pending, 67.0), Some(67.0));
    }
}
