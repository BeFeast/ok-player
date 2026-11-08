//! Keyboard shortcut model for the player shell: the action table with default chords, the
//! `action=shortcut` config-text parsing and serialisation, conflict detection, and chord
//! display labels. Extracted from the GTK shell (`okp-linux-gtk`), which keeps only the capture
//! UI and dispatch wiring; the shell's shortcut tests moved here and are the executable spec.
//! The Windows projection in `src/OkPlayer.Core/Shortcuts.cs` mirrors this contract and test set
//! without moving native-key capture into either core (see `docs/core-compatibility.md`). Pure
//! (no I/O, no UI, no GDK).
//!
//! A chord is one non-modifier key plus any of Ctrl/Alt/Shift, and Cmd on macOS. Keys are
//! identified by their canonical, case-folded keysym name (`space`, `comma`, `Page_Up`, `c`, …).
//! Config text is one `action-id=Chord` binding per line; blank lines and lines starting with
//! `#` or `;` are ignored; an action may bind at most [`MAX_SHORTCUTS_PER_ACTION`] chords.
//! Serialisation writes only bindings that differ from the action's default, in the same
//! `Ctrl+Alt+Shift+Cmd+Key` display form the parser accepts, so a round-trip is faithful. The
//! portable key namespace (the display aliases plus ASCII letters and digits) resolves here;
//! any other token (function keys, locale-specific keysyms) is resolved by the shell through
//! [`KeyNames`] against the real platform keyval tables, exactly as the pre-extraction parser
//! did with `gdk::Key::from_name`.
//!
//! Defaults are per [`Platform`]. The unsuffixed functions use the Linux table, which the
//! Windows projection mirrors; the `_on` variants take the platform. The macOS table keeps
//! every in-window key and moves menu commands to Cmd chords (see
//! [`ShortcutAction::default_shortcut_on`]). Cmd is rejected in Linux config text: the GTK
//! shell masks Super out of key events, so a Cmd binding there could never fire.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::app::Platform;

/// Every player command that can be bound to a keyboard shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    PlayPause,
    SeekBack,
    SeekForward,
    FrameForward,
    FrameBack,
    PreviousItem,
    NextItem,
    VolumeDown,
    VolumeUp,
    Mute,
    OpenFile,
    AddSubtitle,
    OpenUrl,
    CloseMedia,
    SaveScreenshot,
    CopyFrame,
    MediaInfo,
    GoToTime,
    AbLoop,
    SubtitleDelayForward,
    SubtitleDelayBack,
    SubtitleSizeDown,
    SubtitleSizeUp,
    SubtitlePreviousCue,
    SubtitleNextCue,
    Fullscreen,
    EscapeFullscreen,
    OpenSettings,
}

impl ShortcutAction {
    /// Stable identifier used on the left-hand side of a config binding.
    pub fn id(self) -> &'static str {
        match self {
            Self::PlayPause => "play-pause",
            Self::SeekBack => "seek-back",
            Self::SeekForward => "seek-forward",
            Self::FrameForward => "frame-forward",
            Self::FrameBack => "frame-back",
            Self::PreviousItem => "previous-item",
            Self::NextItem => "next-item",
            Self::VolumeDown => "volume-down",
            Self::VolumeUp => "volume-up",
            Self::Mute => "mute",
            Self::OpenFile => "open-file",
            Self::AddSubtitle => "add-subtitle",
            Self::OpenUrl => "open-url",
            Self::CloseMedia => "close-media",
            Self::SaveScreenshot => "save-screenshot",
            Self::CopyFrame => "copy-frame",
            Self::MediaInfo => "media-info",
            Self::GoToTime => "go-to-time",
            Self::AbLoop => "ab-loop",
            Self::SubtitleDelayForward => "subtitle-delay-forward",
            Self::SubtitleDelayBack => "subtitle-delay-back",
            Self::SubtitleSizeDown => "subtitle-size-down",
            Self::SubtitleSizeUp => "subtitle-size-up",
            Self::SubtitlePreviousCue => "subtitle-previous-cue",
            Self::SubtitleNextCue => "subtitle-next-cue",
            Self::Fullscreen => "fullscreen",
            Self::EscapeFullscreen => "escape-fullscreen",
            Self::OpenSettings => "open-settings",
        }
    }

    /// Human-readable name shown in the Settings shortcut editor.
    pub fn label(self) -> &'static str {
        match self {
            Self::PlayPause => "Play / Pause",
            Self::SeekBack => "Seek Back",
            Self::SeekForward => "Seek Forward",
            Self::FrameForward => "Frame Forward",
            Self::FrameBack => "Frame Back",
            Self::PreviousItem => "Previous Item",
            Self::NextItem => "Next Item",
            Self::VolumeDown => "Volume Down",
            Self::VolumeUp => "Volume Up",
            Self::Mute => "Mute",
            Self::OpenFile => "Open File",
            Self::AddSubtitle => "Add Subtitle",
            Self::OpenUrl => "Open URL",
            Self::CloseMedia => "Close Media",
            Self::SaveScreenshot => "Save Screenshot",
            Self::CopyFrame => "Copy Frame",
            Self::MediaInfo => "Media Info",
            Self::GoToTime => "Go to Time",
            Self::AbLoop => "A-B Loop",
            Self::SubtitleDelayForward => "Subtitle Delay Forward",
            Self::SubtitleDelayBack => "Subtitle Delay Back",
            Self::SubtitleSizeDown => "Subtitle Size Down",
            Self::SubtitleSizeUp => "Subtitle Size Up",
            Self::SubtitlePreviousCue => "Previous Subtitle Cue",
            Self::SubtitleNextCue => "Next Subtitle Cue",
            Self::Fullscreen => "Fullscreen",
            Self::EscapeFullscreen => "Exit Fullscreen",
            Self::OpenSettings => "Settings",
        }
    }

    /// Built-in chord in config-text form.
    pub fn default_shortcut(self) -> &'static str {
        match self {
            Self::PlayPause => "Space",
            Self::SeekBack => "Left",
            Self::SeekForward => "Right",
            Self::FrameForward => ".",
            Self::FrameBack => ",",
            Self::PreviousItem => "PageUp",
            Self::NextItem => "PageDown",
            Self::VolumeDown => "Down",
            Self::VolumeUp => "Up",
            Self::Mute => "M",
            Self::OpenFile => "O",
            Self::AddSubtitle => "S",
            Self::OpenUrl => "U",
            Self::CloseMedia => "X",
            Self::SaveScreenshot => "C",
            Self::CopyFrame => "Shift+C",
            Self::MediaInfo => "I",
            Self::GoToTime => "J",
            Self::AbLoop => "L",
            Self::SubtitleDelayForward => "Z",
            Self::SubtitleDelayBack => "Shift+Z",
            Self::SubtitleSizeDown => "[",
            Self::SubtitleSizeUp => "]",
            Self::SubtitlePreviousCue => "Ctrl+Left",
            Self::SubtitleNextCue => "Ctrl+Right",
            Self::Fullscreen => "F",
            Self::EscapeFullscreen => "Escape",
            Self::OpenSettings => "Ctrl+,",
        }
    }

    /// Built-in chord in config-text form for a platform.
    ///
    /// The macOS table differs from the Linux one in two ways. Every Linux Ctrl chord moves to
    /// Cmd (`Cmd+,` for Settings, `Cmd+Left`/`Cmd+Right` for subtitle cues; Ctrl+arrows switch
    /// Spaces on macOS). Menu commands that macOS users reach with a Command chord take one:
    /// `Cmd+O` Open File, `Shift+Cmd+O` Open URL, `Cmd+I` Media Info (the inspector chord).
    /// Every in-window key (Space, arrows, F, M, `,`/`.`, Z/Shift+Z, `[`/`]`, and the rest)
    /// stays as it is on Linux.
    pub fn default_shortcut_on(self, platform: Platform) -> &'static str {
        match platform {
            Platform::Linux => self.default_shortcut(),
            Platform::MacOs => match self {
                Self::OpenFile => "Cmd+O",
                Self::OpenUrl => "Shift+Cmd+O",
                Self::MediaInfo => "Cmd+I",
                Self::SubtitlePreviousCue => "Cmd+Left",
                Self::SubtitleNextCue => "Cmd+Right",
                Self::OpenSettings => "Cmd+,",
                _ => self.default_shortcut(),
            },
        }
    }

    /// The action for a config identifier, or `None` for an unknown one.
    pub fn by_id(id: &str) -> Option<Self> {
        SHORTCUT_ACTIONS
            .iter()
            .copied()
            .find(|action| action.id() == id)
    }
}

/// Every bindable action, in the order the Settings editor and config serialisation use.
pub const SHORTCUT_ACTIONS: &[ShortcutAction] = &[
    ShortcutAction::PlayPause,
    ShortcutAction::SeekBack,
    ShortcutAction::SeekForward,
    ShortcutAction::FrameForward,
    ShortcutAction::FrameBack,
    ShortcutAction::PreviousItem,
    ShortcutAction::NextItem,
    ShortcutAction::VolumeDown,
    ShortcutAction::VolumeUp,
    ShortcutAction::Mute,
    ShortcutAction::OpenFile,
    ShortcutAction::AddSubtitle,
    ShortcutAction::OpenUrl,
    ShortcutAction::CloseMedia,
    ShortcutAction::SaveScreenshot,
    ShortcutAction::CopyFrame,
    ShortcutAction::MediaInfo,
    ShortcutAction::GoToTime,
    ShortcutAction::AbLoop,
    ShortcutAction::SubtitleDelayForward,
    ShortcutAction::SubtitleDelayBack,
    ShortcutAction::SubtitleSizeDown,
    ShortcutAction::SubtitleSizeUp,
    ShortcutAction::SubtitlePreviousCue,
    ShortcutAction::SubtitleNextCue,
    ShortcutAction::Fullscreen,
    ShortcutAction::EscapeFullscreen,
    ShortcutAction::OpenSettings,
];

/// One primary plus at most one secondary chord per action.
pub const MAX_SHORTCUTS_PER_ACTION: usize = 2;

/// The modifier set of a chord. Ctrl, Alt (Option on macOS), Shift, and, on macOS, Cmd take
/// part in shortcuts; the shell masks everything else (Caps Lock, Num Lock, Fn, Super on Linux,
/// …) out of key events before they reach the model. Missing fields deserialise as `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// The macOS Command key. Never set by the Linux shell.
    pub cmd: bool,
}

/// One non-modifier key plus modifiers. The key is stored as its canonical, case-folded keysym
/// name, so chords compare (for conflict detection and dispatch) and serialise consistently no
/// matter whether they came from config text or from a captured key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutChord {
    key: String,
    modifiers: ShortcutModifiers,
}

impl ShortcutChord {
    fn new(key_name: &str, modifiers: ShortcutModifiers) -> Self {
        Self {
            key: folded_key_name(key_name),
            modifiers,
        }
    }

    /// True when a key event with this canonical key name and modifier set triggers the chord.
    pub fn matches(&self, key_name: &str, modifiers: ShortcutModifiers) -> bool {
        self.key == folded_key_name(key_name) && self.modifiers == modifiers
    }

    /// Display label, which is also the config-text form: `Ctrl+Alt+Shift+Cmd+Key` with the key
    /// in its display spelling (`Space`, `,`, `PageUp`, `C`, …). The modifier order is the
    /// macOS menu order (Control, Option, Shift, Command).
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.modifiers.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if self.modifiers.shift {
            parts.push("Shift".to_owned());
        }
        if self.modifiers.cmd {
            parts.push("Cmd".to_owned());
        }
        parts.push(display_key_name(&self.key));
        parts.join("+")
    }
}

/// A chord serialises as its config-text label (`"Shift+Cmd+O"`), the same text the config
/// parser reads.
impl Serialize for ShortcutChord {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.label())
    }
}

/// Reads the label form back. A key token outside the portable set is taken as the canonical
/// platform key name a serialised chord carries (`F1`, `Return`), so every chord this module
/// serialises reads back equal.
impl<'de> Deserialize<'de> for ShortcutChord {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_chord(&text, 0, &SerializedKeyNames).map_err(|error| D::Error::custom(error.message))
    }
}

/// Key namespace for serialised chords: their key names are already canonical.
struct SerializedKeyNames;

impl KeyNames for SerializedKeyNames {
    fn canonicalize_extra(&self, token: &str) -> Option<String> {
        Some(token.to_owned())
    }
}

/// One action ↔ chord pair in the resolved keybinding table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBinding {
    pub action: ShortcutAction,
    pub chord: ShortcutChord,
}

/// A rejected keybinding config. `line` is the 1-based offending config line, or 0 when the
/// error is not tied to one line (cross-binding conflicts, chords parsed outside a config).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutConfigError {
    pub line: usize,
    pub message: String,
}

/// The platform key namespace, injected by the shell so config tokens beyond the portable set
/// resolve against the real keyval tables while the model stays pure and testable.
pub trait KeyNames {
    /// Canonical, case-folded key name for a raw config token outside the portable set, or
    /// `None` when the platform knows no such key. The GTK shell answers from the GDK keyval
    /// tables (`gdk::Key::from_name`, then `to_lower().name()`); lookup stays case-sensitive
    /// there, so `Return` resolves and `return` does not — mirroring the pre-extraction parser.
    fn canonicalize_extra(&self, token: &str) -> Option<String>;
}

/// Key namespace with no platform extras: only the portable set (the display aliases plus
/// ASCII letters and digits) parses. Every default chord lives in the portable set, so this is
/// enough to resolve defaults — and to run the config-text tests — without a platform.
pub struct PortableKeyNames;

impl KeyNames for PortableKeyNames {
    fn canonicalize_extra(&self, _token: &str) -> Option<String> {
        None
    }
}

/// Parse one chord (`Ctrl+Shift+C`, `Space`, `Ctrl+,`) as it appears on the right-hand side of
/// a config binding. `line` is carried into the error for config diagnostics (0 when the text
/// does not come from a config line).
pub fn parse_chord(
    text: &str,
    line: usize,
    key_names: &dyn KeyNames,
) -> Result<ShortcutChord, ShortcutConfigError> {
    let mut modifiers = ShortcutModifiers::default();
    let mut key = None::<String>;

    for token in text
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.ctrl = true,
            "alt" | "option" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "cmd" | "command" => modifiers.cmd = true,
            _ if key.is_none() => {
                key = Some(
                    key_name_from_token(token, key_names)
                        .ok_or_else(|| config_error(line, &format!("Unknown key `{token}`.")))?,
                );
            }
            _ => {
                return Err(config_error(
                    line,
                    "Shortcut can only contain one non-modifier key.",
                ));
            }
        }
    }

    let Some(key) = key else {
        return Err(config_error(line, "Shortcut key is empty."));
    };

    Ok(ShortcutChord::new(&key, modifiers))
}

/// Resolve keybinding config text over the defaults: every action keeps its default chord
/// unless the text overrides it (the first override replaces the default, a second line adds a
/// secondary chord). Errors carry the offending 1-based line, or line 0 for cross-binding
/// conflicts. Uses the Linux defaults; see [`resolved_bindings_from_text_on`].
pub fn resolved_bindings_from_text(
    text: &str,
    key_names: &dyn KeyNames,
) -> Result<Vec<ShortcutBinding>, ShortcutConfigError> {
    resolved_bindings_from_text_on(Platform::Linux, text, key_names)
}

/// [`resolved_bindings_from_text`] over a platform's defaults. A Cmd chord is an error on a
/// platform without a Cmd key, at the line that names it.
pub fn resolved_bindings_from_text_on(
    platform: Platform,
    text: &str,
    key_names: &dyn KeyNames,
) -> Result<Vec<ShortcutBinding>, ShortcutConfigError> {
    let mut bindings = default_bindings_on(platform);
    let overrides = parse_config_overrides(platform, text, key_names)?;
    let override_chords = overrides
        .iter()
        .map(|(_, chord)| chord.clone())
        .collect::<Vec<_>>();
    for action in SHORTCUT_ACTIONS {
        let action_overrides = overrides
            .iter()
            .filter(|(override_action, _)| override_action == action)
            .map(|(_, chord)| chord.clone())
            .collect::<Vec<_>>();
        if action_overrides.is_empty() {
            continue;
        }

        bindings.retain(|binding| binding.action != *action);
        bindings.extend(action_overrides.into_iter().map(|chord| ShortcutBinding {
            action: *action,
            chord,
        }));
    }
    // These defaults were added after keybinding persistence shipped. An older
    // config may already use Ctrl+Left/Right for another action; keep that
    // user's binding instead of rejecting the entire config and falling back
    // to every default.
    bindings.retain(|binding| {
        overrides
            .iter()
            .any(|(action, _)| *action == binding.action)
            || !is_upgrade_shadowable_default(binding.action)
            || !override_chords.contains(&binding.chord)
    });
    validate_conflicts(&bindings)?;
    Ok(bindings)
}

fn is_upgrade_shadowable_default(action: ShortcutAction) -> bool {
    matches!(
        action,
        ShortcutAction::SubtitlePreviousCue | ShortcutAction::SubtitleNextCue
    )
}

/// Serialise bindings back to config text: only actions whose chords differ from the default
/// are written, one `action-id=Chord` line each, in [`SHORTCUT_ACTIONS`] order. All-default
/// bindings serialise to the empty string. Compares against the Linux defaults; see
/// [`config_text_from_bindings_on`].
pub fn config_text_from_bindings(bindings: &[ShortcutBinding]) -> String {
    config_text_from_bindings_on(Platform::Linux, bindings)
}

/// [`config_text_from_bindings`] against a platform's defaults.
pub fn config_text_from_bindings_on(platform: Platform, bindings: &[ShortcutBinding]) -> String {
    let mut lines = Vec::new();
    for action in SHORTCUT_ACTIONS {
        let default_chord = default_chord_for_action_on(platform, *action);
        let chords = chords_for_action_on(platform, bindings, *action);
        if chords.len() == 1 && chords[0] == default_chord {
            continue;
        }

        for chord in chords.into_iter().take(MAX_SHORTCUTS_PER_ACTION) {
            lines.push(format!("{}={}", action.id(), chord.label()));
        }
    }
    lines.join("\n")
}

/// The chords bound to an action, primary first, capped at [`MAX_SHORTCUTS_PER_ACTION`]; an
/// action absent from `bindings` falls back to its Linux default chord.
pub fn chords_for_action(
    bindings: &[ShortcutBinding],
    action: ShortcutAction,
) -> Vec<ShortcutChord> {
    chords_for_action_on(Platform::Linux, bindings, action)
}

/// [`chords_for_action`], falling back to the platform's default chord.
pub fn chords_for_action_on(
    platform: Platform,
    bindings: &[ShortcutBinding],
    action: ShortcutAction,
) -> Vec<ShortcutChord> {
    let chords = bindings
        .iter()
        .filter(|binding| binding.action == action)
        .map(|binding| binding.chord.clone())
        .take(MAX_SHORTCUTS_PER_ACTION)
        .collect::<Vec<_>>();

    if chords.is_empty() {
        vec![default_chord_for_action_on(platform, action)]
    } else {
        chords
    }
}

/// The built-in Linux chord for an action. Defaults live in the portable namespace by
/// construction, so no platform [`KeyNames`] is needed.
pub fn default_chord_for_action(action: ShortcutAction) -> ShortcutChord {
    default_chord_for_action_on(Platform::Linux, action)
}

/// The built-in chord for an action on a platform.
pub fn default_chord_for_action_on(platform: Platform, action: ShortcutAction) -> ShortcutChord {
    parse_chord(action.default_shortcut_on(platform), 0, &PortableKeyNames)
        .expect("default shortcuts should parse")
}

/// The full Linux default binding table, one chord per action, in [`SHORTCUT_ACTIONS`] order.
pub fn default_bindings() -> Vec<ShortcutBinding> {
    default_bindings_on(Platform::Linux)
}

/// A platform's full default binding table, in [`SHORTCUT_ACTIONS`] order.
pub fn default_bindings_on(platform: Platform) -> Vec<ShortcutBinding> {
    SHORTCUT_ACTIONS
        .iter()
        .copied()
        .map(|action| ShortcutBinding {
            action,
            chord: default_chord_for_action_on(platform, action),
        })
        .collect()
}

/// Dispatch: the action bound to a key event, given the event's canonical key name and
/// modifier set, or `None` when nothing is bound to it.
pub fn action_for_key(
    bindings: &[ShortcutBinding],
    key_name: &str,
    modifiers: ShortcutModifiers,
) -> Option<ShortcutAction> {
    let key = folded_key_name(key_name);
    bindings
        .iter()
        .find(|binding| binding.chord.key == key && binding.chord.modifiers == modifiers)
        .map(|binding| binding.action)
}

/// Reject binding tables where two bindings (across any actions) share one chord. The error
/// names both action ids and the chord label, with line 0 (the conflict spans lines).
pub fn validate_conflicts(bindings: &[ShortcutBinding]) -> Result<(), ShortcutConfigError> {
    for (index, left) in bindings.iter().enumerate() {
        if let Some(right) = bindings
            .iter()
            .skip(index + 1)
            .find(|right| right.chord == left.chord)
        {
            return Err(config_error(
                0,
                &format!(
                    "{} conflicts with {} on {}.",
                    right.action.id(),
                    left.action.id(),
                    left.chord.label()
                ),
            ));
        }
    }

    Ok(())
}

/// Build a chord from a captured key event, given the platform's canonical (case-folded) key
/// name. Modifier keys cannot form a chord on their own, and a key the platform cannot name has
/// no config-text form (it could never be parsed back), so both are rejected with the message
/// the capture UI shows.
pub fn chord_from_captured_key(
    key_name: Option<&str>,
    modifiers: ShortcutModifiers,
) -> Result<ShortcutChord, &'static str> {
    match key_name {
        Some(name) if !is_modifier_key_name(name) => Ok(ShortcutChord::new(name, modifiers)),
        _ => Err("Press a non-modifier key."),
    }
}

/// The chord slot being edited in the Settings shortcut editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutSlot {
    Primary,
    Secondary,
}

/// One shortcut editor row's chord state: the action with its primary and optional secondary
/// chord. The shell mirrors its UI rows into this shape for conflict checks and saving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionChords {
    pub action: ShortcutAction,
    pub primary: ShortcutChord,
    pub secondary: Option<ShortcutChord>,
}

/// Editor conflict detection: which action already holds `chord`, ignoring only the slot being
/// reassigned — so re-capturing a slot's current chord is fine, but an action's own other slot
/// still conflicts.
pub fn slot_conflict(
    rows: &[ActionChords],
    action: ShortcutAction,
    slot: ShortcutSlot,
    chord: &ShortcutChord,
) -> Option<ShortcutAction> {
    for row in rows {
        if !(row.action == action && slot == ShortcutSlot::Primary) && row.primary == *chord {
            return Some(row.action);
        }
        if !(row.action == action && slot == ShortcutSlot::Secondary)
            && row.secondary.as_ref() == Some(chord)
        {
            return Some(row.action);
        }
    }
    None
}

/// Flatten editor rows to the binding table that gets validated and serialised: primary first,
/// then the secondary if set, in row order.
pub fn bindings_from_action_chords(rows: &[ActionChords]) -> Vec<ShortcutBinding> {
    rows.iter()
        .flat_map(|row| {
            let mut bindings = vec![ShortcutBinding {
                action: row.action,
                chord: row.primary.clone(),
            }];
            if let Some(chord) = &row.secondary {
                bindings.push(ShortcutBinding {
                    action: row.action,
                    chord: chord.clone(),
                });
            }
            bindings
        })
        .collect()
}

/// Overrides in config order, one entry per parsed line.
fn parse_config_overrides(
    platform: Platform,
    text: &str,
    key_names: &dyn KeyNames,
) -> Result<Vec<(ShortcutAction, ShortcutChord)>, ShortcutConfigError> {
    let mut overrides = Vec::new();

    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        let Some((action_id, shortcut)) = trimmed.split_once('=') else {
            return Err(config_error(
                line_number,
                "Use action=shortcut syntax, one binding per line.",
            ));
        };
        let action_id = action_id.trim();
        let shortcut = shortcut.trim();
        let Some(action) = ShortcutAction::by_id(action_id) else {
            return Err(config_error(
                line_number,
                &format!("Unknown action `{action_id}`."),
            ));
        };
        let existing_count = overrides
            .iter()
            .filter(|(existing_action, _)| *existing_action == action)
            .count();
        if existing_count >= MAX_SHORTCUTS_PER_ACTION {
            return Err(config_error(
                line_number,
                &format!("Action `{action_id}` supports at most two shortcuts."),
            ));
        }

        let chord = parse_chord(shortcut, line_number, key_names)?;
        if chord.modifiers.cmd && platform != Platform::MacOs {
            return Err(config_error(
                line_number,
                "Cmd shortcuts are only available on macOS.",
            ));
        }
        overrides.push((action, chord));
    }

    Ok(overrides)
}

/// Resolve one config token to a canonical key name. The portable set resolves here: the
/// display aliases below plus ASCII letters and digits (folded to lowercase, mirroring the
/// platform's keysym case folding). Any other token is deferred, in its original spelling, to
/// the platform namespace.
fn key_name_from_token(token: &str, key_names: &dyn KeyNames) -> Option<String> {
    let normalized = match token.to_ascii_lowercase().as_str() {
        "," | "comma" => "comma",
        "." | "period" => "period",
        "[" | "bracketleft" => "bracketleft",
        "]" | "bracketright" => "bracketright",
        "esc" | "escape" => "Escape",
        "pageup" | "page_up" => "Page_Up",
        "pagedown" | "page_down" => "Page_Down",
        "space" => "space",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        single if single.len() == 1 && single.as_bytes()[0].is_ascii_alphanumeric() => {
            return Some(single.to_owned());
        }
        _ => return key_names.canonicalize_extra(token),
    };
    Some(normalized.to_owned())
}

/// Case-fold a canonical key name. Platform names arrive pre-folded from the shell; the model
/// only needs to fold the portable single-character names (`C` → `c`), matching the keysym
/// case folding the shell applies to key events.
fn folded_key_name(name: &str) -> String {
    if name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase() {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}

/// Keysym names that are modifiers on their own and therefore cannot anchor a chord.
fn is_modifier_key_name(name: &str) -> bool {
    matches!(
        name,
        "Shift_L"
            | "Shift_R"
            | "Control_L"
            | "Control_R"
            | "Alt_L"
            | "Alt_R"
            | "Meta_L"
            | "Meta_R"
            | "Super_L"
            | "Super_R"
            | "Hyper_L"
            | "Hyper_R"
            | "ISO_Level3_Shift"
            | "Caps_Lock"
    )
}

fn config_error(line: usize, message: &str) -> ShortcutConfigError {
    ShortcutConfigError {
        line,
        message: message.to_owned(),
    }
}

/// Display spelling of a canonical key name, used in chord labels and config text.
fn display_key_name(name: &str) -> String {
    match name {
        "space" => "Space".to_owned(),
        "comma" => ",".to_owned(),
        "period" => ".".to_owned(),
        "bracketleft" => "[".to_owned(),
        "bracketright" => "]".to_owned(),
        "Page_Up" => "PageUp".to_owned(),
        "Page_Down" => "PageDown".to_owned(),
        key if key.len() == 1 => key.to_ascii_uppercase(),
        key => key.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mods() -> ShortcutModifiers {
        ShortcutModifiers::default()
    }

    fn ctrl() -> ShortcutModifiers {
        ShortcutModifiers {
            ctrl: true,
            ..ShortcutModifiers::default()
        }
    }

    fn shift() -> ShortcutModifiers {
        ShortcutModifiers {
            shift: true,
            ..ShortcutModifiers::default()
        }
    }

    fn ctrl_shift() -> ShortcutModifiers {
        ShortcutModifiers {
            ctrl: true,
            shift: true,
            ..ShortcutModifiers::default()
        }
    }

    fn portable_chord(text: &str) -> ShortcutChord {
        parse_chord(text, 0, &PortableKeyNames).expect("chord should parse")
    }

    #[test]
    fn shortcut_parser_accepts_action_overrides() {
        let bindings = resolved_bindings_from_text(
            "\
play-pause=P
copy-frame=Ctrl+Shift+C
",
            &PortableKeyNames,
        )
        .expect("shortcuts should parse");

        assert_eq!(
            action_for_key(&bindings, "p", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(action_for_key(&bindings, "space", no_mods()), None);
        assert_eq!(
            action_for_key(&bindings, "C", ctrl_shift()),
            Some(ShortcutAction::CopyFrame)
        );
    }

    #[test]
    fn shortcut_parser_accepts_secondary_action_binding() {
        let bindings = resolved_bindings_from_text(
            "\
play-pause=Space
play-pause=P
",
            &PortableKeyNames,
        )
        .expect("secondary shortcut should parse");

        assert_eq!(
            action_for_key(&bindings, "space", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            action_for_key(&bindings, "p", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn shortcut_parser_skips_comments_and_blank_lines() {
        let bindings = resolved_bindings_from_text(
            "\
# a comment
; also a comment

play-pause=P
",
            &PortableKeyNames,
        )
        .expect("commented config should parse");

        assert_eq!(
            action_for_key(&bindings, "p", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn shortcut_parser_rejects_unknown_action() {
        let error = resolved_bindings_from_text("dance=Space", &PortableKeyNames)
            .expect_err("unknown action should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("Unknown action"));
    }

    #[test]
    fn shortcut_parser_rejects_unknown_key() {
        let error = resolved_bindings_from_text("play-pause=HyperDrive", &PortableKeyNames)
            .expect_err("unknown key should fail");

        assert_eq!(error.line, 1);
        assert!(error.message.contains("Unknown key"));
    }

    #[test]
    fn shortcut_parser_rejects_conflicting_bindings() {
        let error = resolved_bindings_from_text("play-pause=C", &PortableKeyNames)
            .expect_err("conflict should fail");

        assert_eq!(error.line, 0);
        assert!(error.message.contains("conflicts"));
    }

    #[test]
    fn shortcut_parser_keeps_saved_binding_that_conflicts_with_new_default() {
        let bindings = resolved_bindings_from_text("seek-back=Ctrl+Left", &PortableKeyNames)
            .expect("saved shortcut should override a newly introduced default");

        assert_eq!(
            action_for_key(&bindings, "Left", ctrl()),
            Some(ShortcutAction::SeekBack)
        );
        assert!(!bindings.iter().any(|binding| {
            binding.action == ShortcutAction::SubtitlePreviousCue
                && binding.chord == portable_chord("Ctrl+Left")
        }));
    }

    #[test]
    fn shortcut_parser_rejects_third_action_binding() {
        let error = resolved_bindings_from_text(
            "\
play-pause=Space
play-pause=P
play-pause=Ctrl+P
",
            &PortableKeyNames,
        )
        .expect_err("third shortcut should fail");

        assert_eq!(error.line, 3);
        assert!(error.message.contains("at most two"));
    }

    #[test]
    fn shortcut_defaults_keep_shift_copy_frame_distinct() {
        let bindings = default_bindings();

        assert_eq!(
            action_for_key(&bindings, "c", no_mods()),
            Some(ShortcutAction::SaveScreenshot)
        );
        assert_eq!(
            action_for_key(&bindings, "C", shift()),
            Some(ShortcutAction::CopyFrame)
        );
    }

    #[test]
    fn shortcut_config_text_serializes_only_custom_bindings() {
        let mut bindings = default_bindings();
        let custom = portable_chord("P");
        bindings
            .iter_mut()
            .find(|binding| binding.action == ShortcutAction::PlayPause)
            .expect("play-pause binding should exist")
            .chord = custom;

        assert_eq!(config_text_from_bindings(&bindings), "play-pause=P");
        assert!(
            resolved_bindings_from_text(&config_text_from_bindings(&bindings), &PortableKeyNames)
                .is_ok()
        );
    }

    #[test]
    fn shortcut_config_text_serializes_secondary_bindings() {
        let mut bindings = default_bindings();
        bindings.push(ShortcutBinding {
            action: ShortcutAction::PlayPause,
            chord: portable_chord("P"),
        });

        assert_eq!(
            config_text_from_bindings(&bindings),
            "play-pause=Space\nplay-pause=P"
        );
        let resolved =
            resolved_bindings_from_text(&config_text_from_bindings(&bindings), &PortableKeyNames)
                .expect("serialized secondary shortcut should parse");
        assert_eq!(
            action_for_key(&resolved, "space", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            action_for_key(&resolved, "p", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn shortcut_config_text_returns_blank_for_defaults() {
        assert_eq!(config_text_from_bindings(&default_bindings()), "");
    }

    #[test]
    fn shortcut_capture_rejects_modifier_only_keys() {
        assert_eq!(
            chord_from_captured_key(Some("Shift_L"), shift()),
            Err("Press a non-modifier key.")
        );

        assert_eq!(
            chord_from_captured_key(Some("comma"), ctrl()).map(|chord| chord.label()),
            Ok("Ctrl+,".to_owned())
        );
    }

    #[test]
    fn shortcut_capture_rejects_nameless_keys() {
        // A key the platform cannot name has no config-text form, so it cannot be captured.
        assert_eq!(
            chord_from_captured_key(None, no_mods()),
            Err("Press a non-modifier key.")
        );
    }

    #[test]
    fn shortcut_labels_keep_letter_o_distinct_from_zero() {
        assert_eq!(portable_chord("O").label(), "O");
    }

    #[test]
    fn slot_conflict_ignores_only_the_slot_being_reassigned() {
        let rows = vec![
            ActionChords {
                action: ShortcutAction::PlayPause,
                primary: portable_chord("Space"),
                secondary: Some(portable_chord("P")),
            },
            ActionChords {
                action: ShortcutAction::Fullscreen,
                primary: portable_chord("F"),
                secondary: None,
            },
        ];

        // Re-capturing the chord a slot already holds is not a conflict.
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::PlayPause,
                ShortcutSlot::Primary,
                &portable_chord("Space"),
            ),
            None
        );
        // Another row's primary chord conflicts.
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::PlayPause,
                ShortcutSlot::Primary,
                &portable_chord("F"),
            ),
            Some(ShortcutAction::Fullscreen)
        );
        // The action's own other slot conflicts too.
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::PlayPause,
                ShortcutSlot::Primary,
                &portable_chord("P"),
            ),
            Some(ShortcutAction::PlayPause)
        );
        // A secondary chord elsewhere conflicts with a capture for a different action.
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::Fullscreen,
                ShortcutSlot::Secondary,
                &portable_chord("P"),
            ),
            Some(ShortcutAction::PlayPause)
        );
    }

    #[test]
    fn action_chords_flatten_to_bindings_in_row_order() {
        let rows = vec![
            ActionChords {
                action: ShortcutAction::PlayPause,
                primary: portable_chord("Space"),
                secondary: Some(portable_chord("P")),
            },
            ActionChords {
                action: ShortcutAction::Fullscreen,
                primary: portable_chord("F"),
                secondary: None,
            },
        ];

        assert_eq!(
            bindings_from_action_chords(&rows),
            vec![
                ShortcutBinding {
                    action: ShortcutAction::PlayPause,
                    chord: portable_chord("Space"),
                },
                ShortcutBinding {
                    action: ShortcutAction::PlayPause,
                    chord: portable_chord("P"),
                },
                ShortcutBinding {
                    action: ShortcutAction::Fullscreen,
                    chord: portable_chord("F"),
                },
            ]
        );
    }

    fn cmd() -> ShortcutModifiers {
        ShortcutModifiers {
            cmd: true,
            ..ShortcutModifiers::default()
        }
    }

    fn mac_resolved(text: &str) -> Result<Vec<ShortcutBinding>, ShortcutConfigError> {
        resolved_bindings_from_text_on(Platform::MacOs, text, &PortableKeyNames)
    }

    fn table_labels(bindings: &[ShortcutBinding]) -> Vec<(&'static str, String)> {
        bindings
            .iter()
            .map(|binding| (binding.action.id(), binding.chord.label()))
            .collect()
    }

    fn owned(table: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        table
            .iter()
            .map(|(id, label)| (*id, (*label).to_owned()))
            .collect()
    }

    #[test]
    fn cmd_chord_round_trips_through_parse_format_and_serde() {
        let chord = portable_chord("Cmd+O");
        assert!(chord.matches("o", cmd()));
        assert!(!chord.matches("o", no_mods()));
        assert!(!chord.matches("o", ctrl()));
        assert_eq!(chord.label(), "Cmd+O");
        assert_eq!(portable_chord(&chord.label()), chord);
        assert_eq!(portable_chord("command+o"), chord);

        let json = serde_json::to_string(&chord).expect("chord serializes");
        assert_eq!(json, r#""Cmd+O""#);
        assert_eq!(
            serde_json::from_str::<ShortcutChord>(&json).expect("chord parses"),
            chord
        );

        // Modifier order in the text does not matter; the label uses the macOS menu order.
        let all = portable_chord("Cmd+Shift+Alt+Ctrl+F");
        assert_eq!(all.label(), "Ctrl+Alt+Shift+Cmd+F");
        assert_eq!(portable_chord("Cmd+Shift+O").label(), "Shift+Cmd+O");
        assert_eq!(
            serde_json::from_str::<ShortcutChord>(r#""Ctrl+Alt+Shift+Cmd+F""#)
                .expect("chord parses"),
            all
        );
    }

    #[test]
    fn shortcut_modifiers_serde_carries_cmd_and_defaults_missing_fields() {
        assert_eq!(
            serde_json::to_value(cmd()).expect("modifiers serialize"),
            serde_json::json!({ "ctrl": false, "alt": false, "shift": false, "cmd": true })
        );
        assert_eq!(
            serde_json::from_str::<ShortcutModifiers>(r#"{"cmd":true}"#).expect("modifiers parse"),
            cmd()
        );
        assert_eq!(
            serde_json::from_str::<ShortcutModifiers>("{}").expect("modifiers parse"),
            no_mods()
        );
    }

    #[test]
    fn serialized_chords_with_platform_key_names_read_back_equal() {
        for (key, modifiers) in [
            ("F1", cmd()),
            ("Return", no_mods()),
            ("Page_Up", ctrl()),
            ("comma", cmd()),
            ("space", shift()),
        ] {
            let chord = chord_from_captured_key(Some(key), modifiers).expect("chord captures");
            let json = serde_json::to_string(&chord).expect("chord serializes");
            assert_eq!(
                serde_json::from_str::<ShortcutChord>(&json).expect("chord parses"),
                chord,
                "{json}"
            );
        }
        assert!(serde_json::from_str::<ShortcutChord>(r#""Cmd+""#).is_err());
    }

    #[test]
    fn linux_default_table_is_unchanged() {
        let golden = owned(&[
            ("play-pause", "Space"),
            ("seek-back", "Left"),
            ("seek-forward", "Right"),
            ("frame-forward", "."),
            ("frame-back", ","),
            ("previous-item", "PageUp"),
            ("next-item", "PageDown"),
            ("volume-down", "Down"),
            ("volume-up", "Up"),
            ("mute", "M"),
            ("open-file", "O"),
            ("add-subtitle", "S"),
            ("open-url", "U"),
            ("close-media", "X"),
            ("save-screenshot", "C"),
            ("copy-frame", "Shift+C"),
            ("media-info", "I"),
            ("go-to-time", "J"),
            ("ab-loop", "L"),
            ("subtitle-delay-forward", "Z"),
            ("subtitle-delay-back", "Shift+Z"),
            ("subtitle-size-down", "["),
            ("subtitle-size-up", "]"),
            ("subtitle-previous-cue", "Ctrl+Left"),
            ("subtitle-next-cue", "Ctrl+Right"),
            ("fullscreen", "F"),
            ("escape-fullscreen", "Escape"),
            ("open-settings", "Ctrl+,"),
        ]);

        assert_eq!(table_labels(&default_bindings()), golden);
        assert_eq!(default_bindings_on(Platform::Linux), default_bindings());
        for action in SHORTCUT_ACTIONS {
            assert_eq!(
                action.default_shortcut_on(Platform::Linux),
                action.default_shortcut()
            );
        }
    }

    #[test]
    fn macos_default_table_uses_cmd_for_menu_commands() {
        let golden = owned(&[
            ("play-pause", "Space"),
            ("seek-back", "Left"),
            ("seek-forward", "Right"),
            ("frame-forward", "."),
            ("frame-back", ","),
            ("previous-item", "PageUp"),
            ("next-item", "PageDown"),
            ("volume-down", "Down"),
            ("volume-up", "Up"),
            ("mute", "M"),
            ("open-file", "Cmd+O"),
            ("add-subtitle", "S"),
            ("open-url", "Shift+Cmd+O"),
            ("close-media", "X"),
            ("save-screenshot", "C"),
            ("copy-frame", "Shift+C"),
            ("media-info", "Cmd+I"),
            ("go-to-time", "J"),
            ("ab-loop", "L"),
            ("subtitle-delay-forward", "Z"),
            ("subtitle-delay-back", "Shift+Z"),
            ("subtitle-size-down", "["),
            ("subtitle-size-up", "]"),
            ("subtitle-previous-cue", "Cmd+Left"),
            ("subtitle-next-cue", "Cmd+Right"),
            ("fullscreen", "F"),
            ("escape-fullscreen", "Escape"),
            ("open-settings", "Cmd+,"),
        ]);

        assert_eq!(table_labels(&default_bindings_on(Platform::MacOs)), golden);
    }

    #[test]
    fn macos_default_table_has_no_conflicts() {
        let defaults = default_bindings_on(Platform::MacOs);

        assert_eq!(validate_conflicts(&defaults), Ok(()));
        assert_eq!(mac_resolved(""), Ok(defaults));
    }

    #[test]
    fn macos_defaults_move_every_linux_ctrl_chord_to_cmd() {
        for action in SHORTCUT_ACTIONS {
            let linux = default_chord_for_action(*action);
            let mac = default_chord_for_action_on(Platform::MacOs, *action);

            assert!(!mac.modifiers.ctrl, "{} keeps Ctrl on macOS", action.id());
            if linux.modifiers.ctrl {
                assert_eq!(mac.key, linux.key, "{}", action.id());
                assert_eq!(
                    mac.modifiers,
                    ShortcutModifiers {
                        ctrl: false,
                        cmd: true,
                        ..linux.modifiers
                    },
                    "{}",
                    action.id()
                );
            }
            if !mac.modifiers.cmd {
                assert_eq!(mac, linux, "{} is an in-window key", action.id());
            }
        }
    }

    #[test]
    fn macos_defaults_keep_the_in_window_keys() {
        let linux = default_bindings();
        let mac = default_bindings_on(Platform::MacOs);
        for (key, modifiers) in [
            ("space", no_mods()),
            ("Left", no_mods()),
            ("Right", no_mods()),
            ("Up", no_mods()),
            ("Down", no_mods()),
            ("f", no_mods()),
            ("m", no_mods()),
            ("comma", no_mods()),
            ("period", no_mods()),
            ("z", no_mods()),
            ("z", shift()),
            ("bracketleft", no_mods()),
            ("bracketright", no_mods()),
        ] {
            let action = action_for_key(&linux, key, modifiers);
            assert!(action.is_some(), "{key} is bound on Linux");
            assert_eq!(action_for_key(&mac, key, modifiers), action, "{key}");
        }
    }

    #[test]
    fn macos_defaults_leave_system_chords_alone() {
        let defaults = default_bindings_on(Platform::MacOs);
        for reserved in [
            "Cmd+Q",
            "Cmd+H",
            "Alt+Cmd+H",
            "Cmd+M",
            "Cmd+W",
            "Cmd+Tab",
            "Cmd+grave",
            "Cmd+Space",
            "Ctrl+Space",
            "Ctrl+Cmd+F",
            "Ctrl+Cmd+Q",
            "Shift+Cmd+3",
            "Shift+Cmd+4",
            "Shift+Cmd+5",
            "Ctrl+Left",
            "Ctrl+Right",
            "Ctrl+Up",
            "Ctrl+Down",
            "Cmd+A",
            "Cmd+C",
            "Cmd+V",
            "Cmd+X",
            "Cmd+Z",
            "Shift+Cmd+Z",
        ] {
            let chord =
                parse_chord(reserved, 0, &SerializedKeyNames).expect("reserved chord parses");
            assert!(
                !defaults.iter().any(|binding| binding.chord == chord),
                "a macOS default takes the system chord {reserved}"
            );
        }
    }

    #[test]
    fn macos_dispatch_tells_cmd_chords_from_plain_keys() {
        let bindings = default_bindings_on(Platform::MacOs);

        assert_eq!(
            action_for_key(&bindings, "comma", cmd()),
            Some(ShortcutAction::OpenSettings)
        );
        assert_eq!(
            action_for_key(&bindings, "comma", no_mods()),
            Some(ShortcutAction::FrameBack)
        );
        assert_eq!(
            action_for_key(&bindings, "O", cmd()),
            Some(ShortcutAction::OpenFile)
        );
        assert_eq!(
            action_for_key(
                &bindings,
                "o",
                ShortcutModifiers {
                    shift: true,
                    cmd: true,
                    ..ShortcutModifiers::default()
                }
            ),
            Some(ShortcutAction::OpenUrl)
        );
        assert_eq!(action_for_key(&bindings, "o", ctrl()), None);
        assert_eq!(action_for_key(&bindings, "o", no_mods()), None);
        assert_eq!(
            chords_for_action_on(Platform::MacOs, &[], ShortcutAction::OpenFile),
            vec![portable_chord("Cmd+O")]
        );
    }

    #[test]
    fn conflict_checks_include_cmd() {
        // Cmd+O and plain O are different chords...
        let bindings = mac_resolved("play-pause=O").expect("plain O is free on macOS");
        assert_eq!(
            action_for_key(&bindings, "o", no_mods()),
            Some(ShortcutAction::PlayPause)
        );
        assert_eq!(
            action_for_key(&bindings, "o", cmd()),
            Some(ShortcutAction::OpenFile)
        );

        // ...but two actions on one Cmd chord conflict.
        let error = mac_resolved("play-pause=Cmd+O").expect_err("Cmd+O is taken");
        assert_eq!(error.line, 0);
        assert_eq!(
            error.message,
            "play-pause conflicts with open-file on Cmd+O."
        );

        let rows = vec![ActionChords {
            action: ShortcutAction::OpenFile,
            primary: portable_chord("Cmd+O"),
            secondary: None,
        }];
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::PlayPause,
                ShortcutSlot::Primary,
                &portable_chord("Cmd+O"),
            ),
            Some(ShortcutAction::OpenFile)
        );
        assert_eq!(
            slot_conflict(
                &rows,
                ShortcutAction::PlayPause,
                ShortcutSlot::Primary,
                &portable_chord("O"),
            ),
            None
        );
    }

    #[test]
    fn linux_config_rejects_cmd_chords() {
        let error = resolved_bindings_from_text("# custom\nopen-file=Cmd+O", &PortableKeyNames)
            .expect_err("Cmd cannot fire on Linux");

        assert_eq!(error.line, 2);
        assert!(error.message.contains("macOS"), "{}", error.message);
    }

    #[test]
    fn macos_config_text_serializes_against_the_macos_defaults() {
        let defaults = default_bindings_on(Platform::MacOs);
        assert_eq!(config_text_from_bindings_on(Platform::MacOs, &defaults), "");

        let custom = mac_resolved("open-file=O\nopen-url=Cmd+L").expect("custom chords resolve");
        let text = config_text_from_bindings_on(Platform::MacOs, &custom);
        assert_eq!(text, "open-file=O\nopen-url=Cmd+L");
        assert_eq!(mac_resolved(&text), Ok(custom));

        // The Linux table differs from the macOS one exactly where the platforms differ.
        assert_eq!(
            config_text_from_bindings_on(Platform::MacOs, &default_bindings()),
            "open-file=O\nopen-url=U\nmedia-info=I\nsubtitle-previous-cue=Ctrl+Left\n\
             subtitle-next-cue=Ctrl+Right\nopen-settings=Ctrl+,"
        );
    }
}
