use std::cell::{Cell, RefCell};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::cairo;
use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use okp_core::{AppIdentity, media_formats, natural_compare, time_code};
use okp_mpv::{
    Chapter, InfoSection, InfoTrack, MediaInfo, Mpv, MpvEvent, Track, TrackKind,
    current_render_target_size, resolve_render_target_size,
};
use serde::Deserialize;
use velopack::{
    UpdateCheck, UpdateInfo, UpdateManager, UpdateOptions, VelopackApp, VelopackAsset,
    sources::GithubSource,
};

mod history;
mod screenshots;
mod settings;
mod thumbnails;

const SPEED_PRESETS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
const APP_BUILD_VERSION: &str = env!("OKP_BUILD_VERSION");
const APP_BUILD_SHA: &str = env!("OKP_BUILD_SHA");
const LINUX_UPDATE_REPO_URL: &str = "https://github.com/BeFeast/ok-player";
const LINUX_DEB_RELEASES_API_URL: &str = "https://api.github.com/repos/BeFeast/ok-player/releases";

#[derive(Default)]
struct PlayerState {
    mpv: Option<Mpv>,
    current_file: Option<PathBuf>,
    current_url: Option<String>,
    playlist: Vec<PathBuf>,
    pending_subtitles: Vec<PathBuf>,
    pending_resume: Option<(PathBuf, f64)>,
    pending_preferences: Option<(PathBuf, history::PlaybackPreferences)>,
    thumbnail_request_key: Option<String>,
    hover_thumbnail_request_key: Option<String>,
    chapters_snapshot: Vec<Chapter>,
    modes: PlayModes,
    private_session: bool,
    history: history::HistoryStore,
    settings: settings::SettingsStore,
    render_target_size: Option<okp_mpv::RenderTargetSize>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum RepeatMode {
    #[default]
    Off,
    One,
    All,
}

impl RepeatMode {
    fn cycle(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat Off",
            Self::One => "Repeat One",
            Self::All => "Repeat All",
        }
    }
}

struct PlayModes {
    repeat_mode: RepeatMode,
    shuffle_enabled: bool,
    auto_advance_enabled: bool,
    shuffle_order: Vec<usize>,
    shuffle_cursor: Option<usize>,
    shuffle_seed: u64,
}

impl Default for PlayModes {
    fn default() -> Self {
        Self {
            repeat_mode: RepeatMode::Off,
            shuffle_enabled: false,
            auto_advance_enabled: true,
            shuffle_order: Vec::new(),
            shuffle_cursor: None,
            shuffle_seed: shuffle_seed(),
        }
    }
}

impl PlayModes {
    fn reset_shuffle_order(&mut self) {
        self.shuffle_order.clear();
        self.shuffle_cursor = None;
    }

    fn ensure_shuffle_order(&mut self, playlist_len: usize, current_index: usize) {
        if !self.shuffle_enabled || playlist_len == 0 {
            self.reset_shuffle_order();
            return;
        }

        if self.shuffle_order.len() != playlist_len {
            self.shuffle_order = (0..playlist_len).collect();
            for index in (1..playlist_len).rev() {
                let swap_with = (next_shuffle_value(&mut self.shuffle_seed) as usize) % (index + 1);
                self.shuffle_order.swap(index, swap_with);
            }
        }

        if let Some(position) = self
            .shuffle_order
            .iter()
            .position(|index| *index == current_index)
        {
            self.shuffle_cursor = Some(position);
        }
    }
}

#[derive(Clone, Default)]
struct LaunchArgs {
    file: Option<PathBuf>,
    url: Option<String>,
    subtitles: Vec<PathBuf>,
}

struct Controls {
    open_button: gtk::Button,
    subtitle_button: gtk::MenuButton,
    audio_button: gtk::MenuButton,
    speed_button: gtk::MenuButton,
    previous_button: gtk::Button,
    play_button: gtk::Button,
    next_button: gtk::Button,
    chapters_button: gtk::Button,
    screenshot_button: gtk::Button,
    fullscreen_button: gtk::Button,
    more_button: gtk::MenuButton,
    seek: gtk::Scale,
    elapsed_label: gtk::Label,
    duration_label: gtk::Label,
    volume: gtk::Scale,
    chapter_marks_snapshot: RefCell<Vec<f64>>,
    up_next_revealer: gtk::Revealer,
    up_next_title: gtk::Label,
    up_next_list: gtk::ListBox,
    side_panel_user_visible: Rc<Cell<bool>>,
    side_panel_pinned: Rc<Cell<bool>>,
    side_panel_snapshot: RefCell<SidePanelSnapshot>,
    side_panel_actions: Rc<RefCell<Vec<SidePanelAction>>>,
    thumbnail_sender: mpsc::Sender<String>,
    thumbnail_events: RefCell<mpsc::Receiver<String>>,
}

#[derive(Clone)]
struct PendingLinuxUpdate {
    manager: Option<UpdateManager>,
    target: LinuxUpdateTarget,
}

#[derive(Clone)]
enum LinuxUpdateTarget {
    Info(Box<UpdateInfo>),
    Asset(Box<VelopackAsset>),
    Deb(ManualDebUpdate),
}

enum LinuxUpdateCheckResult {
    UpToDate,
    Available(PendingLinuxUpdate),
    Failed(String),
}

enum LinuxUpdateApplyResult {
    Restarting,
    InstallerOpened(PathBuf),
}

#[derive(Clone, Debug)]
struct ManualDebUpdate {
    version: String,
    name: String,
    url: String,
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: Option<u64>,
}

#[derive(Clone)]
struct EmptySurface {
    revealer: gtk::Revealer,
    panel: gtk::Box,
}

impl EmptySurface {
    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn set_has_media(&self, has_media: bool) {
        self.revealer.set_reveal_child(!has_media);
        self.revealer.set_can_target(!has_media);
    }

    fn set_drop_active(&self, active: bool) {
        if active {
            self.panel.add_css_class("is-drop-target");
        } else {
            self.panel.remove_css_class("is-drop-target");
        }
    }
}

struct ChromeVisibility {
    revealer: gtk::Revealer,
    linked_revealers: Rc<RefCell<Vec<gtk::Revealer>>>,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
    pin_count: Rc<Cell<u32>>,
    auto_hide_enabled: Rc<Cell<bool>>,
    is_revealed: Rc<Cell<bool>>,
}

impl ChromeVisibility {
    fn new() -> Self {
        let revealer = gtk::Revealer::new();
        revealer.add_css_class("okp-chrome-revealer");
        revealer.set_halign(gtk::Align::Fill);
        revealer.set_valign(gtk::Align::End);
        revealer.set_transition_duration(170);
        revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
        revealer.set_reveal_child(true);
        revealer.set_can_target(true);

        Self {
            revealer,
            linked_revealers: Rc::new(RefCell::new(Vec::new())),
            hide_source: Rc::new(RefCell::new(None)),
            pin_count: Rc::new(Cell::new(0)),
            auto_hide_enabled: Rc::new(Cell::new(false)),
            is_revealed: Rc::new(Cell::new(true)),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        self.revealer.set_child(Some(child));
    }

    fn add_linked_revealer(&self, revealer: &gtk::Revealer) {
        Self::set_revealer_state(revealer, self.is_revealed.get());
        self.linked_revealers.borrow_mut().push(revealer.clone());
    }

    fn set_auto_hide_enabled(&self, enabled: bool) {
        let was_enabled = self.auto_hide_enabled.replace(enabled);
        if enabled && self.pin_count.get() == 0 {
            if !was_enabled || (self.is_revealed.get() && self.hide_source.borrow().is_none()) {
                self.schedule_hide();
            }
        } else {
            self.show_persistently();
        }
    }

    fn show_for_activity(&self) {
        self.show_now();
        if self.auto_hide_enabled.get() && self.pin_count.get() == 0 {
            self.schedule_hide();
        }
    }

    fn pin(&self) {
        self.pin_count.set(self.pin_count.get().saturating_add(1));
        self.show_persistently();
    }

    fn unpin(&self) {
        self.pin_count.set(self.pin_count.get().saturating_sub(1));
        if self.auto_hide_enabled.get() && self.pin_count.get() == 0 {
            self.schedule_hide();
        }
    }

    fn show_persistently(&self) {
        self.cancel_hide();
        self.show_now();
    }

    fn show_now(&self) {
        self.is_revealed.set(true);
        self.set_all_revealed(true);
    }

    fn set_all_revealed(&self, revealed: bool) {
        Self::set_revealer_state(&self.revealer, revealed);
        for revealer in self.linked_revealers.borrow().iter() {
            Self::set_revealer_state(revealer, revealed);
        }
    }

    fn set_revealer_state(revealer: &gtk::Revealer, revealed: bool) {
        revealer.set_can_target(revealed);
        revealer.set_reveal_child(revealed);
    }

    fn schedule_hide(&self) {
        if !self.is_revealed.get() {
            return;
        }
        self.cancel_hide();

        let revealer = self.revealer.clone();
        let linked_revealers = Rc::clone(&self.linked_revealers);
        let hide_source = Rc::clone(&self.hide_source);
        let pin_count = Rc::clone(&self.pin_count);
        let auto_hide_enabled = Rc::clone(&self.auto_hide_enabled);
        let is_revealed = Rc::clone(&self.is_revealed);
        let source_id = glib::timeout_add_local(Duration::from_millis(2600), move || {
            hide_source.borrow_mut().take();
            if auto_hide_enabled.get() && pin_count.get() == 0 {
                is_revealed.set(false);
                Self::set_revealer_state(&revealer, false);
                for revealer in linked_revealers.borrow().iter() {
                    Self::set_revealer_state(revealer, false);
                }
            }
            glib::ControlFlow::Break
        });
        self.hide_source.borrow_mut().replace(source_id);
    }

    fn cancel_hide(&self) {
        if let Some(source_id) = self.hide_source.borrow_mut().take() {
            source_id.remove();
        }
    }
}

struct StatusToast {
    revealer: gtk::Revealer,
    label: gtk::Label,
    hide_source: Rc<RefCell<Option<glib::SourceId>>>,
}

impl StatusToast {
    fn new() -> Self {
        let label = gtk::Label::new(None);
        label.add_css_class("okp-status-toast");
        label.set_ellipsize(pango::EllipsizeMode::Middle);
        label.set_max_width_chars(72);

        let revealer = gtk::Revealer::new();
        revealer.set_halign(gtk::Align::Center);
        revealer.set_valign(gtk::Align::Start);
        revealer.set_margin_top(28);
        revealer.set_transition_duration(140);
        revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
        revealer.set_reveal_child(false);
        revealer.set_can_target(false);
        revealer.set_child(Some(&label));

        Self {
            revealer,
            label,
            hide_source: Rc::new(RefCell::new(None)),
        }
    }

    fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    fn show(&self, message: &str) {
        self.label.set_text(message);
        self.revealer.set_reveal_child(true);

        if let Some(source_id) = self.hide_source.borrow_mut().take() {
            source_id.remove();
        }

        let revealer = self.revealer.clone();
        let hide_source = Rc::clone(&self.hide_source);
        let source_id = glib::timeout_add_local(Duration::from_secs(3), move || {
            revealer.set_reveal_child(false);
            hide_source.borrow_mut().take();
            glib::ControlFlow::Break
        });
        self.hide_source.borrow_mut().replace(source_id);
    }
}

struct SeekHoverPreview {
    popover: gtk::Popover,
    thumbnail: gtk::Picture,
    thumbnail_snapshot: RefCell<Option<PathBuf>>,
    time_label: gtk::Label,
    chapter_label: gtk::Label,
}

impl SeekHoverPreview {
    fn new(seek: &gtk::Scale) -> Self {
        let thumbnail = gtk::Picture::new();
        thumbnail.add_css_class("okp-seek-preview-thumb");
        thumbnail.set_size_request(144, 81);
        thumbnail.set_can_shrink(true);
        thumbnail.set_visible(false);

        let time_label = gtk::Label::new(Some("00:00"));
        time_label.add_css_class("okp-seek-preview-time");

        let chapter_label = gtk::Label::new(None);
        chapter_label.add_css_class("okp-seek-preview-chapter");
        chapter_label.set_ellipsize(pango::EllipsizeMode::End);
        chapter_label.set_max_width_chars(32);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("okp-seek-preview");
        content.append(&thumbnail);
        content.append(&time_label);
        content.append(&chapter_label);

        let popover = gtk::Popover::new();
        popover.set_autohide(false);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Top);
        popover.set_child(Some(&content));
        popover.set_parent(seek);

        Self {
            popover,
            thumbnail,
            thumbnail_snapshot: RefCell::new(None),
            time_label,
            chapter_label,
        }
    }

    fn show(
        &self,
        seek: &gtk::Scale,
        x: f64,
        time: f64,
        chapter: Option<&Chapter>,
        thumbnail: Option<PathBuf>,
    ) {
        let width = seek.width().max(1);
        let height = seek.height().max(1);
        let x = x.clamp(0.0, f64::from(width)).round() as i32;
        if let Some(thumbnail_path) = thumbnail {
            let mut snapshot = self.thumbnail_snapshot.borrow_mut();
            if snapshot.as_ref() != Some(&thumbnail_path) {
                self.thumbnail.set_filename(Some(&thumbnail_path));
                *snapshot = Some(thumbnail_path);
            }
            self.thumbnail.set_visible(true);
        } else {
            self.thumbnail.set_visible(false);
            self.thumbnail_snapshot.borrow_mut().take();
        }

        self.time_label.set_text(&format_time(time));
        if let Some(chapter) = chapter {
            let title = chapter
                .title
                .as_deref()
                .filter(|title| !title.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));
            self.chapter_label.set_text(&title);
            self.chapter_label.set_visible(true);
        } else {
            self.chapter_label.set_visible(false);
        }
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(x, 0, 1, height)));
        self.popover.popup();
    }

    fn hide(&self) {
        self.popover.popdown();
    }
}

#[derive(Clone, Default, PartialEq)]
struct SidePanelSnapshot {
    has_media: bool,
    current_file: Option<PathBuf>,
    playlist: Vec<PathBuf>,
    chapters: Vec<Chapter>,
}

#[derive(Clone, Copy)]
enum SidePanelAction {
    None,
    Chapter(f64),
    Playlist(usize),
}

fn main() -> glib::ExitCode {
    VelopackApp::build().set_auto_apply_on_startup(false).run();

    let (argv0, launch_args) = parse_launch_args();
    let app = gtk::Application::builder()
        .application_id("com.befeast.okplayer")
        .build();

    app.connect_activate(move |app| build_window(app, launch_args.clone()));
    app.run_with_args(&[argv0])
}

fn parse_launch_args() -> (String, LaunchArgs) {
    let mut args = env::args_os();
    let argv0 = args
        .next()
        .and_then(|arg| arg.into_string().ok())
        .unwrap_or_else(|| "ok-player".to_owned());
    let mut launch = LaunchArgs::default();

    while let Some(arg) = args.next() {
        if arg == "--sub" {
            if let Some(path) = args.next() {
                launch.subtitles.push(PathBuf::from(path));
            }
            continue;
        }

        if launch.file.is_none() && launch.url.is_none() {
            if let Some(text) = arg.to_str()
                && media_formats::is_playable_url(Some(text))
            {
                launch.url = Some(text.to_owned());
                continue;
            }

            launch.file = Some(PathBuf::from(arg));
        }
    }

    (argv0, launch)
}

fn build_window(app: &gtk::Application, launch_args: LaunchArgs) {
    install_css();

    let identity = AppIdentity::linux();
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let auto_check_updates = state.borrow().settings.auto_check_updates();
    let updating_seek = Rc::new(Cell::new(false));
    let updating_volume = Rc::new(Cell::new(false));
    let status_toast = Rc::new(StatusToast::new());
    let chrome = Rc::new(ChromeVisibility::new());

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&identity.name)
        .default_width(1120)
        .default_height(680)
        .decorated(false)
        .build();
    window.add_css_class("okp-player-window");

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("okp-root");

    let video_area = gtk::GLArea::new();
    video_area.set_hexpand(true);
    video_area.set_vexpand(true);
    video_area.set_auto_render(false);
    video_area.set_required_version(3, 2);
    video_area.add_css_class("okp-video-plane");

    let controls = build_controls(
        &window,
        Rc::clone(&state),
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    let control_bar = controls_bar(&controls);
    let window_chrome = build_player_window_chrome(&window);
    let empty_surface = build_empty_surface(&window, Rc::clone(&state), Rc::clone(&status_toast));
    chrome.set_child(&control_bar);
    chrome.add_linked_revealer(&window_chrome);
    chrome.add_linked_revealer(&controls.up_next_revealer);

    overlay.set_child(Some(&video_area));
    overlay.add_overlay(empty_surface.widget());
    overlay.add_overlay(&window_chrome);
    overlay.add_overlay(chrome.widget());
    overlay.add_overlay(&controls.up_next_revealer);
    overlay.add_overlay(status_toast.widget());
    for resize_handle in build_player_resize_handles(&window) {
        overlay.add_overlay(&resize_handle);
    }
    window.set_child(Some(&overlay));
    connect_chrome_activity(&overlay, Rc::clone(&chrome));

    connect_mpv(&video_area, Rc::clone(&state), launch_args);
    connect_video_clicks(
        &video_area,
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    );
    connect_drop(&window, Rc::clone(&state), empty_surface.clone());
    connect_keyboard(
        &window,
        Rc::clone(&state),
        Rc::clone(&status_toast),
        Rc::clone(&chrome),
    );
    connect_progress_persistence(&window, Rc::clone(&state));
    connect_state_poll(
        &window,
        Rc::clone(&state),
        controls,
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
        Rc::clone(&chrome),
        empty_surface,
    );

    window.present();
    if env::var_os("OKP_OPEN_SETTINGS_ON_STARTUP").is_some() {
        let settings_parent = window.clone();
        let settings_state = Rc::clone(&state);
        let settings_toast = Rc::clone(&status_toast);
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            open_settings_window(&settings_parent, settings_state, settings_toast);
        });
    }
    if auto_check_updates {
        check_updates_on_startup(Rc::clone(&status_toast));
    }
}

fn build_player_window_chrome(window: &gtk::ApplicationWindow) -> gtk::Revealer {
    let revealer = gtk::Revealer::new();
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Start);
    revealer.set_transition_duration(140);
    revealer.set_transition_type(gtk::RevealerTransitionType::SlideDown);
    revealer.set_reveal_child(true);
    revealer.set_can_target(true);

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("okp-window-chrome");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::Start);
    bar.set_margin_top(0);

    let drag_zone = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    drag_zone.add_css_class("okp-window-drag-zone");
    drag_zone.set_hexpand(true);
    drag_zone.set_can_target(true);
    connect_player_window_drag(&drag_zone, window);
    bar.append(&drag_zone);

    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-player-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_margin_top(4);
    controls.set_margin_end(6);

    let minimize = player_window_control("−", "Minimize");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = player_window_control("□", "Maximize");
    sync_player_maximize_icon(&maximize, window);
    let maximize_window = window.clone();
    let maximize_button = maximize.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
        sync_player_maximize_icon(&maximize_button, &maximize_window);
    });
    let notify_button = maximize.clone();
    window.connect_maximized_notify(move |window| {
        sync_player_maximize_icon(&notify_button, window);
    });
    controls.append(&maximize);

    let close = player_window_control("×", "Close");
    close.add_css_class("okp-player-window-close");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    bar.append(&controls);
    revealer.set_child(Some(&bar));
    revealer
}

fn player_window_control(label: &str, tooltip: &str) -> gtk::Button {
    let glyph = gtk::Label::new(None);
    glyph.add_css_class("okp-player-window-control-glyph");

    let button = gtk::Button::new();
    button.set_child(Some(&glyph));
    button.add_css_class("okp-player-window-control");
    button.set_has_frame(false);
    button.set_tooltip_text(Some(tooltip));
    set_player_window_control_label(&button, label);
    button
}

fn sync_player_maximize_icon(button: &gtk::Button, window: &gtk::ApplicationWindow) {
    if window.is_maximized() {
        set_player_window_control_label(button, "❐");
        button.set_tooltip_text(Some("Restore"));
    } else {
        set_player_window_control_label(button, "□");
        button.set_tooltip_text(Some("Maximize"));
    }
}

fn set_player_window_control_label(button: &gtk::Button, label: &str) {
    if let Some(glyph) = button.child().and_downcast::<gtk::Label>() {
        let label = glib::markup_escape_text(label);
        glyph.set_markup(&format!(
            "<span font_desc=\"Sans 15\" foreground=\"#d8dde3\">{label}</span>"
        ));
    }
}

fn connect_player_window_drag(widget: &impl IsA<gtk::Widget>, window: &gtk::ApplicationWindow) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let drag_window = window.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        if n_press == 2 {
            if drag_window.is_maximized() {
                drag_window.unmaximize();
            } else {
                drag_window.maximize();
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            return;
        };
        let Some(surface) = drag_window.surface() else {
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            return;
        };

        toplevel.begin_move(
            &device,
            gesture.current_button() as i32,
            x,
            y,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

fn build_player_resize_handles(window: &gtk::ApplicationWindow) -> Vec<gtk::Box> {
    let specs = [
        (
            gdk::SurfaceEdge::NorthWest,
            gtk::Align::Start,
            gtk::Align::Start,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::North,
            gtk::Align::Fill,
            gtk::Align::Start,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::NorthEast,
            gtk::Align::End,
            gtk::Align::Start,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::West,
            gtk::Align::Start,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::East,
            gtk::Align::End,
            gtk::Align::Fill,
            6,
            -1,
            "ew-resize",
            "okp-resize-edge-vertical",
        ),
        (
            gdk::SurfaceEdge::SouthWest,
            gtk::Align::Start,
            gtk::Align::End,
            16,
            16,
            "nesw-resize",
            "okp-resize-corner",
        ),
        (
            gdk::SurfaceEdge::South,
            gtk::Align::Fill,
            gtk::Align::End,
            -1,
            6,
            "ns-resize",
            "okp-resize-edge-horizontal",
        ),
        (
            gdk::SurfaceEdge::SouthEast,
            gtk::Align::End,
            gtk::Align::End,
            16,
            16,
            "nwse-resize",
            "okp-resize-corner",
        ),
    ];

    specs
        .into_iter()
        .map(
            |(edge, halign, valign, width, height, cursor, class_name)| {
                let handle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                handle.add_css_class("okp-resize-handle");
                handle.add_css_class(class_name);
                handle.set_halign(halign);
                handle.set_valign(valign);
                handle.set_can_target(true);
                handle.set_cursor_from_name(Some(cursor));
                if width > 0 {
                    handle.set_width_request(width);
                }
                if height > 0 {
                    handle.set_height_request(height);
                }
                connect_player_window_resize(&handle, window, edge);
                handle
            },
        )
        .collect()
}

fn connect_player_window_resize(
    widget: &gtk::Box,
    window: &gtk::ApplicationWindow,
    edge: gdk::SurfaceEdge,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(gdk::BUTTON_PRIMARY);
    let resize_window = window.clone();
    let resize_widget = widget.clone();
    gesture.connect_pressed(move |gesture, _, x, y| {
        let debug_resize = env::var_os("OKP_DEBUG_WINDOW_RESIZE").is_some();
        if debug_resize {
            eprintln!("resize press edge={edge:?} local=({x:.1},{y:.1})");
        }

        if resize_window.is_fullscreen() || resize_window.is_maximized() {
            if debug_resize {
                eprintln!("resize ignored: fullscreen/maximized");
            }
            return;
        }

        let Some(device) = gesture.current_event_device() else {
            if debug_resize {
                eprintln!("resize ignored: no device");
            }
            return;
        };
        let Some(surface) = resize_window.surface() else {
            if debug_resize {
                eprintln!("resize ignored: no surface");
            }
            return;
        };
        let Ok(toplevel) = surface.downcast::<gdk::Toplevel>() else {
            if debug_resize {
                eprintln!("resize ignored: surface is not a toplevel");
            }
            return;
        };
        let window_point = resize_widget
            .compute_point(
                &resize_window,
                &gtk::graphene::Point::new(x as f32, y as f32),
            )
            .map(|point| (f64::from(point.x()), f64::from(point.y())))
            .unwrap_or((x, y));
        if debug_resize {
            eprintln!(
                "resize begin edge={edge:?} window=({:.1},{:.1}) button={}",
                window_point.0,
                window_point.1,
                gesture.current_button()
            );
        }

        toplevel.begin_resize(
            edge,
            Some(&device),
            gesture.current_button() as i32,
            window_point.0,
            window_point.1,
            gesture.current_event_time(),
        );
    });
    widget.add_controller(gesture);
}

fn build_empty_surface(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> EmptySurface {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 16);
    panel.add_css_class("okp-empty-panel");
    panel.set_halign(gtk::Align::Center);
    panel.set_valign(gtk::Align::Center);

    let logo = gtk::Image::from_icon_name("com.befeast.okplayer");
    logo.add_css_class("okp-empty-logo");
    logo.set_pixel_size(64);
    panel.append(&logo);

    let title = gtk::Label::new(Some("OK Player"));
    title.add_css_class("okp-empty-title");
    title.set_xalign(0.5);
    panel.append(&title);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    actions.set_halign(gtk::Align::Center);

    let open_button = gtk::Button::with_label("Open media");
    open_button.add_css_class("okp-empty-primary-button");
    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));
    actions.append(&open_button);

    let url_button = gtk::Button::with_label("Open URL");
    url_button.add_css_class("okp-empty-secondary-button");
    let url_parent = window.clone();
    let url_state = Rc::clone(&state);
    let url_toast = Rc::clone(&status_toast);
    url_button.connect_clicked(move |_| {
        open_url_dialog(&url_parent, Rc::clone(&url_state), Rc::clone(&url_toast));
    });
    actions.append(&url_button);

    panel.append(&actions);

    let revealer = gtk::Revealer::new();
    revealer.add_css_class("okp-empty-surface");
    revealer.set_halign(gtk::Align::Fill);
    revealer.set_valign(gtk::Align::Fill);
    revealer.set_transition_duration(180);
    revealer.set_transition_type(gtk::RevealerTransitionType::Crossfade);
    revealer.set_reveal_child(true);
    revealer.set_child(Some(&panel));

    EmptySurface { revealer, panel }
}

fn build_controls(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) -> Controls {
    let play_button = gtk::Button::builder()
        .icon_name("media-playback-start-symbolic")
        .build();
    play_button.set_has_frame(false);
    play_button.add_css_class("okp-control-button");
    play_button.add_css_class("okp-play-button");
    play_button.set_tooltip_text(Some("Play / Pause (Space)"));
    play_button.set_sensitive(false);

    let open_button = gtk::Button::with_label("Open");
    open_button.set_has_frame(false);
    open_button.add_css_class("okp-control-button");
    open_button.add_css_class("okp-chip-button");
    open_button.set_tooltip_text(Some("Open file (O)"));

    let subtitle_button = gtk::MenuButton::builder().label("Sub").build();
    subtitle_button.set_has_frame(false);
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.add_css_class("okp-chip-button");
    subtitle_button.set_tooltip_text(Some("Subtitles"));
    subtitle_button.set_sensitive(false);

    let audio_button = gtk::MenuButton::builder().label("Audio").build();
    audio_button.set_has_frame(false);
    audio_button.add_css_class("okp-control-button");
    audio_button.add_css_class("okp-chip-button");
    audio_button.set_tooltip_text(Some("Audio"));
    audio_button.set_sensitive(false);

    let speed_button = gtk::MenuButton::builder().label("1.00x").build();
    speed_button.set_has_frame(false);
    speed_button.add_css_class("okp-control-button");
    speed_button.add_css_class("okp-speed-chip");
    speed_button.set_tooltip_text(Some("Playback speed"));
    speed_button.set_sensitive(false);

    let previous_button = gtk::Button::builder()
        .icon_name("media-skip-backward-symbolic")
        .build();
    previous_button.set_has_frame(false);
    previous_button.add_css_class("okp-control-button");
    previous_button.add_css_class("okp-transport-button");
    previous_button.set_tooltip_text(Some("Previous item (Page Up)"));
    previous_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");

    let next_button = gtk::Button::builder()
        .icon_name("media-skip-forward-symbolic")
        .build();
    next_button.set_has_frame(false);
    next_button.add_css_class("okp-control-button");
    next_button.add_css_class("okp-transport-button");
    next_button.set_tooltip_text(Some("Next item (Page Down)"));
    next_button.set_sensitive(false);

    let chapters_button = gtk::Button::builder()
        .icon_name("view-list-symbolic")
        .build();
    chapters_button.set_has_frame(false);
    chapters_button.add_css_class("okp-control-button");
    chapters_button.add_css_class("okp-icon-button");
    chapters_button.set_tooltip_text(Some("Chapters / Up Next"));
    chapters_button.set_sensitive(false);

    let screenshot_button = gtk::Button::builder()
        .icon_name("camera-photo-symbolic")
        .build();
    screenshot_button.set_has_frame(false);
    screenshot_button.add_css_class("okp-control-button");
    screenshot_button.add_css_class("okp-icon-button");
    screenshot_button.set_tooltip_text(Some("Save screenshot to Pictures/OK Player (C)"));
    screenshot_button.set_sensitive(false);

    let fullscreen_button = gtk::Button::builder()
        .icon_name("view-fullscreen-symbolic")
        .build();
    fullscreen_button.set_has_frame(false);
    fullscreen_button.add_css_class("okp-control-button");
    fullscreen_button.add_css_class("okp-icon-button");
    fullscreen_button.set_tooltip_text(Some("Enter Fullscreen (F)"));
    fullscreen_button.set_sensitive(false);

    let more_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .build();
    more_button.set_has_frame(false);
    more_button.add_css_class("okp-control-button");
    more_button.add_css_class("okp-icon-button");
    more_button.set_tooltip_text(Some("More commands"));

    let duration_label = gtk::Label::new(Some("00:00"));
    duration_label.add_css_class("okp-time-label");

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    seek.set_sensitive(false);
    seek.add_css_class("okp-seek");

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    volume.set_draw_value(false);
    volume.set_width_request(68);
    volume.set_value(100.0);
    volume.add_css_class("okp-volume");

    let up_next_title = gtk::Label::new(Some("Chapters / Up Next"));
    up_next_title.add_css_class("okp-up-next-title");
    up_next_title.set_xalign(0.0);

    let up_next_list = gtk::ListBox::new();
    up_next_list.add_css_class("okp-up-next-list");
    up_next_list.set_selection_mode(gtk::SelectionMode::None);

    let up_next_scroller = gtk::ScrolledWindow::new();
    up_next_scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    up_next_scroller.set_child(Some(&up_next_list));
    up_next_scroller.set_vexpand(true);

    let up_next_panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    up_next_panel.add_css_class("okp-up-next-panel");
    up_next_panel.set_width_request(320);
    up_next_panel.append(&up_next_title);
    up_next_panel.append(&up_next_scroller);

    let up_next_revealer = gtk::Revealer::new();
    up_next_revealer.set_halign(gtk::Align::End);
    up_next_revealer.set_valign(gtk::Align::Fill);
    up_next_revealer.set_margin_top(24);
    up_next_revealer.set_margin_end(24);
    up_next_revealer.set_margin_bottom(92);
    up_next_revealer.set_transition_duration(170);
    up_next_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
    up_next_revealer.set_reveal_child(true);
    up_next_revealer.set_can_target(true);
    up_next_revealer.set_visible(false);
    up_next_revealer.set_child(Some(&up_next_panel));

    let side_panel_user_visible = Rc::new(Cell::new(false));
    let side_panel_pinned = Rc::new(Cell::new(false));

    let up_next_state = Rc::clone(&state);
    let up_next_actions = Rc::new(RefCell::new(Vec::<SidePanelAction>::new()));
    let row_actions = Rc::clone(&up_next_actions);
    let (thumbnail_sender, thumbnail_receiver) = mpsc::channel();
    up_next_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }

        match row_actions
            .borrow()
            .get(index as usize)
            .copied()
            .unwrap_or(SidePanelAction::None)
        {
            SidePanelAction::None => {}
            SidePanelAction::Chapter(time) => seek_to_chapter(&up_next_state, time),
            SidePanelAction::Playlist(index) => {
                jump_playlist_index(&up_next_state, index);
            }
        }
    });

    let subtitle_popover = gtk::Popover::new();
    prepare_track_popover(&subtitle_popover);
    connect_popover_chrome_pin(&subtitle_popover, Rc::clone(&chrome));
    subtitle_button.set_popover(Some(&subtitle_popover));
    let subtitle_parent = window.clone();
    let subtitle_state = Rc::clone(&state);
    subtitle_popover.connect_show(move |popover| {
        populate_subtitle_popover(popover, &subtitle_parent, Rc::clone(&subtitle_state));
    });

    let audio_popover = gtk::Popover::new();
    prepare_track_popover(&audio_popover);
    connect_popover_chrome_pin(&audio_popover, Rc::clone(&chrome));
    audio_button.set_popover(Some(&audio_popover));
    let audio_state = Rc::clone(&state);
    audio_popover.connect_show(move |popover| {
        populate_audio_popover(popover, Rc::clone(&audio_state));
    });

    let speed_popover = gtk::Popover::new();
    prepare_track_popover(&speed_popover);
    connect_popover_chrome_pin(&speed_popover, Rc::clone(&chrome));
    speed_button.set_popover(Some(&speed_popover));
    let speed_state = Rc::clone(&state);
    speed_popover.connect_show(move |popover| {
        populate_speed_popover(popover, Rc::clone(&speed_state));
    });

    let more_popover = gtk::Popover::new();
    prepare_track_popover(&more_popover);
    connect_popover_chrome_pin(&more_popover, Rc::clone(&chrome));
    more_button.set_popover(Some(&more_popover));
    let more_parent = window.clone();
    let more_state = Rc::clone(&state);
    let more_toast = Rc::clone(&status_toast);
    more_popover.connect_show(move |popover| {
        populate_command_popover(
            popover,
            &more_parent,
            Rc::clone(&more_state),
            Rc::clone(&more_toast),
        );
    });

    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));

    let previous_state = Rc::clone(&state);
    previous_button.connect_clicked(move |_| {
        navigate_playlist(&previous_state, -1);
    });

    let play_state = Rc::clone(&state);
    let play_open_parent = window.clone();
    play_button.connect_clicked(move |_| {
        let has_media = has_loaded_media(&play_state);
        if !has_media {
            open_media_dialog(&play_open_parent, Rc::clone(&play_state));
            return;
        }

        if let Some(mpv) = play_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.cycle_pause()
        {
            eprintln!("Failed to toggle playback: {error}");
        }
    });

    let next_state = Rc::clone(&state);
    next_button.connect_clicked(move |_| {
        navigate_playlist(&next_state, 1);
    });

    let chapters_panel = up_next_revealer.clone();
    let chapters_toggle = chapters_button.clone();
    let chapters_visible = Rc::clone(&side_panel_user_visible);
    let chapters_pinned = Rc::clone(&side_panel_pinned);
    let chapters_chrome = Rc::clone(&chrome);
    chapters_button.connect_clicked(move |_| {
        set_side_panel_user_visible(
            &chapters_panel,
            &chapters_toggle,
            &chapters_visible,
            &chapters_pinned,
            &chapters_chrome,
            !chapters_visible.get(),
        );
    });

    let screenshot_state = Rc::clone(&state);
    let screenshot_toast = Rc::clone(&status_toast);
    screenshot_button
        .connect_clicked(move |_| take_screenshot(&screenshot_state, &screenshot_toast));

    let fullscreen_parent = window.clone();
    fullscreen_button.connect_clicked(move |_| toggle_fullscreen(&fullscreen_parent));

    let seek_state = Rc::clone(&state);
    seek.connect_change_value(move |_, _, value| {
        if !updating_seek.get()
            && let Some(mpv) = seek_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.seek_absolute(value)
        {
            eprintln!("Failed to seek: {error}");
        }

        glib::Propagation::Proceed
    });
    connect_seek_hover(&seek, Rc::clone(&state), thumbnail_sender.clone());

    let volume_state = Rc::clone(&state);
    volume.connect_change_value(move |_, _, value| {
        if !updating_volume.get() {
            set_volume_from_ui(&volume_state, value);
        }

        glib::Propagation::Proceed
    });

    Controls {
        open_button,
        subtitle_button,
        audio_button,
        speed_button,
        previous_button,
        play_button,
        next_button,
        chapters_button,
        screenshot_button,
        fullscreen_button,
        more_button,
        seek,
        elapsed_label,
        duration_label,
        volume,
        chapter_marks_snapshot: RefCell::new(Vec::new()),
        up_next_revealer,
        up_next_title,
        up_next_list,
        side_panel_user_visible,
        side_panel_pinned,
        side_panel_snapshot: RefCell::new(SidePanelSnapshot::default()),
        side_panel_actions: up_next_actions,
        thumbnail_sender,
        thumbnail_events: RefCell::new(thumbnail_receiver),
    }
}

fn controls_bar(controls: &Controls) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bar.add_css_class("okp-controls");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_start(18);
    bar.set_margin_end(18);
    bar.set_margin_bottom(18);

    let transport = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    transport.add_css_class("okp-transport-group");
    transport.append(&controls.previous_button);
    transport.append(&controls.play_button);
    transport.append(&controls.next_button);

    let primary = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    primary.add_css_class("okp-control-group");
    primary.append(&controls.open_button);
    primary.append(&transport);

    let timeline = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    timeline.add_css_class("okp-timeline-group");
    timeline.set_hexpand(true);
    timeline.append(&controls.elapsed_label);
    timeline.append(&controls.seek);
    timeline.append(&controls.duration_label);

    let secondary = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    secondary.add_css_class("okp-control-group");
    secondary.append(&controls.volume);
    secondary.append(&controls.speed_button);
    secondary.append(&controls.subtitle_button);
    secondary.append(&controls.audio_button);
    secondary.append(&controls.chapters_button);
    secondary.append(&controls.screenshot_button);
    secondary.append(&controls.fullscreen_button);
    secondary.append(&controls.more_button);

    bar.append(&primary);
    bar.append(&timeline);
    bar.append(&secondary);

    bar
}

fn connect_chrome_activity(overlay: &gtk::Overlay, chrome: Rc<ChromeVisibility>) {
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |_, _, _| {
        chrome.show_for_activity();
    });
    overlay.add_controller(motion);
}

fn connect_popover_chrome_pin(popover: &gtk::Popover, chrome: Rc<ChromeVisibility>) {
    let show_chrome = Rc::clone(&chrome);
    popover.connect_show(move |_| {
        show_chrome.pin();
    });

    popover.connect_closed(move |_| {
        chrome.unpin();
    });
}

fn prepare_track_popover(popover: &gtk::Popover) {
    popover.add_css_class("okp-track-popover");
    popover.set_has_arrow(false);
}

fn set_side_panel_user_visible(
    revealer: &gtk::Revealer,
    toggle: &gtk::Button,
    user_visible: &Rc<Cell<bool>>,
    pinned: &Rc<Cell<bool>>,
    chrome: &ChromeVisibility,
    visible: bool,
) {
    user_visible.set(visible);
    revealer.set_visible(visible);

    if visible {
        toggle.add_css_class("is-selected");
        if pinned.get() {
            chrome.show_persistently();
        } else {
            chrome.pin();
            pinned.set(true);
        }
    } else {
        toggle.remove_css_class("is-selected");
        if pinned.replace(false) {
            chrome.unpin();
        }
    }
}

fn update_fullscreen_button(button: &gtk::Button, is_fullscreen: bool) {
    if is_fullscreen {
        button.set_icon_name("view-restore-symbolic");
        button.set_tooltip_text(Some("Exit Fullscreen (F / Esc)"));
        button.add_css_class("is-selected");
    } else {
        button.set_icon_name("view-fullscreen-symbolic");
        button.set_tooltip_text(Some("Enter Fullscreen (F)"));
        button.remove_css_class("is-selected");
    }
}

fn connect_seek_hover(
    seek: &gtk::Scale,
    state: Rc<RefCell<PlayerState>>,
    thumbnail_sender: mpsc::Sender<String>,
) {
    let preview = Rc::new(SeekHoverPreview::new(seek));
    let motion = gtk::EventControllerMotion::new();

    let motion_seek = seek.clone();
    let motion_state = Rc::clone(&state);
    let motion_preview = Rc::clone(&preview);
    motion.connect_motion(move |_, x, _| {
        let Some((media_path, duration, chapters)) = seek_hover_snapshot(&motion_state) else {
            motion_preview.hide();
            return;
        };

        let width = f64::from(motion_seek.width().max(1));
        let time = (x.clamp(0.0, width) / width * duration).clamp(0.0, duration);
        let thumbnail = hover_thumbnail_for_time(
            &motion_state,
            &media_path,
            time,
            duration,
            &thumbnail_sender,
        );
        motion_preview.show(
            &motion_seek,
            x,
            time,
            chapter_at_time(&chapters, time),
            thumbnail,
        );
    });

    motion.connect_leave(move |_| {
        preview.hide();
    });

    seek.add_controller(motion);
}

fn seek_hover_snapshot(state: &Rc<RefCell<PlayerState>>) -> Option<(PathBuf, f64, Vec<Chapter>)> {
    let state = state.borrow();
    let current_file = state.current_file.clone()?;

    state
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.playback_state().ok())
        .and_then(|playback| playback.duration)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (current_file, duration, state.chapters_snapshot.clone()))
}

fn chapter_at_time(chapters: &[Chapter], time: f64) -> Option<&Chapter> {
    let mut current = None;
    for chapter in chapters {
        if chapter.time.is_finite() && chapter.time <= time {
            current = Some(chapter);
        } else {
            break;
        }
    }

    current
}

fn hover_thumbnail_for_time(
    state: &Rc<RefCell<PlayerState>>,
    media_path: &Path,
    time: f64,
    duration: f64,
    sender: &mpsc::Sender<String>,
) -> Option<PathBuf> {
    let thumbnail_time = thumbnails::hover_thumbnail_time(time, duration);
    if let Some(path) = thumbnails::existing_hover_thumbnail_path(media_path, thumbnail_time) {
        return Some(path);
    }

    let request_key = thumbnails::hover_request_key(media_path, thumbnail_time);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.hover_thumbnail_request_key.as_deref() == Some(request_key.as_str()) {
            false
        } else {
            state.hover_thumbnail_request_key = Some(request_key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_hover_thumbnail(
            media_path.to_path_buf(),
            thumbnail_time,
            request_key,
            sender.clone(),
        );
    }

    None
}

fn connect_mpv(video_area: &gtk::GLArea, state: Rc<RefCell<PlayerState>>, launch_args: LaunchArgs) {
    let realize_state = Rc::clone(&state);
    video_area.connect_realize(move |area| {
        area.make_current();
        if let Some(error) = area.error() {
            eprintln!("GTK GLArea error: {error}");
            return;
        }

        let mut mpv = match Mpv::new() {
            Ok(mpv) => mpv,
            Err(error) => {
                eprintln!("Failed to create mpv: {error}");
                return;
            }
        };
        let saved_volume = realize_state.borrow().settings.volume();
        if let Err(error) = mpv.set_volume(saved_volume) {
            eprintln!("Failed to restore saved volume: {error}");
        }

        if let Err(error) = mpv.create_render_context() {
            eprintln!("Failed to create mpv render context: {error}");
            return;
        }

        realize_state.borrow_mut().mpv = Some(mpv);

        if let Some(path) = launch_args.file.as_deref() {
            load_media_path(&realize_state, path.to_path_buf());
        } else if let Some(url) = launch_args.url.as_deref() {
            load_media_url(&realize_state, url.to_owned());
        }
        realize_state
            .borrow_mut()
            .pending_subtitles
            .extend(launch_args.subtitles.clone());
    });

    let resize_state = Rc::clone(&state);
    video_area.connect_resize(move |_, width, height| {
        resize_state.borrow_mut().render_target_size =
            (width > 0 && height > 0).then_some(okp_mpv::RenderTargetSize { width, height });
    });

    let render_state = Rc::clone(&state);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let viewport_size = current_render_target_size();
        let widget_width = area.width();
        let widget_height = area.height();
        let scale_factor = area.scale_factor();
        let mut state = render_state.borrow_mut();
        let target_size = resolve_render_target_size(
            viewport_size,
            state.render_target_size,
            widget_width,
            widget_height,
            scale_factor,
        );
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(target_size.width, target_size.height)
        {
            eprintln!("mpv render failed: {error}");
        }

        glib::Propagation::Stop
    });

    let unrealize_state = Rc::clone(&state);
    video_area.connect_unrealize(move |area| {
        area.make_current();
        if let Some(mpv) = unrealize_state.borrow_mut().mpv.as_mut() {
            mpv.destroy_render_context();
        }
    });

    let tick_area = video_area.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_area.queue_render();
        glib::ControlFlow::Continue
    });
}

fn connect_state_poll(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    controls: Controls,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
    chrome: Rc<ChromeVisibility>,
    empty_surface: EmptySurface,
) {
    let window = window.clone();
    glib::timeout_add_local(Duration::from_millis(200), move || {
        drain_mpv_events(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok());
        let has_media = has_loaded_media(&state);
        let has_playlist = state.borrow().playlist.len() > 1;
        empty_surface.set_has_media(has_media);
        drain_thumbnail_events(&controls);
        update_up_next_panel(&controls, &state, &chrome);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);
            chrome.set_auto_hide_enabled(has_media && !playback.paused);

            let duration = playback.duration.unwrap_or(0.0).max(0.0);
            let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
            let time_pos = if duration > 0.0 {
                raw_time.min(duration)
            } else {
                raw_time
            };
            try_pending_resume(&state, duration);

            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls.play_button.set_icon_name(if playback.paused {
                "media-playback-start-symbolic"
            } else {
                "media-playback-pause-symbolic"
            });
            controls
                .play_button
                .set_tooltip_text(Some(if playback.paused {
                    "Play (Space)"
                } else {
                    "Pause (Space)"
                }));
            controls
                .speed_button
                .set_label(&format_speed(playback.speed.unwrap_or(1.0)));
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(has_media && duration > 0.0);

            updating_seek.set(true);
            controls.seek.set_range(0.0, duration.max(1.0));
            controls.seek.set_value(time_pos);
            updating_seek.set(false);

            if let Some(volume) = playback.volume {
                updating_volume.set(true);
                controls.volume.set_value(volume.clamp(0.0, 130.0));
                updating_volume.set(false);
            }

            controls.elapsed_label.set_text(&format_time(time_pos));
            controls.duration_label.set_text(&format_time(duration));
        } else {
            chrome.set_auto_hide_enabled(false);
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.speed_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.chapters_button.set_sensitive(has_media);
            controls.screenshot_button.set_sensitive(has_media);
            controls.fullscreen_button.set_sensitive(has_media);
            controls
                .play_button
                .set_icon_name("media-playback-start-symbolic");
            controls.play_button.set_tooltip_text(Some("Play (Space)"));
            controls.speed_button.set_label("1.00x");
            update_fullscreen_button(&controls.fullscreen_button, window.is_fullscreen());
            controls.seek.set_sensitive(false);
            updating_seek.set(true);
            controls.seek.set_range(0.0, 1.0);
            controls.seek.set_value(0.0);
            updating_seek.set(false);
            controls.elapsed_label.set_text("00:00");
            controls.duration_label.set_text("00:00");
        }

        glib::ControlFlow::Continue
    });
}

fn connect_video_clicks(
    video_area: &gtk::GLArea,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let click = gtk::GestureClick::new();
    click.set_button(1);

    let click_window = window.clone();
    click.connect_released(move |_, press_count, _, _| {
        if press_count == 2 {
            toggle_fullscreen(&click_window);
        }
    });

    video_area.add_controller(click);

    let context_click = gtk::GestureClick::new();
    context_click.set_button(3);

    let context_area = video_area.clone();
    let context_window = window.clone();
    let context_state = Rc::clone(&state);
    let context_toast = Rc::clone(&status_toast);
    context_click.connect_pressed(move |_, _, x, y| {
        show_video_context_menu(
            &context_area,
            &context_window,
            Rc::clone(&context_state),
            Rc::clone(&context_toast),
            x,
            y,
        );
    });

    video_area.add_controller(context_click);
}

fn show_video_context_menu(
    video_area: &gtk::GLArea,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    x: f64,
    y: f64,
) {
    let popover = gtk::Popover::new();
    prepare_track_popover(&popover);
    popover.set_parent(video_area);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(
        x.round() as i32,
        y.round() as i32,
        1,
        1,
    )));
    let content = command_popover_content(&popover, parent, state, status_toast);
    set_track_popover_child(&popover, content);
    popover.connect_closed(|popover| popover.unparent());
    popover.popup();
}

fn update_up_next_panel(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    chrome: &ChromeVisibility,
) {
    let snapshot = {
        let state = state.borrow();
        let has_media = has_loaded_media_state(&state);
        let chapters = state
            .mpv
            .as_ref()
            .map(Mpv::chapters)
            .and_then(Result::ok)
            .unwrap_or_default();

        SidePanelSnapshot {
            has_media,
            current_file: state.current_file.clone(),
            playlist: state.playlist.clone(),
            chapters,
        }
    };

    {
        let mut state = state.borrow_mut();
        if state.chapters_snapshot != snapshot.chapters {
            state.chapters_snapshot = snapshot.chapters.clone();
        }
    }

    controls.chapters_button.set_sensitive(snapshot.has_media);
    if !snapshot.has_media {
        set_side_panel_user_visible(
            &controls.up_next_revealer,
            &controls.chapters_button,
            &controls.side_panel_user_visible,
            &controls.side_panel_pinned,
            chrome,
            false,
        );
        controls.side_panel_snapshot.replace(snapshot);
        controls.side_panel_actions.borrow_mut().clear();
        update_chapter_marks(&controls.seek, &controls.chapter_marks_snapshot, &[]);
        clear_list_box(&controls.up_next_list);
        return;
    }

    let panel_visible = controls.side_panel_user_visible.get();
    controls.up_next_revealer.set_visible(panel_visible);
    if panel_visible {
        controls.chapters_button.add_css_class("is-selected");
    } else {
        controls.chapters_button.remove_css_class("is-selected");
    }

    request_chapter_thumbnail_warm(controls, state, &snapshot);

    if *controls.side_panel_snapshot.borrow() == snapshot {
        return;
    }
    controls.side_panel_snapshot.replace(snapshot.clone());
    update_chapter_marks(
        &controls.seek,
        &controls.chapter_marks_snapshot,
        &snapshot.chapters,
    );

    let current_index = snapshot
        .current_file
        .as_ref()
        .and_then(|current| snapshot.playlist.iter().position(|path| path == current));

    controls.up_next_title.set_text("Chapters / Up Next");
    clear_list_box(&controls.up_next_list);
    let mut actions = Vec::new();

    if !snapshot.chapters.is_empty() {
        controls.up_next_list.append(&panel_heading_row(&format!(
            "Chapters · {}",
            snapshot.chapters.len()
        )));
        actions.push(SidePanelAction::None);

        for chapter in &snapshot.chapters {
            let thumbnail = snapshot
                .current_file
                .as_ref()
                .and_then(|path| thumbnails::existing_thumbnail_path(path, chapter));
            controls
                .up_next_list
                .append(&chapter_row(chapter, thumbnail));
            actions.push(SidePanelAction::Chapter(chapter.time));
        }
    }

    if snapshot.playlist.len() > 1 {
        controls.up_next_list.append(&panel_heading_row(&format!(
            "Up Next · {}",
            snapshot.playlist.len()
        )));
        actions.push(SidePanelAction::None);

        for (index, path) in snapshot.playlist.iter().enumerate() {
            controls
                .up_next_list
                .append(&playlist_row(path, index, current_index));
            actions.push(SidePanelAction::Playlist(index));
        }
    }

    if snapshot.chapters.is_empty() && snapshot.playlist.len() <= 1 {
        controls.up_next_list.append(&panel_heading_row("Chapters"));
        actions.push(SidePanelAction::None);
        controls
            .up_next_list
            .append(&panel_empty_row("No chapters in this media yet."));
        actions.push(SidePanelAction::None);
    }

    controls.side_panel_actions.replace(actions);
}

fn drain_thumbnail_events(controls: &Controls) {
    let mut changed = false;
    while controls.thumbnail_events.borrow().try_recv().is_ok() {
        changed = true;
    }

    if changed {
        controls
            .side_panel_snapshot
            .replace(SidePanelSnapshot::default());
    }
}

fn request_chapter_thumbnail_warm(
    controls: &Controls,
    state: &Rc<RefCell<PlayerState>>,
    snapshot: &SidePanelSnapshot,
) {
    let Some(media_path) = snapshot.current_file.as_ref() else {
        return;
    };
    if snapshot.chapters.is_empty() {
        return;
    }

    let key = thumbnails::request_key(media_path, &snapshot.chapters);
    let should_start = {
        let mut state = state.borrow_mut();
        if state.thumbnail_request_key.as_deref() == Some(key.as_str()) {
            false
        } else {
            state.thumbnail_request_key = Some(key.clone());
            true
        }
    };

    if should_start {
        thumbnails::warm_chapter_thumbnails(
            media_path.clone(),
            snapshot.chapters.clone(),
            key,
            controls.thumbnail_sender.clone(),
        );
    }
}

fn update_chapter_marks(seek: &gtk::Scale, snapshot: &RefCell<Vec<f64>>, chapters: &[Chapter]) {
    let marks = chapters
        .iter()
        .map(|chapter| chapter.time)
        .filter(|time| time.is_finite() && *time > 0.0)
        .collect::<Vec<_>>();
    if *snapshot.borrow() == marks {
        return;
    }

    seek.clear_marks();
    for time in &marks {
        seek.add_mark(*time, gtk::PositionType::Top, None);
    }
    snapshot.replace(marks);
}

fn panel_heading_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-heading-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-heading");
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

fn panel_empty_row(text: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-panel-empty-row");
    row.set_activatable(false);
    row.set_selectable(false);

    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-panel-empty");
    label.set_wrap(true);
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row
}

fn chapter_row(chapter: &Chapter, thumbnail: Option<PathBuf>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_selectable(false);

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let thumbnail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    thumbnail_box.add_css_class("okp-chapter-thumb");
    thumbnail_box.set_size_request(88, 50);
    if let Some(thumbnail) = thumbnail {
        let picture = gtk::Picture::for_filename(thumbnail);
        picture.set_size_request(88, 50);
        picture.set_can_shrink(true);
        thumbnail_box.append(&picture);
    }

    let title_text = chapter
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Chapter {}", chapter.index + 1));

    let label_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    label_box.set_hexpand(true);

    let time = gtk::Label::new(Some(&format_time(chapter.time)));
    time.add_css_class("okp-up-next-marker");
    time.set_xalign(0.0);

    let title = gtk::Label::new(Some(&title_text));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    label_box.append(&time);
    label_box.append(&title);
    row_box.append(&thumbnail_box);
    row_box.append(&label_box);
    row.set_child(Some(&row_box));
    row
}

fn clear_list_box(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn playlist_row(path: &Path, index: usize, current_index: Option<usize>) -> gtk::ListBoxRow {
    let is_current = current_index == Some(index);
    let is_next = current_index.is_some_and(|current| index == current + 1);
    let row = gtk::ListBoxRow::new();
    row.add_css_class("okp-up-next-row");
    row.set_activatable(!is_current);
    row.set_selectable(false);
    row.set_tooltip_text(Some(&path.display().to_string()));
    if is_current {
        row.add_css_class("is-current");
    }

    let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row_box.set_hexpand(true);

    let marker = gtk::Label::new(Some(if is_current {
        "Now"
    } else if is_next {
        "Next"
    } else {
        ""
    }));
    marker.add_css_class("okp-up-next-marker");
    marker.set_width_chars(4);
    marker.set_xalign(0.0);

    let title = gtk::Label::new(Some(&display_file_name(path)));
    title.add_css_class("okp-up-next-file");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(pango::EllipsizeMode::End);

    row_box.append(&marker);
    row_box.append(&title);
    row.set_child(Some(&row_box));
    row
}

fn populate_subtitle_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let content = track_popover_content("Subtitles");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);
    let secondary_subtitle_id = read_secondary_subtitle_id(&state);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_subtitle(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let button = track_button(&track_label(track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_subtitle(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    content.append(&track_section_title("Secondary"));

    let secondary_off_button = track_button("Off", secondary_subtitle_id.is_none());
    let secondary_off_state = Rc::clone(&state);
    let secondary_off_popover = popover.clone();
    secondary_off_button.connect_clicked(move |_| {
        if with_mpv(&secondary_off_state, |mpv| {
            mpv.select_secondary_subtitle(None)
        }) {
            save_current_preferences(&secondary_off_state);
        }
        secondary_off_popover.popdown();
    });
    content.append(&secondary_off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No subtitle tracks"));
    } else {
        for track in &tracks {
            let selected = secondary_subtitle_id == Some(track.id);
            let button = track_button(&track_label_for(track, selected), selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| {
                    mpv.select_secondary_subtitle(Some(track_id))
                }) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    content.append(&divider());
    let add_button = track_button("Add subtitle file...", false);
    let add_state = Rc::clone(&state);
    let add_parent = parent.clone();
    let add_popover = popover.clone();
    add_button.connect_clicked(move |_| {
        add_popover.popdown();
        open_subtitle_dialog(&add_parent, Rc::clone(&add_state));
    });
    content.append(&add_button);

    content.append(&divider());
    content.append(&subtitle_adjustment_rows(popover, parent, &state));

    set_track_popover_child(popover, content);
}

fn populate_audio_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content("Audio");
    let tracks = read_tracks(&state)
        .into_iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect::<Vec<_>>();
    let any_selected = tracks.iter().any(|track| track.selected);

    let off_button = track_button("Off", !any_selected);
    let off_state = Rc::clone(&state);
    let off_popover = popover.clone();
    off_button.connect_clicked(move |_| {
        if with_mpv(&off_state, |mpv| mpv.select_audio(None)) {
            save_current_preferences(&off_state);
        }
        off_popover.popdown();
    });
    content.append(&off_button);

    if tracks.is_empty() {
        content.append(&empty_track_label("No audio tracks"));
    } else {
        for track in tracks {
            let button = track_button(&track_label(&track), track.selected);
            let track_state = Rc::clone(&state);
            let track_popover = popover.clone();
            let track_id = track.id;
            button.connect_clicked(move |_| {
                if with_mpv(&track_state, |mpv| mpv.select_audio(Some(track_id))) {
                    save_current_preferences(&track_state);
                }
                track_popover.popdown();
            });
            content.append(&button);
        }
    }

    set_track_popover_child(popover, content);
}

fn populate_speed_popover(popover: &gtk::Popover, state: Rc<RefCell<PlayerState>>) {
    let content = track_popover_content("Speed");
    let current_speed = read_playback_speed(&state);

    for speed in SPEED_PRESETS {
        let button = track_button(&format_speed(speed), speed_matches(current_speed, speed));
        let speed_state = Rc::clone(&state);
        let speed_popover = popover.clone();
        button.connect_clicked(move |_| {
            if with_mpv(&speed_state, |mpv| mpv.set_speed(speed)) {
                save_current_preferences(&speed_state);
            }
            speed_popover.popdown();
        });
        content.append(&button);
    }

    set_track_popover_child(popover, content);
}

fn populate_command_popover(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let content = command_popover_content(popover, parent, state, status_toast);
    set_track_popover_child(popover, content);
}

fn command_popover_content(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let content = track_popover_content("More");
    let (has_media, repeat_mode, shuffle_enabled, auto_advance_enabled, private_session) = {
        let state = state.borrow();
        (
            has_loaded_media_state(&state),
            state.modes.repeat_mode,
            state.modes.shuffle_enabled,
            state.modes.auto_advance_enabled,
            state.private_session,
        )
    };

    let open_url_button = track_button("Open URL...", false);
    let open_url_parent = parent.clone();
    let open_url_state = Rc::clone(&state);
    let open_url_toast = Rc::clone(&status_toast);
    let open_url_popover = popover.clone();
    open_url_button.connect_clicked(move |_| {
        open_url_popover.popdown();
        open_url_dialog(
            &open_url_parent,
            Rc::clone(&open_url_state),
            Rc::clone(&open_url_toast),
        );
    });
    content.append(&open_url_button);

    let settings_button = track_button("Settings...", false);
    let settings_parent = parent.clone();
    let settings_state = Rc::clone(&state);
    let settings_toast = Rc::clone(&status_toast);
    let settings_popover = popover.clone();
    settings_button.connect_clicked(move |_| {
        settings_popover.popdown();
        open_settings_window(
            &settings_parent,
            Rc::clone(&settings_state),
            Rc::clone(&settings_toast),
        );
    });
    content.append(&settings_button);

    let info_button = track_button("Media Info...", false);
    info_button.set_sensitive(has_media);
    info_button.set_tooltip_text(Some("Media Information (I)"));
    let info_parent = parent.clone();
    let info_state = Rc::clone(&state);
    let info_toast = Rc::clone(&status_toast);
    let info_popover = popover.clone();
    info_button.connect_clicked(move |_| {
        info_popover.popdown();
        open_media_info_window(&info_parent, &info_state, Rc::clone(&info_toast));
    });
    content.append(&info_button);

    let go_to_time_button = track_button("Go to Time...", false);
    go_to_time_button.set_sensitive(has_media);
    go_to_time_button.set_tooltip_text(Some("Go to timecode (J)"));
    let go_to_time_parent = parent.clone();
    let go_to_time_state = Rc::clone(&state);
    let go_to_time_toast = Rc::clone(&status_toast);
    let go_to_time_popover = popover.clone();
    go_to_time_button.connect_clicked(move |_| {
        go_to_time_popover.popdown();
        open_go_to_time_dialog(
            &go_to_time_parent,
            Rc::clone(&go_to_time_state),
            Rc::clone(&go_to_time_toast),
        );
    });
    content.append(&go_to_time_button);

    let screenshot_button = track_button("Save Screenshot", false);
    screenshot_button.set_sensitive(has_media);
    let screenshot_state = Rc::clone(&state);
    let screenshot_toast = Rc::clone(&status_toast);
    let screenshot_popover = popover.clone();
    screenshot_button.connect_clicked(move |_| {
        screenshot_popover.popdown();
        take_screenshot(&screenshot_state, &screenshot_toast);
    });
    content.append(&screenshot_button);

    let close_button = track_button("Close Media", false);
    close_button.set_sensitive(has_media);
    let close_state = Rc::clone(&state);
    let close_toast = Rc::clone(&status_toast);
    let close_popover = popover.clone();
    close_button.connect_clicked(move |_| {
        close_popover.popdown();
        close_current_media(&close_state, &close_toast);
    });
    content.append(&close_button);

    let fullscreen_label = if parent.is_fullscreen() {
        "Exit Fullscreen"
    } else {
        "Enter Fullscreen"
    };
    let fullscreen_button = track_button(fullscreen_label, parent.is_fullscreen());
    let fullscreen_parent = parent.clone();
    let fullscreen_popover = popover.clone();
    fullscreen_button.connect_clicked(move |_| {
        fullscreen_popover.popdown();
        toggle_fullscreen(&fullscreen_parent);
    });
    content.append(&fullscreen_button);

    content.append(&divider());

    let private_button = track_button(
        if private_session {
            "Private Session On"
        } else {
            "Private Session Off"
        },
        private_session,
    );
    let private_state = Rc::clone(&state);
    let private_toast = Rc::clone(&status_toast);
    let private_popover = popover.clone();
    private_button.connect_clicked(move |_| {
        toggle_private_session(&private_state, &private_toast);
        private_popover.popdown();
    });
    content.append(&private_button);

    let clear_history_button = track_button("Clear History...", false);
    let clear_history_parent = parent.clone();
    let clear_history_state = Rc::clone(&state);
    let clear_history_toast = Rc::clone(&status_toast);
    let clear_history_popover = popover.clone();
    clear_history_button.connect_clicked(move |_| {
        clear_history_popover.popdown();
        open_clear_history_dialog(
            &clear_history_parent,
            Rc::clone(&clear_history_state),
            Rc::clone(&clear_history_toast),
        );
    });
    content.append(&clear_history_button);

    content.append(&divider());

    let repeat_button = track_button(repeat_mode.label(), repeat_mode != RepeatMode::Off);
    let repeat_state = Rc::clone(&state);
    let repeat_popover = popover.clone();
    repeat_button.connect_clicked(move |_| {
        cycle_repeat_mode(&repeat_state);
        repeat_popover.popdown();
    });
    content.append(&repeat_button);

    let shuffle_button = track_button(
        if shuffle_enabled {
            "Shuffle On"
        } else {
            "Shuffle Off"
        },
        shuffle_enabled,
    );
    let shuffle_state = Rc::clone(&state);
    let shuffle_popover = popover.clone();
    shuffle_button.connect_clicked(move |_| {
        toggle_shuffle(&shuffle_state);
        shuffle_popover.popdown();
    });
    content.append(&shuffle_button);

    let auto_advance_button = track_button(
        if auto_advance_enabled {
            "Auto-advance On"
        } else {
            "Auto-advance Off"
        },
        auto_advance_enabled,
    );
    let auto_advance_state = Rc::clone(&state);
    let auto_advance_popover = popover.clone();
    auto_advance_button.connect_clicked(move |_| {
        toggle_auto_advance(&auto_advance_state);
        auto_advance_popover.popdown();
    });
    content.append(&auto_advance_button);

    content
}

fn track_popover_content(title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.add_css_class("okp-track-popover-content");
    content.set_width_request(320);

    content.append(&track_section_title(title));
    content
}

fn set_track_popover_child(popover: &gtk::Popover, content: gtk::Box) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.add_css_class("okp-track-popover-scroll");
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_min_content_width(320);
    scroll.set_max_content_height(520);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&content));
    popover.set_child(Some(&scroll));
}

fn track_section_title(title: &str) -> gtk::Label {
    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-track-popover-title");
    title.set_xalign(0.0);
    title
}

fn subtitle_adjustment_rows(
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);

    let (delay_seconds, scale) = read_subtitle_adjustments(state);
    content.append(&subtitle_delay_adjustment_row(
        delay_seconds,
        popover,
        parent,
        state,
    ));
    content.append(&subtitle_adjustment_row(
        "Size",
        &format_scale(scale),
        [
            ("-", SubtitleAdjustment::Scale(-0.1)),
            ("100%", SubtitleAdjustment::SetScale(1.0)),
            ("+", SubtitleAdjustment::Scale(0.1)),
        ],
        popover,
        parent,
        state,
    ));

    content
}

fn subtitle_delay_adjustment_row(
    delay_seconds: f64,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 6);
    row.add_css_class("okp-sub-adjust-row");

    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some("Delay"));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    top.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&format_delay_entry(delay_seconds));
    entry.set_width_chars(8);
    entry.set_placeholder_text(Some("0"));
    top.append(&entry);

    let unit = gtk::Label::new(Some("ms"));
    unit.add_css_class("okp-sub-adjust-unit");
    top.append(&unit);

    let apply_button = gtk::Button::with_label("Apply");
    apply_button.add_css_class("okp-sub-adjust-button");
    top.append(&apply_button);

    let reset_button = gtk::Button::with_label("Reset");
    reset_button.add_css_class("okp-sub-adjust-button");
    top.append(&reset_button);

    row.append(&top);

    let quick = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    quick.set_halign(gtk::Align::End);
    for (text, adjustment) in [
        ("-50", SubtitleAdjustment::Delay(-0.05)),
        ("+50", SubtitleAdjustment::Delay(0.05)),
    ] {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        quick.append(&button);
    }
    row.append(&quick);

    let apply_state = Rc::clone(state);
    let apply_popover = popover.clone();
    let apply_parent = parent.clone();
    let apply_entry = entry.clone();
    apply_button.connect_clicked(move |_| {
        apply_subtitle_delay_entry(
            &apply_entry,
            &apply_popover,
            &apply_parent,
            Rc::clone(&apply_state),
        );
    });

    let activate_state = Rc::clone(state);
    let activate_popover = popover.clone();
    let activate_parent = parent.clone();
    entry.connect_activate(move |entry| {
        apply_subtitle_delay_entry(
            entry,
            &activate_popover,
            &activate_parent,
            Rc::clone(&activate_state),
        );
    });

    let reset_state = Rc::clone(state);
    let reset_popover = popover.clone();
    let reset_parent = parent.clone();
    reset_button.connect_clicked(move |_| {
        apply_subtitle_adjustment(&reset_state, SubtitleAdjustment::SetDelay(0.0));
        populate_subtitle_popover(&reset_popover, &reset_parent, Rc::clone(&reset_state));
    });

    entry.connect_changed(|entry| {
        entry.remove_css_class("is-error");
    });

    row
}

#[derive(Clone, Copy)]
enum SubtitleAdjustment {
    Delay(f64),
    SetDelay(f64),
    Scale(f64),
    SetScale(f64),
}

fn subtitle_adjustment_row(
    title: &str,
    value: &str,
    actions: [(&str, SubtitleAdjustment); 3],
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("okp-sub-adjust-row");

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-sub-adjust-label");
    label.set_xalign(0.0);
    label.set_width_chars(6);
    row.append(&label);

    let value_label = gtk::Label::new(Some(value));
    value_label.add_css_class("okp-sub-adjust-value");
    value_label.set_xalign(1.0);
    value_label.set_width_chars(7);
    row.append(&value_label);

    for (text, adjustment) in actions {
        let button = gtk::Button::with_label(text);
        button.add_css_class("okp-sub-adjust-button");
        let button_state = Rc::clone(state);
        let button_popover = popover.clone();
        let button_parent = parent.clone();
        button.connect_clicked(move |_| {
            apply_subtitle_adjustment(&button_state, adjustment);
            populate_subtitle_popover(&button_popover, &button_parent, Rc::clone(&button_state));
        });
        row.append(&button);
    }

    row
}

fn read_subtitle_adjustments(state: &Rc<RefCell<PlayerState>>) -> (f64, f64) {
    let values = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| {
            (
                mpv.subtitle_delay().unwrap_or(0.0),
                mpv.subtitle_scale().unwrap_or(1.0),
            )
        })
    };

    values.unwrap_or((0.0, 1.0))
}

fn apply_subtitle_adjustment(state: &Rc<RefCell<PlayerState>>, adjustment: SubtitleAdjustment) {
    if with_mpv(state, |mpv| match adjustment {
        SubtitleAdjustment::Delay(delta) => mpv.adjust_subtitle_delay(delta),
        SubtitleAdjustment::SetDelay(value) => mpv.set_subtitle_delay(value),
        SubtitleAdjustment::Scale(delta) => mpv.adjust_subtitle_scale(delta),
        SubtitleAdjustment::SetScale(value) => mpv.set_subtitle_scale(value),
    }) {
        save_current_preferences(state);
    }
}

fn apply_subtitle_delay_entry(
    entry: &gtk::Entry,
    popover: &gtk::Popover,
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let Some(delay_seconds) = parse_delay_entry_seconds(entry.text().as_str()) else {
        entry.add_css_class("is-error");
        entry.grab_focus();
        return;
    };

    apply_subtitle_adjustment(&state, SubtitleAdjustment::SetDelay(delay_seconds));
    populate_subtitle_popover(popover, parent, state);
}

fn parse_delay_entry_seconds(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let seconds = if let Some(value) = lower.strip_suffix("ms") {
        value.trim().parse::<f64>().ok()? / 1000.0
    } else if let Some(value) = lower.strip_suffix('s') {
        value.trim().parse::<f64>().ok()?
    } else {
        lower.parse::<f64>().ok()? / 1000.0
    };

    seconds.is_finite().then(|| seconds.clamp(-600.0, 600.0))
}

fn format_delay_entry(seconds: f64) -> String {
    ((seconds * 1000.0).round() as i64).to_string()
}

fn format_scale(scale: f64) -> String {
    format!("{:.0}%", scale * 100.0)
}

fn read_tracks(state: &Rc<RefCell<PlayerState>>) -> Vec<Track> {
    let tracks = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::tracks)
    };

    match tracks {
        Some(Ok(tracks)) => tracks,
        Some(Err(error)) => {
            eprintln!("Failed to read tracks: {error}");
            Vec::new()
        }
        None => Vec::new(),
    }
}

fn read_secondary_subtitle_id(state: &Rc<RefCell<PlayerState>>) -> Option<i64> {
    let value = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::secondary_subtitle_id)
    };

    match value {
        Some(Ok(value)) => value,
        Some(Err(error)) => {
            eprintln!("Failed to read secondary subtitle track: {error}");
            None
        }
        None => None,
    }
}

fn track_button(text: &str, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-track-row");
    if selected {
        button.add_css_class("is-selected");
    }

    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_ellipsize(pango::EllipsizeMode::End);
    button.set_child(Some(&label));
    button
}

fn empty_track_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("okp-track-empty");
    label.set_xalign(0.0);
    label
}

fn divider() -> gtk::Separator {
    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-track-divider");
    divider
}

fn track_label(track: &Track) -> String {
    track_label_for(track, track.selected)
}

fn track_label_for(track: &Track, selected: bool) -> String {
    let mut parts = Vec::new();
    parts.push(track_base_label(track));

    if track.kind == TrackKind::Audio {
        if let Some(channels) = track.audio_channels.as_deref() {
            parts.push(channels.to_owned());
        }
        if let Some(codec) = track.codec.as_deref() {
            parts.push(codec.to_ascii_uppercase());
        }
    } else if track.external {
        parts.push("EXT".to_owned());
    } else if track.default {
        parts.push("Default".to_owned());
    }

    let label = parts.join(" · ");
    if selected {
        format!("On  {label}")
    } else {
        label
    }
}

fn track_base_label(track: &Track) -> String {
    track
        .title
        .as_deref()
        .or(track.lang.as_deref())
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Track {}", track.id))
}

fn drain_mpv_events(state: &Rc<RefCell<PlayerState>>) {
    let events = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(Mpv::drain_events)
            .unwrap_or_default()
    };

    for event in events {
        match event {
            MpvEvent::FileLoaded => try_pending_playback_preferences(state),
            MpvEvent::EndFile { reason } if reason.is_eof() => {
                if state.borrow().modes.repeat_mode != RepeatMode::One {
                    save_current_progress(state, true);
                }
                advance_playlist_on_eof(state);
            }
            _ => {}
        }
    }
}

fn open_media_dialog(parent: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Open media"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Open", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_media_path(&state, path);
        }
        dialog.close();
    });

    dialog.present();
}

fn open_url_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Open URL")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Open", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Open URL"));

    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://example.com/video.mkv"));
    entry.set_activates_default(true);
    entry.set_width_chars(52);
    content.append(&entry);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            let url = entry.text().trim().to_owned();
            if media_formats::is_playable_url(Some(&url)) {
                load_media_url(&state, url);
            } else {
                status_toast.show("Enter a valid stream URL");
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn open_go_to_time_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let Some((position, duration)) = go_to_time_snapshot(&state) else {
        status_toast.show("Open media first");
        return;
    };

    let dialog = gtk::Dialog::builder()
        .title("Go to Time")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Go", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_end(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.append(&command_dialog_title("Go to Time"));

    let label = gtk::Label::new(Some("Enter a timecode or seconds."));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    content.append(&label);

    let entry = gtk::Entry::new();
    entry.add_css_class("okp-sub-adjust-entry");
    gtk::prelude::EntryExt::set_alignment(&entry, 1.0);
    entry.set_input_purpose(gtk::InputPurpose::Number);
    entry.set_text(&time_code::format(position));
    entry.set_placeholder_text(Some("1:23 or 90"));
    entry.set_activates_default(true);
    entry.set_width_chars(18);
    content.append(&entry);

    let range = if duration.is_finite() && duration > 0.0 {
        format!("Duration {}", time_code::format(duration))
    } else {
        "Duration unknown".to_owned()
    };
    let hint = gtk::Label::new(Some(&range));
    hint.add_css_class("okp-info-label");
    hint.set_xalign(0.0);
    content.append(&hint);

    let focus_entry = entry.clone();
    dialog.connect_response(move |dialog, response| {
        if response != gtk::ResponseType::Accept {
            dialog.close();
            return;
        }

        let text = entry.text();
        let Some(mut target) = time_code::parse(Some(text.as_str())) else {
            entry.add_css_class("is-error");
            status_toast.show("Enter a valid timecode");
            return;
        };

        if let Some((_, duration)) = go_to_time_snapshot(&state) {
            if duration.is_finite() && duration > 0.0 {
                target = target.min(duration);
            }
        } else {
            status_toast.show("Open media first");
            dialog.close();
            return;
        }

        if seek_to_time(&state, target) {
            status_toast.show(&format!("Jumped to {}", time_code::format(target)));
            dialog.close();
        } else {
            status_toast.show("Could not seek");
        }
    });

    dialog.present();
    focus_entry.grab_focus();
    focus_entry.select_region(0, -1);
}

fn go_to_time_snapshot(state: &Rc<RefCell<PlayerState>>) -> Option<(f64, f64)> {
    let state = state.borrow();
    if !has_loaded_media_state(&state) {
        return None;
    }

    let playback = state
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.playback_state().ok())?;
    let position = playback.time_pos.unwrap_or(0.0).max(0.0);
    let duration = playback.duration.unwrap_or(0.0).max(0.0);
    Some((position, duration))
}

fn open_clear_history_dialog(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let dialog = gtk::Dialog::builder()
        .title("Clear History")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.set_decorated(false);
    dialog.add_css_class("okp-command-dialog");
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button("Clear", gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(14);
    content.set_margin_end(14);
    content.set_margin_bottom(14);
    content.set_margin_start(14);
    content.append(&command_dialog_title("Clear History"));

    let message = gtk::Label::new(Some(
        "Clear saved resume positions and per-file playback preferences?",
    ));
    message.set_xalign(0.0);
    message.set_wrap(true);
    content.append(&message);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept {
            clear_history(&state, &status_toast);
        }
        dialog.close();
    });

    dialog.present();
}

fn command_dialog_title(title: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-command-dialog-title");
    label.set_xalign(0.0);
    label
}

fn open_settings_window(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let window = gtk::Window::builder()
        .title("Settings")
        .transient_for(parent)
        .default_width(744)
        .default_height(1030)
        .resizable(false)
        .decorated(false)
        .build();
    window.add_css_class("okp-settings-window");

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("okp-settings-root");

    let stack = gtk::Stack::new();
    stack.add_css_class("okp-settings-stack");
    stack.set_hexpand(true);
    stack.set_vexpand(true);

    let about_page = settings_scroller(&settings_about_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&about_page, Some("about"));

    let updates_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    updates_page.add_css_class("okp-settings-page");
    updates_page.append(&settings_updates_section(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    stack.add_named(&settings_scroller(&updates_page), Some("advanced"));

    let playback_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    playback_page.add_css_class("okp-settings-page");
    let playback = settings_section("Playback");
    playback.append(&settings_volume_row(Rc::clone(&state)));
    playback_page.append(&playback);
    stack.add_named(&settings_scroller(&playback_page), Some("playback"));

    let privacy_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    privacy_page.add_css_class("okp-settings-page");
    let privacy = settings_section("Privacy");
    privacy.append(&settings_private_session_row(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    privacy.append(&settings_clear_history_row(
        parent,
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    privacy_page.append(&privacy);

    let storage = settings_section("Storage");
    let settings_path = state
        .borrow()
        .settings
        .path()
        .to_string_lossy()
        .into_owned();
    storage.append(&settings_value_row("Settings file", &settings_path));
    privacy_page.append(&storage);
    stack.add_named(&settings_scroller(&privacy_page), Some("integration"));

    stack.set_visible_child_name("about");
    root.append(&settings_nav_rail_frame(settings_nav_rail(&stack)));

    let content_overlay = gtk::Overlay::new();
    content_overlay.set_hexpand(true);
    content_overlay.set_vexpand(true);
    content_overlay.set_size_request(552, 1030);
    content_overlay.set_child(Some(&stack));
    content_overlay.add_overlay(&settings_window_controls(&window));
    root.append(&content_overlay);

    let handle = gtk::WindowHandle::new();
    handle.set_child(Some(&root));
    window.set_child(Some(&handle));
    window.present();
}

fn settings_scroller<T: IsA<gtk::Widget>>(child: &T) -> gtk::ScrolledWindow {
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("okp-settings-scroller");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(child));
    scroller
}

fn settings_nav_rail_frame(rail: gtk::Box) -> gtk::ScrolledWindow {
    let frame = gtk::ScrolledWindow::new();
    frame.add_css_class("okp-settings-rail-frame");
    frame.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Never);
    frame.set_min_content_width(191);
    frame.set_max_content_width(191);
    frame.set_propagate_natural_width(false);
    frame.set_size_request(191, 1030);
    frame.set_child(Some(&rail));
    frame
}

#[derive(Clone, Copy)]
enum SettingsNavIcon {
    Appearance,
    Playback,
    Subtitles,
    Video,
    Audio,
    Shortcuts,
    Integration,
    Advanced,
    About,
}

fn settings_nav_rail(stack: &gtk::Stack) -> gtk::Box {
    let rail = gtk::Box::new(gtk::Orientation::Vertical, 2);
    rail.add_css_class("okp-settings-rail");
    rail.set_size_request(191, 1030);

    let title = gtk::Label::new(Some("Settings"));
    title.add_css_class("okp-settings-rail-title");
    title.set_xalign(0.0);
    rail.append(&title);

    let search = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    search.add_css_class("okp-settings-search");
    search.set_size_request(171, 30);
    let search_icon = gtk::Image::from_icon_name("system-search-symbolic");
    search_icon.set_pixel_size(13);
    search.append(&search_icon);
    let search_label = gtk::Label::new(Some("Search"));
    search_label.add_css_class("okp-settings-search-label");
    search_label.set_xalign(0.0);
    search.append(&search_label);
    rail.append(&search);

    let buttons = Rc::new(RefCell::new(Vec::<gtk::Button>::new()));
    let nav_items = [
        ("Appearance", SettingsNavIcon::Appearance, None),
        ("Playback", SettingsNavIcon::Playback, Some("playback")),
        ("Subtitles", SettingsNavIcon::Subtitles, None),
        ("Video", SettingsNavIcon::Video, None),
        ("Audio", SettingsNavIcon::Audio, None),
        ("Shortcuts", SettingsNavIcon::Shortcuts, None),
        (
            "Integration",
            SettingsNavIcon::Integration,
            Some("integration"),
        ),
        ("Advanced", SettingsNavIcon::Advanced, Some("advanced")),
    ];

    for (label, icon, page) in nav_items {
        let row = settings_nav_row(label, icon, false);
        if let Some(page) = page {
            connect_settings_nav_row(&row, page, stack, &buttons);
            buttons.borrow_mut().push(row.clone());
        }
        rail.append(&row);
    }

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    rail.append(&spacer);

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-settings-rail-divider");
    rail.append(&divider);

    let about = settings_nav_row("About", SettingsNavIcon::About, true);
    connect_settings_nav_row(&about, "about", stack, &buttons);
    buttons.borrow_mut().push(about.clone());
    rail.append(&about);

    rail
}

fn settings_window_controls(window: &gtk::Window) -> gtk::Box {
    let controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    controls.add_css_class("okp-settings-window-controls");
    controls.set_halign(gtk::Align::End);
    controls.set_valign(gtk::Align::Start);

    let minimize = settings_window_control("window-minimize-symbolic");
    let minimize_window = window.clone();
    minimize.connect_clicked(move |_| minimize_window.minimize());
    controls.append(&minimize);

    let maximize = settings_window_control("window-maximize-symbolic");
    let maximize_window = window.clone();
    maximize.connect_clicked(move |_| {
        if maximize_window.is_maximized() {
            maximize_window.unmaximize();
        } else {
            maximize_window.maximize();
        }
    });
    controls.append(&maximize);

    let close = settings_window_control("window-close-symbolic");
    let close_window = window.clone();
    close.connect_clicked(move |_| close_window.close());
    controls.append(&close);

    controls
}

fn settings_window_control(icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-window-control");
    button.set_has_frame(false);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(12);
    button.set_child(Some(&icon));
    button
}

fn settings_nav_row(label: &str, icon: SettingsNavIcon, selected: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-settings-nav-row");
    button.set_has_frame(false);
    button.set_size_request(171, 36);
    if selected {
        button.add_css_class("is-selected");
    }

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.set_halign(gtk::Align::Fill);
    content.append(&settings_nav_icon(icon));
    let text = gtk::Label::new(Some(label));
    text.set_xalign(0.0);
    text.set_hexpand(true);
    content.append(&text);
    button.set_child(Some(&content));
    button
}

fn settings_nav_icon(icon: SettingsNavIcon) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class("okp-settings-nav-icon");
    area.set_size_request(16, 16);
    area.set_draw_func(move |area, cr, width, height| {
        draw_settings_nav_icon(area, cr, width, height, icon);
    });
    area
}

fn draw_settings_nav_icon(
    area: &gtk::DrawingArea,
    cr: &cairo::Context,
    width: i32,
    height: i32,
    icon: SettingsNavIcon,
) {
    let color = area.style_context().color();
    let scale = f64::min(width as f64, height as f64) / 16.0;
    let _ = cr.save();
    cr.translate(
        ((width as f64) - (16.0 * scale)) / 2.0,
        ((height as f64) - (16.0 * scale)) / 2.0,
    );
    cr.scale(scale, scale);
    cr.set_source_rgba(
        color.red().into(),
        color.green().into(),
        color.blue().into(),
        color.alpha().into(),
    );
    cr.set_line_width(1.25);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);

    match icon {
        SettingsNavIcon::Appearance => {
            cr.arc(8.0, 8.0, 3.0, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            for index in 0..8 {
                let angle = (index as f64) * std::f64::consts::FRAC_PI_4;
                cr.move_to(8.0 + angle.cos() * 5.2, 8.0 + angle.sin() * 5.2);
                cr.line_to(8.0 + angle.cos() * 6.7, 8.0 + angle.sin() * 6.7);
            }
            let _ = cr.stroke();
        }
        SettingsNavIcon::Playback => {
            cr.move_to(5.25, 3.35);
            cr.line_to(12.3, 8.0);
            cr.line_to(5.25, 12.65);
            cr.close_path();
            let _ = cr.stroke();
        }
        SettingsNavIcon::Subtitles => {
            cairo_rounded_rect(cr, 2.5, 4.0, 11.0, 8.0, 1.2);
            let _ = cr.stroke();
            cr.move_to(5.0, 8.8);
            cr.line_to(7.3, 8.8);
            cr.move_to(8.7, 8.8);
            cr.line_to(11.0, 8.8);
            cr.move_to(5.0, 10.7);
            cr.line_to(10.2, 10.7);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Video => {
            cairo_rounded_rect(cr, 2.5, 3.5, 11.0, 8.2, 1.1);
            let _ = cr.stroke();
            cr.move_to(8.0, 11.7);
            cr.line_to(8.0, 13.2);
            cr.move_to(5.7, 13.2);
            cr.line_to(10.3, 13.2);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Audio => {
            cr.move_to(2.6, 6.2);
            cr.line_to(5.0, 6.2);
            cr.line_to(8.6, 3.7);
            cr.line_to(8.6, 12.3);
            cr.line_to(5.0, 9.8);
            cr.line_to(2.6, 9.8);
            cr.close_path();
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 3.3, -0.72, 0.72);
            let _ = cr.stroke();
            cr.arc(8.7, 8.0, 5.1, -0.62, 0.62);
            let _ = cr.stroke();
        }
        SettingsNavIcon::Shortcuts => {
            cairo_rounded_rect(cr, 2.2, 4.1, 11.6, 7.8, 1.1);
            let _ = cr.stroke();
            for y in [6.7, 9.2] {
                for x in [4.5, 6.8, 9.1, 11.4] {
                    cairo_rounded_rect(cr, x - 0.45, y - 0.35, 0.9, 0.7, 0.2);
                    let _ = cr.fill();
                }
            }
        }
        SettingsNavIcon::Integration => {
            let _ = cr.save();
            cr.translate(8.0, 8.0);
            cr.rotate(-std::f64::consts::FRAC_PI_4);
            cairo_rounded_rect(cr, -6.0, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            cairo_rounded_rect(cr, -1.3, -2.2, 7.3, 4.4, 2.2);
            let _ = cr.stroke();
            let _ = cr.restore();
        }
        SettingsNavIcon::Advanced => {
            cr.move_to(6.6, 2.6);
            cr.curve_to(4.6, 2.6, 5.2, 5.2, 4.0, 6.2);
            cr.curve_to(3.4, 6.8, 3.4, 7.2, 4.0, 7.8);
            cr.curve_to(5.2, 8.8, 4.6, 13.4, 6.6, 13.4);
            cr.move_to(9.4, 2.6);
            cr.curve_to(11.4, 2.6, 10.8, 5.2, 12.0, 6.2);
            cr.curve_to(12.6, 6.8, 12.6, 7.2, 12.0, 7.8);
            cr.curve_to(10.8, 8.8, 11.4, 13.4, 9.4, 13.4);
            let _ = cr.stroke();
        }
        SettingsNavIcon::About => {
            cr.arc(8.0, 8.0, 5.8, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();
            cr.arc(8.0, 5.2, 0.55, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            cr.move_to(8.0, 7.4);
            cr.line_to(8.0, 11.0);
            let _ = cr.stroke();
        }
    }

    let _ = cr.restore();
}

fn cairo_rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let right = x + w;
    let bottom = y + h;
    cr.new_sub_path();
    cr.arc(right - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(right - r, bottom - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        bottom - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

fn connect_settings_nav_row(
    button: &gtk::Button,
    page: &str,
    stack: &gtk::Stack,
    buttons: &Rc<RefCell<Vec<gtk::Button>>>,
) {
    let page = page.to_owned();
    let stack = stack.clone();
    let buttons = Rc::clone(buttons);
    button.connect_clicked(move |button| {
        stack.set_visible_child_name(&page);
        for row in buttons.borrow().iter() {
            row.remove_css_class("is-selected");
        }
        button.add_css_class("is-selected");
    });
}

fn settings_section(title: &str) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 10);
    section.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some(title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    section.append(&title);
    section
}

fn settings_about_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let snapshot = AboutSnapshot::capture(&state);
    let pane = gtk::Box::new(gtk::Orientation::Vertical, 0);
    pane.add_css_class("okp-about-pane");

    pane.append(&about_identity_hero(&snapshot));

    let divider = gtk::Separator::new(gtk::Orientation::Horizontal);
    divider.add_css_class("okp-about-identity-divider");
    pane.append(&divider);

    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 11);
    sheet.add_css_class("okp-about-sheet");
    sheet.append(&about_app_card(&snapshot));
    sheet.append(&about_updates_card(
        Rc::clone(&state),
        Rc::clone(&status_toast),
    ));
    sheet.append(&about_engine_card(&snapshot));
    sheet.append(&about_host_card(&snapshot));
    pane.append(&sheet);

    pane.append(&about_footer(snapshot, status_toast));
    pane
}

#[derive(Clone)]
struct AboutSnapshot {
    version: String,
    channel: String,
    build: String,
    license: String,
    libmpv: String,
    ffmpeg: String,
    render_api: String,
    graphics: String,
    hwdec: String,
    os: String,
    gtk: String,
    cpu: String,
    install: String,
    updates: String,
}

impl AboutSnapshot {
    fn capture(state: &Rc<RefCell<PlayerState>>) -> Self {
        let auto_updates = state.borrow().settings.auto_check_updates();
        Self {
            version: APP_BUILD_VERSION.to_owned(),
            channel: "Linux".to_owned(),
            build: APP_BUILD_SHA.to_owned(),
            license: "GPL-3.0-or-later".to_owned(),
            libmpv: pkg_config_version("mpv").unwrap_or_else(|| "system".to_owned()),
            ffmpeg: ffmpeg_version().unwrap_or_else(|| "system".to_owned()),
            render_api: "libmpv render".to_owned(),
            graphics: "OpenGL · GTK GLArea".to_owned(),
            hwdec: "off".to_owned(),
            os: linux_os_label(),
            gtk: format!(
                "{}.{}.{}",
                gtk::major_version(),
                gtk::minor_version(),
                gtk::micro_version()
            ),
            cpu: env::consts::ARCH.to_owned(),
            install: linux_update_install_status().to_owned(),
            updates: if auto_updates {
                "Automatic".to_owned()
            } else {
                "Manual".to_owned()
            },
        }
    }
}

fn about_identity_hero(snapshot: &AboutSnapshot) -> gtk::Box {
    let hero = gtk::Box::new(gtk::Orientation::Horizontal, 22);
    hero.add_css_class("okp-about-identity");

    let illustration = gtk::Box::new(gtk::Orientation::Vertical, 0);
    illustration.add_css_class("okp-about-illustration");
    illustration.set_halign(gtk::Align::Center);
    illustration.set_valign(gtk::Align::Center);
    illustration.append(&about_illustration());
    hero.append(&illustration);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.set_hexpand(true);

    let wordmark = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    wordmark.add_css_class("okp-about-wordmark");
    let ok = gtk::Label::new(Some("OK"));
    ok.add_css_class("okp-about-wordmark-ok");
    let player = gtk::Label::new(Some(" Player"));
    player.add_css_class("okp-about-wordmark-player");
    wordmark.append(&ok);
    wordmark.append(&player);
    text.append(&wordmark);

    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chips.add_css_class("okp-about-chip-row");
    let version = gtk::Label::new(Some(&snapshot.version));
    version.add_css_class("okp-about-version-chip");
    chips.append(&version);
    let channel = gtk::Label::new(Some(&snapshot.channel));
    channel.add_css_class("okp-about-channel-chip");
    chips.append(&channel);
    text.append(&chips);

    let tagline = gtk::Label::new(Some("The most elegant media player on Linux."));
    tagline.add_css_class("okp-about-tagline");
    tagline.set_xalign(0.0);
    text.append(&tagline);

    let byline = gtk::Label::new(Some("Open source · by Oleg Kossoy"));
    byline.add_css_class("okp-about-byline");
    byline.set_xalign(0.0);
    text.append(&byline);

    hero.append(&text);
    hero
}

fn about_illustration() -> gtk::Widget {
    if let Some(path) = about_illustration_path() {
        let image = gtk::Image::from_file(path);
        image.set_size_request(116, 90);
        image.set_pixel_size(116);
        return image.upcast();
    }

    let image = gtk::Image::from_icon_name("com.befeast.okplayer");
    image.set_size_request(116, 90);
    image.set_pixel_size(90);
    image.upcast()
}

fn about_illustration_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(
        "/usr/share/ok-player/com.befeast.okplayer.about.svg",
    ));
    if let Ok(exe) = env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("com.befeast.okplayer.about.svg"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/linux/com.befeast.okplayer.about.svg"),
    );

    candidates.into_iter().find(|path| path.is_file())
}

fn about_app_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    rows.append(&about_spec_row("Version", &snapshot.version, true, None));
    rows.append(&about_spec_row("Channel", &snapshot.channel, false, None));
    rows.append(&about_spec_row(
        "Build",
        &snapshot.build,
        true,
        Some(("CURRENT", false)),
    ));
    rows.append(&about_spec_row("License", &snapshot.license, true, None));
    about_card("APP", &rows)
}

fn about_engine_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 9);
    rows.append(&about_spec_row("libmpv", &snapshot.libmpv, true, None));
    rows.append(&about_spec_row(
        "FFmpeg",
        &snapshot.ffmpeg,
        true,
        Some(("SYSTEM", false)),
    ));
    rows.append(&about_spec_row(
        "Render API",
        &snapshot.render_api,
        true,
        None,
    ));
    rows.append(&about_spec_row("Graphics", &snapshot.graphics, true, None));
    rows.append(&about_spec_row(
        "Hardware decode",
        &snapshot.hwdec,
        false,
        Some(("OFF", false)),
    ));
    about_card("ENGINE", &rows)
}

fn about_updates_card(state: Rc<RefCell<PlayerState>>, status_toast: Rc<StatusToast>) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);

    let status_row = about_spec_row("Status", "Up to date", false, None);
    let status_label = status_row
        .last_child()
        .and_then(|wrap| wrap.first_child())
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        .unwrap_or_else(|| gtk::Label::new(Some("Not checked")));
    content.append(&status_row);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    auto_row.add_css_class("okp-about-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Check automatically"));
    auto_label.add_css_class("okp-about-row-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some("On launch"));
    auto_detail.add_css_class("okp-about-row-detail");
    auto_detail.set_xalign(0.0);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let auto_switch = about_toggle_button(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    auto_switch.connect_clicked(move |button| {
        let enabled = !button.has_css_class("is-active");
        if enabled {
            button.add_css_class("is-active");
        } else {
            button.remove_css_class("is-active");
        }
        if let Some(knob) = button.first_child() {
            knob.set_halign(if enabled {
                gtk::Align::End
            } else {
                gtk::Align::Start
            });
        }
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
    });
    auto_row.append(&auto_switch);
    content.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    actions.set_halign(gtk::Align::Start);
    let pending_update = Rc::new(RefCell::new(None::<PendingLinuxUpdate>));
    let check_button = gtk::Button::with_label("Check for updates");
    check_button.add_css_class("okp-about-check-button");
    check_button.set_has_frame(false);
    check_button.set_size_request(132, 34);
    let check_status = status_label.clone();
    let check_pending = Rc::clone(&pending_update);
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |button| {
        if let Some(update) = check_pending.borrow().clone() {
            start_update_download(
                button,
                &check_status,
                update,
                Rc::clone(&check_state),
                Rc::clone(&check_toast),
            );
            return;
        }

        button.set_sensitive(false);
        button.set_label("Checking...");
        check_status.set_text("Checking");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(check_for_linux_update());
        });

        let button = button.clone();
        let status = check_status.clone();
        let pending = Rc::clone(&check_pending);
        let toast = Rc::clone(&check_toast);
        glib::timeout_add_local(Duration::from_millis(120), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    apply_update_check_result(&button, &status, &pending, &toast, result);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    button.set_label("Check for updates");
                    status.set_text("Failed");
                    glib::ControlFlow::Break
                }
            }
        });
    });
    actions.append(&check_button);
    content.append(&actions);

    about_card("UPDATES", &content)
}

fn about_host_card(snapshot: &AboutSnapshot) -> gtk::Box {
    let grid = gtk::Grid::new();
    grid.add_css_class("okp-about-host-grid");
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(26);
    grid.set_row_spacing(8);
    grid.attach(
        &about_spec_row("Linux", &snapshot.os, true, None),
        0,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("GTK", &snapshot.gtk, true, None),
        1,
        0,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("CPU", &snapshot.cpu, true, None),
        0,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Install", &snapshot.install, false, None),
        1,
        1,
        1,
        1,
    );
    grid.attach(
        &about_spec_row("Updates", &snapshot.updates, false, Some(("ON", true))),
        0,
        2,
        1,
        1,
    );
    about_card("HOST", &grid)
}

fn about_toggle_button(active: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-toggle");
    button.set_has_frame(false);
    if active {
        button.add_css_class("is-active");
    }

    let knob = gtk::Box::new(gtk::Orientation::Vertical, 0);
    knob.add_css_class("okp-about-toggle-knob");
    knob.set_halign(if active {
        gtk::Align::End
    } else {
        gtk::Align::Start
    });
    knob.set_valign(gtk::Align::Center);
    button.set_child(Some(&knob));
    button
}

fn about_card<T: IsA<gtk::Widget>>(title: &str, content: &T) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("okp-about-card");
    match title {
        "APP" => {
            card.add_css_class("okp-about-card-app");
            card.set_size_request(-1, 151);
        }
        "UPDATES" => {
            card.add_css_class("okp-about-card-updates");
            card.set_size_request(-1, 164);
        }
        "ENGINE" => {
            card.add_css_class("okp-about-card-engine");
            card.set_size_request(-1, 176);
        }
        "HOST" => {
            card.add_css_class("okp-about-card-host");
            card.set_size_request(-1, 125);
        }
        _ => {}
    }

    let label = gtk::Label::new(Some(title));
    label.add_css_class("okp-about-card-title");
    label.set_xalign(0.0);
    card.append(&label);
    card.append(content);
    card
}

fn about_spec_row(label: &str, value: &str, mono: bool, tag: Option<(&str, bool)>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    row.add_css_class("okp-about-row");
    row.set_hexpand(true);

    let key = gtk::Label::new(Some(label));
    key.add_css_class("okp-about-row-label");
    key.set_xalign(0.0);
    key.set_hexpand(true);
    row.append(&key);

    let value_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    value_wrap.set_halign(gtk::Align::End);

    let val = gtk::Label::new(Some(value));
    val.add_css_class(if mono {
        "okp-about-row-value-mono"
    } else {
        "okp-about-row-value"
    });
    val.set_xalign(1.0);
    val.set_ellipsize(pango::EllipsizeMode::End);
    val.set_selectable(true);
    value_wrap.append(&val);

    if let Some((text, accent)) = tag {
        let tag = gtk::Label::new(Some(text));
        tag.add_css_class("okp-about-tag");
        if accent {
            tag.add_css_class("is-accent");
        }
        value_wrap.append(&tag);
    }

    row.append(&value_wrap);
    row
}

fn about_footer(snapshot: AboutSnapshot, status_toast: Rc<StatusToast>) -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer.add_css_class("okp-about-footer");

    let copy = about_action_button("Copy diagnostics", "edit-copy-symbolic");
    let copy_snapshot = snapshot.clone();
    copy.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display
                .clipboard()
                .set_text(&about_diagnostics_text(&copy_snapshot));
        }
        status_toast.show("Diagnostics copied");
    });
    footer.append(&copy);

    let links = gtk::Box::new(gtk::Orientation::Horizontal, 13);
    links.set_halign(gtk::Align::End);
    links.set_hexpand(true);

    let github = about_link_button("GitHub");
    github.connect_clicked(|_| open_external_url("https://github.com/BeFeast/ok-player"));
    links.append(&github);

    let dot = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dot.add_css_class("okp-about-link-dot");
    links.append(&dot);

    let license = about_link_button("License");
    license.connect_clicked(|_| {
        open_external_url("https://github.com/BeFeast/ok-player/blob/main/LICENSE")
    });
    links.append(&license);
    footer.append(&links);

    footer
}

fn about_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-copy-button");
    button.set_has_frame(false);
    button.set_size_request(147, 34);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));
    button
}

fn about_link_button(label: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.add_css_class("okp-about-link-button");
    button.set_has_frame(false);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    content.append(&gtk::Label::new(Some(label)));
    let icon = gtk::Image::from_icon_name("open-in-new-symbolic");
    icon.set_pixel_size(12);
    content.append(&icon);
    button.set_child(Some(&content));
    button
}

fn about_diagnostics_text(snapshot: &AboutSnapshot) -> String {
    format!(
        "OK Player {} ({})\nBuild {} - current\nLicense {}\n\nEngine\n  libmpv           {}\n  FFmpeg           {}\n  Render API       {}\n  Graphics         {}\n  Hardware decode  {}\n\nHost\n  Linux            {}\n  GTK              {}\n  CPU              {}\n  Install          {}\n  Updates          {}",
        snapshot.version,
        snapshot.channel,
        snapshot.build,
        snapshot.license,
        snapshot.libmpv,
        snapshot.ffmpeg,
        snapshot.render_api,
        snapshot.graphics,
        snapshot.hwdec,
        snapshot.os,
        snapshot.gtk,
        snapshot.cpu,
        snapshot.install,
        snapshot.updates
    )
}

fn pkg_config_version(package: &str) -> Option<String> {
    Command::new("pkg-config")
        .args(["--modversion", package])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn ffmpeg_version() -> Option<String> {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| {
            output
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(str::to_owned)
        })
}

fn linux_os_label() -> String {
    if let Ok(os_release) = fs::read_to_string("/etc/os-release")
        && let Some(pretty_name) = os_release.lines().find_map(|line| {
            line.strip_prefix("PRETTY_NAME=")
                .map(|value| value.trim_matches('"').to_owned())
        })
        && !pretty_name.is_empty()
    {
        return pretty_name;
    }

    Command::new("uname")
        .arg("-sr")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Linux".to_owned())
}

fn linux_update_install_status() -> &'static str {
    if linux_update_manager().is_ok() {
        "Self-update enabled"
    } else {
        "Deb installer"
    }
}

fn settings_updates_section(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let section = settings_section("Updates");
    section.append(&settings_value_row("Current version", APP_BUILD_VERSION));
    section.append(&settings_value_row("Channel", "linux"));
    section.append(&settings_value_row("Feed", "GitHub Releases"));
    section.append(&settings_value_row(
        "Install",
        linux_update_install_status(),
    ));

    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let auto_check_enabled = state.borrow().settings.auto_check_updates();
    let status = gtk::Label::new(Some(update_status_intro(auto_check_enabled)));
    status.add_css_class("okp-update-status");
    status.set_xalign(0.0);
    status.set_wrap(true);
    row.append(&status);

    let auto_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    auto_row.add_css_class("okp-settings-switch-row");
    let auto_text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    auto_text.set_hexpand(true);
    let auto_label = gtk::Label::new(Some("Automatic checks"));
    auto_label.add_css_class("okp-info-label");
    auto_label.set_xalign(0.0);
    auto_text.append(&auto_label);
    let auto_detail = gtk::Label::new(Some(
        "Check the linux pre-release feed on startup and show a toast when an update is ready.",
    ));
    auto_detail.add_css_class("okp-update-status");
    auto_detail.set_xalign(0.0);
    auto_detail.set_wrap(true);
    auto_text.append(&auto_detail);
    auto_row.append(&auto_text);

    let auto_state_label = gtk::Label::new(Some(if auto_check_enabled { "On" } else { "Off" }));
    auto_state_label.add_css_class("okp-settings-state-pill");
    auto_row.append(&auto_state_label);

    let auto_switch = gtk::Switch::new();
    auto_switch.set_active(auto_check_enabled);
    let auto_state = Rc::clone(&state);
    let auto_toast = Rc::clone(&status_toast);
    let auto_status = status.clone();
    let auto_state_text = auto_state_label.clone();
    auto_switch.connect_state_set(move |_, enabled| {
        {
            let mut state = auto_state.borrow_mut();
            state.settings.set_auto_check_updates(enabled);
            if let Err(error) = state.settings.save() {
                eprintln!("Failed to save update settings: {error}");
                auto_toast.show("Could not save update setting");
            }
        }
        auto_status.set_text(update_status_intro(enabled));
        auto_state_text.set_text(if enabled { "On" } else { "Off" });
        auto_toast.show(if enabled {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        });
        glib::Propagation::Proceed
    });
    auto_row.append(&auto_switch);
    row.append(&auto_row);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);

    let pending_update = Rc::new(RefCell::new(None::<PendingLinuxUpdate>));

    let check_button = gtk::Button::with_label("Check for Updates");
    check_button.add_css_class("okp-settings-button");
    let check_status = status.clone();
    let check_pending = Rc::clone(&pending_update);
    let check_state = Rc::clone(&state);
    let check_toast = Rc::clone(&status_toast);
    check_button.connect_clicked(move |button| {
        if let Some(update) = check_pending.borrow().clone() {
            start_update_download(
                button,
                &check_status,
                update,
                Rc::clone(&check_state),
                Rc::clone(&check_toast),
            );
            return;
        }

        button.set_sensitive(false);
        button.set_label("Checking...");
        check_status.set_text("Checking GitHub Releases...");

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(check_for_linux_update());
        });

        let button = button.clone();
        let status = check_status.clone();
        let pending = Rc::clone(&check_pending);
        let toast = Rc::clone(&check_toast);
        glib::timeout_add_local(Duration::from_millis(120), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    apply_update_check_result(&button, &status, &pending, &toast, result);
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    button.set_sensitive(true);
                    button.set_label("Check for Updates");
                    status.set_text("Update check failed.");
                    glib::ControlFlow::Break
                }
            }
        });
    });
    actions.append(&check_button);

    let releases_button = gtk::Button::with_label("Open Releases");
    releases_button.add_css_class("okp-settings-button");
    releases_button.connect_clicked(move |_| {
        open_external_url("https://github.com/BeFeast/ok-player/releases")
    });
    actions.append(&releases_button);

    row.append(&actions);
    section.append(&row);

    section
}

fn update_status_intro(auto_check_enabled: bool) -> &'static str {
    if auto_check_enabled {
        "Automatic update checks are on. AppImage installs restart in place; .deb installs download the newest installer inside OK Player."
    } else {
        "Automatic update checks are off. Use Check for Updates any time."
    }
}

fn start_update_download(
    button: &gtk::Button,
    status: &gtk::Label,
    update: PendingLinuxUpdate,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    save_current_progress(&state, false);
    button.set_sensitive(false);
    button.set_label("Downloading...");
    status.set_text(&format!(
        "Downloading {}...",
        update
            .target_version()
            .unwrap_or_else(|| "update".to_owned())
    ));
    status_toast.show("Downloading update");

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(download_and_apply_linux_update(update));
    });

    let button = button.clone();
    let status = status.clone();
    let toast = Rc::clone(&status_toast);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(Ok(LinuxUpdateApplyResult::Restarting)) => {
                button.set_label("Restarting...");
                status.set_text("Restarting to apply update...");
                glib::ControlFlow::Break
            }
            Ok(Ok(LinuxUpdateApplyResult::InstallerOpened(path))) => {
                button.set_sensitive(true);
                button.set_label("Check for Updates");
                status.set_text(&format!(
                    "Downloaded {}. Complete the installer to update.",
                    display_file_name(&path)
                ));
                toast.show("Installer opened");
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text(&format!("Update failed: {error}"));
                toast.show("Update failed");
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                button.set_sensitive(true);
                button.set_label("Check for updates");
                status.set_text("Update failed.");
                glib::ControlFlow::Break
            }
        }
    });
}

fn apply_update_check_result(
    button: &gtk::Button,
    status: &gtk::Label,
    pending: &Rc<RefCell<Option<PendingLinuxUpdate>>>,
    status_toast: &StatusToast,
    result: LinuxUpdateCheckResult,
) {
    button.set_sensitive(true);
    match result {
        LinuxUpdateCheckResult::UpToDate => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text("Up to date");
            status_toast.show("OK Player is up to date");
        }
        LinuxUpdateCheckResult::Available(update) => {
            let status_text = update.available_status();
            let action_label = update.action_label();
            pending.borrow_mut().replace(update);
            button.set_label(action_label);
            status.set_text(&status_text);
            status_toast.show("Update available");
        }
        LinuxUpdateCheckResult::Failed(error) => {
            pending.borrow_mut().take();
            button.set_label("Check for updates");
            status.set_text(&format!("Update check failed: {error}"));
            status_toast.show("Update check failed");
        }
    }
}

fn check_updates_on_startup(status_toast: Rc<StatusToast>) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(check_for_linux_update());
    });

    glib::timeout_add_local(Duration::from_millis(500), move || {
        match receiver.try_recv() {
            Ok(LinuxUpdateCheckResult::Available(update)) => {
                let version = update
                    .target_version()
                    .unwrap_or_else(|| "new version".to_owned());
                status_toast.show(&format!("Update available: {version}"));
                glib::ControlFlow::Break
            }
            Ok(LinuxUpdateCheckResult::Failed(error)) => {
                eprintln!("Startup update check failed: {error}");
                glib::ControlFlow::Break
            }
            Ok(_) => glib::ControlFlow::Break,
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn check_for_linux_update() -> LinuxUpdateCheckResult {
    let manager = match linux_update_manager() {
        Ok(manager) => manager,
        Err(manager_error) => {
            return match check_for_linux_deb_update() {
                Ok(Some(update)) => LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                    manager: None,
                    target: LinuxUpdateTarget::Deb(update),
                }),
                Ok(None) => LinuxUpdateCheckResult::UpToDate,
                Err(deb_error) => {
                    LinuxUpdateCheckResult::Failed(format!("{manager_error}; {deb_error}"))
                }
            };
        }
    };

    if let Some(asset) = manager.get_update_pending_restart() {
        return LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
            manager: Some(manager),
            target: LinuxUpdateTarget::Asset(Box::new(asset)),
        });
    }

    match manager.check_for_updates() {
        Ok(UpdateCheck::UpdateAvailable(update)) => {
            LinuxUpdateCheckResult::Available(PendingLinuxUpdate {
                manager: Some(manager),
                target: LinuxUpdateTarget::Info(update),
            })
        }
        Ok(UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty) => {
            LinuxUpdateCheckResult::UpToDate
        }
        Err(error) => LinuxUpdateCheckResult::Failed(error.to_string()),
    }
}

fn linux_update_manager() -> Result<UpdateManager, String> {
    let source = GithubSource::new(LINUX_UPDATE_REPO_URL, None, true);
    let options = UpdateOptions {
        ExplicitChannel: Some("linux".to_owned()),
        ..Default::default()
    };
    UpdateManager::new(source, Some(options), None).map_err(|error| match error {
        velopack::Error::NotInstalled(_) => "This install cannot self-update.".to_owned(),
        other => other.to_string(),
    })
}

fn download_and_apply_linux_update(
    update: PendingLinuxUpdate,
) -> Result<LinuxUpdateApplyResult, String> {
    match update.target {
        LinuxUpdateTarget::Info(info) => {
            let info = info.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .download_updates(info, None)
                .map_err(|error| error.to_string())?;
            manager
                .apply_updates_and_restart(info)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Asset(asset) => {
            let asset = asset.as_ref();
            let manager = update
                .manager
                .as_ref()
                .ok_or_else(|| "Self-update manager unavailable.".to_owned())?;
            manager
                .apply_updates_and_restart(asset)
                .map_err(|error| error.to_string())?;
            Ok(LinuxUpdateApplyResult::Restarting)
        }
        LinuxUpdateTarget::Deb(update) => {
            let path = download_deb_update(update)?;
            Ok(LinuxUpdateApplyResult::InstallerOpened(path))
        }
    }
}

impl PendingLinuxUpdate {
    fn target_version(&self) -> Option<String> {
        match &self.target {
            LinuxUpdateTarget::Info(info) => Some(info.TargetFullRelease.Version.clone()),
            LinuxUpdateTarget::Asset(asset) => Some(asset.Version.clone()),
            LinuxUpdateTarget::Deb(update) => Some(update.version.clone()),
        }
    }

    fn action_label(&self) -> &'static str {
        match &self.target {
            LinuxUpdateTarget::Info(_) | LinuxUpdateTarget::Asset(_) => "Download and Restart",
            LinuxUpdateTarget::Deb(_) => "Download .deb",
        }
    }

    fn available_status(&self) -> String {
        match &self.target {
            LinuxUpdateTarget::Info(_) | LinuxUpdateTarget::Asset(_) => format!(
                "{} is available.",
                self.target_version()
                    .unwrap_or_else(|| "A new version".to_owned())
            ),
            LinuxUpdateTarget::Deb(update) => {
                format!("{} is available as a .deb installer.", update.version)
            }
        }
    }
}

fn check_for_linux_deb_update() -> Result<Option<ManualDebUpdate>, String> {
    let url = linux_deb_releases_url();
    let mut response = ureq::get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("GitHub .deb update check failed: {error}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| format!("GitHub .deb update check failed: {error}"))?;
    let releases: Vec<GitHubRelease> = serde_json::from_str(&body)
        .map_err(|error| format!("GitHub .deb update feed was invalid: {error}"))?;

    Ok(select_latest_linux_deb_update(releases, APP_BUILD_VERSION))
}

fn linux_deb_releases_url() -> String {
    env::var("OKP_LINUX_DEB_RELEASES_URL").unwrap_or_else(|_| LINUX_DEB_RELEASES_API_URL.to_owned())
}

fn select_latest_linux_deb_update(
    releases: Vec<GitHubRelease>,
    current_version: &str,
) -> Option<ManualDebUpdate> {
    let mut best = None::<ManualDebUpdate>;
    for release in releases {
        if release.draft || !release.prerelease {
            continue;
        }
        let version = release
            .tag_name
            .strip_prefix("linux-v")
            .unwrap_or(&release.tag_name)
            .to_owned();
        if compare_linux_versions(&version, current_version) != std::cmp::Ordering::Greater {
            continue;
        }
        let Some(asset) = release.assets.into_iter().find(|asset| {
            asset.name.starts_with("ok-player_") && asset.name.ends_with("_amd64.deb")
        }) else {
            continue;
        };
        let candidate = ManualDebUpdate {
            version,
            name: asset.name,
            url: asset.browser_download_url,
            size: asset.size,
        };
        if best.as_ref().is_none_or(|current| {
            compare_linux_versions(&candidate.version, &current.version)
                == std::cmp::Ordering::Greater
        }) {
            best = Some(candidate);
        }
    }

    best
}

fn download_deb_update(update: ManualDebUpdate) -> Result<PathBuf, String> {
    let cache_dir = linux_update_cache_dir();
    fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Could not create update cache: {error}"))?;
    let target = cache_dir.join(&update.name);
    let temp = cache_dir.join(format!("{}.part", update.name));

    let mut response = ureq::get(&update.url)
        .header("User-Agent", "OK Player Linux")
        .call()
        .map_err(|error| format!("Download failed: {error}"))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| format!("Download failed: {error}"))?;
    if let Some(expected) = update.size
        && expected > 0
        && bytes.len() as u64 != expected
    {
        return Err(format!(
            "Download size mismatch: expected {expected} bytes, got {}.",
            bytes.len()
        ));
    }

    fs::write(&temp, bytes).map_err(|error| format!("Could not save update: {error}"))?;
    fs::rename(&temp, &target).map_err(|error| format!("Could not finalize update: {error}"))?;
    open_deb_installer(&target)?;
    Ok(target)
}

fn linux_update_cache_dir() -> PathBuf {
    if let Some(cache_dir) =
        env::var_os("OKP_LINUX_UPDATE_CACHE_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(cache_dir);
    }
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache_home).join("ok-player/updates");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home).join(".cache/ok-player/updates");
    }
    env::temp_dir().join("ok-player/updates")
}

fn open_deb_installer(path: &Path) -> Result<(), String> {
    if env::var_os("OKP_SKIP_OPEN_INSTALLER").is_some() {
        return Ok(());
    }

    Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map_err(|error| {
            format!(
                "Downloaded to {}, but could not open installer: {error}",
                path.display()
            )
        })?;
    Ok(())
}

fn compare_linux_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_key = linux_version_sort_key(left);
    let right_key = linux_version_sort_key(right);
    let max_len = left_key.len().max(right_key.len());
    for index in 0..max_len {
        let left_part = left_key.get(index).copied().unwrap_or_default();
        let right_part = right_key.get(index).copied().unwrap_or_default();
        match left_part.cmp(&right_part) {
            std::cmp::Ordering::Equal => {}
            order => return order,
        }
    }
    left.cmp(right)
}

fn linux_version_sort_key(version: &str) -> Vec<u64> {
    let mut key = Vec::new();
    let mut current = String::new();
    for character in version.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            key.push(current.parse().unwrap_or_default());
            current.clear();
        }
    }
    if !current.is_empty() {
        key.push(current.parse().unwrap_or_default());
    }
    key
}

fn open_external_url(url: &str) {
    if let Err(error) = Command::new("xdg-open").arg(url).spawn() {
        eprintln!("Failed to open {url}: {error}");
    }
}

fn settings_volume_row(state: Rc<RefCell<PlayerState>>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 8);
    row.add_css_class("okp-settings-row");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let label = gtk::Label::new(Some("Volume"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);

    let current_volume = state.borrow().settings.volume();
    let value = gtk::Label::new(Some(&format!("{current_volume:.0}%")));
    value.add_css_class("okp-info-value");
    value.set_xalign(1.0);
    header.append(&label);
    header.append(&value);
    row.append(&header);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    scale.set_draw_value(false);
    scale.set_value(current_volume);
    scale.add_css_class("okp-settings-scale");

    let value_label = value.clone();
    scale.connect_change_value(move |_, _, volume| {
        value_label.set_text(&format!("{volume:.0}%"));
        set_volume_from_ui(&state, volume);
        glib::Propagation::Proceed
    });
    row.append(&scale);

    row
}

fn settings_private_session_row(
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("Private Session"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let private_session = state.borrow().private_session;
    let button = gtk::Button::with_label(if private_session { "On" } else { "Off" });
    button.add_css_class("okp-settings-button");
    button.connect_clicked(move |button| {
        toggle_private_session(&state, &status_toast);
        let private_session = state.borrow().private_session;
        button.set_label(if private_session { "On" } else { "Off" });
    });
    row.append(&button);

    row
}

fn settings_clear_history_row(
    parent: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some("History"));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_hexpand(true);
    row.append(&label);

    let button = gtk::Button::with_label("Clear...");
    button.add_css_class("okp-settings-button");
    let parent = parent.clone();
    button.connect_clicked(move |_| {
        open_clear_history_dialog(&parent, Rc::clone(&state), Rc::clone(&status_toast));
    });
    row.append(&button);

    row
}

fn settings_value_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-settings-row");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-label");
    label.set_xalign(0.0);
    label.set_width_chars(14);
    row.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-value");
    value.set_xalign(0.0);
    value.set_hexpand(true);
    value.set_ellipsize(pango::EllipsizeMode::Middle);
    value.set_selectable(true);
    row.append(&value);

    row
}

fn open_subtitle_dialog(parent: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Add subtitle"),
        Some(parent),
        gtk::FileChooserAction::Open,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Add", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);

    dialog.connect_response(move |dialog, response| {
        if response == gtk::ResponseType::Accept
            && let Some(path) = dialog.file().and_then(|file| file.path())
        {
            load_subtitle_path(&state, path);
        }
        dialog.close();
    });

    dialog.present();
}

fn connect_drop(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    empty_surface: EmptySurface,
) {
    let drop_target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    let enter_surface = empty_surface.clone();
    drop_target.connect_enter(move |_, _, _| {
        enter_surface.set_drop_active(true);
        gdk::DragAction::COPY
    });
    let leave_surface = empty_surface.clone();
    drop_target.connect_leave(move |_| {
        leave_surface.set_drop_active(false);
    });
    let drop_surface = empty_surface;
    drop_target.connect_drop(move |_, value, _, _| {
        drop_surface.set_drop_active(false);
        let Ok(files) = value.get::<gdk::FileList>() else {
            return false;
        };

        let Some(path) = files.files().into_iter().find_map(|file| file.path()) else {
            return false;
        };

        if is_subtitle_path(&path) {
            return load_subtitle_path(&state, path);
        }

        if is_media_path(&path) {
            load_media_path(&state, path);
            true
        } else {
            false
        }
    });
    window.add_controller(drop_target);
}

fn connect_keyboard(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) {
    let controller = gtk::EventControllerKey::new();
    let shortcut_window = window.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        chrome.show_for_activity();

        if modifiers.contains(gdk::ModifierType::CONTROL_MASK)
            && !modifiers.intersects(gdk::ModifierType::ALT_MASK)
            && key == gdk::Key::comma
        {
            open_settings_window(
                &shortcut_window,
                Rc::clone(&state),
                Rc::clone(&status_toast),
            );
            return glib::Propagation::Stop;
        }

        if modifiers.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::ALT_MASK) {
            return glib::Propagation::Proceed;
        }

        match key {
            gdk::Key::space => {
                with_mpv(&state, |mpv| mpv.cycle_pause());
                glib::Propagation::Stop
            }
            gdk::Key::Left => {
                with_mpv(&state, |mpv| mpv.seek_relative(-5.0));
                glib::Propagation::Stop
            }
            gdk::Key::Right => {
                with_mpv(&state, |mpv| mpv.seek_relative(5.0));
                glib::Propagation::Stop
            }
            gdk::Key::period => {
                with_mpv(&state, |mpv| mpv.frame_step());
                glib::Propagation::Stop
            }
            gdk::Key::comma => {
                with_mpv(&state, |mpv| mpv.frame_back_step());
                glib::Propagation::Stop
            }
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => {
                navigate_playlist(&state, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => {
                navigate_playlist(&state, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Down => {
                adjust_volume(&state, -5.0);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                adjust_volume(&state, 5.0);
                glib::Propagation::Stop
            }
            gdk::Key::o | gdk::Key::O => {
                open_media_dialog(&shortcut_window, Rc::clone(&state));
                glib::Propagation::Stop
            }
            gdk::Key::s | gdk::Key::S => {
                open_subtitle_dialog(&shortcut_window, Rc::clone(&state));
                glib::Propagation::Stop
            }
            gdk::Key::u | gdk::Key::U => {
                open_url_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            gdk::Key::x | gdk::Key::X => {
                close_current_media(&state, &status_toast);
                glib::Propagation::Stop
            }
            gdk::Key::c | gdk::Key::C => {
                take_screenshot(&state, &status_toast);
                glib::Propagation::Stop
            }
            gdk::Key::i | gdk::Key::I => {
                open_media_info_window(&shortcut_window, &state, Rc::clone(&status_toast));
                glib::Propagation::Stop
            }
            gdk::Key::j | gdk::Key::J => {
                open_go_to_time_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            gdk::Key::z => {
                adjust_subtitle_delay(&state, 0.05);
                glib::Propagation::Stop
            }
            gdk::Key::Z => {
                adjust_subtitle_delay(&state, -0.05);
                glib::Propagation::Stop
            }
            gdk::Key::bracketleft => {
                adjust_subtitle_scale(&state, -0.1);
                glib::Propagation::Stop
            }
            gdk::Key::bracketright => {
                adjust_subtitle_scale(&state, 0.1);
                glib::Propagation::Stop
            }
            gdk::Key::f | gdk::Key::F => {
                toggle_fullscreen(&shortcut_window);
                glib::Propagation::Stop
            }
            gdk::Key::Escape if shortcut_window.is_fullscreen() => {
                shortcut_window.unfullscreen();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);
}

fn connect_progress_persistence(window: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let timer_state = Rc::clone(&state);
    glib::timeout_add_local(Duration::from_secs(10), move || {
        save_current_progress(&timer_state, false);
        glib::ControlFlow::Continue
    });

    let close_state = Rc::clone(&state);
    window.connect_close_request(move |_| {
        save_current_progress(&close_state, false);
        glib::Propagation::Proceed
    });
}

fn with_mpv(
    state: &Rc<RefCell<PlayerState>>,
    command: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) -> bool {
    if let Some(mpv) = state.borrow().mpv.as_ref()
        && let Err(error) = command(mpv)
    {
        eprintln!("mpv command failed: {error}");
        return false;
    }

    state.borrow().mpv.is_some()
}

fn has_loaded_media(state: &Rc<RefCell<PlayerState>>) -> bool {
    has_loaded_media_state(&state.borrow())
}

fn has_loaded_media_state(state: &PlayerState) -> bool {
    state.current_file.is_some() || state.current_url.is_some()
}

fn set_volume_from_ui(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let result = state
        .borrow()
        .mpv
        .as_ref()
        .map(|mpv| mpv.set_volume(volume));
    match result {
        Some(Ok(())) | None => save_volume_setting(state, volume),
        Some(Err(error)) => eprintln!("Failed to set volume: {error}"),
    }
}

fn adjust_volume(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    let updated_volume = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        let volume = match mpv.playback_state() {
            Ok(playback) => playback.volume.unwrap_or(100.0),
            Err(error) => {
                eprintln!("Failed to read volume: {error}");
                return;
            }
        };
        let updated_volume = (volume + delta).clamp(0.0, 130.0);
        if let Err(error) = mpv.set_volume(updated_volume) {
            eprintln!("Failed to set volume: {error}");
            return;
        }
        updated_volume
    };

    save_volume_setting(state, updated_volume);
}

fn save_volume_setting(state: &Rc<RefCell<PlayerState>>, volume: f64) {
    let mut state = state.borrow_mut();
    state.settings.set_volume(volume);
    if let Err(error) = state.settings.save() {
        eprintln!("Failed to save settings: {error}");
    }
}

fn adjust_subtitle_delay(state: &Rc<RefCell<PlayerState>>, delta_seconds: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_delay(delta_seconds)) {
        save_current_preferences(state);
    }
}

fn adjust_subtitle_scale(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    if with_mpv(state, |mpv| mpv.adjust_subtitle_scale(delta)) {
        save_current_preferences(state);
    }
}

fn take_screenshot(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let (has_mpv, current_file, position) = {
        let state = state.borrow();
        let position = state
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok())
            .and_then(|playback| playback.time_pos);
        (state.mpv.is_some(), state.current_file.clone(), position)
    };

    if !has_mpv {
        return;
    }

    let path = screenshots::next_screenshot_path(current_file.as_deref(), position);

    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };
        mpv.screenshot_to_file(&path, true)
    };

    match result {
        Ok(()) => {
            let filename = path
                .file_name()
                .map(|name| name.to_string_lossy())
                .unwrap_or_else(|| "screenshot.png".into());
            eprintln!("Screenshot saved to {}", path.display());
            status_toast.show(&format!("Screenshot saved: {filename}"));
        }
        Err(error) => {
            eprintln!("Failed to save screenshot to {}: {error}", path.display());
            status_toast.show("Screenshot failed");
        }
    }
}

fn open_media_info_window(
    parent: &gtk::ApplicationWindow,
    state: &Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
) {
    let result = {
        let state = state.borrow();
        let Some(mpv) = state.mpv.as_ref() else {
            return;
        };

        mpv.media_info(state.current_file.as_deref())
    };

    match result {
        Ok(media_info) => show_media_info_window(parent, &media_info, status_toast),
        Err(error) => {
            eprintln!("Failed to read media information: {error}");
            status_toast.show("Media information unavailable");
        }
    }
}

fn show_media_info_window(
    parent: &gtk::ApplicationWindow,
    media_info: &MediaInfo,
    status_toast: Rc<StatusToast>,
) {
    let window = gtk::Window::builder()
        .title("Media Information")
        .transient_for(parent)
        .default_width(680)
        .default_height(820)
        .decorated(false)
        .build();
    window.add_css_class("okp-info-window");

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("okp-info-root");

    let page = gtk::Box::new(gtk::Orientation::Vertical, 16);
    page.add_css_class("okp-info-page");
    page.set_margin_top(54);
    page.set_margin_end(36);
    page.set_margin_bottom(24);
    page.set_margin_start(36);

    let header = gtk::Box::new(gtk::Orientation::Vertical, 5);
    header.add_css_class("okp-info-hero");
    let eyebrow = gtk::Label::new(Some("MEDIA INFO"));
    eyebrow.add_css_class("okp-info-eyebrow");
    eyebrow.set_xalign(0.0);
    header.append(&eyebrow);

    let title = gtk::Label::new(Some(&media_info.title));
    title.add_css_class("okp-info-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    header.append(&title);

    if let Some(path) = media_info.path.as_deref() {
        let path_label = gtk::Label::new(Some(path));
        path_label.add_css_class("okp-info-path");
        path_label.set_xalign(0.0);
        path_label.set_ellipsize(pango::EllipsizeMode::Middle);
        header.append(&path_label);
    }
    page.append(&header);

    if let Some(summary) = media_info_summary_widget(media_info) {
        page.append(&summary);
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("okp-info-content");
    for section in &media_info.sections {
        content.append(&media_info_section_widget(section));
    }
    if !media_info.tracks.is_empty() {
        content.append(&media_info_tracks_section(&media_info.tracks));
    }

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&content));
    page.append(&scroller);

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    footer.add_css_class("okp-info-footer");

    let copy_button = media_info_action_button("Copy info", "edit-copy-symbolic");
    copy_button.add_css_class("okp-info-footer-button");
    let copy_text = Rc::new(media_info_copy_text(media_info));
    let copy_toast = Rc::clone(&status_toast);
    copy_button.connect_clicked(move |_| {
        if let Some(display) = gdk::Display::default() {
            display.clipboard().set_text(copy_text.as_str());
            copy_toast.show("Media information copied");
        }
    });
    footer.append(&copy_button);

    let done_button = gtk::Button::with_label("Done");
    done_button.add_css_class("okp-info-footer-button");
    done_button.set_has_frame(false);
    done_button.set_halign(gtk::Align::End);
    done_button.set_hexpand(true);
    let close_window = window.clone();
    done_button.connect_clicked(move |_| close_window.close());
    footer.append(&done_button);
    page.append(&footer);
    root.append(&page);

    let content_overlay = gtk::Overlay::new();
    content_overlay.set_child(Some(&root));
    content_overlay.add_overlay(&settings_window_controls(&window));

    let handle = gtk::WindowHandle::new();
    handle.set_child(Some(&content_overlay));
    window.set_child(Some(&handle));
    window.present();
}

fn media_info_action_button(label: &str, icon_name: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_has_frame(false);

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);
    content.append(&gtk::Label::new(Some(label)));
    button.set_child(Some(&content));

    button
}

fn media_info_summary_widget(media_info: &MediaInfo) -> Option<gtk::Box> {
    let chips = media_info_summary_chips(media_info);
    if chips.is_empty() {
        return None;
    }

    let summary = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    summary.add_css_class("okp-info-summary");
    summary.set_halign(gtk::Align::Start);
    for (label, value) in chips {
        summary.append(&media_info_summary_chip(label, &value));
    }
    Some(summary)
}

fn media_info_summary_chips(media_info: &MediaInfo) -> Vec<(&'static str, String)> {
    let mut chips = Vec::new();

    if let Some(container) = media_info_value(media_info, "File", "Container") {
        chips.push(("Container", container.to_owned()));
    }
    if let Some(duration) = media_info_value(media_info, "File", "Duration") {
        chips.push(("Duration", duration.to_owned()));
    }
    if let Some(resolution) = media_info_value(media_info, "Video", "Resolution") {
        chips.push(("Video", resolution.to_owned()));
    }
    if let Some(codec) = media_info_value(media_info, "Video", "Codec") {
        chips.push(("Codec", codec.to_owned()));
    }

    let audio_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .count();
    if audio_count > 0 {
        chips.push(("Audio", audio_count.to_string()));
    }

    let subtitle_count = media_info
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .count();
    if subtitle_count > 0 {
        chips.push(("Subtitles", subtitle_count.to_string()));
    }

    chips
}

fn media_info_value<'a>(
    media_info: &'a MediaInfo,
    section_title: &str,
    row_label: &str,
) -> Option<&'a str> {
    media_info
        .sections
        .iter()
        .find(|section| section.title == section_title)?
        .rows
        .iter()
        .find(|row| row.label == row_label)
        .map(|row| row.value.as_str())
}

fn media_info_summary_chip(label: &str, value: &str) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Vertical, 2);
    chip.add_css_class("okp-info-chip");

    let label = gtk::Label::new(Some(label));
    label.add_css_class("okp-info-chip-label");
    label.set_xalign(0.0);
    chip.append(&label);

    let value = gtk::Label::new(Some(value));
    value.add_css_class("okp-info-chip-value");
    value.set_xalign(0.0);
    value.set_ellipsize(pango::EllipsizeMode::End);
    value.set_max_width_chars(18);
    chip.append(&value);

    chip
}

fn media_info_section_widget(section: &InfoSection) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let section_title = section.title.to_uppercase();
    let title = gtk::Label::new(Some(&section_title));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for row in &section.rows {
        content.append(&media_info_row(&row.label, &row.value));
    }

    content
}

fn media_info_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("okp-info-row");
    row.set_hexpand(true);

    let label_widget = gtk::Label::new(Some(label));
    label_widget.add_css_class("okp-info-label");
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(15);
    row.append(&label_widget);

    let value_widget = gtk::Label::new(Some(value));
    value_widget.add_css_class("okp-info-value");
    value_widget.set_xalign(0.0);
    value_widget.set_hexpand(true);
    value_widget.set_wrap(true);
    value_widget.set_wrap_mode(pango::WrapMode::WordChar);
    row.append(&value_widget);

    row
}

fn media_info_tracks_section(tracks: &[InfoTrack]) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.add_css_class("okp-info-section");

    let title = gtk::Label::new(Some("Tracks"));
    title.add_css_class("okp-info-section-title");
    title.set_xalign(0.0);
    content.append(&title);

    for track in tracks {
        content.append(&media_info_track_row(track));
    }

    content
}

fn media_info_track_row(track: &InfoTrack) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("okp-info-track-row");
    if track.selected {
        row.add_css_class("is-selected");
    }

    let kind_text = media_info_track_kind_label(track.kind).to_uppercase();
    let kind = gtk::Label::new(Some(&kind_text));
    kind.add_css_class("okp-info-track-kind");
    kind.set_width_chars(8);
    kind.set_xalign(0.0);
    row.append(&kind);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
    body.set_hexpand(true);

    let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    title_row.set_hexpand(true);

    let title = gtk::Label::new(Some(&format!("#{} {}", track.id, track.title)));
    title.add_css_class("okp-info-track-title");
    title.set_xalign(0.0);
    title.set_ellipsize(pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title_row.append(&title);

    if track.selected {
        let current = gtk::Label::new(Some("CURRENT"));
        current.add_css_class("okp-info-track-current");
        title_row.append(&current);
    }
    body.append(&title_row);

    if !track.detail.is_empty() {
        let detail = gtk::Label::new(Some(&track.detail));
        detail.add_css_class("okp-info-track-detail");
        detail.set_xalign(0.0);
        detail.set_wrap(true);
        detail.set_wrap_mode(pango::WrapMode::WordChar);
        body.append(&detail);
    }

    row.append(&body);
    row
}

fn media_info_track_kind_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitle",
    }
}

fn media_info_copy_text(media_info: &MediaInfo) -> String {
    let mut lines = vec![
        "OK Player Media Information".to_owned(),
        format!("App: OK Player {APP_BUILD_VERSION} ({APP_BUILD_SHA})"),
        "Platform: Linux GTK4 / libmpv".to_owned(),
        String::new(),
        media_info.title.clone(),
    ];
    if let Some(path) = media_info.path.as_deref() {
        lines.push(format!("Path: {path}"));
    }

    for section in &media_info.sections {
        lines.push(String::new());
        lines.push(section.title.clone());
        for row in &section.rows {
            lines.push(format!("{}: {}", row.label, row.value));
        }
    }

    if !media_info.tracks.is_empty() {
        lines.push(String::new());
        lines.push("Tracks".to_owned());
        for track in &media_info.tracks {
            let detail = if track.detail.is_empty() {
                String::new()
            } else {
                format!(" - {}", track.detail)
            };
            lines.push(format!(
                "{} #{}: {}{}",
                media_info_track_kind_label(track.kind),
                track.id,
                track.title,
                detail
            ));
        }
    }

    lines.join("\n")
}

fn seek_to_chapter(state: &Rc<RefCell<PlayerState>>, time: f64) {
    if time.is_finite() && time >= 0.0 {
        with_mpv(state, |mpv| mpv.seek_absolute(time));
    }
}

fn seek_to_time(state: &Rc<RefCell<PlayerState>>, time: f64) -> bool {
    time.is_finite() && time >= 0.0 && with_mpv(state, |mpv| mpv.seek_absolute(time))
}

fn toggle_fullscreen(window: &gtk::ApplicationWindow) {
    if window.is_fullscreen() {
        window.unfullscreen();
    } else {
        window.fullscreen();
    }
}

fn cycle_repeat_mode(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.modes.repeat_mode = state.modes.repeat_mode.cycle();
}

fn toggle_shuffle(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.modes.shuffle_enabled = !state.modes.shuffle_enabled;
    state.modes.reset_shuffle_order();

    if state.modes.shuffle_enabled
        && let Some(current_index) = current_playlist_index(&state)
    {
        let playlist_len = state.playlist.len();
        state
            .modes
            .ensure_shuffle_order(playlist_len, current_index);
    }
}

fn toggle_auto_advance(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.modes.auto_advance_enabled = !state.modes.auto_advance_enabled;
}

fn toggle_private_session(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let enabled = {
        let mut state = state.borrow_mut();
        state.private_session = !state.private_session;
        if state.private_session {
            state.pending_resume = None;
            state.pending_preferences = None;
        }
        state.private_session
    };

    status_toast.show(if enabled {
        "Private session on"
    } else {
        "Private session off"
    });
}

fn clear_history(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) {
    let mut state = state.borrow_mut();
    state.history.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
    match state.history.save() {
        Ok(()) => status_toast.show("History cleared"),
        Err(error) => {
            eprintln!("Failed to clear history: {error}");
            status_toast.show("Could not clear history");
        }
    }
}

fn close_current_media(state: &Rc<RefCell<PlayerState>>, status_toast: &StatusToast) -> bool {
    if !has_loaded_media(state) {
        return false;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(Mpv::stop)
    };

    match result {
        Some(Ok(())) | None => {
            clear_loaded_media_state(state);
            status_toast.show("Media closed");
            true
        }
        Some(Err(error)) => {
            eprintln!("Failed to close media: {error}");
            status_toast.show("Could not close media");
            false
        }
    }
}

fn clear_loaded_media_state(state: &Rc<RefCell<PlayerState>>) {
    let mut state = state.borrow_mut();
    state.current_file = None;
    state.current_url = None;
    state.playlist.clear();
    state.modes.reset_shuffle_order();
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
}

fn load_media_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    load_media_path_internal(state, path, true);
}

fn load_media_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    if !is_media_url(&url) {
        return;
    }

    save_current_progress(state, false);

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_url(&url))
    };

    match result {
        Some(Ok(())) => remember_loaded_url(state, url),
        Some(Err(error)) => eprintln!("Failed to load URL '{url}': {error}"),
        None => remember_loaded_url(state, url),
    }
}

fn load_media_path_internal(state: &Rc<RefCell<PlayerState>>, path: PathBuf, save_previous: bool) {
    if !is_media_path(&path) {
        return;
    }
    if save_previous {
        save_current_progress(state, false);
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => remember_loaded_media(state, path),
        Some(Err(error)) => eprintln!("Failed to load media '{}': {error}", path.display()),
        None => remember_loaded_media(state, path),
    }
}

fn remember_loaded_media(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    let playlist = build_folder_playlist(&path);
    let resume_path = path.clone();
    let preferences_path = path.clone();
    let mut state = state.borrow_mut();
    let resume = if state.private_session {
        None
    } else {
        state.history.resume_position(&path)
    };
    let preferences = if state.private_session {
        None
    } else {
        state.history.playback_preferences(&path)
    };
    let playlist_changed = state.playlist != playlist;
    state.current_file = Some(path);
    state.current_url = None;
    state.playlist = playlist;
    if playlist_changed {
        state.modes.reset_shuffle_order();
    }
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    if let Some(current_index) = current_playlist_index(&state) {
        let playlist_len = state.playlist.len();
        state
            .modes
            .ensure_shuffle_order(playlist_len, current_index);
    }
    state.pending_subtitles.clear();
    state.pending_resume = resume.map(|position| (resume_path, position));
    state.pending_preferences = preferences.map(|preferences| (preferences_path, preferences));
}

fn remember_loaded_url(state: &Rc<RefCell<PlayerState>>, url: String) {
    let mut state = state.borrow_mut();
    state.current_file = None;
    state.current_url = Some(url);
    state.playlist.clear();
    state.modes.reset_shuffle_order();
    state.thumbnail_request_key = None;
    state.hover_thumbnail_request_key = None;
    state.chapters_snapshot.clear();
    state.pending_subtitles.clear();
    state.pending_resume = None;
    state.pending_preferences = None;
}

fn navigate_playlist(state: &Rc<RefCell<PlayerState>>, direction: isize) -> bool {
    let Some(path) = playlist_target_path(state, direction, true) else {
        return false;
    };

    load_media_path_internal(state, path, true);
    true
}

fn jump_playlist_index(state: &Rc<RefCell<PlayerState>>, index: usize) -> bool {
    let path = {
        let state = state.borrow();
        state.playlist.get(index).cloned()
    };

    let Some(path) = path else {
        return false;
    };

    {
        let mut state = state.borrow_mut();
        if state.modes.shuffle_enabled {
            state.modes.shuffle_cursor = state
                .modes
                .shuffle_order
                .iter()
                .position(|item| *item == index);
        }
    }

    load_media_path_internal(state, path, true);
    true
}

fn advance_playlist_on_eof(state: &Rc<RefCell<PlayerState>>) -> bool {
    let repeat_mode = state.borrow().modes.repeat_mode;
    if repeat_mode == RepeatMode::One {
        return restart_current_file(state);
    }

    if !state.borrow().modes.auto_advance_enabled {
        return false;
    }

    let wrap = repeat_mode == RepeatMode::All;
    let Some(next_file) = playlist_target_path(state, 1, wrap) else {
        return false;
    };

    load_media_path_internal(state, next_file, false);
    true
}

fn restart_current_file(state: &Rc<RefCell<PlayerState>>) -> bool {
    let path = {
        let state = state.borrow();
        let Some(path) = state.current_file.clone() else {
            return false;
        };
        let Some(mpv) = state.mpv.as_ref() else {
            return false;
        };
        if let Err(error) = mpv.load_file(&path) {
            eprintln!("Failed to repeat '{}': {error}", path.display());
            return false;
        }
        path
    };

    let preferences = state.borrow().history.playback_preferences(&path);
    let mut state = state.borrow_mut();
    state.pending_resume = None;
    state.pending_preferences = preferences.map(|preferences| (path, preferences));
    true
}

fn playlist_target_path(
    state: &Rc<RefCell<PlayerState>>,
    direction: isize,
    wrap: bool,
) -> Option<PathBuf> {
    let mut state = state.borrow_mut();
    if state.playlist.len() < 2 {
        return None;
    }

    let current_index = current_playlist_index(&state).unwrap_or(0);
    let next_index = if state.modes.shuffle_enabled {
        shuffled_target_index(&mut state, current_index, direction, wrap)?
    } else {
        ordered_target_index(state.playlist.len(), current_index, direction, wrap)?
    };

    state.playlist.get(next_index).cloned()
}

fn ordered_target_index(
    playlist_len: usize,
    current_index: usize,
    direction: isize,
    wrap: bool,
) -> Option<usize> {
    let target = current_index as isize + direction;
    if wrap {
        Some(target.rem_euclid(playlist_len as isize) as usize)
    } else if (0..playlist_len as isize).contains(&target) {
        Some(target as usize)
    } else {
        None
    }
}

fn shuffled_target_index(
    state: &mut PlayerState,
    current_index: usize,
    direction: isize,
    wrap: bool,
) -> Option<usize> {
    let playlist_len = state.playlist.len();
    state
        .modes
        .ensure_shuffle_order(playlist_len, current_index);
    let cursor = state.modes.shuffle_cursor.unwrap_or(0);
    let target_cursor =
        ordered_target_index(state.modes.shuffle_order.len(), cursor, direction, wrap)?;
    state.modes.shuffle_cursor = Some(target_cursor);
    state.modes.shuffle_order.get(target_cursor).copied()
}

fn current_playlist_index(state: &PlayerState) -> Option<usize> {
    let current_file = state.current_file.as_ref()?;
    state.playlist.iter().position(|path| path == current_file)
}

fn try_pending_resume(state: &Rc<RefCell<PlayerState>>, duration: f64) {
    if !duration.is_finite() || duration <= 0.0 {
        return;
    }

    let pending = {
        let state = state.borrow();
        state.pending_resume.clone()
    };
    let Some((path, target)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_resume = None;
        return;
    }

    if target > duration {
        return;
    }

    if target <= duration * 0.05 || target >= history::completion_start(duration) {
        state.borrow_mut().pending_resume = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.seek_absolute(target))
    };
    if matches!(result, Some(Ok(()))) {
        state.borrow_mut().pending_resume = None;
    } else if let Some(Err(error)) = result {
        eprintln!("Failed to resume '{}': {error}", path.display());
    }
}

fn try_pending_playback_preferences(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let state = state.borrow();
        state.pending_preferences.clone()
    };
    let Some((path, preferences)) = pending else {
        return;
    };

    let is_current = state
        .borrow()
        .current_file
        .as_ref()
        .is_some_and(|current| current == &path);
    if !is_current {
        state.borrow_mut().pending_preferences = None;
        return;
    }

    let result = {
        let state = state.borrow();
        state
            .mpv
            .as_ref()
            .map(|mpv| apply_playback_preferences(mpv, &preferences))
    };

    match result {
        Some(Ok(())) => state.borrow_mut().pending_preferences = None,
        Some(Err(error)) => eprintln!("Failed to restore playback preferences: {error}"),
        None => {}
    }
}

fn apply_playback_preferences(
    mpv: &Mpv,
    preferences: &history::PlaybackPreferences,
) -> Result<(), okp_mpv::MpvError> {
    let tracks = mpv.tracks()?;

    if let Some(enabled) = preferences.audio_enabled {
        if !enabled {
            mpv.select_audio(None)?;
        } else if let Some(track_id) = preferences.audio_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio && track.id == track_id)
        {
            mpv.select_audio(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.subtitle_enabled {
        if !enabled {
            mpv.select_subtitle(None)?;
        } else if let Some(track_id) = preferences.subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_subtitle(Some(track_id))?;
        }
    }

    if let Some(enabled) = preferences.secondary_subtitle_enabled {
        if !enabled {
            mpv.select_secondary_subtitle(None)?;
        } else if let Some(track_id) = preferences.secondary_subtitle_track_id
            && tracks
                .iter()
                .any(|track| track.kind == TrackKind::Subtitle && track.id == track_id)
        {
            mpv.select_secondary_subtitle(Some(track_id))?;
        }
    }

    if let Some(delay) = preferences.subtitle_delay.and_then(finite_option) {
        mpv.set_subtitle_delay(delay)?;
    }
    if let Some(scale) = preferences.subtitle_scale.and_then(finite_option) {
        mpv.set_subtitle_scale(scale)?;
    }
    if let Some(speed) = preferences.speed.and_then(finite_option) {
        mpv.set_speed(speed)?;
    }

    Ok(())
}

fn save_current_preferences(state: &Rc<RefCell<PlayerState>>) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(preferences) = state.mpv.as_ref().map(read_current_playback_preferences) else {
            return;
        };

        (path, preferences)
    };

    let (path, preferences) = snapshot;
    let mut state = state.borrow_mut();
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save playback preferences: {error}");
    }
}

fn read_current_playback_preferences(mpv: &Mpv) -> history::PlaybackPreferences {
    let tracks = mpv.tracks().unwrap_or_else(|error| {
        eprintln!("Failed to read tracks for preferences: {error}");
        Vec::new()
    });
    let selected_audio = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Audio && track.selected);
    let selected_subtitle = tracks
        .iter()
        .find(|track| track.kind == TrackKind::Subtitle && track.selected);
    let secondary_subtitle_id = mpv.secondary_subtitle_id().ok().flatten().filter(|id| {
        tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.id == *id)
    });
    let has_audio_tracks = tracks.iter().any(|track| track.kind == TrackKind::Audio);
    let has_subtitle_tracks = tracks.iter().any(|track| track.kind == TrackKind::Subtitle);

    history::PlaybackPreferences {
        audio_enabled: has_audio_tracks.then_some(selected_audio.is_some()),
        audio_track_id: selected_audio.map(|track| track.id),
        subtitle_enabled: has_subtitle_tracks.then_some(selected_subtitle.is_some()),
        subtitle_track_id: selected_subtitle.map(|track| track.id),
        secondary_subtitle_enabled: has_subtitle_tracks.then_some(secondary_subtitle_id.is_some()),
        secondary_subtitle_track_id: secondary_subtitle_id,
        subtitle_delay: mpv.subtitle_delay().ok().and_then(finite_option),
        subtitle_scale: mpv.subtitle_scale().ok().and_then(finite_option),
        speed: mpv.speed().ok().and_then(finite_option),
    }
}

fn finite_option(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn read_playback_speed(state: &Rc<RefCell<PlayerState>>) -> f64 {
    state
        .borrow()
        .mpv
        .as_ref()
        .and_then(|mpv| mpv.speed().ok())
        .and_then(finite_option)
        .unwrap_or(1.0)
}

fn format_speed(speed: f64) -> String {
    format!("{:.2}x", speed.clamp(0.25, 4.0))
}

fn speed_matches(left: f64, right: f64) -> bool {
    (left - right).abs() < 0.005
}

fn save_current_progress(state: &Rc<RefCell<PlayerState>>, finished: bool) {
    let snapshot = {
        let state = state.borrow();
        if state.private_session {
            return;
        }
        let Some(path) = state.current_file.clone() else {
            return;
        };
        let Some(playback) = state.mpv.as_ref().and_then(|mpv| mpv.playback_state().ok()) else {
            return;
        };
        let preferences = state
            .mpv
            .as_ref()
            .map(read_current_playback_preferences)
            .unwrap_or_default();

        (path, playback, preferences)
    };

    let (path, playback, preferences) = snapshot;
    let Some(duration) = playback.duration else {
        return;
    };
    let position = playback.time_pos.unwrap_or(0.0);
    if !duration.is_finite() || duration <= 0.0 || !position.is_finite() {
        return;
    }

    let mut state = state.borrow_mut();
    state
        .history
        .record(&path, position.clamp(0.0, duration), duration, finished);
    state.history.record_preferences(&path, preferences);
    if let Err(error) = state.history.save() {
        eprintln!("Failed to save history: {error}");
    }
}

fn build_folder_playlist(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return vec![path.to_path_buf()];
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return vec![path.to_path_buf()];
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_media_path(path))
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        let left = left.file_name().and_then(|name| name.to_str());
        let right = right.file_name().and_then(|name| name.to_str());
        natural_compare::compare(left, right)
    });

    if files.is_empty() {
        vec![path.to_path_buf()]
    } else {
        files
    }
}

fn load_subtitle_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) -> bool {
    if !is_subtitle_path(&path) || !has_loaded_media(state) {
        return false;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
    };

    match result {
        Some(Ok(())) => true,
        Some(Err(error)) => {
            eprintln!(
                "Subtitle queued until media is ready '{}': {error}",
                path.display()
            );
            state.borrow_mut().pending_subtitles.push(path);
            false
        }
        None => false,
    }
}

fn try_pending_subtitles(state: &Rc<RefCell<PlayerState>>) {
    let pending = {
        let mut state = state.borrow_mut();
        if !has_loaded_media_state(&state) || state.pending_subtitles.is_empty() {
            return;
        }

        std::mem::take(&mut state.pending_subtitles)
    };

    let mut retry = Vec::new();
    for path in pending {
        let result = {
            let state = state.borrow();
            state.mpv.as_ref().map(|mpv| mpv.add_subtitle_file(&path))
        };

        if !matches!(result, Some(Ok(()))) {
            retry.push(path);
        }
    }

    if !retry.is_empty() {
        state.borrow_mut().pending_subtitles.extend(retry);
    }
}

fn is_media_path(path: &Path) -> bool {
    media_formats::is_media(path)
}

fn is_media_url(url: &str) -> bool {
    media_formats::is_playable_url(Some(url))
}

fn is_subtitle_path(path: &Path) -> bool {
    media_formats::is_subtitle(path)
}

fn display_file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "00:00".to_owned();
    }

    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn shuffle_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9_7F4A_7C15)
}

fn next_shuffle_value(seed: &mut u64) -> u64 {
    let mut value = (*seed).max(1);
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *seed = value;
    value
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };

    let provider = gtk::CssProvider::new();
    provider.load_from_data(
        "
        .okp-root {
            background: #050507;
        }

        window.okp-player-window {
            background: #050507;
        }

        .okp-window-chrome {
            min-height: 40px;
            background: transparent;
        }

        .okp-window-drag-zone {
            min-height: 40px;
            background: transparent;
        }

        .okp-player-window-controls {
            min-height: 32px;
            border-radius: 12px;
            background: rgba(14, 15, 18, 0.42);
            border: 1px solid rgba(255, 255, 255, 0.07);
        }

        .okp-player-window-controls button,
        button.okp-player-window-control {
            min-width: 42px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 8px;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.84);
            font-size: 15px;
            font-weight: 400;
        }

        .okp-player-window-controls button:hover,
        button.okp-player-window-control:hover {
            background: rgba(255, 255, 255, 0.12);
            color: rgba(255, 255, 255, 0.96);
        }

        label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.86);
            font-size: 15px;
            font-weight: 400;
        }

        button.okp-player-window-control:hover label.okp-player-window-control-glyph {
            color: rgba(255, 255, 255, 0.98);
        }

        .okp-player-window-controls button:active,
        button.okp-player-window-control:active {
            background: rgba(255, 255, 255, 0.18);
        }

        button.okp-player-window-close:hover {
            background: rgba(219, 59, 59, 0.86);
            color: #ffffff;
        }

        .okp-resize-handle {
            background: transparent;
        }

        .okp-resize-edge-horizontal {
            min-height: 6px;
        }

        .okp-resize-edge-vertical {
            min-width: 6px;
        }

        .okp-resize-corner {
            min-width: 16px;
            min-height: 16px;
        }

        .okp-video-plane {
            background: #050507;
        }

        .okp-empty-surface {
            background: rgba(5, 5, 7, 0.94);
        }

        .okp-empty-panel {
            min-width: 300px;
            padding: 28px;
            border-radius: 8px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(18, 19, 23, 0.84);
        }

        .okp-empty-panel.is-drop-target {
            border-color: rgba(40, 179, 170, 0.82);
            background: rgba(22, 48, 49, 0.92);
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.18);
        }

        .okp-empty-logo {
            color: #28b3aa;
        }

        .okp-empty-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 24px;
            font-weight: 750;
        }

        .okp-empty-primary-button,
        .okp-empty-secondary-button {
            min-height: 36px;
            padding: 6px 14px;
            border-radius: 7px;
        }

        .okp-empty-primary-button {
            background: #28b3aa;
            color: #051011;
        }

        .okp-empty-secondary-button {
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-controls {
            padding: 8px 10px;
            border-radius: 18px;
            background: rgba(13, 14, 18, 0.86);
            border: 1px solid rgba(255, 255, 255, 0.11);
            box-shadow: 0 18px 48px rgba(0, 0, 0, 0.48);
        }

        .okp-control-group {
            padding: 3px;
            border-radius: 14px;
            background: rgba(255, 255, 255, 0.045);
            border: 1px solid rgba(255, 255, 255, 0.055);
        }

        .okp-transport-group {
            padding: 0;
            border-radius: 12px;
            background: transparent;
        }

        .okp-timeline-group {
            min-height: 36px;
            padding: 0 2px;
        }

        button.okp-control-button,
        menubutton.okp-control-button > button {
            min-width: 34px;
            min-height: 32px;
            padding: 0 9px;
            border-radius: 9px;
            border: 1px solid transparent;
            background: transparent;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.86);
            font-size: 12px;
            font-weight: 600;
        }

        button.okp-control-button:hover,
        menubutton.okp-control-button > button:hover {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.96);
        }

        button.okp-control-button:active,
        menubutton.okp-control-button > button:active,
        button.okp-control-button:checked,
        menubutton.okp-control-button > button:checked {
            background: rgba(40, 179, 170, 0.24);
            border-color: rgba(40, 179, 170, 0.42);
            color: rgba(255, 255, 255, 0.98);
        }

        button.okp-control-button:disabled,
        menubutton.okp-control-button > button:disabled {
            background: transparent;
            border-color: transparent;
            color: rgba(255, 255, 255, 0.32);
        }

        button.okp-play-button {
            min-width: 42px;
            border-radius: 11px;
            background: rgba(40, 179, 170, 0.92);
            color: #ffffff;
        }

        button.okp-play-button:hover {
            background: rgba(55, 207, 197, 0.96);
        }

        button.okp-play-button:disabled {
            background: rgba(255, 255, 255, 0.11);
            color: rgba(255, 255, 255, 0.34);
        }

        button.okp-transport-button {
            min-width: 34px;
        }

        button.okp-chip-button,
        menubutton.okp-chip-button > button {
            min-width: 48px;
            background: rgba(255, 255, 255, 0.055);
        }

        button.okp-icon-button,
        menubutton.okp-icon-button > button {
            min-width: 34px;
            padding: 0;
        }

        menubutton.okp-speed-chip > button {
            min-width: 56px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(40, 179, 170, 0.98);
            font-feature-settings: 'tnum';
        }

        .okp-control-button.is-selected {
            background: rgba(40, 179, 170, 0.22);
        }

        .okp-time-label {
            min-width: 50px;
            color: rgba(255, 255, 255, 0.84);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-status-toast {
            padding: 8px 12px;
            border-radius: 8px;
            background: rgba(14, 15, 18, 0.9);
            box-shadow: 0 12px 34px rgba(0, 0, 0, 0.38);
            color: rgba(255, 255, 255, 0.9);
            font-size: 13px;
            font-weight: 600;
        }

        .okp-seek {
            min-width: 120px;
        }

        scale.okp-seek trough,
        scale.okp-volume trough {
            min-height: 3px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.23);
            border: none;
        }

        scale.okp-seek highlight,
        scale.okp-volume highlight {
            min-height: 3px;
            border-radius: 999px;
            background: #28b3aa;
        }

        scale.okp-seek slider,
        scale.okp-volume slider {
            min-width: 13px;
            min-height: 13px;
            margin: -5px;
            border-radius: 999px;
            background: rgba(255, 255, 255, 0.96);
            box-shadow: 0 2px 8px rgba(0, 0, 0, 0.42);
        }

        .okp-seek-preview {
            padding: 7px 10px;
            border-radius: 7px;
            background: rgba(14, 15, 18, 0.92);
            box-shadow: 0 10px 28px rgba(0, 0, 0, 0.34);
        }

        .okp-seek-preview-thumb {
            margin-bottom: 6px;
            border-radius: 5px;
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-seek-preview-time {
            color: rgba(255, 255, 255, 0.92);
            font-size: 12px;
            font-weight: 700;
            font-feature-settings: 'tnum';
        }

        .okp-seek-preview-chapter {
            margin-top: 2px;
            color: rgba(255, 255, 255, 0.62);
            font-size: 11px;
        }

        .okp-volume {
            min-width: 92px;
        }

        .okp-up-next-panel {
            padding: 12px;
            border-radius: 8px;
            background: rgba(14, 15, 18, 0.88);
            box-shadow: 0 14px 42px rgba(0, 0, 0, 0.42);
        }

        .okp-up-next-title {
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 700;
        }

        .okp-up-next-list {
            background: transparent;
        }

        .okp-panel-heading-row {
            padding: 4px 10px 2px 10px;
        }

        .okp-panel-heading {
            color: rgba(255, 255, 255, 0.52);
            font-size: 11px;
            font-weight: 700;
        }

        .okp-panel-empty-row {
            padding: 10px;
            border-radius: 7px;
            background: rgba(255, 255, 255, 0.045);
        }

        .okp-panel-empty {
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-up-next-row {
            min-height: 38px;
            padding: 8px 10px;
            border-radius: 7px;
            color: rgba(255, 255, 255, 0.78);
        }

        .okp-chapter-thumb {
            min-width: 88px;
            min-height: 50px;
            border-radius: 5px;
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-up-next-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-up-next-row.is-current {
            background: rgba(98, 181, 255, 0.18);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-up-next-marker {
            color: rgba(98, 181, 255, 0.95);
            font-size: 11px;
            font-weight: 700;
        }

        .okp-up-next-file {
            color: inherit;
            font-size: 13px;
        }

        .okp-track-popover-content {
            padding: 10px;
            background: #121317;
        }

        popover.okp-track-popover {
            padding: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        popover.okp-track-popover > contents,
        popover.okp-track-popover contents {
            padding: 0;
            border-radius: 10px;
            background: #121317;
            border: 1px solid rgba(255, 255, 255, 0.12);
            box-shadow: 0 18px 46px rgba(0, 0, 0, 0.46);
        }

        popover.okp-track-popover arrow {
            min-width: 0;
            min-height: 0;
            background: transparent;
            border: none;
            box-shadow: none;
        }

        .okp-track-popover-scroll {
            background: #121317;
        }

        .okp-track-popover-title {
            margin: 0 4px 6px 4px;
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 700;
        }

        button.okp-track-row {
            min-height: 34px;
            padding: 7px 9px;
            border-radius: 7px;
            background: transparent;
            border: none;
            box-shadow: none;
            color: rgba(255, 255, 255, 0.82);
        }

        button.okp-track-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        button.okp-track-row.is-selected {
            background: rgba(40, 179, 170, 0.18);
            color: rgba(255, 255, 255, 0.96);
        }

        .okp-track-empty {
            margin: 6px 9px;
            color: rgba(255, 255, 255, 0.55);
            font-size: 13px;
        }

        .okp-track-divider {
            margin: 5px 3px;
        }

        .okp-sub-adjust-row {
            margin: 0 2px;
        }

        .okp-sub-adjust-label {
            color: rgba(255, 255, 255, 0.62);
            font-size: 12px;
        }

        .okp-sub-adjust-value {
            color: rgba(255, 255, 255, 0.9);
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry {
            min-width: 74px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            border: 1px solid rgba(255, 255, 255, 0.14);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            font-feature-settings: 'tnum';
        }

        entry.okp-sub-adjust-entry:focus {
            border-color: rgba(40, 179, 170, 0.72);
            box-shadow: 0 0 0 2px rgba(40, 179, 170, 0.16);
        }

        entry.okp-sub-adjust-entry.is-error {
            border-color: rgba(255, 104, 104, 0.88);
            box-shadow: 0 0 0 2px rgba(255, 104, 104, 0.18);
        }

        .okp-sub-adjust-unit {
            color: rgba(255, 255, 255, 0.58);
            font-size: 12px;
        }

        .okp-sub-adjust-button {
            min-width: 44px;
            min-height: 28px;
            padding: 4px 7px;
            border-radius: 6px;
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.86);
        }

        .okp-sub-adjust-button:hover {
            background: rgba(255, 255, 255, 0.13);
        }

        .okp-info-window {
            background: #eef4f9;
        }

        window.okp-command-dialog {
            background: #101115;
            color: rgba(255, 255, 255, 0.9);
            border-radius: 8px;
        }

        window.okp-command-dialog > contents {
            background: #101115;
        }

        .okp-command-dialog-title {
            color: rgba(255, 255, 255, 0.96);
            font-size: 16px;
            font-weight: 700;
        }

        window.okp-command-dialog entry {
            min-height: 34px;
            border-radius: 7px;
            border: 1px solid rgba(40, 179, 170, 0.42);
            background: rgba(255, 255, 255, 0.055);
            color: rgba(255, 255, 255, 0.92);
            box-shadow: none;
        }

        window.okp-command-dialog button {
            min-width: 72px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            border: 1px solid rgba(255, 255, 255, 0.12);
            background: rgba(255, 255, 255, 0.08);
            color: rgba(255, 255, 255, 0.9);
            box-shadow: none;
        }

        window.okp-command-dialog button:hover {
            background: rgba(255, 255, 255, 0.13);
            color: rgba(255, 255, 255, 0.98);
        }

        window.okp-command-dialog button:active {
            background: rgba(40, 179, 170, 0.28);
            border-color: rgba(40, 179, 170, 0.48);
        }

        window.okp-command-dialog .okp-info-label {
            color: rgba(255, 255, 255, 0.62);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 500;
        }

        .okp-settings-window {
            background: transparent;
        }

        .okp-info-root {
            background: #eef4f9;
            color: #161616;
        }

        .okp-settings-root {
            background: #eef4f9;
            color: #161616;
            border: none;
            border-radius: 0;
        }

        .okp-settings-rail-frame {
            background: #eaf0f5;
        }

        .okp-settings-rail {
            padding: 16px 10px 14px 10px;
            background: #eaf0f5;
            border-right: 1px solid #dde3e7;
        }

        .okp-settings-rail-title {
            margin-left: 5px;
            margin-bottom: 20px;
            color: #3b3f42;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-search {
            min-height: 30px;
            margin-bottom: 11px;
            padding: 7px 10px;
            border-radius: 7px;
            background: #f9fbfc;
            border: 1px solid #d5dce2;
            color: #6c747a;
        }

        .okp-settings-search-label {
            color: #6c747a;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-settings-nav-row {
            min-height: 36px;
            padding: 8px 10px;
            border: none;
            border-radius: 7px;
            background: transparent;
            box-shadow: none;
            color: #3f464b;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-settings-nav-row:hover {
            background: rgba(0, 0, 0, 0.035);
        }

        .okp-settings-nav-row.is-selected {
            background: #cfe5e8;
            box-shadow: inset 3px 0 0 #10938a;
            color: #0a655f;
            font-weight: 600;
        }

        .okp-settings-nav-icon {
            min-width: 16px;
            min-height: 16px;
            color: inherit;
        }

        .okp-settings-rail-divider {
            margin: 6px 9px 8px;
            background: #dbe2e7;
        }

        .okp-settings-window-controls {
            min-height: 32px;
        }

        .okp-settings-window-control {
            min-width: 48px;
            min-height: 32px;
            padding: 0;
            border: none;
            border-radius: 0;
            background: transparent;
            box-shadow: none;
            color: #161616;
        }

        .okp-settings-window-control:hover {
            background: rgba(0, 0, 0, 0.06);
        }

        .okp-settings-stack {
            background: #eef4f9;
        }

        .okp-settings-scroller {
            background: #eef4f9;
        }

        .okp-settings-page {
            padding: 70px 44px 28px 24px;
        }

        .okp-info-page {
            background: #eef4f9;
        }

        .okp-info-hero {
            min-height: 82px;
        }

        .okp-info-eyebrow {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-title {
            color: #161616;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 28px;
            font-weight: 650;
        }

        .okp-info-path {
            color: rgba(0, 0, 0, 0.46);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-info-content {
            padding-right: 4px;
        }

        window.okp-info-window scrolledwindow {
            background: #eef4f9;
        }

        window.okp-info-window scrollbar {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar trough {
            background: transparent;
            border: none;
        }

        window.okp-info-window scrollbar slider {
            min-width: 4px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.22);
        }

        .okp-settings-content {
            padding-right: 4px;
        }

        .okp-about-pane {
            padding: 70px 44px 28px 24px;
            background: #eef4f9;
        }

        .okp-about-identity {
            min-height: 112px;
        }

        .okp-about-illustration {
            min-width: 118px;
            min-height: 94px;
        }

        .okp-about-wordmark {
            color: #161616;
            font-family: 'Segoe UI Variable Display', 'Segoe UI', sans-serif;
            font-size: 30px;
            letter-spacing: 0;
        }

        .okp-about-wordmark-ok {
            font-weight: 700;
        }

        .okp-about-wordmark-player {
            font-weight: 300;
        }

        .okp-about-chip-row {
            margin-top: 10px;
        }

        .okp-about-version-chip {
            padding: 3px 9px;
            border-radius: 6px;
            background: #e2e8ec;
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 11.5px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-about-channel-chip {
            padding: 4px 9px;
            border-radius: 6px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
            text-transform: uppercase;
        }

        .okp-about-tagline {
            margin-top: 11px;
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 13px;
            font-weight: 400;
        }

        .okp-about-byline {
            margin-top: 3px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-identity-divider {
            margin: 22px 0;
            background: rgba(0, 0, 0, 0.07);
        }

        .okp-about-card {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-about-card-title {
            margin-bottom: 13px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-row {
            min-height: 14px;
        }

        .okp-about-row-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-about-row-detail {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
            font-weight: 400;
        }

        .okp-about-row-value,
        .okp-about-row-value-mono {
            color: #161616;
            font-size: 12.5px;
            font-weight: 500;
        }

        .okp-about-row-value {
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
        }

        .okp-about-row-value-mono {
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-feature-settings: 'tnum';
        }

        .okp-about-host-grid {
            min-width: 0;
        }

        .okp-about-tag {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(0, 0, 0, 0.05);
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-about-tag.is-accent {
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
        }

        .okp-about-footer {
            margin-top: 8px;
            padding-top: 17px;
            border-top: 1px solid rgba(0, 0, 0, 0.07);
        }

        .okp-about-copy-button {
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-copy-button:hover {
            background: #d9e1e7;
        }

        .okp-about-check-button {
            min-width: 132px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 400;
        }

        .okp-about-check-button:hover {
            background: #f8fafb;
        }

        .okp-about-toggle {
            min-width: 39px;
            min-height: 22px;
            padding: 3px;
            border: none;
            border-radius: 999px;
            background: #ccd5dc;
            box-shadow: none;
        }

        .okp-about-toggle.is-active {
            background: #0067c0;
        }

        .okp-about-toggle-knob {
            min-width: 16px;
            min-height: 16px;
            border-radius: 999px;
            background: #ffffff;
        }

        .okp-about-link-button {
            min-height: 24px;
            padding: 0;
            border: none;
            background: transparent;
            box-shadow: none;
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-about-link-dot {
            min-width: 3px;
            min-height: 3px;
            margin-top: 10px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.40);
        }

        .okp-update-status {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-settings-switch-row {
            min-height: 42px;
            padding: 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-settings-state-pill {
            min-width: 34px;
            padding: 3px 8px;
            border-radius: 999px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
        }

        .okp-info-section {
            padding: 14px 16px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-section-title {
            margin-bottom: 10px;
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-row {
            min-height: 22px;
        }

        .okp-info-label {
            color: rgba(0, 0, 0, 0.50);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 400;
        }

        .okp-info-value {
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 500;
            font-feature-settings: 'tnum';
        }

        .okp-info-summary {
            padding: 0;
        }

        .okp-info-chip {
            min-width: 78px;
            padding: 8px 10px;
            border-radius: 8px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
        }

        .okp-info-chip-label {
            color: rgba(0, 0, 0, 0.40);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 10px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-chip-value {
            color: #161616;
            font-family: 'Cascadia Code', 'Cascadia Mono', monospace;
            font-size: 12px;
            font-weight: 600;
            font-feature-settings: 'tnum';
        }

        .okp-settings-row {
            min-height: 34px;
        }

        .okp-settings-scale trough {
            min-height: 6px;
            border-radius: 999px;
            background: rgba(0, 0, 0, 0.13);
        }

        .okp-settings-scale highlight {
            min-height: 6px;
            border-radius: 999px;
            background: #0067c0;
        }

        .okp-settings-scale slider {
            min-width: 18px;
            min-height: 18px;
            border-radius: 999px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.13);
        }

        .okp-settings-button {
            min-width: 82px;
            min-height: 32px;
            border-radius: 7px;
            background: #ffffff;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
        }

        .okp-settings-button:hover {
            background: #f8fafb;
        }

        .okp-info-track-row {
            min-height: 44px;
            padding: 8px 9px;
            border-radius: 7px;
            background: #f8fafb;
            border: 1px solid rgba(0, 0, 0, 0.04);
        }

        .okp-info-track-row.is-selected {
            background: rgba(16, 147, 138, 0.10);
            border-color: rgba(16, 147, 138, 0.18);
        }

        .okp-info-track-kind {
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-title {
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12.5px;
            font-weight: 600;
        }

        .okp-info-track-current {
            padding: 2px 6px;
            border-radius: 5px;
            background: rgba(16, 147, 138, 0.12);
            color: #0a655f;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 8.5px;
            font-weight: 600;
            letter-spacing: 0;
        }

        .okp-info-track-detail {
            color: rgba(0, 0, 0, 0.48);
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 11.5px;
        }

        .okp-info-footer-button {
            min-width: 82px;
            min-height: 34px;
            padding: 0 14px;
            border-radius: 7px;
            background: #e2e8ec;
            border: 1px solid rgba(0, 0, 0, 0.06);
            box-shadow: none;
            color: #161616;
            font-family: 'Segoe UI Variable Text', 'Segoe UI', sans-serif;
            font-size: 12px;
            font-weight: 600;
        }

        .okp-info-footer-button:hover {
            background: #d9e1e7;
        }
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_delay(input: &str, expected: f64) {
        let actual = parse_delay_entry_seconds(input).expect("delay should parse");
        assert!((actual - expected).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_subtitle_delay_entry_as_milliseconds_by_default() {
        assert_delay("250", 0.25);
        assert_delay("-125", -0.125);
        assert_delay("+500ms", 0.5);
    }

    #[test]
    fn parses_subtitle_delay_entry_seconds_suffix() {
        assert_delay("1.5s", 1.5);
        assert_delay("-0.25s", -0.25);
    }

    #[test]
    fn rejects_invalid_subtitle_delay_entry() {
        assert!(parse_delay_entry_seconds("").is_none());
        assert!(parse_delay_entry_seconds("soon").is_none());
        assert!(parse_delay_entry_seconds("nan").is_none());
    }

    #[test]
    fn clamps_subtitle_delay_entry_to_ten_minutes() {
        assert_delay("999999999", 600.0);
        assert_delay("-999999999", -600.0);
    }

    fn github_asset(name: &str) -> GitHubAsset {
        GitHubAsset {
            name: name.to_owned(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: Some(42),
        }
    }

    fn github_release(
        tag_name: &str,
        draft: bool,
        prerelease: bool,
        assets: Vec<GitHubAsset>,
    ) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag_name.to_owned(),
            draft,
            prerelease,
            assets,
        }
    }

    #[test]
    fn linux_version_compare_orders_alpha_numbers_naturally() {
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.10", "0.1.0-linux-alpha.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.45", "0.1.0-linux-alpha.45"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_linux_versions("0.1.0-linux-alpha.44", "0.1.0-linux-alpha.45"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn selects_latest_linux_deb_prerelease_newer_than_current() {
        let update = select_latest_linux_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.46",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.46_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.47",
                    true,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.47_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.48",
                    false,
                    false,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.48_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.49",
                    false,
                    true,
                    vec![github_asset("com.befeast.okplayer.AppImage")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        )
        .expect("alpha46 .deb should be selected");

        assert_eq!(update.version, "0.1.0-linux-alpha.46");
        assert_eq!(update.name, "ok-player_0.1.0-linux-alpha.46_amd64.deb");
        assert_eq!(update.size, Some(42));
    }

    #[test]
    fn deb_update_selection_returns_none_when_only_current_or_older_exist() {
        let update = select_latest_linux_deb_update(
            vec![
                github_release(
                    "linux-v0.1.0-linux-alpha.44",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.44_amd64.deb")],
                ),
                github_release(
                    "linux-v0.1.0-linux-alpha.45",
                    false,
                    true,
                    vec![github_asset("ok-player_0.1.0-linux-alpha.45_amd64.deb")],
                ),
            ],
            "0.1.0-linux-alpha.45",
        );

        assert!(update.is_none());
    }
}
