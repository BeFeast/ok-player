//! The command registry, the shortcut table and `invoke_command` over the
//! `okp_session_*` C ABI with a real libmpv engine, the way the macOS menu bar
//! and key handling use them.

mod common;

use std::ffi::{CStr, CString, c_char};

use common::{engine_lock, headless_config, wait_for, write_media_fixture};
use okp_core::app::Platform;
use okp_ffi::{
    okp_session_dispatch, okp_session_free, okp_session_new, okp_session_query, okp_string_free,
};
use okp_session::protocol::{
    DispatchReply, DispatchResult, QueryReply, QueryResult, RegistryCommand, RejectReason,
    SessionSnapshot, ShortcutRow, Status,
};
use okp_test_fixtures::unique_temp_dir;
use serde_json::{Value, json};

struct Shell {
    session: *mut okp_ffi::OkpSession,
}

impl Shell {
    fn new(config: &Value) -> Result<Self, String> {
        let config = CString::new(config.to_string()).unwrap();
        let mut error = [0 as c_char; 256];
        let session = unsafe { okp_session_new(config.as_ptr(), error.as_mut_ptr(), error.len()) };
        if session.is_null() {
            let message = unsafe { CStr::from_ptr(error.as_ptr()) };
            return Err(message.to_string_lossy().into_owned());
        }
        Ok(Self { session })
    }

    fn query(&self, query: Value) -> QueryResult {
        let text = CString::new(json!({"schema_version": 1, "query": query}).to_string()).unwrap();
        let reply: QueryReply = serde_json::from_str(&take(unsafe {
            okp_session_query(self.session, text.as_ptr())
        }))
        .unwrap();
        reply.result
    }

    fn dispatch(&self, command: Value) -> DispatchResult {
        let text =
            CString::new(json!({"schema_version": 1, "command": command}).to_string()).unwrap();
        let reply: DispatchReply = serde_json::from_str(&take(unsafe {
            okp_session_dispatch(self.session, text.as_ptr())
        }))
        .unwrap();
        reply.result
    }

    fn invoke(&self, id: &str) -> DispatchResult {
        self.dispatch(json!({"invoke_command": {"id": id}}))
    }

    fn registry(&self) -> Vec<RegistryCommand> {
        match self.query(json!({"command_registry": {}})) {
            QueryResult::CommandRegistry { commands } => commands,
            other => panic!("the registry query was refused: {other:?}"),
        }
    }

    fn command(&self, id: &str) -> RegistryCommand {
        self.registry()
            .into_iter()
            .find(|command| command.id == id)
            .unwrap_or_else(|| panic!("{id} is in the registry"))
    }

    fn snapshot(&self) -> SessionSnapshot {
        match self.query(json!({"snapshot": {}})) {
            QueryResult::Snapshot { snapshot } => snapshot,
            other => panic!("the snapshot query was refused: {other:?}"),
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        unsafe { okp_session_free(self.session) };
    }
}

fn take(text: *mut c_char) -> String {
    assert!(!text.is_null(), "the ABI never returns a null document");
    let owned = unsafe { CStr::from_ptr(text) }
        .to_str()
        .expect("documents are UTF-8")
        .to_owned();
    unsafe { okp_string_free(text) };
    owned
}

fn macos_config(root: &std::path::Path, shell_commands: &[&str]) -> Value {
    let mut config = headless_config(root);
    config.app.platform = Platform::MacOs;
    config.shell_commands = shell_commands.iter().map(|&id| id.to_owned()).collect();
    serde_json::to_value(config).unwrap()
}

fn refusal(result: &DispatchResult) -> Option<RejectReason> {
    match result {
        DispatchResult::Rejected { reason, .. } => Some(*reason),
        _ => None,
    }
}

#[test]
fn a_macos_shell_reads_the_registry_and_runs_commands_through_the_session() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-registry");
    let media = write_media_fixture(root.path());
    let shell = Shell::new(&macos_config(root.path(), &["open-file"])).expect("session starts");

    // The shortcut table carries the macOS chords and the command each runs.
    let QueryResult::ShortcutTable { shortcuts } = shell.query(json!({"shortcut_table": {}}))
    else {
        panic!("the shortcut table query was refused");
    };
    let row = |action: &str| -> ShortcutRow {
        shortcuts
            .iter()
            .find(|row| row.action.id() == action)
            .cloned()
            .unwrap_or_else(|| panic!("{action} is in the table"))
    };
    assert_eq!(row("open-file").primary.label(), "Cmd+O");
    assert_eq!(row("open-settings").primary.label(), "Cmd+,");
    assert_eq!(row("mute").primary.label(), "M");
    assert_eq!(row("save-screenshot").command, "save-frame");
    assert_eq!(
        shell
            .command("open-file")
            .shortcut
            .map(|chord| chord.label()),
        Some("Cmd+O".to_owned())
    );

    // Unknown and unavailable commands are refused; the shell's own command
    // goes back to it.
    assert_eq!(
        refusal(&shell.invoke("eject")),
        Some(RejectReason::UnknownCommand)
    );
    assert!(!shell.command("open-url").available);
    assert_eq!(
        refusal(&shell.invoke("open-url")),
        Some(RejectReason::Unavailable)
    );
    assert!(shell.command("open-file").available);
    assert_eq!(shell.invoke("open-file"), DispatchResult::Perform {});

    // Mute works without media and shows checked.
    assert!(!shell.command("mute").checked);
    assert!(matches!(
        shell.invoke("mute"),
        DispatchResult::Accepted { .. }
    ));
    assert!(shell.command("mute").checked);
    assert!(shell.snapshot().mute);

    // Nothing to pause or close yet.
    assert!(!shell.command("play-pause").enabled);
    assert_eq!(
        refusal(&shell.invoke("play-pause")),
        Some(RejectReason::NoActiveMedia)
    );

    let open = shell.dispatch(json!({"open": {"source": {"kind": "local", "location": media}}}));
    assert!(matches!(open, DispatchResult::Accepted { .. }), "{open:?}");
    assert!(
        wait_for(|| matches!(shell.snapshot().status, Status::Playing | Status::Paused)),
        "the fixture never loaded"
    );
    assert!(shell.command("play-pause").enabled);
    assert!(shell.command("close-media").enabled);
    // One open file: there is no other item to step to.
    assert!(!shell.command("next-item").enabled);

    let before = shell.snapshot().status;
    assert!(matches!(
        shell.invoke("play-pause"),
        DispatchResult::Accepted { .. }
    ));
    let toggled = if before == Status::Paused {
        Status::Playing
    } else {
        Status::Paused
    };
    assert!(
        wait_for(|| shell.snapshot().status == toggled),
        "play-pause did not toggle {before:?}"
    );

    let volume = shell.snapshot().volume.expect("libmpv reports the volume");
    assert!(matches!(
        shell.invoke("volume-down"),
        DispatchResult::Accepted { .. }
    ));
    assert!(
        wait_for(|| shell.snapshot().volume == Some(volume - 5.0)),
        "volume-down did not lower {volume} by five: {:?}",
        shell.snapshot().volume
    );

    assert!(matches!(
        shell.invoke("close-media"),
        DispatchResult::Accepted { .. }
    ));
    assert_eq!(shell.snapshot().status, Status::Idle);
    assert!(!shell.command("play-pause").enabled);

    drop(shell);
    root.close().expect("fixture directory is removed");
}

/// Registry rows that need playing or paused media, and the ones that do not.
const NEED_ACTIVE_MEDIA: [&str; 5] = [
    "play-pause",
    "seek-back",
    "seek-forward",
    "subtitle-delay-forward",
    "subtitle-delay-back",
];
const WORK_AFTER_THE_END: [&str; 4] = ["close-media", "volume-down", "volume-up", "mute"];

fn assert_ended_rows(shell: &Shell, after: &str) {
    for id in NEED_ACTIVE_MEDIA {
        assert!(!shell.command(id).enabled, "{id} is enabled {after}");
        assert_eq!(
            refusal(&shell.invoke(id)),
            Some(RejectReason::NoActiveMedia),
            "{id} {after}"
        );
    }
    for id in WORK_AFTER_THE_END {
        assert!(shell.command(id).enabled, "{id} is disabled {after}");
    }
}

#[test]
fn after_a_file_ends_or_fails_to_load_only_the_commands_that_still_work_are_enabled() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-registry-ended");
    let media = write_media_fixture(root.path());
    let shell = Shell::new(&macos_config(root.path(), &[])).expect("session starts");

    let open = shell.dispatch(json!({"open": {"source": {"kind": "local", "location": media}}}));
    assert!(matches!(open, DispatchResult::Accepted { .. }), "{open:?}");
    assert!(
        wait_for(|| matches!(shell.snapshot().status, Status::Playing | Status::Paused)),
        "the fixture never loaded"
    );
    for id in NEED_ACTIVE_MEDIA.into_iter().chain(WORK_AFTER_THE_END) {
        assert!(shell.command(id).enabled, "{id} is disabled while loaded");
    }
    // Play out the last half second of the two-second fixture.
    let seek =
        shell.dispatch(json!({"seek": {"seconds": 1.5, "mode": "absolute", "precision": "exact"}}));
    assert!(matches!(seek, DispatchResult::Accepted { .. }), "{seek:?}");
    let play = shell.dispatch(json!({"set_paused": {"paused": false}}));
    assert!(matches!(play, DispatchResult::Accepted { .. }), "{play:?}");
    assert!(
        wait_for(|| shell.snapshot().status == Status::Ended),
        "the fixture never reached its end: {:?}",
        shell.snapshot()
    );
    assert!(shell.snapshot().source.is_some(), "the source stays set");
    assert_ended_rows(&shell, "after the end of the file");

    let missing = root.path().join("missing.mkv");
    let open = shell.dispatch(json!({"open": {"source": {"kind": "local", "location": missing}}}));
    assert!(matches!(open, DispatchResult::Accepted { .. }), "{open:?}");
    assert!(
        wait_for(|| shell.snapshot().status == Status::Ended),
        "a missing file never ended: {:?}",
        shell.snapshot()
    );
    assert_ended_rows(&shell, "after a failed load");

    assert!(matches!(
        shell.invoke("close-media"),
        DispatchResult::Accepted { .. }
    ));
    assert!(!shell.command("close-media").enabled);

    drop(shell);
    root.close().expect("fixture directory is removed");
}

#[test]
fn a_config_naming_an_unknown_shell_command_is_refused() {
    let _engine = engine_lock();
    let root = unique_temp_dir("okp-session-registry-config");
    let error = Shell::new(&macos_config(root.path(), &["open-file", "open-flie"]))
        .err()
        .expect("the session refuses the config");
    assert_eq!(error, "shell_commands names an unknown command `open-flie`");
    root.close().expect("fixture directory is removed");
}
