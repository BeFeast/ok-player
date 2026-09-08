use okp_core::player::{
    CommandOutcome, EndReason, PlayerCommand, PlayerError, PlayerErrorKind, PlayerEvent,
    PlayerMachine,
};

/// Couples command validation to engine dispatch without teaching a native shell a
/// second lifecycle. The live libmpv adapter and its behavioral tests both use this
/// boundary.
pub(crate) struct SessionMachine {
    machine: PlayerMachine,
}

impl SessionMachine {
    pub(crate) fn new() -> Self {
        Self {
            machine: PlayerMachine::new(),
        }
    }

    pub(crate) fn machine(&self) -> &PlayerMachine {
        &self.machine
    }

    pub(crate) fn apply_event(&mut self, event: PlayerEvent) {
        self.machine.apply_event(event);
    }

    /// Validate and optimistically apply `command`, forwarding it only when the
    /// portable core accepts it. Engine failures are folded back into the core error
    /// state; failed opens additionally become an ended load rather than remaining
    /// stuck in `Opening`.
    pub(crate) fn dispatch<F>(
        &mut self,
        command: &PlayerCommand,
        forward: F,
    ) -> (CommandOutcome, Result<(), String>)
    where
        F: FnOnce(&PlayerCommand, u64) -> Result<(), String>,
    {
        let outcome = self.machine.apply_command(command);
        let CommandOutcome::Accepted { request_id } = outcome else {
            return (outcome, Ok(()));
        };

        let result = forward(command, request_id);
        if let Err(message) = &result {
            self.machine.apply_event(PlayerEvent::Error(PlayerError {
                kind: if matches!(command, PlayerCommand::Open(_)) {
                    PlayerErrorKind::LoadFailed
                } else {
                    PlayerErrorKind::CommandFailed
                },
                message: message.clone(),
            }));
            if matches!(command, PlayerCommand::Open(_)) {
                self.machine
                    .apply_event(PlayerEvent::Ended(EndReason::Error));
            }
        }
        (outcome, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use okp_core::player::{OpenRequest, PlaybackStatus};
    use okp_core::playlist::PlaylistItem;
    use std::cell::Cell;
    use std::path::PathBuf;

    #[test]
    fn accepted_command_forwards_the_core_request_id() {
        let mut session = SessionMachine::new();
        let forwarded_id = Cell::new(0);
        let command =
            PlayerCommand::Open(OpenRequest::new(PlaylistItem::Local("/media/a.mp4".into())));
        let (outcome, result) = session.dispatch(&command, |_, request_id| {
            forwarded_id.set(request_id);
            Ok(())
        });
        let CommandOutcome::Accepted { request_id } = outcome else {
            panic!("open rejected")
        };
        assert!(result.is_ok());
        assert_ne!(request_id, 0);
        assert_eq!(forwarded_id.get(), request_id);
    }

    #[test]
    fn replaced_file_stop_then_loaded_remains_playable() {
        let mut session = SessionMachine::new();
        for path in ["/media/a.mp4", "/media/b.mp4"] {
            let command = PlayerCommand::Open(OpenRequest::new(PlaylistItem::Local(path.into())));
            let (outcome, result) = session.dispatch(&command, |_, _| Ok(()));
            assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
            assert!(result.is_ok());
        }
        session.apply_event(PlayerEvent::Ended(EndReason::Stopped));
        session.apply_event(PlayerEvent::Loaded {
            duration: Some(12.0),
        });
        assert_eq!(session.machine().status(), PlaybackStatus::Playing);
        let (outcome, _) = session.dispatch(&PlayerCommand::TogglePause, |_, _| Ok(()));
        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        assert_eq!(session.machine().status(), PlaybackStatus::Paused);
    }

    #[test]
    fn rejected_command_never_reaches_engine() {
        let mut session = SessionMachine::new();
        let forwarded = Cell::new(false);

        let (outcome, engine) = session.dispatch(&PlayerCommand::TogglePause, |_, _| {
            forwarded.set(true);
            Ok(())
        });

        assert!(matches!(outcome, CommandOutcome::Rejected(_)));
        assert!(engine.is_ok());
        assert!(!forwarded.get());
        assert_eq!(session.machine().status(), PlaybackStatus::Idle);
    }

    #[test]
    fn real_events_reconcile_open_pause_resume_and_close() {
        let mut session = SessionMachine::new();
        let open = PlayerCommand::Open(OpenRequest::new(PlaylistItem::Local(PathBuf::from(
            "/media/smoke.mp4",
        ))));

        let (outcome, engine) = session.dispatch(&open, |_, _| Ok(()));
        assert!(matches!(outcome, CommandOutcome::Accepted { .. }));
        assert!(engine.is_ok());
        assert_eq!(session.machine().status(), PlaybackStatus::Opening);

        session.apply_event(PlayerEvent::Loaded {
            duration: Some(6.0),
        });
        assert_eq!(session.machine().status(), PlaybackStatus::Playing);

        let (_, engine) = session.dispatch(&PlayerCommand::SetPaused(true), |_, _| Ok(()));
        assert!(engine.is_ok());
        assert_eq!(session.machine().status(), PlaybackStatus::Paused);

        session.apply_event(PlayerEvent::Property(
            okp_core::player::PropertyChange::Paused(false),
        ));
        assert_eq!(session.machine().status(), PlaybackStatus::Playing);

        let (_, engine) = session.dispatch(&PlayerCommand::Close, |_, _| Ok(()));
        assert!(engine.is_ok());
        assert_eq!(session.machine().status(), PlaybackStatus::Idle);
    }

    #[test]
    fn failed_engine_open_becomes_an_ended_core_error() {
        let mut session = SessionMachine::new();
        let open = PlayerCommand::Open(OpenRequest::new(PlaylistItem::Local(PathBuf::from(
            "/missing.mp4",
        ))));

        let (_, engine) = session.dispatch(&open, |_, _| Err("engine refused fixture".to_owned()));

        assert_eq!(engine.unwrap_err(), "engine refused fixture");
        assert_eq!(session.machine().status(), PlaybackStatus::Ended);
        assert_eq!(
            session
                .machine()
                .snapshot()
                .last_error
                .as_ref()
                .map(|error| error.message.as_str()),
            Some("engine refused fixture")
        );
    }
}
