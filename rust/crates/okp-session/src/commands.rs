//! The command registry and the shortcut table a shell builds its menus and key
//! handling from.
//!
//! The registry is the core player command registry
//! ([`okp_core::player_commands`]) followed by the bindable keyboard actions
//! that no registry command covers ([`okp_core::shortcuts`]): seeking, volume,
//! mute, playlist steps, subtitle nudges. Ids and labels are the core's own.
//! Each row is resolved against the session's current state, so enabled,
//! checked and state-dependent labels ("Exit fullscreen") come from here and
//! a shell hard-codes none of them.
//!
//! Some commands the session runs itself (`invoke_command` forwards them as
//! session commands). The rest are carried out by the shell (dialogs, window
//! and clipboard actions); the shell lists the ones it implements in its
//! session config, and every other one is reported unavailable so the shell
//! can hide it.

use okp_core::app::Platform;
use okp_core::player_commands::{
    PLAYER_COMMAND_MENU_TOP_LEVEL, PlayerCommandContext, PlayerCommandGroup, PlayerCommandId,
    PlayerCommandMenuEntry, PlayerCommandMenuPage, PlayerCommandSurface, player_command_registry,
    resolve_player_commands,
};
use okp_core::shortcuts::{
    SHORTCUT_ACTIONS, ShortcutAction, ShortcutBinding, ShortcutChord, chords_for_action_on,
    default_bindings_on, default_chord_for_action_on,
};
use serde::{Deserialize, Serialize};

use crate::protocol::{
    Command, RejectReason, SeekMode, SeekPrecision, SessionSnapshot, SourceKind, Status,
};
use crate::transport::audio_muted;

/// Seconds the seek keys jump, as in the GTK shell.
const SEEK_STEP_SECONDS: f64 = 5.0;
/// Volume points the volume keys move, as in the GTK shell.
const VOLUME_STEP: f64 = 5.0;
/// Seconds the subtitle delay keys move the delay, as in the GTK shell.
const SUBTITLE_DELAY_STEP_SECONDS: f64 = 0.05;

/// One resolved command of the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryCommand {
    /// Stable id; `invoke_command` takes it.
    pub id: String,
    pub group: CommandGroup,
    /// Where the command sits in the player command menu (its first level or
    /// one of its four pages); `null` for a keyboard-only command.
    pub menu: Option<CommandMenu>,
    pub label: String,
    /// Whether this shell can run the command at all. An unavailable command
    /// is one neither the session nor the shell implements; hide it.
    pub available: bool,
    /// Whether the command applies to the current state.
    pub enabled: bool,
    pub checked: bool,
    /// The primary chord bound to the command on this platform, if any.
    pub shortcut: Option<ShortcutChord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandGroup {
    PlaybackNavigation,
    TracksSubtitles,
    ViewWindow,
    MediaFile,
    Tools,
    Settings,
}

/// A place in the player command menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandMenu {
    TopLevel,
    Video,
    Playback,
    Window,
    ToolsAdvanced,
}

/// One bindable action of the shortcut table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShortcutRow {
    pub action: ShortcutAction,
    /// The registry command the action runs (pass it to `invoke_command`).
    pub command: String,
    pub primary: ShortcutChord,
    pub secondary: Option<ShortcutChord>,
}

/// Window state only the shell knows, for the commands that depend on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowState {
    pub fullscreen: bool,
    pub mini_player: bool,
}

/// The keyboard actions no core registry command covers, with the group each
/// is listed under.
const KEYBOARD_COMMANDS: &[(ShortcutAction, CommandGroup)] = &[
    (ShortcutAction::SeekBack, CommandGroup::PlaybackNavigation),
    (
        ShortcutAction::SeekForward,
        CommandGroup::PlaybackNavigation,
    ),
    (
        ShortcutAction::FrameForward,
        CommandGroup::PlaybackNavigation,
    ),
    (ShortcutAction::FrameBack, CommandGroup::PlaybackNavigation),
    (
        ShortcutAction::PreviousItem,
        CommandGroup::PlaybackNavigation,
    ),
    (ShortcutAction::NextItem, CommandGroup::PlaybackNavigation),
    (ShortcutAction::VolumeDown, CommandGroup::TracksSubtitles),
    (ShortcutAction::VolumeUp, CommandGroup::TracksSubtitles),
    (ShortcutAction::Mute, CommandGroup::TracksSubtitles),
    (ShortcutAction::AddSubtitle, CommandGroup::TracksSubtitles),
    (
        ShortcutAction::SubtitleDelayForward,
        CommandGroup::TracksSubtitles,
    ),
    (
        ShortcutAction::SubtitleDelayBack,
        CommandGroup::TracksSubtitles,
    ),
    (
        ShortcutAction::SubtitleSizeDown,
        CommandGroup::TracksSubtitles,
    ),
    (
        ShortcutAction::SubtitleSizeUp,
        CommandGroup::TracksSubtitles,
    ),
    (
        ShortcutAction::SubtitlePreviousCue,
        CommandGroup::TracksSubtitles,
    ),
    (
        ShortcutAction::SubtitleNextCue,
        CommandGroup::TracksSubtitles,
    ),
    (ShortcutAction::EscapeFullscreen, CommandGroup::ViewWindow),
];

/// The commands the session runs itself; see [`session_command`].
pub(crate) const SESSION_COMMANDS: &[&str] = &[
    "play-pause",
    "close-media",
    "seek-back",
    "seek-forward",
    "volume-down",
    "volume-up",
    "mute",
    "subtitle-delay-forward",
    "subtitle-delay-back",
];

/// What `invoke_command` does with an id.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Invocation {
    /// Dispatch this session command.
    Run(Command),
    /// The shell carries the command out.
    Perform,
    Refuse(RejectReason, String),
}

/// The registry as one shell sees it: the platform's shortcut table and the
/// commands the shell carries out itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandRegistry {
    platform: Platform,
    shell_commands: Vec<String>,
}

impl CommandRegistry {
    /// Fails on an id that is not in the registry, so a misspelt capability
    /// cannot quietly hide a command.
    pub fn new(platform: Platform, shell_commands: &[String]) -> Result<Self, String> {
        if let Some(unknown) = shell_commands.iter().find(|id| !is_registry_id(id)) {
            return Err(format!(
                "shell_commands names an unknown command `{unknown}`"
            ));
        }
        Ok(Self {
            platform,
            shell_commands: shell_commands.to_vec(),
        })
    }

    /// Every command, core registry order first, resolved against `snapshot`
    /// and the session playlist's `playlist_items`.
    #[must_use]
    pub fn resolve(
        &self,
        snapshot: &SessionSnapshot,
        window: WindowState,
        playlist_items: usize,
    ) -> Vec<RegistryCommand> {
        let context = player_context(snapshot, window, playlist_items);
        let bindings = default_bindings_on(self.platform);
        let player = resolve_player_commands(PlayerCommandSurface::More, context, |_| None)
            .into_iter()
            .map(|command| {
                let id = command.id.id();
                let shortcut = player_command_registry()
                    .iter()
                    .find(|spec| spec.id == command.id)
                    .and_then(|spec| spec.shortcut);
                RegistryCommand {
                    id: id.to_owned(),
                    group: command.group.into(),
                    menu: menu_of(command.id),
                    label: command.label,
                    available: self.is_available(id),
                    enabled: session_command_enabled(id, snapshot.status)
                        .unwrap_or(command.enabled),
                    checked: command.checked,
                    shortcut: shortcut
                        .map(|action| primary_chord(self.platform, &bindings, action)),
                }
            });
        let keyboard = KEYBOARD_COMMANDS.iter().map(|&(action, group)| {
            let (enabled, checked) = keyboard_state(action, &context, snapshot);
            RegistryCommand {
                id: action.id().to_owned(),
                group,
                menu: None,
                label: action.label().to_owned(),
                available: self.is_available(action.id()),
                enabled,
                checked,
                shortcut: Some(primary_chord(self.platform, &bindings, action)),
            }
        });
        player.chain(keyboard).collect()
    }

    /// Every bindable action with its chords on this platform and the command it
    /// runs. The session has no user bindings yet, so these are the platform
    /// defaults: one chord per action.
    #[must_use]
    pub fn shortcut_table(&self) -> Vec<ShortcutRow> {
        let bindings = default_bindings_on(self.platform);
        SHORTCUT_ACTIONS
            .iter()
            .map(|&action| {
                let mut chords = chords_for_action_on(self.platform, &bindings, action).into_iter();
                let primary = chords
                    .next()
                    .unwrap_or_else(|| default_chord_for_action_on(self.platform, action));
                ShortcutRow {
                    action,
                    command: command_for_action(action).to_owned(),
                    primary,
                    secondary: chords.next(),
                }
            })
            .collect()
    }

    /// The actions whose key repeats while held: each auto-repeat runs the
    /// action again. Every other action runs once per physical press
    /// ([`ShortcutAction::repeats_while_held`]).
    #[must_use]
    pub fn repeating_shortcuts(&self) -> Vec<ShortcutAction> {
        SHORTCUT_ACTIONS
            .iter()
            .copied()
            .filter(|action| action.repeats_while_held())
            .collect()
    }

    /// Decide what invoking `id` means in the state `snapshot` describes. Only
    /// availability is checked here: a command the session runs is validated by
    /// the core state machine like any other, and a shell gates the commands it
    /// carries out on the registry state it queried.
    pub(crate) fn invocation(&self, id: &str, snapshot: &SessionSnapshot) -> Invocation {
        if !is_registry_id(id) {
            return Invocation::Refuse(
                RejectReason::UnknownCommand,
                format!("`{id}` is not a registry command"),
            );
        }
        if !self.is_available(id) {
            return Invocation::Refuse(
                RejectReason::Unavailable,
                format!("`{id}` is not available in this shell"),
            );
        }
        match session_command(id, snapshot) {
            Some(command) => Invocation::Run(command),
            None => Invocation::Perform,
        }
    }

    fn is_available(&self, id: &str) -> bool {
        SESSION_COMMANDS.contains(&id) || self.shell_commands.iter().any(|shell| shell == id)
    }
}

/// The session command a session-run registry command stands for, given the
/// current state; `None` for a command the shell carries out. The volume steps
/// and mute are the transport controls' own `nudge_volume` and `toggle_mute`,
/// so a key, a menu item and the controls share one volume model: a step builds
/// on the level asked for last, and M unmutes what the mute button muted.
fn session_command(id: &str, snapshot: &SessionSnapshot) -> Option<Command> {
    let seek = |seconds| Command::Seek {
        seconds,
        mode: SeekMode::Relative,
        precision: SeekPrecision::Exact,
    };
    let volume = |delta| Command::NudgeVolume { delta };
    let subtitle_delay = |delta| Command::SetSubtitleDelay {
        seconds: snapshot.subtitle_delay + delta,
    };
    Some(match id {
        "play-pause" => Command::TogglePause {},
        "close-media" => Command::Close {},
        "seek-back" => seek(-SEEK_STEP_SECONDS),
        "seek-forward" => seek(SEEK_STEP_SECONDS),
        "volume-down" => volume(-VOLUME_STEP),
        "volume-up" => volume(VOLUME_STEP),
        "mute" => Command::ToggleMute {},
        "subtitle-delay-forward" => subtitle_delay(SUBTITLE_DELAY_STEP_SECONDS),
        "subtitle-delay-back" => subtitle_delay(-SUBTITLE_DELAY_STEP_SECONDS),
        _ => return None,
    })
}

/// Whether the core state machine accepts the session command `id` stands for
/// in `status`; `None` for a command the shell carries out. A media source alone
/// is not enough: the source stays set while a file opens, after it ends and
/// after a load error, and the machine refuses pausing, seeking and subtitle
/// timing in all three.
fn session_command_enabled(id: &str, status: Status) -> Option<bool> {
    let active = matches!(status, Status::Playing | Status::Paused);
    Some(match id {
        "play-pause"
        | "seek-back"
        | "seek-forward"
        | "subtitle-delay-forward"
        | "subtitle-delay-back" => active,
        "close-media" => status != Status::Idle,
        // Volume and mute are engine-global and apply without media.
        "volume-down" | "volume-up" | "mute" => true,
        _ => return None,
    })
}

/// The core command context for the session's state and its playlist of
/// `playlist_items` items. Open media is a playlist of at least one, whatever
/// the playlist reports while a folder is read.
fn player_context(
    snapshot: &SessionSnapshot,
    window: WindowState,
    playlist_items: usize,
) -> PlayerCommandContext {
    let source = snapshot.source.as_ref();
    PlayerCommandContext {
        has_media: source.is_some(),
        has_local_media: source.is_some_and(|source| source.kind == SourceKind::Local),
        has_online_media: source.is_some_and(|source| source.kind == SourceKind::Url),
        has_video_geometry: snapshot.video.is_some(),
        playlist_count: playlist_items.max(usize::from(source.is_some())),
        auto_advance_enabled: false,
        compact_mode: window.mini_player,
        fullscreen: window.fullscreen,
        ..PlayerCommandContext::default()
    }
}

/// Enabled and checked for a keyboard-only command.
fn keyboard_state(
    action: ShortcutAction,
    context: &PlayerCommandContext,
    snapshot: &SessionSnapshot,
) -> (bool, bool) {
    let enabled = session_command_enabled(action.id(), snapshot.status).unwrap_or(match action {
        ShortcutAction::PreviousItem | ShortcutAction::NextItem => context.playlist_count > 1,
        ShortcutAction::EscapeFullscreen => context.fullscreen || context.compact_mode,
        // Every other keyboard command acts on the open media.
        _ => context.has_media,
    });
    let checked = action == ShortcutAction::Mute && audio_muted(snapshot.volume, snapshot.mute);
    (enabled, checked)
}

/// The registry command an action runs: the core command bound to it, or the
/// keyboard-only command of the same id.
fn command_for_action(action: ShortcutAction) -> &'static str {
    player_command_registry()
        .iter()
        .find(|spec| spec.shortcut == Some(action))
        .map_or_else(|| action.id(), |spec| spec.id.id())
}

fn is_registry_id(id: &str) -> bool {
    player_command_registry()
        .iter()
        .any(|spec| spec.id.id() == id)
        || KEYBOARD_COMMANDS
            .iter()
            .any(|(action, _)| action.id() == id)
}

fn primary_chord(
    platform: Platform,
    bindings: &[ShortcutBinding],
    action: ShortcutAction,
) -> ShortcutChord {
    chords_for_action_on(platform, bindings, action)
        .into_iter()
        .next()
        .unwrap_or_else(|| default_chord_for_action_on(platform, action))
}

fn menu_of(id: PlayerCommandId) -> Option<CommandMenu> {
    if PLAYER_COMMAND_MENU_TOP_LEVEL.contains(&PlayerCommandMenuEntry::Command(id)) {
        return Some(CommandMenu::TopLevel);
    }
    [
        (PlayerCommandMenuPage::Video, CommandMenu::Video),
        (PlayerCommandMenuPage::Playback, CommandMenu::Playback),
        (PlayerCommandMenuPage::Window, CommandMenu::Window),
        (
            PlayerCommandMenuPage::ToolsAdvanced,
            CommandMenu::ToolsAdvanced,
        ),
    ]
    .into_iter()
    .find(|(page, _)| page.commands().contains(&id))
    .map(|(_, menu)| menu)
}

impl From<PlayerCommandGroup> for CommandGroup {
    fn from(group: PlayerCommandGroup) -> Self {
        match group {
            PlayerCommandGroup::PlaybackNavigation => Self::PlaybackNavigation,
            PlayerCommandGroup::TracksSubtitles => Self::TracksSubtitles,
            PlayerCommandGroup::ViewWindow => Self::ViewWindow,
            PlayerCommandGroup::MediaFile => Self::MediaFile,
            PlayerCommandGroup::Tools => Self::Tools,
            PlayerCommandGroup::Settings => Self::Settings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{EndReason, MediaSource, VideoGeometry};
    use std::collections::HashSet;

    fn registry(platform: Platform, shell_commands: &[&str]) -> CommandRegistry {
        let shell_commands: Vec<String> = shell_commands.iter().map(|&id| id.to_owned()).collect();
        CommandRegistry::new(platform, &shell_commands).expect("known command ids")
    }

    fn playing(kind: SourceKind, location: &str) -> SessionSnapshot {
        SessionSnapshot {
            status: Status::Playing,
            source: Some(MediaSource {
                kind,
                location: location.to_owned(),
            }),
            volume: Some(100.0),
            ..SessionSnapshot::default()
        }
    }

    fn row<'a>(commands: &'a [RegistryCommand], id: &str) -> &'a RegistryCommand {
        commands
            .iter()
            .find(|command| command.id == id)
            .unwrap_or_else(|| panic!("{id} is in the registry"))
    }

    fn state(commands: &[RegistryCommand], id: &str) -> (bool, bool) {
        let command = row(commands, id);
        (command.enabled, command.checked)
    }

    #[test]
    fn enabled_and_checked_follow_the_session_state() {
        let registry = registry(Platform::MacOs, &[]);
        let window = WindowState::default();

        let idle = registry.resolve(&SessionSnapshot::default(), window, 0);
        assert_eq!(state(&idle, "play-pause"), (false, false));
        assert_eq!(state(&idle, "close-media"), (false, false));
        assert_eq!(state(&idle, "seek-forward"), (false, false));
        assert_eq!(state(&idle, "subtitle-delay-back"), (false, false));
        // Volume and mute work without media.
        assert_eq!(state(&idle, "volume-up"), (true, false));
        assert_eq!(state(&idle, "mute"), (true, false));

        let mut film = playing(SourceKind::Local, "/media/film.mkv");
        film.mute = true;
        let local = registry.resolve(&film, window, 0);
        assert_eq!(state(&local, "play-pause"), (true, false));
        assert_eq!(state(&local, "close-media"), (true, false));
        assert_eq!(state(&local, "seek-forward"), (true, false));
        assert_eq!(
            state(&local, "mute"),
            (true, true),
            "mute is checked while muted"
        );
        // Muted the way `toggle_mute` mutes: the level is 0, libmpv's flag is off.
        let toggled = SessionSnapshot {
            volume: Some(0.0),
            mute: false,
            ..film.clone()
        };
        assert_eq!(
            state(&registry.resolve(&toggled, window, 0), "mute"),
            (true, true),
            "mute is checked at level 0"
        );
        assert_eq!(state(&local, "add-to-queue"), (true, false));
        assert_eq!(state(&local, "save-video"), (false, false), "local media");
        assert_eq!(
            state(&local, "fit-window-to-media"),
            (false, false),
            "no video yet"
        );
        // One open item is a playlist of one: nothing to step to.
        assert_eq!(state(&local, "next-item"), (false, false));
        assert_eq!(state(&local, "previous-item"), (false, false));
        assert_eq!(state(&local, "save-playlist"), (true, false));

        film.video = Some(VideoGeometry {
            width: 1920,
            height: 800,
            display_aspect: 2.4,
        });
        let with_video = registry.resolve(&film, window, 0);
        assert_eq!(state(&with_video, "fit-window-to-media"), (true, false));
        assert_eq!(state(&with_video, "zoom-in"), (true, false));

        let stream = registry.resolve(
            &playing(SourceKind::Url, "https://example.com/a.m3u8"),
            window,
            0,
        );
        assert_eq!(state(&stream, "save-video"), (true, false));
        assert_eq!(state(&stream, "add-to-queue"), (false, false));
    }

    #[test]
    fn the_session_playlist_enables_the_item_steps() {
        let registry = registry(Platform::MacOs, &[]);
        let film = playing(SourceKind::Local, "/media/film.mkv");
        let window = WindowState::default();

        let folder = registry.resolve(&film, window, 3);
        assert_eq!(state(&folder, "next-item"), (true, false));
        assert_eq!(state(&folder, "previous-item"), (true, false));

        // A playlist of one, or open media while its folder is still read.
        for items in [0, 1] {
            let alone = registry.resolve(&film, window, items);
            assert_eq!(state(&alone, "next-item"), (false, false), "{items}");
            assert_eq!(state(&alone, "save-playlist"), (true, false), "{items}");
        }
        let idle = registry.resolve(&SessionSnapshot::default(), window, 0);
        assert_eq!(state(&idle, "save-playlist"), (false, false));
    }

    #[test]
    fn held_keys_repeat_only_the_steps() {
        let registry = registry(Platform::MacOs, &[]);
        let repeating = registry.repeating_shortcuts();
        for action in [
            ShortcutAction::SeekBack,
            ShortcutAction::SeekForward,
            ShortcutAction::VolumeUp,
            ShortcutAction::VolumeDown,
        ] {
            assert!(repeating.contains(&action), "{action:?}");
        }
        for action in [
            ShortcutAction::PlayPause,
            ShortcutAction::Mute,
            ShortcutAction::Fullscreen,
            ShortcutAction::NextItem,
        ] {
            assert!(!repeating.contains(&action), "{action:?}");
        }
        // Every repeating action is one a shell can bind.
        let table = registry.shortcut_table();
        for action in &repeating {
            assert!(table.iter().any(|row| row.action == *action), "{action:?}");
        }
    }

    #[test]
    fn media_that_is_opening_or_has_ended_cannot_be_paused_seeked_or_retimed() {
        let registry = registry(Platform::MacOs, &[]);
        let refused_without_active_media = [
            "play-pause",
            "seek-back",
            "seek-forward",
            "subtitle-delay-forward",
            "subtitle-delay-back",
        ];
        let always_open = ["close-media", "volume-down", "volume-up", "mute"];

        for (status, end_reason) in [
            (Status::Opening, None),
            (Status::Ended, Some(EndReason::Eof)),
            (Status::Ended, Some(EndReason::Error)),
        ] {
            // The source stays set in all three.
            let snapshot = SessionSnapshot {
                status,
                end_reason,
                ..playing(SourceKind::Local, "/media/film.mkv")
            };
            let commands = registry.resolve(&snapshot, WindowState::default(), 0);
            for id in refused_without_active_media {
                assert!(!row(&commands, id).enabled, "{id} while {status:?}");
            }
            for id in always_open {
                assert!(row(&commands, id).enabled, "{id} while {status:?}");
            }
        }

        let paused = SessionSnapshot {
            status: Status::Paused,
            ..playing(SourceKind::Local, "/media/film.mkv")
        };
        let commands = registry.resolve(&paused, WindowState::default(), 0);
        for id in refused_without_active_media.into_iter().chain(always_open) {
            assert!(row(&commands, id).enabled, "{id} while paused");
        }
    }

    #[test]
    fn the_shells_window_state_drives_the_window_commands() {
        let registry = registry(Platform::MacOs, &[]);
        let snapshot = playing(SourceKind::Local, "/media/film.mkv");

        let windowed = registry.resolve(&snapshot, WindowState::default(), 0);
        assert_eq!(row(&windowed, "fullscreen").label, "Enter fullscreen");
        assert_eq!(state(&windowed, "fullscreen"), (true, false));
        assert_eq!(state(&windowed, "escape-fullscreen"), (false, false));

        let fullscreen = registry.resolve(
            &snapshot,
            WindowState {
                fullscreen: true,
                mini_player: false,
            },
            0,
        );
        assert_eq!(row(&fullscreen, "fullscreen").label, "Exit fullscreen");
        assert_eq!(state(&fullscreen, "fullscreen"), (true, true));
        assert_eq!(state(&fullscreen, "escape-fullscreen"), (true, false));

        let mini = registry.resolve(
            &snapshot,
            WindowState {
                fullscreen: false,
                mini_player: true,
            },
            0,
        );
        assert!(row(&mini, "mini-player").checked);
        assert!(row(&mini, "escape-fullscreen").enabled);
    }

    #[test]
    fn macos_hints_and_the_shortcut_table_use_cmd_chords() {
        let mac = registry(Platform::MacOs, &[]);
        let commands = mac.resolve(&SessionSnapshot::default(), WindowState::default(), 0);
        let hint = |id| {
            row(&commands, id)
                .shortcut
                .as_ref()
                .map(ShortcutChord::label)
        };
        assert_eq!(hint("open-file").as_deref(), Some("Cmd+O"));
        assert_eq!(hint("open-url").as_deref(), Some("Shift+Cmd+O"));
        assert_eq!(hint("media-info").as_deref(), Some("Cmd+I"));
        assert_eq!(hint("open-settings").as_deref(), Some("Cmd+,"));
        assert_eq!(hint("subtitle-next-cue").as_deref(), Some("Cmd+Right"));
        assert!(
            row(&commands, "open-file")
                .shortcut
                .as_ref()
                .unwrap()
                .modifiers()
                .cmd
        );
        // In-window keys stay single keys.
        assert_eq!(hint("play-pause").as_deref(), Some("Space"));
        assert_eq!(hint("mute").as_deref(), Some("M"));
        assert_eq!(hint("seek-back").as_deref(), Some("Left"));
        assert_eq!(hint("open-folder"), None);

        let table = mac.shortcut_table();
        let open_file = table
            .iter()
            .find(|row| row.action == ShortcutAction::OpenFile)
            .unwrap();
        assert_eq!(open_file.primary.label(), "Cmd+O");
        assert_eq!(open_file.secondary, None);
        assert!(
            !table.iter().any(|row| row.primary.modifiers().ctrl),
            "no Ctrl chord survives on macOS"
        );

        let linux = registry(Platform::Linux, &[]);
        let commands = linux.resolve(&SessionSnapshot::default(), WindowState::default(), 0);
        assert_eq!(
            row(&commands, "open-file")
                .shortcut
                .as_ref()
                .unwrap()
                .label(),
            "O"
        );
        assert_eq!(
            row(&commands, "open-settings")
                .shortcut
                .as_ref()
                .unwrap()
                .label(),
            "Ctrl+,"
        );
    }

    #[test]
    fn the_registry_holds_every_core_command_and_every_other_action_once() {
        let commands = registry(Platform::MacOs, &[]).resolve(
            &SessionSnapshot::default(),
            WindowState::default(),
            0,
        );
        let ids: Vec<&str> = commands.iter().map(|command| command.id.as_str()).collect();
        let unique: HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "ids are unique");

        let core: Vec<&str> = player_command_registry()
            .iter()
            .map(|spec| spec.id.id())
            .collect();
        assert_eq!(
            ids[..core.len()],
            core[..],
            "core commands first, in core order"
        );

        // Every bindable action runs exactly one registry command, and every
        // keyboard-only command is an action no core command is bound to.
        let table = registry(Platform::MacOs, &[]).shortcut_table();
        assert_eq!(table.len(), SHORTCUT_ACTIONS.len());
        for shortcut in &table {
            assert!(unique.contains(shortcut.command.as_str()), "{shortcut:?}");
        }
        let save = table
            .iter()
            .find(|row| row.action == ShortcutAction::SaveScreenshot)
            .unwrap();
        assert_eq!(save.command, "save-frame");
        let keyboard_only: Vec<ShortcutAction> = SHORTCUT_ACTIONS
            .iter()
            .copied()
            .filter(|&action| {
                !player_command_registry()
                    .iter()
                    .any(|spec| spec.shortcut == Some(action))
            })
            .collect();
        assert_eq!(
            KEYBOARD_COMMANDS
                .iter()
                .map(|(action, _)| *action)
                .collect::<Vec<_>>(),
            keyboard_only
        );
        assert_eq!(ids.len(), core.len() + keyboard_only.len());
        for id in SESSION_COMMANDS {
            assert!(unique.contains(id), "{id} is a registry command");
        }
    }

    #[test]
    fn only_session_commands_and_the_shells_own_commands_are_available() {
        let snapshot = SessionSnapshot::default();
        let bare = registry(Platform::MacOs, &[]).resolve(&snapshot, WindowState::default(), 0);
        let available: Vec<&str> = bare
            .iter()
            .filter(|command| command.available)
            .map(|command| command.id.as_str())
            .collect();
        assert_eq!(
            available,
            [
                "play-pause",
                "close-media",
                "seek-back",
                "seek-forward",
                "volume-down",
                "volume-up",
                "mute",
                "subtitle-delay-forward",
                "subtitle-delay-back",
            ]
        );

        let shell = registry(Platform::MacOs, &["open-file", "fullscreen"]).resolve(
            &snapshot,
            WindowState::default(),
            0,
        );
        assert!(row(&shell, "open-file").available);
        assert!(row(&shell, "fullscreen").available);
        assert!(!row(&shell, "open-url").available);

        assert_eq!(
            CommandRegistry::new(Platform::MacOs, &["open-flie".to_owned()]),
            Err("shell_commands names an unknown command `open-flie`".to_owned())
        );
    }

    #[test]
    fn an_invocation_runs_a_session_command_or_hands_over_to_the_shell() {
        let registry = registry(Platform::MacOs, &["open-file"]);
        let mut snapshot = playing(SourceKind::Local, "/media/film.mkv");
        snapshot.subtitle_delay = 0.25;

        assert_eq!(
            registry.invocation("play-pause", &snapshot),
            Invocation::Run(Command::TogglePause {})
        );
        assert_eq!(
            registry.invocation("close-media", &snapshot),
            Invocation::Run(Command::Close {})
        );
        assert_eq!(
            registry.invocation("mute", &snapshot),
            Invocation::Run(Command::ToggleMute {})
        );
        assert_eq!(
            registry.invocation("seek-back", &snapshot),
            Invocation::Run(Command::Seek {
                seconds: -5.0,
                mode: SeekMode::Relative,
                precision: SeekPrecision::Exact,
            })
        );
        // The volume keys are nudges: the session builds them on the level it
        // asked for last and clamps them to the core range.
        assert_eq!(
            registry.invocation("volume-up", &snapshot),
            Invocation::Run(Command::NudgeVolume { delta: 5.0 })
        );
        assert_eq!(
            registry.invocation("volume-down", &snapshot),
            Invocation::Run(Command::NudgeVolume { delta: -5.0 })
        );
        assert_eq!(
            registry.invocation("subtitle-delay-back", &snapshot),
            Invocation::Run(Command::SetSubtitleDelay { seconds: 0.2 })
        );

        assert_eq!(
            registry.invocation("open-file", &snapshot),
            Invocation::Perform
        );
        assert!(matches!(
            registry.invocation("open-url", &snapshot),
            Invocation::Refuse(RejectReason::Unavailable, _)
        ));
        assert!(matches!(
            registry.invocation("eject", &snapshot),
            Invocation::Refuse(RejectReason::UnknownCommand, _)
        ));
    }
}
