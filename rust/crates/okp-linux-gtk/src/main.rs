use std::cell::RefCell;
use std::env;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use okp_core::AppIdentity;
use okp_mpv::Mpv;
use velopack::VelopackApp;

#[derive(Default)]
struct PlayerState {
    mpv: Option<Mpv>,
}

fn main() -> glib::ExitCode {
    VelopackApp::build().set_auto_apply_on_startup(false).run();

    let argv0 = env::args().next().unwrap_or_else(|| "ok-player".to_owned());
    let file = env::args_os().nth(1).map(PathBuf::from);
    let app = gtk::Application::builder()
        .application_id("com.befeast.okplayer")
        .build();

    app.connect_activate(move |app| build_window(app, file.clone()));
    app.run_with_args(&[argv0])
}

fn build_window(app: &gtk::Application, file: Option<PathBuf>) {
    let identity = AppIdentity::linux();
    let state = Rc::new(RefCell::new(PlayerState::default()));

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

    overlay.set_child(Some(&video_area));
    window.set_child(Some(&overlay));

    connect_mpv(&video_area, Rc::clone(&state), file);

    window.present();
}

fn connect_mpv(video_area: &gtk::GLArea, state: Rc<RefCell<PlayerState>>, file: Option<PathBuf>) {
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

        if let Some(path) = file.as_deref()
            && let Err(error) = mpv.load_file(path)
        {
            eprintln!("Failed to load media '{}': {error}", path.display());
        }

        realize_state.borrow_mut().mpv = Some(mpv);
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
