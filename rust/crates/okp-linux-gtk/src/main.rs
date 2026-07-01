use std::cell::{Cell, RefCell};
use std::env;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use okp_core::{AppIdentity, media_formats};
use okp_mpv::Mpv;
use velopack::VelopackApp;

#[derive(Default)]
struct PlayerState {
    mpv: Option<Mpv>,
    current_file: Option<PathBuf>,
    pending_subtitles: Vec<PathBuf>,
}

#[derive(Clone, Default)]
struct LaunchArgs {
    file: Option<PathBuf>,
    subtitles: Vec<PathBuf>,
}

struct Controls {
    open_button: gtk::Button,
    subtitle_button: gtk::Button,
    play_button: gtk::Button,
    seek: gtk::Scale,
    elapsed_label: gtk::Label,
    duration_label: gtk::Label,
    volume: gtk::Scale,
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
    window.set_child(Some(&overlay));

    connect_mpv(&video_area, Rc::clone(&state), launch_args);
    connect_drop(&window, Rc::clone(&state));
    connect_keyboard(&window, Rc::clone(&state));
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

    let subtitle_button = gtk::Button::with_label("Sub");
    subtitle_button.add_css_class("okp-control-button");
    subtitle_button.set_sensitive(false);

    let elapsed_label = gtk::Label::new(Some("00:00"));
    elapsed_label.add_css_class("okp-time-label");

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

    let open_parent = window.clone();
    let open_state = Rc::clone(&state);
    open_button.connect_clicked(move |_| open_media_dialog(&open_parent, Rc::clone(&open_state)));

    let subtitle_parent = window.clone();
    let subtitle_state = Rc::clone(&state);
    subtitle_button.connect_clicked(move |_| {
        open_subtitle_dialog(&subtitle_parent, Rc::clone(&subtitle_state));
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
        play_button,
        seek,
        elapsed_label,
        duration_label,
        volume,
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
    bar.append(&controls.play_button);
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
        let mut state = render_state.borrow_mut();
        if let Some(mpv) = state.mpv.as_mut()
            && let Err(error) = mpv.render(area.width(), area.height())
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
        let playback = state
            .borrow()
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.playback_state().ok());
        let has_media = state.borrow().current_file.is_some();

        if let Some(playback) = playback {
            try_pending_subtitles(&state);

            let duration = playback.duration.unwrap_or(0.0).max(0.0);
            let raw_time = playback.time_pos.unwrap_or(0.0).max(0.0);
            let time_pos = if duration > 0.0 {
                raw_time.min(duration)
            } else {
                raw_time
            };

            controls.play_button.set_sensitive(has_media);
            controls.subtitle_button.set_sensitive(has_media);
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
            controls.play_button.set_label("Play");
            controls.seek.set_sensitive(false);
        }

        glib::ControlFlow::Continue
    });
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

fn with_mpv(
    state: &Rc<RefCell<PlayerState>>,
    command: impl FnOnce(&Mpv) -> Result<(), okp_mpv::MpvError>,
) {
    if let Some(mpv) = state.borrow().mpv.as_ref()
        && let Err(error) = command(mpv)
    {
        eprintln!("mpv command failed: {error}");
    }
}

fn adjust_volume(state: &Rc<RefCell<PlayerState>>, delta: f64) {
    with_mpv(state, |mpv| {
        let volume = mpv.playback_state()?.volume.unwrap_or(100.0);
        mpv.set_volume(volume + delta)
    });
}

fn toggle_fullscreen(window: &gtk::ApplicationWindow) {
    if window.is_fullscreen() {
        window.unfullscreen();
    } else {
        window.fullscreen();
    }
}

fn load_media_path(state: &Rc<RefCell<PlayerState>>, path: PathBuf) {
    if !is_media_path(&path) {
        return;
    }

    let result = {
        let state = state.borrow();
        state.mpv.as_ref().map(|mpv| mpv.load_file(&path))
    };

    match result {
        Some(Ok(())) => state.borrow_mut().current_file = Some(path),
        Some(Err(error)) => eprintln!("Failed to load media '{}': {error}", path.display()),
        None => state.borrow_mut().current_file = Some(path),
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
        ",
    );
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
