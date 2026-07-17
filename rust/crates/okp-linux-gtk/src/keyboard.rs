use super::*;

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
        if widget.has_css_class("is-capturing") {
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
        // The in-player Media Information surface owns its keyboard scope. Let
        // focused modal controls receive keys without also driving playback.
        if media_info_modal_is_open(&shortcut_window) {
            return glib::Propagation::Proceed;
        }

        if let Some(propagation) =
            handle_player_space(&shortcut_window, &space_latch, key, modifiers, || {
                chrome.show_for_activity();
                if env::var_os("OKP_DEBUG_INTERACTIONS").is_some() {
                    eprintln!("interaction: keyboard-play-pause-dispatch context=player");
                }
                toggle_play_pause(&state);
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
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekBack) => {
                seek_relative_with_readout(&state, &status_toast, -5.0);
                glib::Propagation::Stop
            }
            Some(ShortcutAction::SeekForward) => {
                seek_relative_with_readout(&state, &status_toast, 5.0);
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

pub(crate) fn connect_progress_persistence(
    window: &gtk::ApplicationWindow,
    state: Rc<RefCell<PlayerState>>,
) {
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
