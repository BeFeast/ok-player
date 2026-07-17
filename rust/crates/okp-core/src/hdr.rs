//! Portable HDR source classification and handling-state presentation.
//!
//! HDR output remains engine-managed until OK Player has a verified platform
//! passthrough/tone-mapping implementation. Shells may report this state, but
//! must not present it as a configurable control.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicRangeState {
    Hdr,
    Sdr,
    Unknown,
}

impl DynamicRangeState {
    pub fn from_media_info_value(value: Option<&str>) -> Self {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Self::Unknown;
        };
        let value = value.to_ascii_lowercase();

        if matches!(value.as_str(), "no" | "none" | "off" | "sdr") {
            Self::Sdr
        } else if value.contains("hdr")
            || value.contains("dolby vision")
            || value.contains("pq")
            || value.contains("st 2084")
            || value.contains("st2084")
            || value.contains("hlg")
        {
            Self::Hdr
        } else {
            Self::Unknown
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdrHandlingState {
    EngineManaged,
    Unavailable,
}

impl HdrHandlingState {
    pub const fn key(self) -> &'static str {
        match self {
            Self::EngineManaged => "engine-managed",
            Self::Unavailable => "unavailable",
        }
    }

    pub const fn settings_label(self) -> &'static str {
        match self {
            Self::EngineManaged => "Automatic",
            Self::Unavailable => "Unavailable",
        }
    }

    pub const fn diagnostic_label(self) -> &'static str {
        match self {
            Self::EngineManaged => "Automatic · engine-managed",
            Self::Unavailable => "Unavailable",
        }
    }

    pub const fn detail(self) -> &'static str {
        match self {
            Self::EngineManaged => {
                "mpv manages HDR output automatically. Tone-mapping and passthrough controls are unavailable."
            }
            Self::Unavailable => "HDR output handling is unavailable on this platform.",
        }
    }

    pub const fn controls_available(self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_compact_media_info_dynamic_range_labels() {
        for value in ["HDR", "HDR (PQ / ST 2084, BT.2020)", "HLG", "Dolby Vision"] {
            assert_eq!(
                DynamicRangeState::from_media_info_value(Some(value)),
                DynamicRangeState::Hdr
            );
        }
        for value in ["SDR", "No", "none", "off"] {
            assert_eq!(
                DynamicRangeState::from_media_info_value(Some(value)),
                DynamicRangeState::Sdr
            );
        }
        for value in [None, Some(""), Some("Unknown"), Some("BT.709")] {
            assert_eq!(
                DynamicRangeState::from_media_info_value(value),
                DynamicRangeState::Unknown
            );
        }
    }

    #[test]
    fn engine_managed_state_is_informational_not_configurable() {
        let state = HdrHandlingState::EngineManaged;

        assert_eq!(state.key(), "engine-managed");
        assert_eq!(state.settings_label(), "Automatic");
        assert_eq!(state.diagnostic_label(), "Automatic · engine-managed");
        assert!(!state.controls_available());
        assert!(state.detail().contains("controls are unavailable"));
    }

    #[test]
    fn unavailable_state_has_compact_honest_labels() {
        let state = HdrHandlingState::Unavailable;

        assert_eq!(state.settings_label(), "Unavailable");
        assert_eq!(state.diagnostic_label(), "Unavailable");
        assert!(!state.controls_available());
    }
}
