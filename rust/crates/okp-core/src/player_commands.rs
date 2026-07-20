//! Canonical player command registry shared by every command surface.
//!
//! Native shells render these resolved rows and map [`PlayerCommandId`] to
//! platform actions. Identity, grouping, order, labels, keywords, shortcut
//! association, enabled/checked state, surface policy, and filtering stay in
//! the portable core so an overflow popover and a context menu cannot drift.

use crate::playlist::RepeatMode;
use crate::shortcuts::ShortcutAction;
use crate::video_geometry::{VideoAspect, VideoGeometry, VideoGeometryAction};
use crate::window_fit::WindowSize;

pub const PLAYER_COMMAND_SURFACE_PREFERRED_WIDTH: i32 = 340;
pub const PLAYER_COMMAND_SURFACE_MAX_HEIGHT: i32 = 560;
pub const PLAYER_COMMAND_SURFACE_WORKAREA_INSET: i32 = 32;

/// Bound the searchable command surface inside the active work area. The
/// native shell still owns anchoring and popup placement; the portable policy
/// guarantees that neither surface asks the compositor for an oversized box.
pub fn player_command_surface_size(work_width: i32, work_height: i32) -> WindowSize {
    let available_width = work_width
        .saturating_sub(PLAYER_COMMAND_SURFACE_WORKAREA_INSET)
        .max(1);
    let available_height = work_height
        .saturating_sub(PLAYER_COMMAND_SURFACE_WORKAREA_INSET)
        .max(80);
    WindowSize {
        width: available_width.min(PLAYER_COMMAND_SURFACE_PREFERRED_WIDTH),
        height: available_height.min(PLAYER_COMMAND_SURFACE_MAX_HEIGHT),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCommandSurface {
    More,
    ContextMenu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCommandSurfacePolicy {
    Both,
}

impl PlayerCommandSurfacePolicy {
    const fn includes(self, _surface: PlayerCommandSurface) -> bool {
        match self {
            Self::Both => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlayerCommandGroup {
    PlaybackNavigation,
    TracksSubtitles,
    ViewWindow,
    MediaFile,
    Tools,
    Settings,
}

impl PlayerCommandGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::PlaybackNavigation => "Playback & navigation",
            Self::TracksSubtitles => "Tracks & subtitles",
            Self::ViewWindow => "View & window",
            Self::MediaFile => "Media & file",
            Self::Tools => "Tools",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayerCommandId {
    PlayPause,
    GoToTime,
    CopyCurrentTime,
    AddBookmark,
    AbLoop,
    RepeatMode,
    Shuffle,
    AutoAdvance,
    PlaybackSpeed,
    Subtitles,
    AudioTrack,
    ChaptersUpNext,
    FitWindowToMedia,
    MiniPlayer,
    Fullscreen,
    AspectAuto,
    AspectWide,
    AspectStandard,
    AspectCinema,
    ZoomIn,
    ZoomOut,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    CenterImage,
    RotateClockwise,
    FillScreen,
    Deinterlace,
    ResetVideo,
    OpenFile,
    OpenUrl,
    OpenFolder,
    OpenPlaylist,
    AddToQueue,
    PlayNext,
    SavePlaylist,
    CloseMedia,
    MediaInfo,
    OpenFileLocation,
    SaveFrame,
    SaveFrameWithSubtitles,
    CopyFrame,
    ExportClip,
    PrivateSession,
    ClearHistory,
    OpenSettings,
}

impl PlayerCommandId {
    pub const fn id(self) -> &'static str {
        match self {
            Self::PlayPause => "play-pause",
            Self::GoToTime => "go-to-time",
            Self::CopyCurrentTime => "copy-current-time",
            Self::AddBookmark => "add-bookmark",
            Self::AbLoop => "ab-loop",
            Self::RepeatMode => "repeat-mode",
            Self::Shuffle => "shuffle",
            Self::AutoAdvance => "auto-advance",
            Self::PlaybackSpeed => "playback-speed",
            Self::Subtitles => "subtitles",
            Self::AudioTrack => "audio-track",
            Self::ChaptersUpNext => "chapters-up-next",
            Self::FitWindowToMedia => "fit-window-to-media",
            Self::MiniPlayer => "mini-player",
            Self::Fullscreen => "fullscreen",
            Self::AspectAuto => "aspect-auto",
            Self::AspectWide => "aspect-wide",
            Self::AspectStandard => "aspect-standard",
            Self::AspectCinema => "aspect-cinema",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::PanLeft => "pan-left",
            Self::PanRight => "pan-right",
            Self::PanUp => "pan-up",
            Self::PanDown => "pan-down",
            Self::CenterImage => "center-image",
            Self::RotateClockwise => "rotate-clockwise",
            Self::FillScreen => "fill-screen",
            Self::Deinterlace => "deinterlace",
            Self::ResetVideo => "reset-video",
            Self::OpenFile => "open-file",
            Self::OpenUrl => "open-url",
            Self::OpenFolder => "open-folder",
            Self::OpenPlaylist => "open-playlist",
            Self::AddToQueue => "add-to-queue",
            Self::PlayNext => "play-next",
            Self::SavePlaylist => "save-playlist",
            Self::CloseMedia => "close-media",
            Self::MediaInfo => "media-info",
            Self::OpenFileLocation => "open-file-location",
            Self::SaveFrame => "save-frame",
            Self::SaveFrameWithSubtitles => "save-frame-with-subtitles",
            Self::CopyFrame => "copy-frame",
            Self::ExportClip => "export-clip",
            Self::PrivateSession => "private-session",
            Self::ClearHistory => "clear-history",
            Self::OpenSettings => "open-settings",
        }
    }
}

/// The four curated second-level pages shared by the overflow and context
/// menus. The full searchable registry remains available independently of
/// this hierarchy, so rare commands stay reachable without taxing the default
/// menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCommandMenuPage {
    Video,
    Playback,
    Window,
    ToolsAdvanced,
}

impl PlayerCommandMenuPage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Playback => "Playback",
            Self::Window => "Window",
            Self::ToolsAdvanced => "Tools / Advanced",
        }
    }

    pub const fn commands(self) -> &'static [PlayerCommandId] {
        match self {
            Self::Video => VIDEO_MENU_COMMANDS,
            Self::Playback => PLAYBACK_MENU_COMMANDS,
            Self::Window => WINDOW_MENU_COMMANDS,
            Self::ToolsAdvanced => TOOLS_ADVANCED_MENU_COMMANDS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCommandMenuEntry {
    Command(PlayerCommandId),
    Submenu(PlayerCommandMenuPage),
    Separator,
}

/// The complete first level, in visual order. Close Media and Settings remain
/// bottom-anchored after the four long-tail pages while every core playback
/// action stays visible without scrolling at the default window size.
pub const PLAYER_COMMAND_MENU_TOP_LEVEL: &[PlayerCommandMenuEntry] = &[
    PlayerCommandMenuEntry::Command(PlayerCommandId::PlayPause),
    PlayerCommandMenuEntry::Command(PlayerCommandId::Subtitles),
    PlayerCommandMenuEntry::Command(PlayerCommandId::AudioTrack),
    PlayerCommandMenuEntry::Command(PlayerCommandId::ChaptersUpNext),
    PlayerCommandMenuEntry::Command(PlayerCommandId::SaveFrame),
    PlayerCommandMenuEntry::Command(PlayerCommandId::Fullscreen),
    PlayerCommandMenuEntry::Separator,
    PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Video),
    PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Playback),
    PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Window),
    PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::ToolsAdvanced),
    PlayerCommandMenuEntry::Separator,
    PlayerCommandMenuEntry::Command(PlayerCommandId::CloseMedia),
    PlayerCommandMenuEntry::Command(PlayerCommandId::OpenSettings),
];

/// Stable first-level labels. Search results and second-level pages retain the
/// richer resolved labels (for example, Enter/Exit fullscreen), while the
/// default surface uses the concise product vocabulary from the PRD.
pub const fn player_command_menu_top_level_label(id: PlayerCommandId) -> Option<&'static str> {
    match id {
        PlayerCommandId::PlayPause => Some("Play / Pause"),
        PlayerCommandId::Subtitles => Some("Subtitles"),
        PlayerCommandId::AudioTrack => Some("Audio"),
        PlayerCommandId::ChaptersUpNext => Some("Chapters"),
        PlayerCommandId::SaveFrame => Some("Screenshot"),
        PlayerCommandId::Fullscreen => Some("Fullscreen"),
        PlayerCommandId::CloseMedia => Some("Close Media"),
        PlayerCommandId::OpenSettings => Some("Settings..."),
        _ => None,
    }
}

const VIDEO_MENU_COMMANDS: &[PlayerCommandId] = &[
    PlayerCommandId::AspectAuto,
    PlayerCommandId::AspectWide,
    PlayerCommandId::AspectStandard,
    PlayerCommandId::AspectCinema,
    PlayerCommandId::ZoomIn,
    PlayerCommandId::ZoomOut,
    PlayerCommandId::PanLeft,
    PlayerCommandId::PanRight,
    PlayerCommandId::PanUp,
    PlayerCommandId::PanDown,
    PlayerCommandId::CenterImage,
    PlayerCommandId::RotateClockwise,
    PlayerCommandId::FillScreen,
    PlayerCommandId::Deinterlace,
    PlayerCommandId::ResetVideo,
];

const PLAYBACK_MENU_COMMANDS: &[PlayerCommandId] = &[
    PlayerCommandId::GoToTime,
    PlayerCommandId::CopyCurrentTime,
    PlayerCommandId::AddBookmark,
    PlayerCommandId::AbLoop,
    PlayerCommandId::RepeatMode,
    PlayerCommandId::Shuffle,
    PlayerCommandId::AutoAdvance,
    PlayerCommandId::PlaybackSpeed,
];

const WINDOW_MENU_COMMANDS: &[PlayerCommandId] = &[
    PlayerCommandId::FitWindowToMedia,
    PlayerCommandId::MiniPlayer,
];

const TOOLS_ADVANCED_MENU_COMMANDS: &[PlayerCommandId] = &[
    PlayerCommandId::OpenFile,
    PlayerCommandId::OpenUrl,
    PlayerCommandId::OpenFolder,
    PlayerCommandId::OpenPlaylist,
    PlayerCommandId::AddToQueue,
    PlayerCommandId::PlayNext,
    PlayerCommandId::SavePlaylist,
    PlayerCommandId::MediaInfo,
    PlayerCommandId::OpenFileLocation,
    PlayerCommandId::SaveFrameWithSubtitles,
    PlayerCommandId::CopyFrame,
    PlayerCommandId::ExportClip,
    PlayerCommandId::PrivateSession,
    PlayerCommandId::ClearHistory,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerCommandSpec {
    pub id: PlayerCommandId,
    pub group: PlayerCommandGroup,
    pub label: &'static str,
    pub keywords: &'static [&'static str],
    pub shortcut: Option<ShortcutAction>,
    pub surfaces: PlayerCommandSurfacePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerCommandContext {
    pub has_media: bool,
    pub has_local_media: bool,
    pub has_video_geometry: bool,
    pub playlist_count: usize,
    pub repeat_mode: RepeatMode,
    pub shuffle_enabled: bool,
    pub auto_advance_enabled: bool,
    pub private_session: bool,
    pub ab_loop_active: bool,
    pub compact_mode: bool,
    pub fullscreen: bool,
    pub video_geometry: VideoGeometry,
}

impl Default for PlayerCommandContext {
    fn default() -> Self {
        Self {
            has_media: false,
            has_local_media: false,
            has_video_geometry: false,
            playlist_count: 0,
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            auto_advance_enabled: true,
            private_session: false,
            ab_loop_active: false,
            compact_mode: false,
            fullscreen: false,
            video_geometry: VideoGeometry::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPlayerCommand {
    pub id: PlayerCommandId,
    pub group: PlayerCommandGroup,
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    pub shortcut: Option<String>,
}

const BOTH: PlayerCommandSurfacePolicy = PlayerCommandSurfacePolicy::Both;

const COMMANDS: &[PlayerCommandSpec] = &[
    command(
        PlayerCommandId::PlayPause,
        PlayerCommandGroup::PlaybackNavigation,
        "Play / Pause",
        &["play", "pause", "resume", "transport"],
        Some(ShortcutAction::PlayPause),
    ),
    command(
        PlayerCommandId::GoToTime,
        PlayerCommandGroup::PlaybackNavigation,
        "Go to time...",
        &["seek", "jump", "timecode"],
        Some(ShortcutAction::GoToTime),
    ),
    command(
        PlayerCommandId::CopyCurrentTime,
        PlayerCommandGroup::PlaybackNavigation,
        "Copy current time",
        &["clipboard", "timecode", "timestamp"],
        None,
    ),
    command(
        PlayerCommandId::AddBookmark,
        PlayerCommandGroup::PlaybackNavigation,
        "Add bookmark",
        &["chapter", "marker", "save position"],
        None,
    ),
    command(
        PlayerCommandId::AbLoop,
        PlayerCommandGroup::PlaybackNavigation,
        "A-B loop",
        &["repeat selection", "loop range"],
        Some(ShortcutAction::AbLoop),
    ),
    command(
        PlayerCommandId::RepeatMode,
        PlayerCommandGroup::PlaybackNavigation,
        "Repeat",
        &["loop", "repeat one", "repeat all"],
        None,
    ),
    command(
        PlayerCommandId::Shuffle,
        PlayerCommandGroup::PlaybackNavigation,
        "Shuffle",
        &["random", "playlist order"],
        None,
    ),
    command(
        PlayerCommandId::AutoAdvance,
        PlayerCommandGroup::PlaybackNavigation,
        "Auto-advance",
        &["next item", "playlist continue"],
        None,
    ),
    command(
        PlayerCommandId::PlaybackSpeed,
        PlayerCommandGroup::TracksSubtitles,
        "Playback speed",
        &["rate", "tempo", "faster", "slower"],
        None,
    ),
    command(
        PlayerCommandId::Subtitles,
        PlayerCommandGroup::TracksSubtitles,
        "Subtitles",
        &["captions", "subtitle track", "delay", "style"],
        None,
    ),
    command(
        PlayerCommandId::AudioTrack,
        PlayerCommandGroup::TracksSubtitles,
        "Audio track",
        &["language", "soundtrack", "audio stream"],
        None,
    ),
    command(
        PlayerCommandId::ChaptersUpNext,
        PlayerCommandGroup::TracksSubtitles,
        "Chapters & Up Next",
        &["playlist", "queue", "bookmarks", "navigation"],
        None,
    ),
    command(
        PlayerCommandId::FitWindowToMedia,
        PlayerCommandGroup::ViewWindow,
        "Fit window to media",
        &[
            "resize",
            "native size",
            "video dimensions",
            "aspect",
            "window",
        ],
        None,
    ),
    command(
        PlayerCommandId::MiniPlayer,
        PlayerCommandGroup::ViewWindow,
        "Mini player",
        &["picture in picture", "pip", "compact", "window"],
        None,
    ),
    command(
        PlayerCommandId::Fullscreen,
        PlayerCommandGroup::ViewWindow,
        "Fullscreen",
        &["full screen", "window", "display"],
        Some(ShortcutAction::Fullscreen),
    ),
    command(
        PlayerCommandId::AspectAuto,
        PlayerCommandGroup::ViewWindow,
        "Aspect ratio: Auto",
        &["video geometry", "original aspect"],
        None,
    ),
    command(
        PlayerCommandId::AspectWide,
        PlayerCommandGroup::ViewWindow,
        "Aspect ratio: 16:9",
        &["video geometry", "widescreen"],
        None,
    ),
    command(
        PlayerCommandId::AspectStandard,
        PlayerCommandGroup::ViewWindow,
        "Aspect ratio: 4:3",
        &["video geometry", "standard"],
        None,
    ),
    command(
        PlayerCommandId::AspectCinema,
        PlayerCommandGroup::ViewWindow,
        "Aspect ratio: 2.35:1",
        &["video geometry", "cinema", "scope"],
        None,
    ),
    command(
        PlayerCommandId::ZoomIn,
        PlayerCommandGroup::ViewWindow,
        "Zoom in",
        &["video geometry", "magnify"],
        None,
    ),
    command(
        PlayerCommandId::ZoomOut,
        PlayerCommandGroup::ViewWindow,
        "Zoom out",
        &["video geometry", "shrink"],
        None,
    ),
    command(
        PlayerCommandId::PanLeft,
        PlayerCommandGroup::ViewWindow,
        "Pan left",
        &["video geometry", "move image"],
        None,
    ),
    command(
        PlayerCommandId::PanRight,
        PlayerCommandGroup::ViewWindow,
        "Pan right",
        &["video geometry", "move image"],
        None,
    ),
    command(
        PlayerCommandId::PanUp,
        PlayerCommandGroup::ViewWindow,
        "Pan up",
        &["video geometry", "move image"],
        None,
    ),
    command(
        PlayerCommandId::PanDown,
        PlayerCommandGroup::ViewWindow,
        "Pan down",
        &["video geometry", "move image"],
        None,
    ),
    command(
        PlayerCommandId::CenterImage,
        PlayerCommandGroup::ViewWindow,
        "Center image",
        &["video geometry", "reset pan"],
        None,
    ),
    command(
        PlayerCommandId::RotateClockwise,
        PlayerCommandGroup::ViewWindow,
        "Rotate 90°",
        &["video geometry", "clockwise", "orientation"],
        None,
    ),
    command(
        PlayerCommandId::FillScreen,
        PlayerCommandGroup::ViewWindow,
        "Fill screen (crop bars)",
        &["video geometry", "crop", "letterbox"],
        None,
    ),
    command(
        PlayerCommandId::Deinterlace,
        PlayerCommandGroup::ViewWindow,
        "Deinterlace",
        &["video geometry", "interlaced"],
        None,
    ),
    command(
        PlayerCommandId::ResetVideo,
        PlayerCommandGroup::ViewWindow,
        "Reset video geometry",
        &["video geometry", "default", "restore"],
        None,
    ),
    command(
        PlayerCommandId::OpenFile,
        PlayerCommandGroup::MediaFile,
        "Open file...",
        &["browse", "media", "video", "audio"],
        Some(ShortcutAction::OpenFile),
    ),
    command(
        PlayerCommandId::OpenUrl,
        PlayerCommandGroup::MediaFile,
        "Open URL...",
        &["link", "stream", "network"],
        Some(ShortcutAction::OpenUrl),
    ),
    command(
        PlayerCommandId::OpenFolder,
        PlayerCommandGroup::MediaFile,
        "Open folder...",
        &["directory", "playlist", "media files"],
        None,
    ),
    command(
        PlayerCommandId::OpenPlaylist,
        PlayerCommandGroup::MediaFile,
        "Open playlist...",
        &["m3u", "queue"],
        None,
    ),
    command(
        PlayerCommandId::AddToQueue,
        PlayerCommandGroup::MediaFile,
        "Add to queue...",
        &["playlist", "append", "up next"],
        None,
    ),
    command(
        PlayerCommandId::PlayNext,
        PlayerCommandGroup::MediaFile,
        "Play next...",
        &["playlist", "queue", "insert"],
        None,
    ),
    command(
        PlayerCommandId::SavePlaylist,
        PlayerCommandGroup::MediaFile,
        "Save playlist...",
        &["m3u", "queue", "export"],
        None,
    ),
    command(
        PlayerCommandId::CloseMedia,
        PlayerCommandGroup::MediaFile,
        "Close media",
        &["unload", "stop", "close file"],
        Some(ShortcutAction::CloseMedia),
    ),
    command(
        PlayerCommandId::MediaInfo,
        PlayerCommandGroup::MediaFile,
        "Media info...",
        &["details", "codec", "streams", "metadata"],
        Some(ShortcutAction::MediaInfo),
    ),
    command(
        PlayerCommandId::OpenFileLocation,
        PlayerCommandGroup::MediaFile,
        "Open file location",
        &["folder", "file manager", "reveal"],
        None,
    ),
    command(
        PlayerCommandId::SaveFrame,
        PlayerCommandGroup::Tools,
        "Screenshot",
        &["screenshot", "capture", "image"],
        Some(ShortcutAction::SaveScreenshot),
    ),
    command(
        PlayerCommandId::SaveFrameWithSubtitles,
        PlayerCommandGroup::Tools,
        "Save frame with subtitles",
        &["screenshot", "capture", "captions"],
        None,
    ),
    command(
        PlayerCommandId::CopyFrame,
        PlayerCommandGroup::Tools,
        "Copy frame to clipboard",
        &["screenshot", "capture", "image"],
        Some(ShortcutAction::CopyFrame),
    ),
    command(
        PlayerCommandId::ExportClip,
        PlayerCommandGroup::Tools,
        "Export clip/GIF...",
        &["encode", "selection", "video clip", "animated gif"],
        None,
    ),
    command(
        PlayerCommandId::PrivateSession,
        PlayerCommandGroup::Tools,
        "Private session",
        &["privacy", "history", "incognito"],
        None,
    ),
    command(
        PlayerCommandId::ClearHistory,
        PlayerCommandGroup::Tools,
        "Clear history...",
        &["privacy", "recents", "resume"],
        None,
    ),
    command(
        PlayerCommandId::OpenSettings,
        PlayerCommandGroup::Settings,
        "Settings...",
        &["preferences", "configuration", "options"],
        Some(ShortcutAction::OpenSettings),
    ),
];

const fn command(
    id: PlayerCommandId,
    group: PlayerCommandGroup,
    label: &'static str,
    keywords: &'static [&'static str],
    shortcut: Option<ShortcutAction>,
) -> PlayerCommandSpec {
    PlayerCommandSpec {
        id,
        group,
        label,
        keywords,
        shortcut,
        surfaces: BOTH,
    }
}

pub fn player_command_registry() -> &'static [PlayerCommandSpec] {
    COMMANDS
}

pub fn resolve_player_commands(
    surface: PlayerCommandSurface,
    context: PlayerCommandContext,
    mut shortcut_label: impl FnMut(ShortcutAction) -> Option<String>,
) -> Vec<ResolvedPlayerCommand> {
    COMMANDS
        .iter()
        .filter(|spec| spec.surfaces.includes(surface))
        .map(|spec| resolve_command(*spec, context, &mut shortcut_label))
        .collect()
}

pub fn filter_player_commands(
    commands: &[ResolvedPlayerCommand],
    query: &str,
) -> Vec<ResolvedPlayerCommand> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return commands.to_vec();
    }

    commands
        .iter()
        .filter(|command| {
            let spec = COMMANDS
                .iter()
                .find(|spec| spec.id == command.id)
                .expect("resolved command must come from the registry");
            command.label.to_lowercase().contains(&query)
                || spec
                    .keywords
                    .iter()
                    .any(|keyword| keyword.to_lowercase().contains(&query))
        })
        .cloned()
        .collect()
}

pub fn dispatch_player_command(
    surface: PlayerCommandSurface,
    id: PlayerCommandId,
    mut handler: impl FnMut(PlayerCommandId),
) -> bool {
    let available = COMMANDS
        .iter()
        .any(|spec| spec.id == id && spec.surfaces.includes(surface));
    if available {
        handler(id);
    }
    available
}

fn resolve_command(
    spec: PlayerCommandSpec,
    context: PlayerCommandContext,
    shortcut_label: &mut impl FnMut(ShortcutAction) -> Option<String>,
) -> ResolvedPlayerCommand {
    use PlayerCommandId as Id;

    let geometry = geometry_action(spec.id);
    let enabled = match spec.id {
        Id::OpenFile
        | Id::OpenUrl
        | Id::OpenFolder
        | Id::OpenPlaylist
        | Id::ClearHistory
        | Id::OpenSettings
        | Id::PrivateSession => true,
        Id::AddToQueue | Id::PlayNext | Id::OpenFileLocation | Id::AddBookmark => {
            context.has_local_media
        }
        Id::SavePlaylist => context.playlist_count > 0,
        Id::FitWindowToMedia => context.has_video_geometry,
        Id::ExportClip => false,
        _ if geometry.is_some() => geometry.is_some_and(|action| {
            context
                .video_geometry
                .action_enabled(context.has_video_geometry, action)
        }),
        _ => context.has_media,
    };

    let checked = match spec.id {
        Id::AbLoop => context.ab_loop_active,
        Id::RepeatMode => context.repeat_mode != RepeatMode::Off,
        Id::Shuffle => context.shuffle_enabled,
        Id::AutoAdvance => context.auto_advance_enabled,
        Id::MiniPlayer => context.compact_mode,
        Id::Fullscreen => context.fullscreen,
        Id::AspectAuto => context.video_geometry.aspect == VideoAspect::Auto,
        Id::AspectWide => context.video_geometry.aspect == VideoAspect::Wide,
        Id::AspectStandard => context.video_geometry.aspect == VideoAspect::Standard,
        Id::AspectCinema => context.video_geometry.aspect == VideoAspect::Cinema,
        Id::FillScreen => context.video_geometry.fill_screen,
        Id::Deinterlace => context.video_geometry.deinterlace,
        Id::PrivateSession => context.private_session,
        _ => false,
    };

    let label = match spec.id {
        Id::RepeatMode => match context.repeat_mode {
            RepeatMode::Off => "Repeat: Off",
            RepeatMode::One => "Repeat: One",
            RepeatMode::All => "Repeat: All",
        },
        Id::Shuffle if context.shuffle_enabled => "Shuffle: On",
        Id::Shuffle => "Shuffle: Off",
        Id::AutoAdvance if context.auto_advance_enabled => "Auto-advance: On",
        Id::AutoAdvance => "Auto-advance: Off",
        Id::Fullscreen if context.fullscreen => "Exit fullscreen",
        Id::Fullscreen => "Enter fullscreen",
        Id::PrivateSession if context.private_session => "Private session: On",
        Id::PrivateSession => "Private session: Off",
        _ => spec.label,
    };

    ResolvedPlayerCommand {
        id: spec.id,
        group: spec.group,
        label: label.to_owned(),
        enabled,
        checked,
        shortcut: spec.shortcut.and_then(shortcut_label),
    }
}

pub const fn geometry_action(id: PlayerCommandId) -> Option<VideoGeometryAction> {
    use PlayerCommandId as Id;
    match id {
        Id::AspectAuto => Some(VideoGeometryAction::SetAspect(VideoAspect::Auto)),
        Id::AspectWide => Some(VideoGeometryAction::SetAspect(VideoAspect::Wide)),
        Id::AspectStandard => Some(VideoGeometryAction::SetAspect(VideoAspect::Standard)),
        Id::AspectCinema => Some(VideoGeometryAction::SetAspect(VideoAspect::Cinema)),
        Id::ZoomIn => Some(VideoGeometryAction::ZoomIn),
        Id::ZoomOut => Some(VideoGeometryAction::ZoomOut),
        Id::PanLeft => Some(VideoGeometryAction::PanLeft),
        Id::PanRight => Some(VideoGeometryAction::PanRight),
        Id::PanUp => Some(VideoGeometryAction::PanUp),
        Id::PanDown => Some(VideoGeometryAction::PanDown),
        Id::CenterImage => Some(VideoGeometryAction::Center),
        Id::RotateClockwise => Some(VideoGeometryAction::RotateClockwise),
        Id::FillScreen => Some(VideoGeometryAction::ToggleFillScreen),
        Id::Deinterlace => Some(VideoGeometryAction::ToggleDeinterlace),
        Id::ResetVideo => Some(VideoGeometryAction::Reset),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn curated_menu_has_exact_core_actions_and_four_second_level_pages() {
        assert_eq!(
            PLAYER_COMMAND_MENU_TOP_LEVEL,
            &[
                PlayerCommandMenuEntry::Command(PlayerCommandId::PlayPause),
                PlayerCommandMenuEntry::Command(PlayerCommandId::Subtitles),
                PlayerCommandMenuEntry::Command(PlayerCommandId::AudioTrack),
                PlayerCommandMenuEntry::Command(PlayerCommandId::ChaptersUpNext),
                PlayerCommandMenuEntry::Command(PlayerCommandId::SaveFrame),
                PlayerCommandMenuEntry::Command(PlayerCommandId::Fullscreen),
                PlayerCommandMenuEntry::Separator,
                PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Video),
                PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Playback),
                PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::Window),
                PlayerCommandMenuEntry::Submenu(PlayerCommandMenuPage::ToolsAdvanced),
                PlayerCommandMenuEntry::Separator,
                PlayerCommandMenuEntry::Command(PlayerCommandId::CloseMedia),
                PlayerCommandMenuEntry::Command(PlayerCommandId::OpenSettings),
            ]
        );
        assert_eq!(
            [
                PlayerCommandId::PlayPause,
                PlayerCommandId::Subtitles,
                PlayerCommandId::AudioTrack,
                PlayerCommandId::ChaptersUpNext,
                PlayerCommandId::SaveFrame,
                PlayerCommandId::Fullscreen,
                PlayerCommandId::CloseMedia,
                PlayerCommandId::OpenSettings,
            ]
            .map(|id| player_command_menu_top_level_label(id).unwrap()),
            [
                "Play / Pause",
                "Subtitles",
                "Audio",
                "Chapters",
                "Screenshot",
                "Fullscreen",
                "Close Media",
                "Settings...",
            ]
        );
    }

    #[test]
    fn curated_hierarchy_accounts_for_every_registry_command_once() {
        let mut menu_commands = PLAYER_COMMAND_MENU_TOP_LEVEL
            .iter()
            .filter_map(|entry| match entry {
                PlayerCommandMenuEntry::Command(id) => Some(*id),
                PlayerCommandMenuEntry::Submenu(_) | PlayerCommandMenuEntry::Separator => None,
            })
            .collect::<Vec<_>>();
        for page in [
            PlayerCommandMenuPage::Video,
            PlayerCommandMenuPage::Playback,
            PlayerCommandMenuPage::Window,
            PlayerCommandMenuPage::ToolsAdvanced,
        ] {
            menu_commands.extend_from_slice(page.commands());
        }

        let unique = menu_commands.iter().copied().collect::<HashSet<_>>();
        let registry = player_command_registry()
            .iter()
            .map(|command| command.id)
            .collect::<HashSet<_>>();
        assert_eq!(menu_commands.len(), unique.len());
        assert_eq!(unique, registry);
    }

    #[test]
    fn more_and_context_resolve_to_exact_registry_parity() {
        let context = PlayerCommandContext {
            has_media: true,
            has_local_media: true,
            has_video_geometry: true,
            playlist_count: 3,
            repeat_mode: RepeatMode::All,
            shuffle_enabled: true,
            auto_advance_enabled: false,
            private_session: true,
            ab_loop_active: true,
            compact_mode: true,
            fullscreen: false,
            video_geometry: VideoGeometry {
                zoom: 1.25,
                fill_screen: true,
                ..VideoGeometry::default()
            },
        };
        let resolve = |surface| {
            resolve_player_commands(surface, context, |action| {
                Some(action.default_shortcut().to_owned())
            })
        };
        assert_eq!(
            resolve(PlayerCommandSurface::More),
            resolve(PlayerCommandSurface::ContextMenu)
        );
    }

    #[test]
    fn search_is_case_insensitive_across_labels_and_curated_keywords() {
        let commands = resolve_player_commands(
            PlayerCommandSurface::More,
            PlayerCommandContext::default(),
            |_| None,
        );
        assert_eq!(
            filter_player_commands(&commands, "WINDOW")
                .iter()
                .map(|command| command.id)
                .collect::<Vec<_>>(),
            vec![
                PlayerCommandId::FitWindowToMedia,
                PlayerCommandId::MiniPlayer,
                PlayerCommandId::Fullscreen
            ]
        );
        assert_eq!(
            filter_player_commands(&commands, "native size")
                .iter()
                .map(|command| command.id)
                .collect::<Vec<_>>(),
            vec![PlayerCommandId::FitWindowToMedia]
        );
    }

    #[test]
    fn filtering_keeps_registry_order_and_group_context() {
        let commands = resolve_player_commands(
            PlayerCommandSurface::ContextMenu,
            PlayerCommandContext::default(),
            |_| None,
        );
        let filtered = filter_player_commands(&commands, "playlist");
        assert!(filtered.windows(2).all(|pair| {
            let left = commands
                .iter()
                .position(|command| command.id == pair[0].id)
                .unwrap();
            let right = commands
                .iter()
                .position(|command| command.id == pair[1].id)
                .unwrap();
            left < right
        }));
        assert!(
            filtered
                .iter()
                .any(|command| command.group == PlayerCommandGroup::PlaybackNavigation)
        );
        assert!(
            filtered
                .iter()
                .any(|command| command.group == PlayerCommandGroup::MediaFile)
        );
    }

    #[test]
    fn media_geometry_controls_fit_eligibility_without_hiding_the_command() {
        let without_geometry = resolve_player_commands(
            PlayerCommandSurface::More,
            PlayerCommandContext::default(),
            |_| None,
        );
        let fit = without_geometry
            .iter()
            .find(|command| command.id == PlayerCommandId::FitWindowToMedia)
            .unwrap();
        assert!(!fit.enabled);

        let with_geometry = resolve_player_commands(
            PlayerCommandSurface::More,
            PlayerCommandContext {
                has_media: true,
                has_video_geometry: true,
                ..PlayerCommandContext::default()
            },
            |_| None,
        );
        assert!(
            with_geometry
                .iter()
                .find(|command| command.id == PlayerCommandId::FitWindowToMedia)
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn both_surfaces_dispatch_the_same_fit_handler() {
        for surface in [
            PlayerCommandSurface::More,
            PlayerCommandSurface::ContextMenu,
        ] {
            let mut handled = Vec::new();
            assert!(dispatch_player_command(
                surface,
                PlayerCommandId::FitWindowToMedia,
                |id| handled.push(id)
            ));
            assert_eq!(handled, vec![PlayerCommandId::FitWindowToMedia]);
        }
    }

    #[test]
    fn command_surface_is_bounded_inside_wide_and_narrow_workareas() {
        assert_eq!(
            player_command_surface_size(1920, 1040),
            WindowSize {
                width: 340,
                height: 560,
            }
        );
        assert_eq!(
            player_command_surface_size(320, 240),
            WindowSize {
                width: 288,
                height: 208,
            }
        );
        assert_eq!(
            player_command_surface_size(24, 72),
            WindowSize {
                width: 1,
                height: 80,
            }
        );
    }
}
