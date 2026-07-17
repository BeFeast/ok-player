//! Portable capability gating for gapless playlist transitions.
//!
//! libmpv can preserve an audio device across compatible entries when it owns the
//! playlist transition. That option is not sufficient when a shell waits for
//! `EndFile` and only then sends a fresh `loadfile` command: by that point the old
//! entry has already ended and a continuous transition cannot be promised.

/// How the next playlist entry reaches the playback engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistTransitionPath {
    /// The engine owns the queued entries and advances without waiting for the shell.
    EngineManaged,
    /// The shell reacts to end-of-file and sends a new load command afterwards.
    ShellManagedAfterEndFile,
}

/// Whether a gapless preference may honestly be enabled for a transition path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaplessPlaybackCapability {
    Available,
    Deferred,
}

impl GaplessPlaybackCapability {
    pub const fn for_transition_path(path: PlaylistTransitionPath) -> Self {
        match path {
            PlaylistTransitionPath::EngineManaged => Self::Available,
            PlaylistTransitionPath::ShellManagedAfterEndFile => Self::Deferred,
        }
    }

    pub const fn allows_enablement(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Apply a stored preference only when the active engine path can honor it.
pub const fn effective_gapless_enabled(
    requested: bool,
    capability: GaplessPlaybackCapability,
) -> bool {
    requested && capability.allows_enablement()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_managed_playlist_can_offer_gapless_playback() {
        let capability =
            GaplessPlaybackCapability::for_transition_path(PlaylistTransitionPath::EngineManaged);

        assert_eq!(capability, GaplessPlaybackCapability::Available);
        assert!(effective_gapless_enabled(true, capability));
        assert!(!effective_gapless_enabled(false, capability));
    }

    #[test]
    fn after_end_file_loading_is_deliberately_deferred() {
        let capability = GaplessPlaybackCapability::for_transition_path(
            PlaylistTransitionPath::ShellManagedAfterEndFile,
        );

        assert_eq!(capability, GaplessPlaybackCapability::Deferred);
        assert!(!capability.allows_enablement());
        assert!(!effective_gapless_enabled(true, capability));
    }
}
