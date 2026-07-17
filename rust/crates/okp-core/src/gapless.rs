//! Portable gapless-playback capability and preference gating.
//!
//! mpv can attempt gapless audio only when consecutive items are already in its
//! engine-managed playlist. A shell that waits for end-of-file and then issues a
//! replacement `loadfile` command cannot prepare the next decoder/output chain in
//! advance, so it must expose gapless playback as deferred rather than treating the
//! mere presence of mpv's `gapless-audio` option as support.

/// The queue ownership model used by a player backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GaplessPlaybackCapability {
    /// Consecutive entries are owned by mpv, so its gapless-audio path can prepare
    /// the next item before the current one reaches EOF.
    EngineManagedPlaylist,
    /// The application chooses and loads the next item only after observing EOF.
    ApplicationManagedAfterEof,
}

impl GaplessPlaybackCapability {
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::EngineManagedPlaylist)
    }
}

/// Effective state presented by a settings surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GaplessPlaybackSettingState {
    Off,
    On,
    Deferred,
}

/// A persisted preference resolved against the active playback backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GaplessPlaybackState {
    capability: GaplessPlaybackCapability,
    requested: bool,
}

impl GaplessPlaybackState {
    #[must_use]
    pub const fn new(capability: GaplessPlaybackCapability, requested: bool) -> Self {
        Self {
            capability,
            requested,
        }
    }

    #[must_use]
    pub const fn capability(self) -> GaplessPlaybackCapability {
        self.capability
    }

    #[must_use]
    pub const fn requested(self) -> bool {
        self.requested
    }

    #[must_use]
    pub const fn setting_state(self) -> GaplessPlaybackSettingState {
        if !self.capability.is_available() {
            GaplessPlaybackSettingState::Deferred
        } else if self.requested {
            GaplessPlaybackSettingState::On
        } else {
            GaplessPlaybackSettingState::Off
        }
    }

    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self.setting_state(), GaplessPlaybackSettingState::On)
    }

    /// Update the persisted preference only when the backend can honor it.
    /// Returns whether the request was accepted.
    pub fn try_set_enabled(&mut self, enabled: bool) -> bool {
        if !self.capability.is_available() {
            return false;
        }

        self.requested = enabled;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_playlist_resolves_the_persisted_preference() {
        let mut state =
            GaplessPlaybackState::new(GaplessPlaybackCapability::EngineManagedPlaylist, false);

        assert_eq!(state.setting_state(), GaplessPlaybackSettingState::Off);
        assert!(state.try_set_enabled(true));
        assert!(state.requested());
        assert!(state.is_enabled());
        assert_eq!(state.setting_state(), GaplessPlaybackSettingState::On);
    }

    #[test]
    fn application_queue_is_deferred_and_cannot_claim_the_setting() {
        let mut state =
            GaplessPlaybackState::new(GaplessPlaybackCapability::ApplicationManagedAfterEof, true);

        assert_eq!(state.setting_state(), GaplessPlaybackSettingState::Deferred);
        assert!(!state.is_enabled());
        assert!(!state.try_set_enabled(false));
        assert!(
            state.requested(),
            "unsupported shells preserve future intent"
        );
    }
}
