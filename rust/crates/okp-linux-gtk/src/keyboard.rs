use super::*;

const APP_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(2);

struct AppShutdownWatchdog {
    cancel: Option<mpsc::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl AppShutdownWatchdog {
    fn arm() -> Self {
        let (cancel, cancellation) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("okp-app-shutdown-watchdog".to_owned())
            .spawn(move || {
                if cancellation.recv_timeout(APP_SHUTDOWN_DEADLINE).is_err() {
                    eprintln!(
                        "Application shutdown exceeded the {:?} deadline; exiting now",
                        APP_SHUTDOWN_DEADLINE
                    );
                    exit_without_destructors(0);
                }
            })
            .unwrap_or_else(|error| {
                eprintln!("Failed to arm the application shutdown watchdog: {error}");
                exit_without_destructors(1);
            });
        Self {
            cancel: Some(cancel),
            join: Some(join),
        }
    }
}

impl Drop for AppShutdownWatchdog {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn exit_without_destructors(status: libc::c_int) -> ! {
    // SAFETY: `_exit` is the final shutdown fallback. It does not return or
    // access Rust-owned state after terminating the current process.
    unsafe { libc::_exit(status) }
}

pub(crate) fn shortcut_modifiers_from_event(modifiers: gdk::ModifierType) -> ShortcutModifiers {
    ShortcutModifiers {
        ctrl: modifiers.contains(gdk::ModifierType::CONTROL_MASK),
        alt: modifiers.contains(gdk::ModifierType::ALT_MASK),
        shift: modifiers.contains(gdk::ModifierType::SHIFT_MASK),
    }
}

pub(crate) fn resolved_shortcut_bindings(
    settings: &settings::SettingsStore,
) -> Result<Vec<ShortcutBinding>, shortcuts::ShortcutConfigError> {
    shortcuts::resolved_bindings_from_text(settings.raw_keybindings_config(), &GdkKeyNames)
}

pub(crate) fn keyboard_action_for_event(
    settings: &settings::SettingsStore,
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> Option<ShortcutAction> {
    let bindings = resolved_shortcut_bindings(settings).unwrap_or_else(|error| {
        eprintln!(
            "Ignoring custom keybindings at line {}: {}",
            error.line, error.message
        );
        shortcuts::default_bindings()
    });

    let key_name = key.to_lower().name()?;
    shortcuts::action_for_key(
        &bindings,
        key_name.as_str(),
        shortcut_modifiers_from_event(modifiers),
    )
}

pub(crate) fn shortcut_chord_from_event(
    key: gdk::Key,
    modifiers: gdk::ModifierType,
) -> Result<ShortcutChord, &'static str> {
    let key_name = key.to_lower().name();
    shortcuts::chord_from_captured_key(
        key_name.as_deref(),
        shortcut_modifiers_from_event(modifiers),
    )
}

pub(crate) fn is_canonical_player_space(key: gdk::Key, modifiers: gdk::ModifierType) -> bool {
    key == gdk::Key::space
        && shortcut_modifiers_from_event(modifiers) == ShortcutModifiers::default()
}

fn focus_owns_space(window: &impl IsA<gtk::Window>) -> bool {
    let mut current = gtk::prelude::GtkWindowExt::focus(window);
    while let Some(widget) = current {
        if widget.has_css_class("is-capturing")
            || (widget.has_css_class("okp-status-toast-path") && widget.is_mapped())
        {
            return true;
        }
        if let Ok(editable) = widget.clone().downcast::<gtk::Editable>()
            && editable.is_editable()
        {
            return true;
        }
        if let Ok(text_view) = widget.clone().downcast::<gtk::TextView>()
            && text_view.is_editable()
        {
            return true;
        }
        current = widget.parent();
    }
    false
}

fn handle_player_space(
    window: &impl IsA<gtk::Window>,
    latch: &RefCell<KeyPressLatch>,
    key: gdk::Key,
    modifiers: gdk::ModifierType,
    dispatch: impl FnOnce(),
) -> Option<glib::Propagation> {
    if !is_canonical_player_space(key, modifiers) {
        return None;
    }
    if latch.borrow().is_pressed() {
        return Some(glib::Propagation::Stop);
    }
    if focus_owns_space(window) {
        return Some(glib::Propagation::Proceed);
    }

    latch.borrow_mut().press();
    dispatch();
    Some(glib::Propagation::Stop)
}

fn connect_space_release(controller: &gtk::EventControllerKey, latch: Rc<RefCell<KeyPressLatch>>) {
    controller.connect_key_released(move |_, key, _, _| {
        if key == gdk::Key::space {
            latch.borrow_mut().release();
        }
    });
}

pub(crate) fn connect_companion_play_pause_space(
    window: &gtk::Window,
    state: Rc<RefCell<PlayerState>>,
) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let space_latch = Rc::new(RefCell::new(KeyPressLatch::default()));
    connect_space_release(&controller, Rc::clone(&space_latch));
    let shortcut_window = window.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        handle_player_space(&shortcut_window, &space_latch, key, modifiers, || {
            if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                eprintln!("interaction: keyboard-play-pause-dispatch context=settings");
            }
            toggle_play_pause(&state);
        })
        .unwrap_or(glib::Propagation::Proceed)
    });
    window.add_controller(controller);
}

pub(crate) fn connect_keyboard(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
    status_toast: Rc<StatusToast>,
    chrome: Rc<ChromeVisibility>,
) {
    let controller = gtk::EventControllerKey::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    let space_latch = Rc::new(RefCell::new(KeyPressLatch::default()));
    connect_space_release(&controller, Rc::clone(&space_latch));
    let shortcut_window = window.clone();
    controller.connect_key_pressed(move |_, key, _, modifiers| {
        if let Some(propagation) =
            handle_player_space(&shortcut_window, &space_latch, key, modifiers, || {
                chrome.show_for_activity();
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: keyboard-play-pause-dispatch context=player");
                }
                toggle_play_pause(&state);
                log_keyboard_interaction("play-pause");
            })
        {
            return propagation;
        }
        chrome.show_for_activity();

        let action = {
            let state = state.borrow();
            keyboard_action_for_event(&state.settings, key, modifiers)
        };

        match action {
            Some(ShortcutAction::PlayPause) => {
                toggle_play_pause(&state);
                log_keyboard_interaction("play-pause");
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekBack) => {
                seek_relative_with_readout(&state, &status_toast, -5.0);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekForward) => {
                seek_relative_with_readout(&state, &status_toast, 5.0);
                log_keyboard_interaction("seek-forward");
                glib::Propagation::Stop
            }
            Some(ShortcutAction::FrameForward) => {
                frame_step_with_readout(&state, &status_toast, true);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::FrameBack) => {
                frame_step_with_readout(&state, &status_toast, false);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::PreviousItem) => {
                navigate_playlist(&state, -1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::NextItem) => {
                navigate_playlist(&state, 1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::VolumeDown) => {
                adjust_volume(&state, &status_toast, -5.0);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::VolumeUp) => {
                adjust_volume(&state, &status_toast, 5.0);
                log_keyboard_interaction("volume-up");
                glib::Propagation::Stop
            }
            Some(ShortcutAction::Mute) => {
                toggle_volume_mute(&state);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenFile) => {
                open_media_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            Some(ShortcutAction::AddSubtitle) => {
                open_subtitle_dialog(&shortcut_window, Rc::clone(&state));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenUrl) => {
                open_url_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            Some(ShortcutAction::CloseMedia) => {
                close_current_media(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::CopyFrame) => {
                copy_frame_to_clipboard(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SaveScreenshot) => {
                save_screenshot(&state, &status_toast, false);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::MediaInfo) => {
                open_media_info_window(&shortcut_window, &state, Rc::clone(&status_toast));
                glib::Propagation::Stop
            }
            Some(ShortcutAction::GoToTime) => {
                open_go_to_time_dialog(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            Some(ShortcutAction::AbLoop) => {
                toggle_ab_loop(&state, &status_toast);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleDelayForward) => {
                adjust_subtitle_delay(&state, 0.05);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleDelayBack) => {
                adjust_subtitle_delay(&state, -0.05);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleSizeDown) => {
                adjust_subtitle_scale(&state, -0.1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleSizeUp) => {
                adjust_subtitle_scale(&state, 0.1);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitlePreviousCue) => {
                with_mpv(&state, |mpv| mpv.seek_previous_subtitle_cue());
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SubtitleNextCue) => {
                with_mpv(&state, |mpv| mpv.seek_next_subtitle_cue());
                glib::Propagation::Stop
            }
            Some(ShortcutAction::Fullscreen) => {
                toggle_fullscreen(&shortcut_window, &state);
                log_keyboard_interaction("fullscreen");
                glib::Propagation::Stop
            }
            Some(ShortcutAction::EscapeFullscreen) if shortcut_window.is_fullscreen() => {
                shortcut_window.unfullscreen();
                glib::Propagation::Stop
            }
            Some(ShortcutAction::EscapeFullscreen) if restore_compact_mode(&shortcut_window) => {
                glib::Propagation::Stop
            }
            Some(ShortcutAction::OpenSettings) => {
                open_settings_window(
                    &shortcut_window,
                    Rc::clone(&state),
                    Rc::clone(&status_toast),
                );
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(controller);
}

fn log_keyboard_interaction(action: &str) {
    if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
        eprintln!("interaction: keyboard={action}");
    }
}

pub(crate) fn connect_progress_persistence(
    app: &gtk::Application,
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
    let timer_state = Rc::clone(&state);
    glib::timeout_add_local(Duration::from_secs(10), move || {
        save_current_progress(&timer_state, false);
        glib::ControlFlow::Continue
    });

    let close_state = Rc::clone(&state);
    let close_app = app.clone();
    let close_started = Rc::new(Cell::new(false));
    window.connect_close_request(move |window| {
        if close_started.replace(true) {
            return glib::Propagation::Stop;
        }
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!("window close lifecycle: close-request");
        }
        close_companion_windows(&close_state);
        save_current_progress(&close_state, false);
        // Unmap before any destroy-path libmpv work. After minimize + secondary
        // present (#518), a still-mapped shell can survive Alt+F4 while unrealize
        // joins render teardown — the candidate waiter then sees IsViewable forever.
        // Pull engine + native render loop out before hide/unrealize so GTK
        // cannot block this handler inside destroy_render_context / join.
        // Candidate headless-launch-smoke then saw an unmapped shell + live process.
        let (engine, render_loop) = {
            let mut state = close_state.borrow_mut();
            (state.mpv.take(), state.native_render_loop.take())
        };
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!("window close lifecycle: engine detached");
        }
        window.set_visible(false);
        if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
            eprintln!("window close lifecycle: window hidden");
        }
        let close_app = close_app.clone();
        glib::idle_add_local_once(move || {
            let shutdown_watchdog = AppShutdownWatchdog::arm();
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!("window close lifecycle: idle quit");
            }
            // Quit before any libmpv Drop. Mpv::drop can block in
            // terminate_destroy; doing that on the GTK thread starves
            // Application::quit and leaves a headless residual process. The
            // watchdog makes the process boundary finite even if native or
            // libmpv teardown stops responding.
            close_app.quit();
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!("window close lifecycle: quit requested");
            }
            if let Some(mut render_loop) = render_loop
                && !render_loop.stop_and_join()
            {
                eprintln!(
                    "Native render shutdown missed its deadline; exiting without unsafe teardown"
                );
                exit_without_destructors(0);
            }
            drop(engine);
            if env::var_os("OKP_DEBUG_WINDOW_FIT").is_some() {
                eprintln!("window close lifecycle: engine teardown complete");
            }
            drop(shutdown_watchdog);
        });
        glib::Propagation::Proceed
    });
}
