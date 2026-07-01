use std::cell::{Cell, RefCell};
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::gdk;
use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use okp_core::{AppIdentity, media_formats, natural_compare};
use okp_mpv::{Chapter, Mpv, MpvEvent, Track, TrackKind};
use velopack::VelopackApp;

mod history;
mod thumbnails;

#[derive(Default)]
struct PlayerState {
    mpv: Option<Mpv>,
    current_file: Option<PathBuf>,
    playlist: Vec<PathBuf>,
    pending_subtitles: Vec<PathBuf>,
    pending_resume: Option<(PathBuf, f64)>,
    pending_preferences: Option<(PathBuf, history::PlaybackPreferences)>,
    thumbnail_request_key: Option<String>,
    modes: PlayModes,
    history: history::HistoryStore,
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
    subtitles: Vec<PathBuf>,
}

struct Controls {
    open_button: gtk::Button,
    subtitle_button: gtk::MenuButton,
    audio_button: gtk::MenuButton,
    previous_button: gtk::Button,
    play_button: gtk::Button,
    next_button: gtk::Button,
    repeat_button: gtk::Button,
    shuffle_button: gtk::Button,
    auto_advance_button: gtk::Button,
    seek: gtk::Scale,
    elapsed_label: gtk::Label,
    duration_label: gtk::Label,
    volume: gtk::Scale,
    chapter_marks_snapshot: RefCell<Vec<f64>>,
    up_next_panel: gtk::Box,
    up_next_title: gtk::Label,
    up_next_list: gtk::ListBox,
    side_panel_snapshot: RefCell<SidePanelSnapshot>,
    side_panel_actions: Rc<RefCell<Vec<SidePanelAction>>>,
    thumbnail_sender: mpsc::Sender<String>,
    thumbnail_events: RefCell<mpsc::Receiver<String>>,
}

#[derive(Clone, Default, PartialEq)]
struct SidePanelSnapshot {
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

        if launch.file.is_none() {
            launch.file = Some(PathBuf::from(arg));
        }
    }

    (argv0, launch)
}

fn build_window(app: &gtk::Application, launch_args: LaunchArgs) {
    install_css();

    let identity = AppIdentity::linux();
    let state = Rc::new(RefCell::new(PlayerState::default()));
    let updating_seek = Rc::new(Cell::new(false));
    let updating_volume = Rc::new(Cell::new(false));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(&identity.name)
        .default_width(1120)
        .default_height(680)
        .build();

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
    );

    overlay.set_child(Some(&video_area));
    overlay.add_overlay(&controls_bar(&controls));
    overlay.add_overlay(&controls.up_next_panel);
    window.set_child(Some(&overlay));

    connect_mpv(&video_area, Rc::clone(&state), launch_args);
    connect_drop(&window, Rc::clone(&state));
    connect_keyboard(&window, Rc::clone(&state));
    connect_progress_persistence(&window, Rc::clone(&state));
    connect_state_poll(
        Rc::clone(&state),
        controls,
        Rc::clone(&updating_seek),
        Rc::clone(&updating_volume),
    );

    window.present();
}

fn build_controls(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
) -> Controls {
    let play_button = gtk::Button::with_label("Play");
    play_button.add_css_class("okp-control-button");
    play_button.set_sensitive(false);

    let open_button = gtk::Button::with_label("Open");
    open_button.add_css_class("okp-control-button");

    let subtitle_button = gtk::MenuButton::builder().label("Sub").build();
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.set_sensitive(false);

    let audio_button = gtk::MenuButton::builder().label("Audio").build();
    audio_button.add_css_class("okp-control-button");
    audio_button.set_sensitive(false);

    let previous_button = gtk::Button::with_label("Prev");
    previous_button.add_css_class("okp-control-button");
    previous_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");

    let next_button = gtk::Button::with_label("Next");
    next_button.add_css_class("okp-control-button");
    next_button.set_sensitive(false);

    let repeat_button = gtk::Button::with_label(RepeatMode::Off.label());
    repeat_button.add_css_class("okp-control-button");

    let shuffle_button = gtk::Button::with_label("Shuffle Off");
    shuffle_button.add_css_class("okp-control-button");

    let auto_advance_button = gtk::Button::with_label("Auto On");
    auto_advance_button.add_css_class("okp-control-button");

    let duration_label = gtk::Label::new(Some("00:00"));
    duration_label.add_css_class("okp-time-label");

    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 1.0);
    seek.set_draw_value(false);
    seek.set_hexpand(true);
    seek.set_sensitive(false);
    seek.add_css_class("okp-seek");

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 130.0, 1.0);
    volume.set_draw_value(false);
    volume.set_width_request(116);
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
    up_next_panel.set_halign(gtk::Align::End);
    up_next_panel.set_valign(gtk::Align::Fill);
    up_next_panel.set_margin_top(24);
    up_next_panel.set_margin_end(24);
    up_next_panel.set_margin_bottom(92);
    up_next_panel.set_width_request(320);
    up_next_panel.set_visible(false);
    up_next_panel.append(&up_next_title);
    up_next_panel.append(&up_next_scroller);

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
    subtitle_popover.add_css_class("okp-track-popover");
    subtitle_button.set_popover(Some(&subtitle_popover));
    let subtitle_parent = window.clone();
    let subtitle_state = Rc::clone(&state);
    subtitle_popover.connect_show(move |popover| {
        populate_subtitle_popover(popover, &subtitle_parent, Rc::clone(&subtitle_state));
    });

    let audio_popover = gtk::Popover::new();
    audio_popover.add_css_class("okp-track-popover");
    audio_button.set_popover(Some(&audio_popover));
    let audio_state = Rc::clone(&state);
    audio_popover.connect_show(move |popover| {
        populate_audio_popover(popover, Rc::clone(&audio_state));
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
        let has_media = play_state.borrow().current_file.is_some();
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

    let repeat_state = Rc::clone(&state);
    repeat_button.connect_clicked(move |_| cycle_repeat_mode(&repeat_state));

    let shuffle_state = Rc::clone(&state);
    shuffle_button.connect_clicked(move |_| toggle_shuffle(&shuffle_state));

    let auto_advance_state = Rc::clone(&state);
    auto_advance_button.connect_clicked(move |_| toggle_auto_advance(&auto_advance_state));

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

    let volume_state = Rc::clone(&state);
    volume.connect_change_value(move |_, _, value| {
        if !updating_volume.get()
            && let Some(mpv) = volume_state.borrow().mpv.as_ref()
            && let Err(error) = mpv.set_volume(value)
        {
            eprintln!("Failed to set volume: {error}");
        }

        glib::Propagation::Proceed
    });

    Controls {
        open_button,
        subtitle_button,
        audio_button,
        previous_button,
        play_button,
        next_button,
        repeat_button,
        shuffle_button,
        auto_advance_button,
        seek,
        elapsed_label,
        duration_label,
        volume,
        chapter_marks_snapshot: RefCell::new(Vec::new()),
        up_next_panel,
        up_next_title,
        up_next_list,
        side_panel_snapshot: RefCell::new(SidePanelSnapshot::default()),
        side_panel_actions: up_next_actions,
        thumbnail_sender,
        thumbnail_events: RefCell::new(thumbnail_receiver),
    }
}

fn controls_bar(controls: &Controls) -> gtk::Box {
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bar.add_css_class("okp-controls");
    bar.set_halign(gtk::Align::Fill);
    bar.set_valign(gtk::Align::End);
    bar.set_margin_start(18);
    bar.set_margin_end(18);
    bar.set_margin_bottom(18);

    bar.append(&controls.open_button);
    bar.append(&controls.subtitle_button);
    bar.append(&controls.audio_button);
    bar.append(&controls.previous_button);
    bar.append(&controls.play_button);
    bar.append(&controls.next_button);
    bar.append(&controls.repeat_button);
    bar.append(&controls.shuffle_button);
    bar.append(&controls.auto_advance_button);
    bar.append(&controls.elapsed_label);
    bar.append(&controls.seek);
    bar.append(&controls.duration_label);
    bar.append(&controls.volume);

    bar
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

        if let Err(error) = mpv.create_render_context() {
            eprintln!("Failed to create mpv render context: {error}");
            return;
        }

        realize_state.borrow_mut().mpv = Some(mpv);

        if let Some(path) = launch_args.file.as_deref() {
            load_media_path(&realize_state, path.to_path_buf());
        }
        realize_state
            .borrow_mut()
            .pending_subtitles
            .extend(launch_args.subtitles.clone());
    });

    let render_state = Rc::clone(&state);
    video_area.connect_render(move |area, _context| {
        area.make_current();
        area.attach_buffers();
        let scale = area.scale_factor().max(1);
        let width = area.width() * scale;
        let height = area.height() * scale;
        let mut state = render_state.borrow_mut();
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(width, height)
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
    state: Rc<RefCell<PlayerState>>,
    controls: Controls,
    updating_seek: Rc<Cell<bool>>,
    updating_volume: Rc<Cell<bool>>,
) {
    glib::timeout_add_local(Duration::from_millis(200), move || {
        drain_mpv_events(&state);

        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok());
        let has_media = state.borrow().current_file.is_some();
        let has_playlist = state.borrow().playlist.len() > 1;
        drain_thumbnail_events(&controls);
        update_up_next_panel(&controls, &state);
        update_mode_buttons(&controls, &state);

        if let Some(playback) = playback {
            try_pending_subtitles(&state);

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
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls
                .play_button
                .set_label(if playback.paused { "Play" } else { "Pause" });
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
            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
            controls.audio_button.set_sensitive(has_media);
            controls.previous_button.set_sensitive(has_playlist);
            controls.next_button.set_sensitive(has_playlist);
            controls.play_button.set_label("Play");
            controls.seek.set_sensitive(false);
        }

        glib::ControlFlow::Continue
    });
}

fn update_mode_buttons(controls: &Controls, state: &Rc<RefCell<PlayerState>>) {
    let (repeat_mode, shuffle_enabled, auto_advance_enabled) = {
        let state = state.borrow();
        (
            state.modes.repeat_mode,
            state.modes.shuffle_enabled,
            state.modes.auto_advance_enabled,
        )
    };

    controls.repeat_button.set_label(repeat_mode.label());
    set_button_active(&controls.repeat_button, repeat_mode != RepeatMode::Off);

    controls.shuffle_button.set_label(if shuffle_enabled {
        "Shuffle On"
    } else {
        "Shuffle Off"
    });
    set_button_active(&controls.shuffle_button, shuffle_enabled);

    controls
        .auto_advance_button
        .set_label(if auto_advance_enabled {
            "Auto On"
        } else {
            "Auto Off"
        });
    set_button_active(&controls.auto_advance_button, auto_advance_enabled);
}

fn set_button_active(button: &gtk::Button, active: bool) {
    if active {
        button.add_css_class("is-selected");
    } else {
        button.remove_css_class("is-selected");
    }
}

fn update_up_next_panel(controls: &Controls, state: &Rc<RefCell<PlayerState>>) {
    let snapshot = {
        let state = state.borrow();
        let chapters = state
            .mpv
            .as_ref()
            .map(Mpv::chapters)
            .and_then(Result::ok)
            .unwrap_or_default();

        SidePanelSnapshot {
            current_file: state.current_file.clone(),
            playlist: state.playlist.clone(),
            chapters,
        }
    };
    let is_visible = !snapshot.chapters.is_empty() || snapshot.playlist.len() > 1;

    controls.up_next_panel.set_visible(is_visible);
    if !is_visible {
        controls.side_panel_snapshot.replace(snapshot);
        controls.side_panel_actions.borrow_mut().clear();
        update_chapter_marks(&controls.seek, &controls.chapter_marks_snapshot, &[]);
        clear_list_box(&controls.up_next_list);
        return;
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

    popover.set_child(Some(&content));
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

    popover.set_child(Some(&content));
}

fn track_popover_content(title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    content.add_css_class("okp-track-popover-content");
    content.set_width_request(270);

    content.append(&track_section_title(title));
    content
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
    content.append(&subtitle_adjustment_row(
        "Delay",
        &format_delay(delay_seconds),
        [
            ("-50", SubtitleAdjustment::Delay(-0.05)),
            ("Reset", SubtitleAdjustment::SetDelay(0.0)),
            ("+50", SubtitleAdjustment::Delay(0.05)),
        ],
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

fn format_delay(seconds: f64) -> String {
    let millis = (seconds * 1000.0).round() as i64;
    if millis > 0 {
        format!("+{millis} ms")
    } else {
        format!("{millis} ms")
    }
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

fn connect_drop(window: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let drop_target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    drop_target.connect_drop(move |_, value, _, _| {
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

fn connect_keyboard(window: &gtk::ApplicationWindow, state: Rc<RefCell<PlayerState>>) {
    let controller = gtk::EventControllerKey::new();
    let shortcut_window = window.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
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

fn adjust_volume(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    with_mpv(state, |mpv| {
        let volume = mpv.playback_state()?.volume.unwrap_or(100.0);
        mpv.set_volume(volume + delta)
    });
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

fn seek_to_chapter(state: &Rc<RefCell<PlayerState>>, time: f64) {
    if time.is_finite() && time >= 0.0 {
        with_mpv(state, |mpv| mpv.seek_absolute(time));
    }
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

fn load_media_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    load_media_path_internal(state, path, true);
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
    let resume = state.history.resume_position(&path);
    let preferences = state.history.playback_preferences(&path);
    let playlist_changed = state.playlist != playlist;
    state.current_file = Some(path);
    state.playlist = playlist;
    if playlist_changed {
        state.modes.reset_shuffle_order();
    }
    state.thumbnail_request_key = None;
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

    Ok(())
}

fn save_current_preferences(state: &Rc<RefCell<PlayerState>>) {
    let snapshot = {
        let state = state.borrow();
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
    }
}

fn finite_option(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn save_current_progress(state: &Rc<RefCell<PlayerState>>, finished: bool) {
    let snapshot = {
        let state = state.borrow();
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
    if !is_subtitle_path(&path) || state.borrow().current_file.is_none() {
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
        if state.current_file.is_none() || state.pending_subtitles.is_empty() {
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

        .okp-video-plane {
            background: #050507;
        }

        .okp-controls {
            padding: 10px 12px;
            border-radius: 8px;
            background: rgba(12, 13, 16, 0.86);
            box-shadow: 0 10px 34px rgba(0, 0, 0, 0.38);
        }

        .okp-control-button {
            min-width: 72px;
            min-height: 34px;
        }

        .okp-control-button.is-selected {
            background: rgba(98, 181, 255, 0.22);
        }

        .okp-time-label {
            min-width: 52px;
            color: rgba(255, 255, 255, 0.84);
            font-feature-settings: 'tnum';
        }

        .okp-seek {
            min-width: 260px;
        }

        .okp-volume {
            min-width: 116px;
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
            background: rgba(18, 19, 23, 0.94);
        }

        .okp-track-popover-title {
            margin: 0 4px 6px 4px;
            color: rgba(255, 255, 255, 0.92);
            font-size: 13px;
            font-weight: 700;
        }

        .okp-track-row {
            min-height: 34px;
            padding: 7px 9px;
            border-radius: 7px;
            background: transparent;
            color: rgba(255, 255, 255, 0.82);
        }

        .okp-track-row:hover {
            background: rgba(255, 255, 255, 0.08);
        }

        .okp-track-row.is-selected {
            background: rgba(98, 181, 255, 0.18);
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
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
